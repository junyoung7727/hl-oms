# 01: alert-direct crate

**What to build:** Spec D1: config file check, message split, outbox, send outcomes, routes with the pinned-IP fallback, repeat rule, the sender thread.

**Blocked by:** none

**Status:** done

- [x] Test `config_refuses_unsafe_files`: symlink, mode 0640, path under a directory with `.git`, bad token, empty chat_id are refused; no error text contains the token.
- [x] Test `split_matches_pr3`: parts ≤ 3,500 UTF-16 units, cut at line ends, an overlong line (with non-BMP characters) cut by characters; joining the parts gives the text back.
- [x] Test `outbox_bound_and_drop_notice`: 60 parts → 50 kept, 10 counted; after a full delivery one drop notice is sent and the count resets.
- [x] Test `send_outcomes`: fake answers 200 ok → delivered; 400 and 413 → dropped and counted, the next part goes; 401, 403, 429, 500, a closed connection and a timeout → kept, flush stops, delivered on the next flush.
- [x] Test `fallback_route_and_last_good_first`: route 1 a dead loopback port, route 2 a fake reached through `resolve(host → 127.0.0.1:port)`; the message arrives with `Host: <host>`; the next send tries route 2 first.
- [x] Test `repeat_rule_with_injected_clock`: start → 1 message; +4 min → none; +5 min → 1 (latest text); +10 min → 1; clear → 1 `해소:`; clear again → none.
- [x] Test `set_never_waits`: with a fake that never answers, 1,000 `set`/`once` calls return within 50 ms in total.
