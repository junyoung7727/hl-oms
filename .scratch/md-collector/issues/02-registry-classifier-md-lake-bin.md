# 02: classifier and the md-lake binary

**What to build:** `registry::classify` for Bybit (spec D1) and the `md-lake seal | seal-days | verify` binary (D9).

**Blocked by:** 01

**Status:** done

- [x] Test `classify_ws_by_conn_slot`: the same `orderbook.50.BTCUSDT` payload on `conn_id` 5 and on `(2<<28)+5` gives linear and spot; a control frame gives `None`; a note gives `None`.
- [x] Test `classify_rest_by_category`: an instruments body with `result.category = "inverse"` → `{inverse, instruments, _all}`; a body without it → `None`.
- [x] Test `md_lake_unknown_venue`: `md-lake seal --venue nope --lake DIR ...` exits non-zero, and the directory tree under DIR (prepared with files) is byte-identical and gains no entry.
- [x] Test `md_lake_seal_and_verify`: open files written with md-lake's writer for two markets; `md-lake seal --now-ns` puts each record in the right market's day file; `md-lake verify --json` prints the verify result.
