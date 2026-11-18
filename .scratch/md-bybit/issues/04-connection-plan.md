# 04: Connection plan, subscribe frames and keepalive

**What to build:** `plan`, `subscribe_frames`, `keepalive`, `BybitAdapter::with_max_topics_per_conn` and `BybitAdapter::with_conn_budget`. Spec: "Plan", "Subscribe frames".

**Blocked by:** 01

**Status:** done

- [x] Test `plans_for_all_three_markets_stay_within_the_limits`: synthetic linear 932, spot 509, inverse 26 symbols × (`orderbook.50`, `publicTrade`, `tickers`), each market its own adapter with budgets (0, 100), (100, 100), (200, 50) → every topic exactly once per market; every connection ≤ 200 topics and ≤ 21,000 args characters; all channels of a symbol on one connection; `conn_index` inside each budget, no index used twice across markets; total connections ≤ 250.
- [x] Test `plan_refuses_what_bybit_would_refuse`: a plan that needs more connections than its budget (max 1 topic per connection, budget 10, 11 topics), depth 7, an unknown channel.
- [x] Test `subscribe_frames_hold_at_most_ten_args`: frames are valid JSON with `op` `subscribe`, distinct `req_id`, ≤ 10 args, and together hold the plan's topics in order.
- [x] Test `keepalive_is_a_ping_every_20_seconds`.
