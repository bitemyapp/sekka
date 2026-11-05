import importlib.util
import json
import os
from pathlib import Path
import sys
import socket
import tempfile
import threading
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location(
    "lag_recorder", Path(__file__).resolve().parents[1] / "nix/scheduling/lag-recorder.py"
)
recorder = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = recorder
spec.loader.exec_module(recorder)


class CounterTest(unittest.TestCase):
    def test_proc_stat_comm_does_not_shift_fault_and_identity_fields(self):
        # comm may contain spaces and closing parentheses; the final ') ' is
        # the delimiter, not the first one in the name.
        fields = ["S", "1", "1", "1", "0", "0", "0", "12", "0", "3", "0",
                  "40", "2", "0", "0", "20", "0", "1", "0", "1234", "4096", "17"]
        parsed = recorder.parse_proc_stat("123 (name ) with spaces) " + " ".join(fields))
        self.assertEqual(parsed["comm"], "name ) with spaces")
        self.assertEqual(parsed["minor_faults"], 12)
        self.assertEqual(parsed["major_faults"], 3)
        self.assertEqual(parsed["cpu_ticks"], 42)
        self.assertEqual(parsed["start_ticks"], 1234)
        self.assertEqual(parsed["rss_pages"], 17)

    def test_counter_reset_and_missing_baseline_are_not_activity(self):
        self.assertEqual(recorder.counter_delta(104, 100), 4)
        self.assertEqual(recorder.counter_delta(100, 100), 0)
        self.assertIsNone(recorder.counter_delta(2, 100))
        self.assertIsNone(recorder.counter_delta(104, None))
        self.assertIsNone(recorder.counter_delta(None, 100))


    def test_pid_reuse_does_not_inherit_old_cpu_or_fault_history(self):
        before = dict(pid=10, start_ticks=100, cpu_ticks=1000, minor_faults=40, major_faults=8)
        after = dict(pid=10, start_ticks=100, cpu_ticks=1100, minor_faults=44, major_faults=10)
        same = recorder.process_delta(after, before, 2, 100)
        self.assertEqual(same["cpu_percent_one_core"], 50)
        self.assertEqual(same["delta"]["major_faults"], 2)
        reused = recorder.process_delta({**after, "start_ticks": 200}, before, 2, 100)
        self.assertIsNone(reused["cpu_percent_one_core"])
        self.assertIsNone(reused["delta"]["major_faults"])

    def test_disk_rates_use_sector_bytes_and_elapsed_time(self):
        old = dict(reads=10, sectors_read=100, read_ms=30, writes=5,
                   sectors_written=20, write_ms=10, io_ms=200, in_flight=5)
        new = dict(reads=14, sectors_read=108, read_ms=50, writes=7,
                   sectors_written=24, write_ms=30, io_ms=600, in_flight=0)
        measured = recorder.derive_disk(new, old, 2)
        self.assertEqual(measured["read_bytes_per_second"], 2048)
        self.assertEqual(measured["write_bytes_per_second"], 1024)
        self.assertEqual(measured["busy_percent"], 20)
        self.assertEqual(measured["read_await_ms"], 5)
        self.assertEqual(measured["write_await_ms"], 10)
        reset = recorder.derive_disk({**new, "reads": 1, "sectors_read": 1}, old, 2)
        self.assertIsNone(reset["read_bytes_per_second"])
        self.assertIsNone(reset["read_await_ms"])


class IncidentTest(unittest.TestCase):
    @staticmethod
    def sample(second):
        return {"sample_id": second, "monotonic_seconds": second}

    def test_trigger_retains_bounded_ordered_history_and_waits_for_quiet(self):
        tracker = recorder.IncidentTracker(1, quiet_seconds=2, history_size=2)
        for second in range(3):
            tracker.observe(self.sample(second), [])
        records = tracker.observe(self.sample(3), ["memory"])
        self.assertEqual(records[0]["type"], "event_start")
        self.assertEqual([r["sample_id"] for r in records if r["type"] == "sample"], [1, 2, 3])
        self.assertEqual([r["context"] for r in records if r["type"] == "sample"],
                         ["prehistory", "prehistory", "incident"])
        tracker.observe(self.sample(4), ["memory"])
        self.assertNotIn("event_end", [r["type"] for r in tracker.observe(self.sample(5), [])])
        self.assertEqual(tracker.observe(self.sample(6), [])[-1]["type"], "event_end")
        next_event = tracker.observe(self.sample(7), [], manual=True)
        self.assertEqual(next_event[0]["type"], "event_start")
        self.assertNotEqual(next_event[0]["event_id"], records[0]["event_id"])
        self.assertEqual(next_event[0]["reasons"], ["manual"])

    def test_details_are_rate_limited_during_repeated_triggers(self):
        tracker = recorder.IncidentTracker(1)
        self.assertTrue(tracker.details_due(100))
        self.assertFalse(tracker.details_due(100))
        self.assertFalse(tracker.details_due(129.9))
        self.assertTrue(tracker.details_due(130))

    def test_unavailable_compositor_is_not_reported_as_a_stall(self):
        sample = {"compositor_ipc": {"status": "unavailable", "latency_seconds": 2}}
        self.assertEqual(recorder.trigger_reasons(sample, 1), [])
        sample["compositor_ipc"]["status"] = "timeout"
        self.assertIn("compositor_ipc_timeout", recorder.trigger_reasons(sample, 1))


class StorageTest(unittest.TestCase):
    def test_rotation_keeps_recent_complete_private_records_within_limit(self):
        with tempfile.TemporaryDirectory() as root, \
                patch.object(recorder, "LOG_BYTES", 256), patch.object(recorder, "LOG_BACKUPS", 2):
            directory = Path(root) / "capture"
            with recorder.PrivateLog(directory) as log:
                for number in range(30):
                    log.write({"sequence": number, "data": "x" * 70})
            self.assertEqual(directory.stat().st_mode & 0o777, 0o700)
            records = []
            for name in ("recorder.jsonl.2", "recorder.jsonl.1", "recorder.jsonl"):
                path = directory / name
                self.assertLessEqual(path.stat().st_size, 256)
                self.assertEqual(path.stat().st_mode & 0o777, 0o600)
                records.extend(json.loads(line)["sequence"] for line in path.read_text().splitlines())
            self.assertEqual(records, list(range(records[0], 30)))
            self.assertGreater(records[0], 0)
            self.assertFalse((directory / "recorder.jsonl.3").exists())

    def test_second_writer_cannot_take_or_release_the_first_writers_lock(self):
        with tempfile.TemporaryDirectory() as root:
            with recorder.PrivateLog(root) as first:
                for _ in range(2):
                    with self.assertRaisesRegex(RuntimeError, "already owns"):
                        recorder.PrivateLog(root)
                first.write({"sequence": 1})
            with recorder.PrivateLog(root) as next_writer:
                next_writer.write({"sequence": 2})
            self.assertEqual([json.loads(line)["sequence"] for line in
                              (Path(root) / "recorder.jsonl").read_text().splitlines()], [1, 2])

    def test_symlink_log_is_refused_without_touching_its_target(self):
        with tempfile.TemporaryDirectory() as root:
            directory = Path(root) / "capture"
            directory.mkdir()
            target = Path(root) / "other-file"
            target.write_text("preserve")
            (directory / "recorder.jsonl").symlink_to(target)
            with self.assertRaises(RuntimeError):
                recorder.PrivateLog(directory)
            self.assertEqual(target.read_text(), "preserve")


class JournalTest(unittest.TestCase):
    def test_journal_timeout_preserves_window_until_success(self):
        worker = recorder.DetailWorker({})
        worker.journal_since = 100
        outcomes = [
            {"status": "timeout", "returncode": -9, "output": "", "stderr": ""},
            {"status": "ok", "returncode": 0, "output": "", "stderr": ""},
        ]
        with patch.object(recorder.time, "time", return_value=200), \
                patch.object(recorder, "bounded_command", side_effect=outcomes):
            self.assertEqual(worker.journal()["status"], "timeout")
            self.assertEqual(worker.journal_since, 100)
            self.assertEqual(worker.journal()["status"], "ok")
            self.assertEqual(worker.journal_since, 200)


class ProbeTest(unittest.TestCase):
    def probe(self, reply, allowed=True, hold=False, timeout=1):
        with tempfile.TemporaryDirectory() as root, \
                socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as server:
            path = str(Path(root) / "ipc.sock")
            server.bind(path)
            server.listen(1)
            server.settimeout(2)
            release = threading.Event()
            received, errors = [], []

            def serve():
                try:
                    with server.accept()[0] as client:
                        client.settimeout(2)
                        received.append(client.recv(1024))
                        if hold:
                            release.wait(2)
                        if reply:
                            try:
                                client.sendall(reply)
                            except BrokenPipeError:
                                pass
                except Exception as exc:
                    errors.append(exc)

            thread = threading.Thread(target=serve)
            thread.start()
            try:
                result = recorder.probe_socket(path, {os.getpid()} if allowed else set(), timeout=timeout)
            finally:
                release.set()
                thread.join(3)
            self.assertFalse(thread.is_alive())
            self.assertEqual(errors, [])
            return result, received

    def test_untrusted_peer_receives_no_request(self):
        result, received = self.probe(b"", allowed=False)
        self.assertEqual(result["status"], "unavailable")
        self.assertEqual(result["error"], "untrusted_peer")
        self.assertEqual(received, [b""])

    def test_only_read_only_version_request_is_sent(self):
        result, received = self.probe(b'{"response":"version","protocol":1}\n')
        self.assertEqual(result["status"], "ok")
        self.assertEqual(result["version"]["protocol"], 1)
        self.assertEqual(received, [b'{"request":"version"}\n'])

    def test_unresponsive_authenticated_peer_times_out(self):
        result, _ = self.probe(b"", hold=True, timeout=0.05)
        self.assertEqual(result["status"], "timeout")

    def test_unterminated_reply_cannot_grow_without_bound(self):
        result, _ = self.probe(b"x" * 16384)
        self.assertEqual(result["status"], "unavailable")
        self.assertEqual(result["error"], "reply_limit")


if __name__ == "__main__":
    unittest.main()
