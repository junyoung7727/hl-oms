#!/usr/bin/env python3
"""Telegram alerts for a running oms-node (user decision U-11).

Reads the node's read-only `/api/status` every 30 seconds and sends a
Telegram message when a condition appears or clears: a hold reason
(`core.holds`, REDUCING with its rule), a margin report that is not fresh
(`core.margin.fresh`), the fatal state, an unacknowledged mismatch, a snapshot error, a restart of the node, or a node that does not
answer. It is a separate process: it changes nothing in the node, and the
node does not know it exists.

What it does not do: it does not see the venue (it trusts the node's
status), it does not alert on a condition that appears and clears between
two polls, and nobody is told when this script itself stops.

Config: `~/.config/hl-oms/telegram.json` (`{"token", "chat_id"}`, mode 0600),
outside the repository. Standard library only.

    python3 tools/alert/hl_alert.py test     # one test message
    python3 tools/alert/hl_alert.py once     # print the current conditions
    python3 tools/alert/hl_alert.py run      # poll and alert until stopped
"""

from __future__ import annotations

import argparse
import http.client
import json
import re
import socket
import ssl
import stat
import sys
import time
import urllib.request
from datetime import datetime, timedelta, timezone
from pathlib import Path

CONFIG = Path.home() / ".config" / "hl-oms" / "telegram.json"
STATUS_URL = "http://127.0.0.1:8188/api/status"
TG_HOST = "api.telegram.org"
# The Linux PC's network blocks the primary address of api.telegram.org.
# These Telegram fronts answer; TLS still checks the certificate for TG_HOST.
TG_FALLBACK_IPS = ("149.154.167.220",)
KST = timezone(timedelta(hours=9))
# Telegram's limit is 4096 UTF-16 units per message; this leaves room.
MSG_LIMIT = 3500
TOKEN_RE = re.compile(r"[0-9]{1,20}:[A-Za-z0-9_-]{1,100}")

HOLDS = {
    "no_config": "설정이 아직 활성화되지 않음. 모든 신규 주문 거절",
    "link_down": "거래소 연결 끊김. 모든 신규 주문 거절",
    "resync": "재연결 뒤 거래소 상태를 아직 다시 확인하지 않음. 일반 주문 거절",
    "in_doubt": "결과를 모르는 주문이 있음. 일반 주문 거절",
    "mismatch": "확인하지 않은 불일치가 있음. 일반 주문 거절",
    "operator_account": "운영자가 계좌를 중지함. 모든 신규 주문 거절",
    "storage_low": "저널 저장 공간 부족. 모든 신규 주문 거절",
    "reducing": "감소 전용(REDUCING). 일반 주문 거절, 운영자만 해제",
    "account_mode": "거래소 계정 모드를 모르거나 허용 목록에 없음. 일반 주문 거절",
}


def conditions(st: dict) -> dict[str, str]:
    """The alertable conditions in one status document: key -> text."""
    out: dict[str, str] = {}
    if st.get("fatal"):
        out["fatal"] = f"치명 상태: {st['fatal']}"
    core = st.get("core") or {}
    margin = core.get("margin") or {}
    for h in core.get("holds") or []:
        if isinstance(h, dict) and "venue_setting" in h:
            inst = h["venue_setting"]
            out[f"hold:venue_setting:{inst}"] = f"{inst}의 거래소 증거금 설정을 모르거나 정책과 다름. {inst} 일반 주문 거절"
            continue
        name = h if isinstance(h, str) else json.dumps(h, ensure_ascii=False)
        text = HOLDS.get(name, f"차단 사유 {name}")
        if name == "reducing" and margin.get("reducing"):
            r = margin["reducing"]
            text += f". 규칙 {r.get('rule')}: {r.get('detail')}"
        out[f"hold:{name}"] = text
    # Not a hold: without a fresh report plain orders are refused at the
    # order check, and resting ones are cancelled after stale_cancel_us.
    if margin.get("fresh") is False:
        age = margin.get("age_us")
        seen = "아직 받은 증거금 보고가 없음" if age is None else f"마지막 증거금 보고가 {age // 1_000_000}초 전"
        out["margin_stale"] = f"{seen}. 30초가 넘으면 일반 주문 거절, 60초가 넘으면 걸린 일반 주문 취소"
    # A strategy halt is not in the account-wide `core.holds`.
    for s in core.get("operator_halt_strategy") or {}:
        out[f"hold:operator_strategy:{s}"] = f"운영자가 전략 {s}를 중지함. 그 전략의 신규 주문 거절"
    for m in core.get("mismatches_unacked") or []:
        out[f"mismatch:{m.get('id')}"] = f"불일치 {m.get('kind')} ({m.get('subject')}): {m.get('detail')}"
    err = (st.get("journal") or {}).get("last_snapshot_error")
    if err:
        out["snapshot_error"] = f"스냅샷 실패: {err}"
    return out


class Watcher:
    """Turns successive polls into messages. `None` is a failed poll."""

    def __init__(self, down_after: int = 2, interval_s: int = 30):
        self.down_after = down_after
        self.interval_s = interval_s
        self.active: dict[str, str] | None = None  # None before the first poll
        self.venue = "?"
        self.started_at = None
        self.failures = 0
        self.seen = False

    def step(self, st: dict | None) -> list[str]:
        events = []
        if st is None:
            self.failures += 1
            if self.active is None:
                self.active = {}
                return [self._msg(f"감시 시작({self.interval_s}초마다 확인). 노드에 연결되지 않음")]
            if not self.seen or self.failures < self.down_after:
                return []
            # A failed poll says nothing about the other conditions: keep them.
            cur = {**self.active, "node_down": "노드 응답 없음(연속 조회 실패)"}
        else:
            cur = conditions(st)  # first: if it fails, nothing below has changed
            self.failures = 0
            self.seen = True
            node = st.get("node") or {}
            self.venue = node.get("venue") or "?"
            if self.started_at is not None and node.get("started_at") != self.started_at:
                # A fatal stop exits at once, so `fatal` is rarely seen by a poll;
                # under systemd the node may be back before the next poll.
                events.append("🔴 노드가 다시 시작됨. 직전 종료 코드와 로그의 마지막 줄을 확인(2 = 치명 상태)")
            self.started_at = node.get("started_at")
        if self.active is None:
            self.active = cur
            lines = [f"🔴 {t}" for t in cur.values()] or ["이상 없음"]
            return [self._msg(f"감시 시작({self.interval_s}초마다 확인). 현재 상태:\n" + "\n".join(lines))]
        prev, self.active = self.active, cur
        lines = events + [f"🔴 발생: {cur[k]}" for k in cur if k not in prev] + [f"🟢 해제: {prev[k]}" for k in prev if k not in cur]
        return [self._msg("\n".join(lines))] if lines else []

    def _msg(self, body: str) -> str:
        at = datetime.now(KST).strftime("%m-%d %H:%M:%S KST")
        return f"[HL-OMS {self.venue}] {at}\n{body}"


def split_message(text: str, limit: int = MSG_LIMIT) -> list[str]:
    """Cuts at line ends so that each part is within `limit` UTF-16 units."""
    units = lambda s: len(s.encode("utf-16-le")) // 2
    parts, cur = [], ""
    for line in text.split("\n"):
        while units(line) > limit:  # one overlong line: cut it by characters
            n = limit
            while units(line[:n]) > limit:
                n -= 1
            if cur:
                parts.append(cur)
                cur = ""
            parts.append(line[:n])
            line = line[n:]
        cand = line if not cur else cur + "\n" + line
        if cur and units(cand) > limit:
            parts.append(cur)
            cur = line
        else:
            cur = cand
    parts.append(cur)
    return parts


class Rejected(Exception):
    """Telegram refused this message for good (a 4xx other than 429)."""


class Outbox:
    """Messages not yet delivered. A failed send is retried on the next flush;
    a message Telegram refuses is dropped and counted, so it cannot block the rest."""

    def __init__(self, send, limit: int = 50):
        self.send = send
        self.limit = limit
        self.queue: list[str] = []
        self.dropped = 0

    def add(self, msgs: list[str]) -> None:
        for m in msgs:
            self.queue.extend(split_message(m))
        over = len(self.queue) - self.limit
        if over > 0:
            del self.queue[:over]
            self.dropped += over

    def flush(self) -> None:
        try:
            while self.queue:
                try:
                    self.send(self.queue[0])
                    print("sent:", self.queue.pop(0).replace("\n", " | "), flush=True)
                except Rejected as e:
                    print(f"telegram refused a message: {e}", file=sys.stderr, flush=True)
                    self.queue.pop(0)
                    self.dropped += 1
            if self.dropped:
                self.send(f"[HL-OMS] 전송 실패로 알림 {self.dropped}개를 버림")
                self.dropped = 0
        except Exception as e:  # keep the rest for the next flush; never print the token
            print(f"telegram send failed: {type(e).__name__}", file=sys.stderr, flush=True)


class ConfigError(Exception):
    pass


def read_config(path: Path = CONFIG) -> tuple[str, str]:
    try:
        st = path.lstat()
        if not stat.S_ISREG(st.st_mode) or stat.S_IMODE(st.st_mode) & 0o077:
            raise ConfigError(f"{path}: must be a regular file only its owner can read (0600)")
        raw = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError) as e:
        raise ConfigError(f"{path}: {type(e).__name__}") from None
    if not isinstance(raw, dict):
        raise ConfigError(f"{path}: must hold a JSON object")
    token, chat = raw.get("token"), raw.get("chat_id")
    if chat in (None, ""):
        raise ConfigError(f"{path}: needs token and chat_id")
    if not isinstance(token, str) or not TOKEN_RE.fullmatch(token):
        raise ConfigError(f"{path}: token is not of the form <digits>:<letters>")  # never print it
    return token, str(chat)


class _PinnedHTTPS(http.client.HTTPSConnection):
    """HTTPS to a given IP with SNI and certificate check for TG_HOST."""

    def __init__(self, ip: str, timeout: float):
        super().__init__(TG_HOST, 443, timeout=timeout, context=ssl.create_default_context())
        self._ip = ip

    def connect(self):
        sock = socket.create_connection((self._ip, 443), self.timeout)
        try:
            self.sock = self._context.wrap_socket(sock, server_hostname=TG_HOST)
        except BaseException:
            sock.close()
            raise


def telegram_sender(token: str, chat_id: str, timeout: float = 10.0):
    good: list[str | None] = [None]

    def send(text: str) -> None:
        body = json.dumps({"chat_id": chat_id, "text": text, "disable_web_page_preview": True})
        path = f"/bot{token}/sendMessage"
        last = None
        # The route that worked last goes first: a blocked one costs a timeout.
        routes = sorted((None, *TG_FALLBACK_IPS), key=lambda r: r != good[0])
        for ip in routes:
            try:
                if ip is None:
                    conn = http.client.HTTPSConnection(TG_HOST, timeout=timeout, context=ssl.create_default_context())
                else:
                    conn = _PinnedHTTPS(ip, timeout)
                conn.request("POST", path, body, {"Content-Type": "application/json"})
                resp = conn.getresponse()
                data = json.loads(resp.read() or b"{}")
                conn.close()
                if not data.get("ok"):
                    msg = f"telegram answered {resp.status}: {data.get('description')}"
                    raise Rejected(msg) if 400 <= resp.status < 500 and resp.status != 429 else RuntimeError(msg)
                good[0] = ip
                return
            except (OSError, http.client.HTTPException, ValueError) as e:
                # A read timeout after delivery can send the message twice: a
                # duplicate is better than a lost alert.
                last = e
        raise last or OSError("no route to telegram")

    return send


# The node is on loopback: a proxy from the environment must not get the polls.
_NO_PROXY = urllib.request.build_opener(urllib.request.ProxyHandler({}))


def fetch_status(url: str, timeout: float = 5.0) -> dict | None:
    try:
        with _NO_PROXY.open(url, timeout=timeout) as r:
            st = json.loads(r.read())
        return st if isinstance(st, dict) else None
    except (OSError, ValueError, http.client.HTTPException):
        return None


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("cmd", choices=["run", "once", "test"])
    p.add_argument("--url", default=STATUS_URL)
    p.add_argument("--interval", type=int, default=30, help="seconds between polls")
    p.add_argument("--config", type=Path, default=CONFIG)
    args = p.parse_args(argv)
    if args.interval < 5:
        p.error("--interval must be at least 5 seconds")

    if args.cmd == "once":
        st = fetch_status(args.url)
        print(json.dumps(None if st is None else conditions(st), ensure_ascii=False, indent=1))
        return 0 if st is not None else 2
    try:
        send = telegram_sender(*read_config(args.config))
    except ConfigError as e:
        print(e, file=sys.stderr)
        return 1
    if args.cmd == "test":
        try:
            send(Watcher()._msg("테스트 메시지. 알림 경로가 동작해요"))
        except Exception as e:  # the message may hold the URL, and with it the token
            print(f"telegram send failed: {type(e).__name__}", file=sys.stderr)
            return 2
        return 0
    w, out = Watcher(interval_s=args.interval), Outbox(send)
    broken = False
    while True:
        try:
            out.add(w.step(fetch_status(args.url)))
            broken = False
        except Exception as e:  # a status this script does not understand must not stop it silently
            print(f"status not understood: {type(e).__name__}: {e}", file=sys.stderr, flush=True)
            if not broken:
                out.add([w._msg(f"⚠️ 감시 스크립트가 status를 해석하지 못함({type(e).__name__}). 이 동안 알림이 멈춤")])
            broken = True
        out.flush()
        time.sleep(args.interval)


if __name__ == "__main__":
    sys.exit(main())
