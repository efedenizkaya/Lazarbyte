# SDD ledger — plan: Faz 1 workspace split (conversation-approved plan, no plan file)

Plan: Faz 1 — split the monolithic breadcrumb-rs crate into
lazarbyte-core + lazarbyte-cli without changing behaviour. Approved by the
project owner in conversation. Artifacts live in
`.superpowers/sdd/faz-1/`.

Global constraints:
- No behaviour change; upstream 137 tests stay green.
- Report `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
  `cargo test`, `cargo deny check` at every commit.
- Keep bin name `bcrumb-rs` and manifest `tool` value `"breadcrumb-rs 0.1.0"`.
- Never touch `C:\.git`.

## Progress

Task 1: Licensing (Apache-2.0, LICENSE-MIT, NOTICE, THIRD_PARTY_LICENSES.md) — complete (commit 2abb07c; tests 137 pass, fmt/clippy clean)
Task 2: Workspace split (crates/lazarbyte-core, crates/lazarbyte-cli, tests moved) — complete (commit 7286fae; tests 137 pass, fmt/clippy clean)
