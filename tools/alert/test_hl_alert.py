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

    def test_a_restart_of_the_node_is_sent(self):
        self.w.step(status(started_at=1))
        msgs = self.w.step(status(started_at=2))
        self.assertEqual(len(msgs), 1)
        self.assertIn("다시 시작", msgs[0])

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
        self.assertEqual(len(sent), 3)
        self.assertIn("1", sent[0])
        self.assertIn("버림", sent[0])
        self.assertEqual(sent[1:], ["m2", "m3"])


class Config(unittest.TestCase):
    def write(self, mode, body=None):
        d = tempfile.mkdtemp()
        p = Path(d) / "telegram.json"
        p.write_text(json.dumps(body or {"token": "1:x", "chat_id": "42"}))
        os.chmod(p, mode)
        return p

    def test_a_private_file_is_read(self):
        self.assertEqual(a.read_config(self.write(0o600)), ("1:x", "42"))

    def test_a_file_others_can_read_is_refused(self):
        with self.assertRaises(a.ConfigError):
            a.read_config(self.write(0o644))

    def test_a_file_without_chat_id_is_refused(self):
        with self.assertRaises(a.ConfigError):
            a.read_config(self.write(0o600, {"token": "1:x"}))


if __name__ == "__main__":
    unittest.main()
