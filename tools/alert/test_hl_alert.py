"""Tests of the Telegram alert script (U-11). Run: python3 -m unittest discover -s tools/alert"""

import json
import os
import stat
import tempfile
import unittest
from pathlib import Path

import hl_alert as a


def status(holds=(), fatal=None, reducing=None, mismatches=(), snapshot_error=None, started_at=1, venue="sim"):
    return {
        "fatal": fatal,
        "node": {"venue": venue, "started_at": started_at, "pid": 7},
        "core": {
            "holds": list(holds),
            "mismatches_unacked": list(mismatches),
            "margin": None if reducing is None else {"reducing": reducing},
        },
        "journal": {"last_snapshot_error": snapshot_error},
    }


class Conditions(unittest.TestCase):
    def test_a_quiet_node_has_no_condition(self):
        self.assertEqual(a.conditions(status()), {})

    def test_each_hold_is_its_own_condition(self):
        c = a.conditions(status(holds=["link_down", "in_doubt"]))
        self.assertEqual(sorted(c), ["hold:in_doubt", "hold:link_down"])
        self.assertIn("거래소 연결", c["hold:link_down"])

    def test_a_hold_on_one_instrument_names_the_instrument(self):
        c = a.conditions(status(holds=[{"venue_setting": "BTC"}]))
        self.assertEqual(list(c), ["hold:venue_setting:BTC"])
        self.assertIn("BTC", c["hold:venue_setting:BTC"])

    def test_an_unknown_hold_is_shown_by_its_raw_name(self):
        c = a.conditions(status(holds=["brand_new_reason"]))
        self.assertIn("brand_new_reason", c["hold:brand_new_reason"])

    def test_reducing_shows_the_rule_and_what_it_saw(self):
        r = {"rule": "daily_loss", "detail": "value 94 < base 100 - 5%", "since": 5}
        c = a.conditions(status(holds=["reducing"], reducing=r))
        self.assertIn("daily_loss", c["hold:reducing"])
        self.assertIn("value 94", c["hold:reducing"])

    def test_a_stale_margin_report_is_a_condition_though_it_is_not_a_hold(self):
        st = status()
        st["core"]["margin"] = {"fresh": False, "age_us": 45_000_000, "reducing": None}
        c = a.conditions(st)
        self.assertEqual(list(c), ["margin_stale"])
        self.assertIn("45초", c["margin_stale"])

    def test_no_margin_report_yet_is_stale_too(self):
        st = status()
        st["core"]["margin"] = {"fresh": False, "age_us": None, "reducing": None}
        self.assertIn("아직", a.conditions(st)["margin_stale"])

    def test_a_fresh_report_or_no_margin_policy_is_quiet(self):
        st = status()
        st["core"]["margin"] = {"fresh": True, "age_us": 4_000_000, "reducing": None}
        self.assertEqual(a.conditions(st), {})
        self.assertEqual(a.conditions(status()), {})

    def test_a_strategy_halt_is_a_condition_per_strategy(self):
        st = status()
        st["core"]["operator_halt_strategy"] = {"demo": {"note": "x", "at": 1}}
        self.assertEqual(list(a.conditions(st)), ["hold:operator_strategy:demo"])
        self.assertIn("demo", a.conditions(st)["hold:operator_strategy:demo"])

    def test_fatal_mismatch_and_snapshot_error_are_conditions(self):
        m = {"id": 3, "kind": "overfill", "subject": "order 9", "detail": "cum 2 > qty 1"}
        c = a.conditions(status(fatal="journal write failed", mismatches=[m], snapshot_error="ENOSPC"))
        self.assertEqual(sorted(c), ["fatal", "mismatch:3", "snapshot_error"])
        self.assertIn("journal write failed", c["fatal"])
        self.assertIn("overfill", c["mismatch:3"])
        self.assertIn("ENOSPC", c["snapshot_error"])


class Watching(unittest.TestCase):
    def setUp(self):
        self.w = a.Watcher(down_after=2)

    def test_the_first_poll_reports_start_and_current_conditions(self):
        msgs = self.w.step(status(holds=["link_down"]))
        self.assertEqual(len(msgs), 1)
        self.assertIn("감시 시작", msgs[0])
        self.assertIn("거래소 연결", msgs[0])

    def test_no_change_sends_nothing(self):
        self.w.step(status())
        self.assertEqual(self.w.step(status()), [])

    def test_a_new_condition_and_its_clearing_are_both_sent(self):
        self.w.step(status())
        up = self.w.step(status(holds=["operator_account"]))
        self.assertEqual(len(up), 1)
        self.assertIn("발생", up[0])
        down = self.w.step(status())
        self.assertEqual(len(down), 1)
        self.assertIn("해제", down[0])

    def test_one_failed_poll_is_not_an_alert_two_are(self):
        self.w.step(status())
        self.assertEqual(self.w.step(None), [])
        msgs = self.w.step(None)
        self.assertEqual(len(msgs), 1)
        self.assertIn("노드 응답 없음", msgs[0])
        self.assertIn("해제", self.w.step(status())[0])

    def test_a_node_that_was_never_reached_is_reported_once_at_start(self):
        msgs = self.w.step(None)
        self.assertEqual(len(msgs), 1)
        self.assertIn("감시 시작", msgs[0])
        self.assertIn("연결되지 않음", msgs[0])
        self.assertEqual(self.w.step(None), [])

    def test_a_restart_of_the_node_is_sent_as_an_alert(self):
        self.w.step(status(started_at=1))
        msgs = self.w.step(status(started_at=2))
        self.assertEqual(len(msgs), 1)
        self.assertIn("🔴", msgs[0])
        self.assertIn("다시 시작", msgs[0])
        self.assertIn("종료 코드", msgs[0])

    def test_a_status_that_cannot_be_read_does_not_lose_the_restart(self):
        self.w.step(status(started_at=1))
        bad = status(started_at=2)
        bad["core"]["holds"] = 5  # not a list
        with self.assertRaises(Exception):
            self.w.step(bad)
        self.assertIn("다시 시작", self.w.step(status(started_at=2))[0])

    def test_every_message_names_the_venue(self):
        msgs = self.w.step(status(venue="hyperliquid:testnet"))
        self.assertIn("hyperliquid:testnet", msgs[0])


class Delivery(unittest.TestCase):
    def test_a_failed_send_is_retried_on_the_next_flush(self):
        sent, ok = [], [False]

        def send(text):
            if not ok[0]:
                raise OSError("down")
            sent.append(text)

        out = a.Outbox(send, limit=10)
        out.add(["m1"])
        out.flush()
        self.assertEqual(sent, [])
        ok[0] = True
        out.flush()
        self.assertEqual(sent, ["m1"])

    def test_a_full_outbox_drops_the_oldest_and_says_so(self):
        sent = []
        out = a.Outbox(sent.append, limit=2)
        out.add(["m1", "m2", "m3"])
        out.flush()
        self.assertEqual(sent[:2], ["m2", "m3"])
        self.assertIn("1개를 버림", sent[2])


    def test_a_message_telegram_refuses_is_dropped_and_the_rest_go(self):
        sent = []

        def send(text):
            if text == "bad":
                raise a.Rejected("400")
            sent.append(text)

        out = a.Outbox(send, limit=10)
        out.add(["bad", "m2"])
        out.flush()
        self.assertEqual(sent[0], "m2")
        self.assertIn("1개를 버림", sent[1])

    def test_a_long_message_is_split_within_the_telegram_limit(self):
        long = "\n".join(f"🔴 발생: 불일치 {i} " + "x" * 30 for i in range(300))
        parts = a.split_message(long)
        self.assertGreater(len(parts), 1)
        self.assertTrue(all(len(p.encode("utf-16-le")) // 2 <= 4096 for p in parts))
        self.assertEqual("\n".join(parts), long)


class Config(unittest.TestCase):
    def write(self, mode, body=None):
        d = tempfile.TemporaryDirectory()
        self.addCleanup(d.cleanup)
        p = Path(d.name) / "telegram.json"
        p.write_text(json.dumps(body or {"token": "123456:abc_DEF-9", "chat_id": "42"}))
        os.chmod(p, mode)
        return p

    def test_a_private_file_is_read(self):
        self.assertEqual(a.read_config(self.write(0o600)), ("123456:abc_DEF-9", "42"))
        self.assertEqual(a.read_config(self.write(0o400))[1], "42")

    def test_a_file_others_can_read_is_refused(self):
        with self.assertRaises(a.ConfigError):
            a.read_config(self.write(0o644))

    def test_a_file_without_chat_id_is_refused(self):
        with self.assertRaises(a.ConfigError):
            a.read_config(self.write(0o600, {"token": "123456:abc"}))

    def test_a_malformed_token_is_refused_without_showing_it(self):
        with self.assertRaises(a.ConfigError) as e:
            a.read_config(self.write(0o600, {"token": "123456:abc def", "chat_id": "42"}))
        self.assertNotIn("abc", str(e.exception))

    def test_a_file_that_is_not_an_object_is_refused(self):
        with self.assertRaises(a.ConfigError):
            a.read_config(self.write(0o600, ["x"]))


if __name__ == "__main__":
    unittest.main()
