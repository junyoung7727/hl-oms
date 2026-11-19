# 08: docs, deploy template, full gate

**What to build:** `docs/operations/md-archiver.md` (commands, exit codes, runbook), ADR-020 (instrument in effect, carry-in replay of D−1, gap file as build marker, retention keeps gap/instrument, `first_update_id` metadata), glossary terms, `deploy/systemd/md-archiver@.service` and `.timer` (not installed or enabled), regenerated `docs/generated`, the full local gate.

**Blocked by:** 07

**Status:** done

- [x] Full gate of `pr_plan.json` U7 `checks` passes.
