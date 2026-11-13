import copy
import importlib.util
from pathlib import Path
import unittest


spec = importlib.util.spec_from_file_location(
    "lag_summary", Path(__file__).resolve().parents[1] / "nix/scheduling/lag_summary.py"
)
summary = importlib.util.module_from_spec(spec)
spec.loader.exec_module(summary)


def reading(value):
    return {"status": "ok", "value": value}


def pressure(stall=61.5):
    return reading({
        "some": {"avg10": 82.5, "avg60": 43.0, "avg300": 12.25,
                 "total": 987654321, "delta_total_usec": 615000,
                 "stall_percent": stall},
        "full": {"avg10": 58.0, "avg60": 24.0, "avg300": 9.0,
                 "total": 87654321, "delta_total_usec": 495000,
                 "stall_percent": 49.5},
    })


def process(pid=101, comm="aizome"):
    return {
        "status": "ok", "pid": pid, "start_ticks": 765432, "comm": comm,
        "state": "D", "rss_bytes": 734003200,
        "memory_status": reading({"VmRSS": 734003200, "VmSwap": 125829120}),
        "wchan": reading("folio_wait_bit_common"),
        "schedstat": reading({"runtime_ns": 1000000000, "runqueue_wait_ns": 9000000000,
                              "timeslices": 91}),
        "cgroup": reading("/user.slice/user-1000.slice/user@1000.service/session.slice/aizome.service"),
        "exe": reading("/nix/store/full-executable-path"),
        "interval": {"per_second": {"cpu_ticks": 90, "minor_faults": 700, "major_faults": 18},
                     "delta": {"cpu_ticks": 180, "minor_faults": 1400, "major_faults": 36},
                     "unavailable": {}, "cpu_percent_one_core": 90,
                     "identity_status": "same_process"},
    }


def sample():
    return {
        "type": "sample", "schema": 1, "run_id": "run-one", "boot_id": "boot-one",
        "sample_id": 123, "wall_utc": "2026-11-05T12:30:00+00:00", "monotonic_seconds": 1800.25,
        "elapsed_seconds": 4.5, "sampling_gap_seconds": 3.5,
        "schedule_lateness_seconds": 3.25, "sample_duration_seconds": 0.17,
        "worker_busy": True, "event_id": 3, "context": "incident",
        "trigger_reasons": ["memory.full.avg10>=10%", "sampling_delay", "compositor_ipc_timeout"],
        "pressure": {name: pressure() for name in ("cpu", "memory", "io")},
        "cpu": {"status": "ok", "value": {"user": 10000},
                "percent": {"user": 15.5, "system": 10.5, "idle": 4, "iowait": 70}},
        "loadavg": reading({"one": 31.2, "five": 18.7, "fifteen": 7.2, "runnable": 32, "tasks": 850}),
        "meminfo_bytes": {**reading({"MemTotal": 16 << 30, "MemAvailable": 70 << 20,
                                     "SwapTotal": 32 << 30, "SwapFree": 10 << 30,
                                     "Zswap": 2 << 30, "Zswapped": 9 << 30}), "not_reported": []},
        "vmstat": {"status": "ok", "not_reported": [],
                   "interval": {"per_second": {"pgfault": 5000, "pgmajfault": 700,
                                               "pswpin": 250, "pswpout": 125,
                                               "zswpin": 80, "zswpout": 40, "zswpwb": 8,
                                               "pgscan_direct": 900, "pgsteal_direct": 850,
                                               "allocstall_normal": 17, "workingset_refault_anon": 600},
                                "unavailable": {}}},
        "diskstats": {"status": "ok", "devices": {
            "259:0:nvme0n1": {"sysfs_inode": 450, "counters": {"in_flight": 12, "reads": 10000},
                               "interval": {"read_bytes_per_second": 2097152,
                                            "write_bytes_per_second": 1048576,
                                            "busy_percent": 99.5, "await_ms": 17.0,
                                            "read_await_ms": 14.0, "write_await_ms": 23.0,
                                            "per_second": {"reads": 80, "writes": 40},
                                            "unavailable": {}}}}, "unavailable_devices": []},
        "aizome": {"processes": [process()], "errors": [], "bounded_scan": False},
        "compositor_ipc": {"status": "timeout", "error": "deadline_exceeded",
                           "latency_seconds": 0.1008, "peer_pid": 101},
    }


class CompactSampleTest(unittest.TestCase):
    def test_stall_fault_swap_io_and_desktop_evidence_keep_units(self):
        source = sample()
        result = summary.compact_sample(source)
        for resource in ("cpu", "memory", "io"):
            for scope in ("some", "full"):
                for field in ("avg10", "avg60", "avg300", "stall_percent"):
                    self.assertEqual(result["pressure"][resource]["value"][scope][field],
                                     source["pressure"][resource]["value"][scope][field])
        self.assertEqual(result["cpu"]["percent"]["iowait"], 70)
        self.assertEqual(result["loadavg"]["value"]["runnable"], 32)
        self.assertEqual(result["meminfo_bytes"]["value"]["MemAvailable"], 70 << 20)
        self.assertEqual(result["meminfo_bytes"]["value"]["SwapFree"], 10 << 30)
        self.assertEqual(result["meminfo_bytes"]["value"]["Zswap"], 2 << 30)
        self.assertEqual(result["meminfo_bytes"]["value"]["Zswapped"], 9 << 30)
        self.assertEqual(result["vmstat"]["interval"], source["vmstat"]["interval"])
        disk = result["diskstats"]["devices"]["259:0:nvme0n1"]
        self.assertEqual(disk["interval"], source["diskstats"]["devices"]["259:0:nvme0n1"]["interval"])
        self.assertEqual(disk["counters"]["in_flight"], 12)
        desktop = result["aizome"]["processes"][0]
        self.assertEqual((desktop["pid"], desktop["start_ticks"]), (101, 765432))
        self.assertEqual(desktop["interval"]["per_second"]["major_faults"], 18)
        self.assertEqual(desktop["interval"]["cpu_percent_one_core"], 90)
        self.assertEqual(desktop["rss_bytes"], 734003200)
        self.assertEqual(desktop["memory_status"]["value"]["VmSwap"], 125829120)
        self.assertEqual(desktop["schedstat"]["value"]["runqueue_wait_ns"], 9000000000)
        self.assertEqual(desktop["wchan"]["value"], "folio_wait_bit_common")
        self.assertEqual(desktop["state"], "D")
        self.assertEqual(result["compositor_ipc"], source["compositor_ipc"])

    def test_identity_events_and_actual_sampling_times_survive_reprojection(self):
        source = sample()
        source["reasons"] = ["manual"]
        result = summary.compact_sample(source)
        self.assertEqual(result["type"], "summary")
        for field in summary.IDENTITY_FIELDS:
            self.assertEqual(result[field], source[field])
        self.assertEqual(result["reasons"], ["manual"])
        self.assertEqual(result["trigger_reasons"], source["trigger_reasons"])
        replay = summary.compact_sample({**source, "context": "prehistory", "event_id": 4})
        self.assertEqual(tuple(result[key] for key in ("run_id", "boot_id", "sample_id")),
                         tuple(replay[key] for key in ("run_id", "boot_id", "sample_id")))
        self.assertNotEqual(result["event_id"], replay["event_id"])
        quiet = summary.compact_sample({key: value for key, value in source.items()
                                        if key not in ("event_id", "reasons", "trigger_reasons")})
        self.assertEqual(quiet["sampling_gap_seconds"], 3.5)
        self.assertNotIn("event_id", quiet)

    def test_configured_protection_and_ancestor_usage_are_not_effective_guarantees(self):
        manager = "/user.slice/user-1000.slice/user@1000.service"
        paths = ["/", "/user.slice", "/user.slice/user-1000.slice", manager,
                 manager + "/session.slice", manager + "/session.slice/aizome.service",
                 manager + "/app.slice/app-background.slice", "/system.slice/nix-daemon.service"]
        source = sample()
        source["cgroups"] = {path: {
            "inode": index + 900,
            "memory.current": reading((index + 1) << 28),
            "memory.low": reading((index + 1) << 20), "memory.min": reading(index << 20),
            "memory.high": reading("max" if index == 0 else 2 << 30),
            "memory.max": reading(8 << 30), "memory.swap.current": reading(index << 26),
            "memory.zswap.current": reading(index << 24),
            "memory.stat": reading({"pgfault": 20000 + index, "pgmajfault": 99 + index,
                                    "workingset_refault_file": 54, "pgscan_direct": 88}),
            "memory.events": reading({"low": 17, "high": 33, "oom_kill": 1}),
            "memory.events.local": reading({"low": 2, "high": 3, "oom_kill": 0}),
            "cpu.stat": reading({"usage_usec": 150000, "throttled_usec": 55000}),
            "memory.pressure": pressure(), "cpu.pressure": pressure(), "io.pressure": pressure(),
        } for index, path in enumerate(paths)}
        result = summary.compact_sample(source)
        for path in paths:
            for field in ("inode", "memory.current", "memory.low", "memory.min",
                          "memory.high", "memory.max", "memory.swap.current", "memory.zswap.current"):
                self.assertEqual(result["cgroups"][path][field], source["cgroups"][path][field])
        for path in paths[4:]:
            group = result["cgroups"][path]
            self.assertEqual(group["memory.stat"]["value"]["pgmajfault"],
                             source["cgroups"][path]["memory.stat"]["value"]["pgmajfault"])
            self.assertEqual(group["memory.stat"]["value"]["workingset_refault_file"], 54)
            self.assertEqual(group["memory.stat"]["value"]["pgscan_direct"], 88)
            self.assertEqual(group["memory.events"]["value"]["oom_kill"], 1)
            for field in ("memory.pressure", "cpu.pressure", "io.pressure"):
                self.assertEqual(group[field]["value"]["full"]["avg10"], 58.0)
                self.assertEqual(group[field]["value"]["full"]["stall_percent"], 49.5)
        self.assertEqual(result["cgroups"]["/"]["memory.high"]["value"], "max")
        self.assertNotIn("memory.stat", result["cgroups"]["/"])
        self.assertNotIn("memory.pressure", result["cgroups"][manager])

    def test_missing_metrics_resets_and_read_failures_do_not_become_zero(self):
        source = sample()
        failure = {"status": "unavailable", "error": "PermissionError", "errno": 13,
                   "message": "permission denied"}
        source["pressure"]["io"] = failure.copy()
        psi = source["pressure"]["memory"]["value"]["full"]
        psi.update(stall_percent=None, interval_status="counter_reset")
        source["meminfo_bytes"]["value"].pop("Zswap")
        source["meminfo_bytes"]["not_reported"] = ["Zswap"]
        source["vmstat"]["not_reported"] = ["zswpwb"]
        source["vmstat"]["interval"] = {
            "per_second": {"pswpin": None, "pswpout": None, "pgmajfault": None, "pgfault": 0},
            "unavailable": {"pswpin": "counter_reset", "pswpout": "no_previous",
                            "pgmajfault": "invalid_interval"},
        }
        source["cpu"]["percent"]["iowait"] = None
        source["aizome"]["processes"][0]["wchan"] = failure.copy()
        source["aizome"]["processes"][0]["interval"].update(
            identity_status="new_identity", cpu_percent_one_core=None,
            per_second={"major_faults": None}, unavailable={"major_faults": "no_previous"})
        source["cgroups"] = {"/missing": failure.copy(), "/": {"memory.low": failure.copy()}}
        source["cgroups"]["/user.slice/session.slice"] = {
            "memory.stat": failure.copy(),
            "memory.pressure": reading({"full": {
                "avg10": 10, "stall_percent": None, "interval_status": "no_previous"}}),
        }
        source["diskstats"]["devices"]["259:0:nvme0n1"]["interval"].update(
            read_bytes_per_second=None, read_await_ms=None,
            per_second={"sectors_read": None}, unavailable={"sectors_read": "counter_reset"})
        result = summary.compact_sample(source)
        self.assertEqual(result["pressure"]["io"], failure)
        self.assertIsNone(result["pressure"]["memory"]["value"]["full"]["stall_percent"])
        self.assertEqual(result["pressure"]["memory"]["value"]["full"]["interval_status"], "counter_reset")
        self.assertNotIn("Zswap", result["meminfo_bytes"]["value"])
        self.assertEqual(result["meminfo_bytes"]["not_reported"], ["Zswap"])
        self.assertEqual(result["vmstat"], source["vmstat"])
        self.assertIsNone(result["cpu"]["percent"]["iowait"])
        self.assertEqual(result["cgroups"], source["cgroups"])
        desktop = result["aizome"]["processes"][0]
        self.assertEqual(desktop["wchan"], failure)
        self.assertEqual(desktop["interval"]["identity_status"], "new_identity")
        self.assertIsNone(desktop["interval"]["cpu_percent_one_core"])
        self.assertEqual(desktop["interval"]["unavailable"], {"major_faults": "no_previous"})
        disk = result["diskstats"]["devices"]["259:0:nvme0n1"]["interval"]
        self.assertIsNone(disk["read_bytes_per_second"])
        self.assertEqual(disk["unavailable"], {"sectors_read": "counter_reset"})

    def test_caps_expose_loss_and_keep_compositor_and_important_groups(self):
        source = sample()
        source["cgroups"] = {
            f"/user.slice/user-1000.slice/user@1000.service/session.slice/app-{number}.scope":
                {"memory.current": reading(number)} for number in range(20)}
        for path in ("/", "/user.slice/user-1000.slice/user@1000.service/session.slice/aizome.service",
                     "/user.slice/user-1000.slice/user@1000.service/session.slice",
                     "/user.slice", "/user.slice/user-1000.slice",
                     "/user.slice/user-1000.slice/user@1000.service",
                     "/user.slice/user-1000.slice/user@1000.service/app.slice/app-background.slice",
                     "/system.slice/nix-daemon.service"):
            source["cgroups"][path] = {"memory.low": reading(42)}
        source["diskstats"]["devices"] = {f"8:{number}:disk{number}": {
            "sysfs_inode": number, "interval": {"busy_percent": number}}
            for number in range(10)}
        source["diskstats"]["truncated_devices"] = True
        source["diskstats"]["unavailable_devices"] = [{"device": "8:90:gone", "status": "unavailable", "error": "FileNotFoundError"}]
        source["aizome"]["processes"] = [process(number, "aizome-shell") for number in range(12)] + [process(900)]
        source["aizome"]["bounded_scan"] = True
        source["aizome"]["errors"] = [{"pid": 901, "status": "unavailable", "error": "pid_reused"}]
        result = summary.compact_sample(source)
        self.assertTrue(result["cgroups_truncated"])
        self.assertEqual(result["cgroups_omitted"], 12)
        self.assertEqual(len(result["cgroups"]), 16)
        for path, group in source["cgroups"].items():
            if "memory.low" in group:
                self.assertEqual(result["cgroups"][path]["memory.low"]["value"], 42)
        self.assertTrue(result["diskstats"]["summary_truncated_devices"])
        self.assertEqual(result["diskstats"]["summary_omitted_devices"], 2)
        self.assertEqual(len(result["diskstats"]["devices"]), 8)
        self.assertEqual(result["diskstats"]["devices"]["8:7:disk7"]["interval"]["busy_percent"], 7)
        self.assertTrue(result["diskstats"]["truncated_devices"])
        self.assertEqual(result["diskstats"]["unavailable_devices"], source["diskstats"]["unavailable_devices"])
        self.assertTrue(result["aizome"]["processes_truncated"])
        self.assertEqual(result["aizome"]["processes_omitted"], 5)
        self.assertEqual(len(result["aizome"]["processes"]), 8)
        self.assertEqual(result["aizome"]["processes"][0]["pid"], 900)
        self.assertTrue(result["aizome"]["bounded_scan"])
        self.assertEqual(result["aizome"]["errors"], source["aizome"]["errors"])

    def test_ipc_failures_and_attempt_caps_are_not_replaced_by_version_metadata(self):
        source = sample()
        source["compositor_ipc"] = {
            "status": "unavailable", "error": "session_socket_unavailable",
            "attempts": [{"socket": f"/run/user/1000/aizome.{number}.sock", "status": "unavailable",
                          "error": "untrusted_peer", "peer_pid": number} for number in range(18)],
            "version": {"build": {"huge": "not timeline evidence"}},
        }
        result = summary.compact_sample(source)["compositor_ipc"]
        self.assertEqual(result["error"], "session_socket_unavailable")
        self.assertEqual(result["attempts"][15]["peer_pid"], 15)
        self.assertEqual(result["attempts"][15]["error"], "untrusted_peer")
        self.assertTrue(result["attempts_truncated"])
        self.assertEqual(result["attempts_omitted"], 2)
        self.assertNotIn("version", result)
        successful = summary.compact_sample({"compositor_ipc": {
            "status": "ok", "peer_pid": 101, "latency_seconds": 0.072,
            "version": {"version": "v1", "build": {"large": "metadata"}},
        }})["compositor_ipc"]
        self.assertEqual(successful, {"status": "ok", "peer_pid": 101, "latency_seconds": 0.072})

    def test_projection_is_independent_and_does_not_capture_environment_or_arguments(self):
        source = sample()
        source["environ"] = {"SECRET": "private"}
        source["argv"] = ["private-command"]
        source["aizome"]["processes"][0]["environ"] = {"SECRET": "private"}
        source["aizome"]["processes"][0]["argv"] = ["private-command"]
        before = copy.deepcopy(source)
        result = summary.compact_sample(source)
        self.assertEqual(source, before)
        self.assertNotIn("environ", result)
        self.assertNotIn("argv", result)
        for key in ("environ", "argv", "exe"):
            self.assertNotIn(key, result["aizome"]["processes"][0])
        result["pressure"]["memory"]["value"]["full"]["avg10"] = 0
        result["trigger_reasons"].append("changed")
        result["aizome"]["processes"][0]["memory_status"]["value"]["VmSwap"] = 0
        self.assertEqual(source, before)


if __name__ == "__main__":
    unittest.main()
