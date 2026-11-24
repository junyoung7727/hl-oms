# hl-oms: instructions for agents

Read `README.md` first. It is the map of the documents (`docs/`), the registry (`registry/*.toml`) and the acceptance checks.

Rules that the code does not show:

- This repository is public. Never commit keys, wallet addresses, journals, snapshots or logs. `tools/ci/secret_scan.sh` must pass.
- Never run a node against Hyperliquid testnet or mainnet, and never send an order, without the owner's written approval (user decisions U-01, U-09).
- The tree is not rustfmt-clean. Do not run `cargo fmt` over existing files. Only files that a change adds must pass `rustfmt --check` (`tools/ci/rustfmt_added.sh`).
- After any change under `crates/` or `registry/`, run `cargo run -p xtask -- docs` and commit `docs/generated/`. CI fails when they differ.
- A claim (`registry/claims.toml`) may name only tests that exist. A journal event must be listed by a contract in `registry/contracts.toml`. Add the claim and the event in the same change that adds the test and the event.
- Reviewers read `CODING_STANDARDS.md`. A change to a check (`tools/ci/`, lint settings, test removal) waits for the owner (`tools/ci/owner_review.sh`, ADR-033).

## Agent skills

### Issue tracker

Issues and specs are local markdown files under `.scratch/<feature-slug>/` in this repository, committed to git. GitHub Issues are not used. See `docs/agents/issue-tracker.md`.

### Triage labels

The five default role strings (`needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`), written on the `Status:` line of each issue file. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: one `GLOSSARY.md` at the repository root and decisions in `docs/adr/`. See `docs/agents/domain.md`.
