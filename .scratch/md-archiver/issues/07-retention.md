# 07: --retain-l1-days

**What to build:** Spec D9.

**Blocked by:** 06

**Status:** done

- [x] Test `retention_needs_sealed_and_clean`: only dates older than N with day-sealed L0 and a clean audit lose `book_msg`/`trade`/`ticker`; `not_audited`, `fail`, missing or cut audit, `contract_ver` 2, a date not day-sealed keep everything; `gap` and `instrument` stay; nothing under `crypto/l0/` or `crypto/audit/` changes; the date is not rebuilt by the next run.
