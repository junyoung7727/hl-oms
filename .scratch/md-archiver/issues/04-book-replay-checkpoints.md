# 04: book_msg rows, replay and checkpoint rows

**What to build:** Spec D4 for order book topics: rows of every connection, per-connection sequence tracking (`seq_gap`, `resnapshot`), followed connection and `handover`, the book-state end rules, checkpoint rows per hour (quiet hours included), D−1 carry-in.

**Blocked by:** 02

**Status:** done

- [x] Test `golden_book_msg`: builds of 2026-01-01, 2026-01-02 and 2026-01-05 give exactly the golden `book_msg` files (hour 00 checkpoint from the L0 of the date before; quiet hours 03, 04; no checkpoint at 06:00 and at 10:00 of the restart date).
- [x] Test `checkpoint_equals_reference_replay`: on a synthetic day with many deltas, every checkpoint equals an independent replay from the day's first snapshot.
- [x] Test `one_seq_gap_one_row`: a synthetic missing `u` gives exactly one `seq_gap` row and no checkpoint until the next snapshot.
- [x] Test `overlap_keeps_both_connections`: both connections' rows are in `book_msg` with their `conn_id`, and exactly one `handover` row is written.
- [x] Test `key_uniqueness`: no two `book_msg` rows share the key over all files of the golden build.
