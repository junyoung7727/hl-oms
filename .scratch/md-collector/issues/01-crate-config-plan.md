# 01: crate, config, plan groups, conn_id ranges

**What to build:** Crate `md-collector` (lib + bins `md-collector`, `md-lake`), the TOML config with its load-time checks (spec D2), the plan groups (D3), the registry with Bybit's market slots (D1), and the md-feed additive `ConnIds::range` + public `next_rest_frame_no`.

**Blocked by:** None

**Status:** ready-for-agent

- [ ] Test `config_example_loads`: the documented example parses; defaults fill the optional keys.
- [ ] Test `unknown_field_refused`, `unknown_venue_refused`, `unknown_market_refused`, `market_listed_twice_refused`, `conn_budget_sum_refused` (> 250).
- [ ] Test `traded_not_collected_refused`: a traded (market, channel) not collected, and a traded symbol not in a symbol list, each make the load fail with a message naming the entry.
- [ ] Test `plan_groups_split_traded`: traded topics are on connections that hold no other topic; consecutive `conn_index` ranges; the sum stays in `conn_budget`; a symbol with two book depths gets two groups.
- [ ] Test `market_slots_pinned`: Bybit linear/inverse/spot = 0/1/2; ranges `[1, 2^28)`, `[2^28, 2^29)`, `[2^29, 3·2^28)`.
- [ ] Test (md-feed) `conn_ids_range_shares_rest_counter`: two ranges hand out ids only inside their range and share one REST `frame_no` sequence.
