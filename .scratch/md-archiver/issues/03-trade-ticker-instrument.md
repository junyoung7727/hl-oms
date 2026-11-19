# 03: trade, ticker and instrument rows

**What to build:** Spec D4 (trade followed connection, handover, de-dup over D−1 and D, drop counts; ticker rows; instrument rows), parse_error rows, small-channel day files.

**Blocked by:** 02

**Status:** done

- [x] Test `golden_trade_ticker_instrument`: the build of 2026-01-02 gives exactly the golden `trade`, `ticker` and `instrument` files.
- [x] Test `trade_on_two_connections`: the trade appears once (earliest `recv_ns`) and the handover row of the pair has `dropped_trade_rows` 1; L0 trade count = L1 rows + Σ dropped (contract 7).
- [x] Test `trade_dedup_against_the_date_before`: a D row whose key was kept in D−1 is dropped.
- [x] Test `parse_error_rows`: a price that is not exact and a record whose route is another topic give one `parse_error` row each and no data row.
