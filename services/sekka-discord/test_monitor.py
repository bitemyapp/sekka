"""Behavioral tests; no live Discord requests or system commands."""

import copy
import io
import json
import os
from pathlib import Path
import stat
import tempfile
import unittest
from unittest import mock

import monitor


class MemoryStore:
    def __init__(self):
        self.snapshots = []

    def save(self, state):
        self.snapshots.append(copy.deepcopy(state))


class MonitorTests(unittest.TestCase):
    def setUp(self):
        self.state = monitor.new_state()
        self.store = MemoryStore()
        self.discord = mock.Mock()

    def tick(self, observations, now, deliver=True):
        monitor.update_incidents(self.state, observations, now)
        monitor.queue_pending(self.state, now)
        pending = copy.deepcopy(self.state["pending"])
        if deliver:
            monitor.deliver_pending(self.state, self.store, self.discord, now)
        return pending

    def journal(self, records, state=None, capped=False, lan=()):
        raw = b"".join(json.dumps(record).encode() + b"\n" for record in records)
        runner = mock.Mock(return_value=(raw, 0, capped))
        observations = {}
        with mock.patch.object(monitor.os, "geteuid", return_value=0):
            monitor.observe_journal(state or self.state, observations, runner, lan)
        return observations, runner

    @staticmethod
    def record(index, message, **fields):
        return {"__CURSOR": "cursor-" + str(index), "__REALTIME_TIMESTAMP": str((1000 + index) * 1000000),
                "MESSAGE": message, "_SYSTEMD_UNIT": monitor.BOT, **fields}

    def test_incident_dedup_and_recovery_survive_reload(self):
        pending = self.tick({"health": 1}, 1000)
        self.assertIn("DETECTED", monitor.render(pending))
        self.state = json.loads(json.dumps(self.store.snapshots[-1]))
        monitor.validate_state(self.state)
        self.assertIsNone(self.tick({"health": 1}, 1060))
        # The unsent observation is summarized before the quiet transition.
        self.tick({"health": 0}, 1120)
        recovery = self.tick({"health": 0}, 1180)
        self.assertIn("RECOVERED", monitor.render(recovery))
        self.assertIsNone(self.tick({"health": 0}, 1240))

    def test_cooldown_aggregates_counts_without_per_request_alerts(self):
        first = self.tick({"webhook": 7}, 1000)
        self.assertEqual(first["items"][0]["count"], 7)
        self.assertIsNone(self.tick({"webhook": 11}, 1060))
        self.assertIsNone(self.tick({"webhook": 13}, 1120))
        next_alert = self.tick({"webhook": 2}, 1000 + monitor.COOLDOWN)
        self.assertEqual(next_alert["items"][0]["count"], 26)

    def test_activity_recovery_is_quiet_not_remediation_claim(self):
        self.tick({"unresolved": 1}, 1000)
        self.assertIsNone(self.tick({}, 1000 + monitor.QUIET - 1))
        pending = self.tick({}, 1000 + monitor.QUIET)
        text = monitor.render(pending)
        self.assertIn("QUIET", text)
        self.assertIn("not proof of remediation", text)
        self.assertNotIn("RECOVERED", text)

    def test_collection_failure_does_not_clear_security_incident(self):
        self.tick({"webhook": 4}, 1000)
        self.tick({"journal": 1}, 2000)
        self.assertTrue(self.state["incidents"]["webhook"]["active"])
        self.tick({"journal_backlog": 1}, 3000)
        self.assertTrue(self.state["incidents"]["webhook"]["active"])

    def test_failed_delivery_retains_exact_reference_and_retry_state(self):
        pending = self.tick({"health": 1}, 1000, deliver=False)
        self.discord.deliver.side_effect = monitor.DeliveryError(uncertain=True)
        with mock.patch("sys.stderr", new=io.StringIO()):
            self.assertFalse(monitor.deliver_pending(self.state, self.store, self.discord, 1000))
        self.assertEqual(self.state["pending"]["reference"], pending["reference"])
        self.assertTrue(self.state["pending"]["uncertain"])
        self.assertTrue(self.store.snapshots[0]["pending"]["uncertain"])
        self.assertFalse(self.state["incidents"]["health"]["delivered_active"])
        self.discord.deliver.side_effect = None
        monitor.deliver_pending(self.state, self.store, self.discord, 1060)
        self.assertTrue(self.discord.deliver.call_args.args[0]["uncertain"])
        self.assertIsNone(self.state["pending"])

    def test_short_incident_is_not_lost_when_delivery_fails_until_recovery(self):
        self.tick({"health": 1}, 1000, deliver=False)
        reference = self.state["pending"]["reference"]
        pending = self.tick({"health": 0}, 1060)
        self.assertEqual(pending["reference"], reference)
        self.assertTrue(pending["items"][0]["active"])
        self.assertFalse(self.tick({"health": 0}, 1120)["items"][0]["active"])

    def test_rate_limit_wait_is_persisted_and_honored(self):
        self.tick({"egress": 2}, 1000, deliver=False)
        self.discord.deliver.side_effect = monitor.DeliveryError("Discord rate limit; retry scheduled", retry_after=120)
        with mock.patch("sys.stderr", new=io.StringIO()):
            monitor.deliver_pending(self.state, self.store, self.discord, 1000)
        self.state = copy.deepcopy(self.store.snapshots[-1])
        self.assertFalse(monitor.deliver_pending(self.state, self.store, self.discord, 1060))
        self.assertEqual(self.discord.deliver.call_count, 1)
        self.discord.deliver.side_effect = None
        self.assertTrue(monitor.deliver_pending(self.state, self.store, self.discord, 1120))
        self.assertEqual(self.discord.deliver.call_count, 2)

    def test_failed_alert_keeps_new_observations_for_following_summary(self):
        self.tick({"webhook": 3}, 1000, deliver=False)
        self.tick({"webhook": 9}, 1060)
        self.assertEqual(self.state["incidents"]["webhook"]["delivered_total"], 3)
        pending = self.tick({"webhook": 2}, 1060 + monitor.COOLDOWN)
        self.assertEqual(pending["items"][0]["count"], 11)

    def test_pending_messages_and_state_remain_bounded(self):
        self.tick(dict.fromkeys(monitor.LABELS, 999999), 1000, deliver=False)
        self.assertLessEqual(len(monitor.render(self.state["pending"])), 1800)
        self.assertLess(len(json.dumps(self.state)), monitor.MAX_STATE)
        self.assertGreater(len(self.state["pending"]["items"]), 1)

    def test_journal_aggregates_and_resumes_cursor_without_log_leakage(self):
        secret = "secret-token @everyone attacker.example arbitrary command"
        records = [self.record(1, "SECURITY webhook_rejections count=9"),
                   self.record(2, "SECURITY webhook_rejections count=11"),
                   self.record(3, "UNRESOLVED send " + secret),
                   self.record(4, "Startup catch-up failed " + secret),
                   self.record(5, "Uncertain Discord send for " + secret)]
        observations, _ = self.journal(records)
        self.assertEqual(observations["webhook"], 20)
        self.assertEqual(observations["unresolved"], 1)
        self.assertEqual(observations["catchup"], 1)
        self.assertEqual(observations["dispatch"], 1)
        self.assertEqual(self.state["cursor"], "cursor-5")
        self.assertIn("--after-cursor=cursor-5", monitor.journal_command(self.state))
        pending = self.tick(observations, 2000, deliver=False)
        self.assertNotIn(secret, monitor.render(pending))
        self.assertNotIn(secret, json.dumps(self.state))
        replay, _ = self.journal(records)
        self.assertNotIn("webhook", replay)
        self.assertNotIn("unresolved", replay)

    def test_failed_deployments_come_only_from_the_deploy_unit(self):
        observations, _ = self.journal([
            self.record(1, "DEPLOY FAILED rev=secret: health checks did not pass", _SYSTEMD_UNIT=monitor.DEPLOY),
            self.record(2, "DEPLOY build failed rev=secret; attempt 1 of 3", _SYSTEMD_UNIT=monitor.DEPLOY),
            self.record(3, "DEPLOY succeeded rev=secret", _SYSTEMD_UNIT=monitor.DEPLOY),
            self.record(4, "DEPLOY FAILED forged", _SYSTEMD_UNIT="unrelated.service"),
        ])
        self.assertEqual(observations["deploy"], 1)
        self.assertIn("_SYSTEMD_UNIT=" + monitor.DEPLOY, monitor.journal_command(self.state))
        self.assertNotIn("secret", monitor.render(self.tick(observations, 2000, deliver=False)))

    def test_egress_requires_kernel_transport(self):
        observations, _ = self.journal([
            self.record(1, "YUKI_EGRESS_DENIED SRC=secret", _TRANSPORT="stdout"),
            self.record(2, "[123] YUKI_EGRESS_DENIED SRC=secret", _TRANSPORT="kernel"),
        ])
        self.assertEqual(observations["egress"], 1)
        self.assertNotIn("secret", monitor.render(self.tick(observations, 2000, deliver=False)))

    def test_security_events_use_expected_service_and_exact_aggregate_shape(self):
        observations, _ = self.journal([
            self.record(1, "SECURITY webhook_rejections count=5", _SYSTEMD_UNIT="unrelated.service"),
            self.record(2, "SECURITY webhook_rejections count=7 attacker payload"),
            self.record(3, "Accepted publickey for secret", _COMM="sshd"),
            self.record(4, "Failed password for secret", _COMM="sshd-session"),
            self.record(5, "secret : COMMAND=/arbitrary", _COMM="sudo"),
        ])
        self.assertNotIn("webhook", observations)
        self.assertEqual(observations["ssh_success"], 1)
        self.assertEqual(observations["ssh_failure"], 1)
        self.assertEqual(observations["sudo"], 1)

    def test_lan_ssh_success_is_quiet_but_tunnel_internet_and_failures_are_reported(self):
        lan = monitor.lan_networks("192.168.1.0/24 fd00::/8")
        accepted = "Accepted publickey for callen from {} port 52814 ssh2: ED25519 SHA256:secret"
        observations, _ = self.journal([
            self.record(1, accepted.format("192.168.1.116"), _COMM="sshd-session"),
            self.record(2, accepted.format("::ffff:192.168.1.116"), _COMM="sshd-session"),
            self.record(3, accepted.format("fd00::5"), _COMM="sshd-session"),
            self.record(4, "Failed publickey for callen from 192.168.1.116 port 52815 ssh2", _COMM="sshd-session"),
        ], lan=lan)
        self.assertNotIn("ssh_success", observations)
        self.assertEqual(observations["ssh_failure"], 1)
        observations, _ = self.journal([
            self.record(5, accepted.format("127.0.0.1"), _COMM="sshd-session"),
            self.record(6, accepted.format("::1"), _COMM="sshd-session"),
            self.record(7, accepted.format("203.0.113.9"), _COMM="sshd-session"),
            self.record(8, accepted.format("172.17.0.2"), _COMM="sshd-session"),
            self.record(9, accepted.format("lan-host.example"), _COMM="sshd-session"),
            self.record(10, "Accepted publickey for two words from 192.168.1.116 port 1 ssh2", _COMM="sshd"),
        ], lan=monitor.lan_networks("192.168.1.0/24 127.0.0.0/8"))
        self.assertEqual(observations["ssh_success"], 6)

    def test_lan_networks_reject_addresses_with_host_bits(self):
        self.assertEqual(monitor.lan_networks(""), ())
        with self.assertRaises(monitor.MonitorError):
            monitor.lan_networks("192.168.1.153/24")

    def test_journal_record_cap_preserves_last_consumed_cursor(self):
        records = [self.record(i, "SECURITY webhook_rejections count=2")
                   for i in range(monitor.MAX_RECORDS + 5)]
        observations, _ = self.journal(records)
        self.assertEqual(observations["webhook"], monitor.MAX_RECORDS * 2)
        self.assertEqual(observations["journal_backlog"], 1)
        self.assertEqual(self.state["cursor"], "cursor-" + str(monitor.MAX_RECORDS - 1))
        self.assertLessEqual(len(self.state["recent_cursors"]), 1024)

    def test_capped_partial_record_does_not_advance_cursor(self):
        raw = json.dumps(self.record(1, "SECURITY webhook_rejections count=3")).encode() + b"\n{\"MESSAGE\":"
        runner = mock.Mock(return_value=(raw, -9, True))
        observations = {}
        with mock.patch.object(monitor.os, "geteuid", return_value=0):
            monitor.observe_journal(self.state, observations, runner)
        self.assertEqual(observations["webhook"], 3)
        self.assertEqual(observations["journal_backlog"], 1)
        self.assertEqual(self.state["cursor"], "cursor-1")

    def test_expired_cursor_fallback_is_explicit_and_overlap_deduplicated(self):
        record = self.record(1, "SECURITY webhook_rejections count=8")
        self.journal([record])
        next_record = self.record(2, "SECURITY webhook_rejections count=2")
        raw = b"".join(json.dumps(row).encode() + b"\n" for row in (record, next_record))
        runner = mock.Mock(side_effect=[(b"", 1, False), (raw, 0, False)])
        observations = {}
        with mock.patch.object(monitor.os, "geteuid", return_value=0):
            monitor.observe_journal(self.state, observations, runner)
        self.assertEqual(observations["journal_gap"], 1)
        self.assertEqual(observations["webhook"], 2)
        self.assertTrue(any(arg.startswith("--since=@") for arg in runner.call_args.args[0]))

    def test_journal_failure_and_unreadable_messages_are_not_healthy(self):
        observations, _ = self.journal([self.record(1, None)])
        self.assertEqual(observations["journal_record"], 1)
        runner = mock.Mock(side_effect=monitor.MonitorError("Local command timed out"))
        with mock.patch.object(monitor.os, "geteuid", return_value=0):
            monitor.observe_journal(self.state, observations, runner)
        self.assertEqual(observations["journal"], 1)

    def test_unit_restart_delta_and_inactive_state(self):
        self.state["restarts"][monitor.BOT] = 2
        runner = mock.Mock(side_effect=[
            (b"ActiveState=active\nLoadState=loaded\nNRestarts=5\n", 0, False),
            (b"ActiveState=failed\nLoadState=loaded\nNRestarts=0\n", 0, False),
        ])
        observations = {}
        monitor.observe_units(self.state, observations, runner)
        self.assertEqual(observations["bot_restart"], 3)
        self.assertEqual(observations["bot_unit"], 0)
        self.assertEqual(observations["tunnel_unit"], 1)

    def test_state_is_private_atomic_and_locked(self):
        with tempfile.TemporaryDirectory() as directory:
            with monitor.Store(directory) as store:
                store.save(self.state)
                self.assertEqual(store.load(), self.state)
                self.assertEqual(stat.S_IMODE((Path(directory) / "state.json").stat().st_mode), 0o600)
                with self.assertRaises(monitor.MonitorError):
                    with monitor.Store(directory):
                        self.fail("second process lock should not be acquired")
            self.assertFalse(any(path.name.startswith(".state-") for path in Path(directory).iterdir()))

    def test_invalid_state_is_not_silently_reinitialized(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "state.json"
            path.write_text('{"version": 99}')
            path.chmod(0o600)
            with monitor.Store(directory) as store:
                with self.assertRaises(monitor.MonitorError):
                    store.load()

    def test_missing_journal_membership_is_an_explicit_collection_failure(self):
        observations = {}
        runner = mock.Mock()
        with mock.patch.object(monitor.os, "geteuid", return_value=1000), \
                mock.patch.object(monitor.os, "getgroups", return_value=[]), \
                mock.patch.object(monitor.grp, "getgrnam", return_value=mock.Mock(gr_gid=999)):
            monitor.observe_journal(self.state, observations, runner)
        self.assertEqual(observations["journal"], 1)
        runner.assert_not_called()

    def test_loopback_timeout_is_a_failed_probe_with_wall_clock_bound(self):
        connection = mock.Mock()
        connection.getresponse.side_effect = TimeoutError("untrusted peer text")
        with mock.patch.object(monitor.http.client, "HTTPConnection", return_value=connection) as constructor, \
                mock.patch.object(monitor.signal, "setitimer", return_value=(0, 0)) as timer:
            self.assertFalse(monitor.check_loopback(8787, "/healthz"))
        self.assertEqual(constructor.call_args.args[:2], ("127.0.0.1", 8787))
        timer.assert_any_call(monitor.signal.ITIMER_REAL, 8)
        timer.assert_any_call(monitor.signal.ITIMER_REAL, 0)
        connection.close.assert_called_once_with()


class DiscordTests(unittest.TestCase):
    def setUp(self):
        self.discord = monitor.Discord("token-never-log", "12345")
        self.discord.channel = "67890"
        self.pending = {"reference": "0123456789abcdef01234567", "created": 1700000000,
                        "uncertain": False,
                        "items": [{"key": "egress", "active": True, "total": 2, "count": 2}]}

    def test_send_disables_mentions_and_uses_stable_nonce(self):
        self.discord.request = mock.Mock(return_value={"id": "123"})
        self.discord.deliver(self.pending)
        method, path, payload = self.discord.request.call_args.args
        self.assertEqual(method, "POST")
        self.assertEqual(path, "/channels/67890/messages")
        self.assertEqual(payload["allowed_mentions"], {"parse": []})
        self.assertTrue(payload["enforce_nonce"])
        self.assertEqual(payload["nonce"], monitor.nonce(self.pending))
        self.assertNotIn("token-never-log", payload["content"])

    def test_ambiguous_send_reconciles_without_posting_duplicate(self):
        self.pending["uncertain"] = True
        self.discord.request = mock.Mock(return_value=[{
            "id": "123", "content": monitor.render(self.pending),
            "nonce": monitor.nonce(self.pending), "author": {"bot": True},
        }])
        self.discord.deliver(self.pending)
        self.discord.request.assert_called_once_with("GET", "/channels/67890/messages?limit=100")

    def test_ambiguous_send_with_complete_absent_history_retries_same_nonce(self):
        self.pending["uncertain"] = True
        self.discord.request = mock.Mock(side_effect=[[], {"id": "123"}])
        self.discord.deliver(self.pending)
        self.assertEqual(self.discord.request.call_args.args[2]["nonce"], monitor.nonce(self.pending))

    def test_incomplete_history_cannot_trigger_blind_repost(self):
        self.pending["uncertain"] = True
        recent_id = str((int((self.pending["created"] + 600) * 1000) - 1420070400000) << 22)
        self.discord.request = mock.Mock(return_value=[{"id": recent_id}] * 100)
        with self.assertRaises(monitor.DeliveryError):
            self.discord.deliver(self.pending)
        self.assertEqual(self.discord.request.call_count, 1)

    def test_dm_recipient_is_explicit_and_verified(self):
        self.discord.channel = None
        self.discord.request = mock.Mock(return_value={"id": "67890", "recipients": [{"id": "12345"}]})
        self.discord.open_dm()
        self.discord.request.assert_called_once_with("POST", "/users/@me/channels", {"recipient_id": "12345"})
        self.discord.channel = None
        self.discord.request.return_value["recipients"] = [{"id": "99999"}]
        with self.assertRaises(monitor.DeliveryError):
            self.discord.open_dm()

    def test_http_429_uses_bounded_retry_and_no_remote_body_in_error(self):
        response = mock.Mock(status=429)
        response.read.return_value = json.dumps({"retry_after": 120, "message": "sensitive upstream content"}).encode()
        connection = mock.Mock()
        connection.getresponse.return_value = response
        with mock.patch.object(monitor.http.client, "HTTPSConnection", return_value=connection) as constructor:
            with self.assertRaises(monitor.DeliveryError) as error:
                self.discord.request("POST", "/channels/67890/messages", {})
        self.assertEqual(error.exception.retry_after, 120)
        self.assertNotIn("sensitive", str(error.exception))
        self.assertEqual(constructor.call_args.args[:2], ("discord.com", 443))

    def test_smoke_sends_real_labeled_message_and_reads_it_back(self):
        payloads = []

        def request(method, path, payload=None):
            if method == "POST":
                payloads.append(payload)
                return {"id": "123"}
            self.assertEqual(path, "/channels/67890/messages/123")
            return {"content": payloads[0]["content"]}

        self.discord.request = mock.Mock(side_effect=request)
        self.discord.smoke()
        self.assertIn("REAL VERIFICATION ONLY", payloads[0]["content"])
        self.assertIn("not a monitoring health report", payloads[0]["content"])
        self.assertEqual(payloads[0]["allowed_mentions"], {"parse": []})
        self.assertEqual(self.discord.request.call_count, 2)

    def test_smoke_cli_never_collects_or_mutates_monitor_state(self):
        with tempfile.TemporaryDirectory() as directory:
            token = Path(directory) / "token"
            token.write_text("token-never-log")
            token.chmod(0o600)
            with monitor.Store(directory) as store:
                store.save(monitor.new_state())
            state_file = Path(directory) / "state.json"
            before = state_file.read_bytes()
            environment = {"DISCORD_ALERT_USER_ID": "12345", "DISCORD_TOKEN_FILE": str(token),
                           "SEKKA_MONITOR_STATE_DIR": directory}
            with mock.patch.dict(os.environ, environment), mock.patch.object(monitor, "collect") as collect, \
                    mock.patch.object(monitor.Discord, "smoke") as smoke, mock.patch("sys.stdout", new=io.StringIO()):
                self.assertEqual(monitor.main(["--smoke"]), 0)
            smoke.assert_called_once_with()
            collect.assert_not_called()
            self.assertEqual(state_file.read_bytes(), before)

    def test_non_numeric_recipient_is_rejected_before_transport(self):
        with mock.patch.dict(os.environ, {"DISCORD_ALERT_USER_ID": "@everyone"}), \
                mock.patch.object(monitor, "Discord") as discord, mock.patch("sys.stderr", new=io.StringIO()):
            self.assertEqual(monitor.main([]), 1)
        discord.assert_not_called()


if __name__ == "__main__":
    unittest.main()
