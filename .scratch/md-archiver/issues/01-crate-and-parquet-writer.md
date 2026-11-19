# 01: crate skeleton and the schema-driven Parquet writer

**What to build:** Spec D1 (crate, dependencies, `version`), D6 (generic writer from the vendored table JSON, types, zstd 9, metadata, row-group cut, determinism).

**Blocked by:** none

**Status:** ready-for-agent

- [ ] Test `schema_read_back_equals_contract`: for each of the five tables, an empty-row and a one-row file read back with the `parquet` reader have the contract's column names, order, physical/logical types and nullability, and the metadata keys.
- [ ] Test `writer_is_deterministic`: the same rows written twice give byte-identical files; rows with lists (empty and non-empty) and nulls read back equal.
- [ ] Test `row_groups_are_cut`: with a small cut size, many rows give several row groups and read back in order.
- [ ] `cargo tree -i zstd-sys` shows one copy (recorded in the PR).
