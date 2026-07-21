# `signoz-cli doctor` Implementation Plan

> Execute each task with strict red-green-refactor discipline. Do not add production behavior before the associated test fails for the expected reason.

**Goal:** Add a secret-safe staged doctor command and teach the shared Codex/Claude Skill to use it.

**Architecture:** Keep the CLI dispatcher as the orchestrator, add a small `doctor` module for stable report rows, split configuration target selection from credential lookup, and carry an optional exit category on successful dispatch output. Reuse the existing auth identity operation and transport.

**Tech stack:** Rust 2024, clap, tokio, serde/serde_json, reqwest, wiremock, Python integration validator.

---

### Task 1: Define the command and report contract

**Files:**
- Modify: `crates/signoz-cli/tests/help.rs`
- Modify: `crates/signoz-cli/tests/cli.rs`
- Modify: `crates/signoz-cli/src/args.rs`

1. Add `doctor` to the asserted root command tree.
2. Add CLI tests for a healthy ordered five-check JSON report and a missing-connection report with exit 3/stdout-only behavior.
3. Run the targeted tests and confirm they fail because clap does not know `doctor`.
4. Add only the `Command::Doctor` parser variant.
5. Run the help test and confirm it passes while behavior tests still fail.

### Task 2: Separate target selection from credential lookup

**Files:**
- Modify: `crates/signoz-cli/src/config.rs`

1. Add unit tests showing target selection succeeds without a key, preserves explicit URL/profile/environment/current-profile precedence, and credential resolution retains existing keyring behavior.
2. Run the config tests and confirm the new API is missing.
3. Introduce `ResolvedTarget`, `resolve_target`, and `resolve_credentials`; keep `resolve` as their composition.
4. Run config tests and all existing tests to prove no behavior regression.

### Task 3: Implement staged diagnosis and nonzero report exit

**Files:**
- Create: `crates/signoz-cli/src/doctor.rs`
- Modify: `crates/signoz-cli/src/lib.rs`
- Modify: `crates/signoz-cli/src/dispatch.rs`
- Modify: `crates/signoz-cli/src/main.rs`
- Modify: `crates/signoz-cli/tests/cli.rs`

1. Extend behavior tests with unauthorized, upstream, and malformed-success cases. Assert ordered states, exit categories, stdout/stderr placement, request route, and secret redaction.
2. Run targeted tests and confirm they fail at dispatch/output behavior.
3. Add serializable doctor row/status types and exact safe messages.
4. Route `Command::Doctor` before ordinary command preparation and execute each stage in dependency order.
5. Add `failure_code: Option<ErrorCode>` to `DispatchResult`; normal results use `None` and doctor failures carry the category.
6. Emit the report first in `main`, then exit through the existing `exit_code` mapping when requested.
7. Run targeted doctor, help, config, and existing CLI tests.

### Task 4: Update agent-facing contracts and docs

**Files:**
- Modify: `plugins/signoz-cli/skills/signoz-cli/SKILL.md`
- Modify: `scripts/validate-agent-integration.py`
- Modify: `tests/test_agent_integration.py`
- Modify: `README.md`
- Refresh: plugin marketplace cachebuster metadata if required by plugin tooling

1. Add a mutation-style validator test requiring `signoz-cli doctor` in the Skill, and confirm it fails.
2. Update the shared Skill with doctor preflight/troubleshooting and nonzero-report parsing guidance.
3. Update README usage, command surface, and the documented stdout exception.
4. Refresh plugin cachebuster metadata using the repository/plugin helper.
5. Run Agent/Skill/plugin/Claude validators and Python tests.

### Task 5: Full verification and review

1. Run `cargo fmt --all -- --check`.
2. Run `cargo clippy --workspace --all-targets --all-features -- -D warnings`.
3. Run `cargo test --workspace` with loopback access for Wiremock.
4. Run `cargo build --workspace --release` and verify the executable is named `signoz-cli`.
5. Run all repository integration validators.
6. Inspect `git diff --check`, the complete diff, and secret-bearing fixtures.
7. Request an independent code review, address findings with tests, rerun verification, and commit the completed branch.
