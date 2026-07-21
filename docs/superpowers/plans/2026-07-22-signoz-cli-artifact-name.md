# `signoz-cli` Artifact Name Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `signoz-cli` the sole Rust executable name and the sole CLI command emitted by documentation and agent integrations.

**Architecture:** Change the Cargo binary target and clap root name at the executable boundary without changing the client library, configuration, authentication, or API contracts. Enforce the name in Rust and Python integration tests, then migrate every user-facing command example and plugin description atomically.

**Tech Stack:** Rust 2024, Cargo, clap, assert_cmd, Python unittest, Markdown Agent Skill, Codex and Claude Code plugin manifests.

---

### Task 1: Add failing executable-name contracts

**Files:**
- Modify: `crates/signoz-cli/tests/help.rs`
- Modify: `crates/signoz-cli/tests/cli.rs`
- Modify: `tests/test_agent_integration.py`
- Modify: `scripts/validate-agent-integration.py`
- Modify: `tests/skill/scenarios.json`

- [ ] **Step 1: Assert the clap and Cargo executable names**

Add this assertion before the subcommand checks in `command_tree_is_valid_and_complete`:

```rust
assert_eq!(command.get_name(), "signoz-cli");
```

Replace each process fixture target with:

```rust
Command::cargo_bin("signoz-cli").unwrap()
```

- [ ] **Step 2: Run the Rust test to verify RED**

Run: `cargo test -p signoz-cli --test help command_tree_is_valid_and_complete -- --exact`

Expected: FAIL because the current clap root command is `signoz`.

- [ ] **Step 3: Make the Agent validator require the new executable**

Change mandatory command tokens to:

```python
required = [
    "signoz-cli version",
    "signoz-cli auth check",
    # existing non-command workflow tokens remain unchanged
]
```

Add a validator error for command lines that start with the old executable:

```python
if re.search(r"(?m)^\s*signoz(?:\s|$)", skill):
    errors.append("skill contains legacy signoz command")
```

Update scenario command tokens and mutation fixtures from `signoz` to `signoz-cli` while retaining the explicit legacy-command rejection fixture.

- [ ] **Step 4: Run the Agent test to verify RED**

Run: `python3 -m unittest tests/test_agent_integration.py`

Expected: FAIL because the existing Skill still emits `signoz` commands and lacks required `signoz-cli` preflights.

- [ ] **Step 5: Commit the red contracts**

```bash
git add crates/signoz-cli/tests/help.rs crates/signoz-cli/tests/cli.rs tests/test_agent_integration.py scripts/validate-agent-integration.py tests/skill/scenarios.json
git commit -m "test: require signoz-cli executable name"
```

### Task 2: Rename the Rust executable

**Files:**
- Modify: `crates/signoz-cli/Cargo.toml`
- Modify: `crates/signoz-cli/src/args.rs`
- Modify: `crates/signoz-cli/src/main.rs`

- [ ] **Step 1: Rename the Cargo target and clap command**

Use the single binary target:

```toml
[[bin]]
name = "signoz-cli"
path = "src/main.rs"
```

Set the clap root name to `signoz-cli`:

```rust
#[command(name = "signoz-cli", version, about = "Query SigNoz from the command line")]
```

Update runtime help hints and verbose prefixes to say `signoz-cli`.

- [ ] **Step 2: Verify GREEN for the focused Rust contracts**

Run: `cargo test -p signoz-cli --test help && cargo test -p signoz-cli --test cli version_is_independent_of_connection_configuration -- --exact`

Expected: both focused test invocations PASS.

- [ ] **Step 3: Verify the physical artifact name**

Run: `cargo build -p signoz-cli && test -x target/debug/signoz-cli`

Expected: exit 0 and `target/debug/signoz-cli` exists.

- [ ] **Step 4: Commit the executable rename**

```bash
git add crates/signoz-cli/Cargo.toml crates/signoz-cli/src/args.rs crates/signoz-cli/src/main.rs
git commit -m "build: rename executable to signoz-cli"
```

### Task 3: Migrate documentation and Agent integrations

**Files:**
- Modify: `README.md`
- Modify: `plugins/signoz-cli/skills/signoz-cli/SKILL.md`
- Modify: `plugins/signoz-cli/.codex-plugin/plugin.json`
- Modify: `tests/skill/baseline.md`
- Modify: `docs/superpowers/specs/2026-07-22-signoz-rust-cli-agent-integration-design.md`
- Modify: `docs/superpowers/plans/2026-07-22-signoz-rust-cli.md`
- Modify: `docs/superpowers/plans/2026-07-22-signoz-cli-agent-integration.md`

- [ ] **Step 1: Replace only user-facing command invocations**

Change command examples and prose that names the binary from `signoz` to `signoz-cli`. Preserve `SigNoz`, `signoz-client`, plugin/Skill paths, environment variables, configuration identifiers, API URLs, and domain names.

The mandatory Skill preflight becomes:

```sh
signoz-cli version --output json
signoz-cli auth check --output json
```

Update the Codex manifest long description to name the Rust `signoz-cli` binary.

- [ ] **Step 2: Run Agent integration tests and validators**

Run:

```bash
python3 -m unittest tests/test_agent_integration.py
python3 scripts/validate-agent-integration.py .
python3 /Users/caspian/.codex/skills/.system/skill-creator/scripts/quick_validate.py plugins/signoz-cli/skills/signoz-cli
python3 /Users/caspian/.codex/skills/.system/plugin-creator/scripts/validate_plugin.py plugins/signoz-cli
```

Expected: every command exits 0.

- [ ] **Step 3: Scan for legacy command invocations**

Run:

```bash
rg -n '(?m)^\s*(?:[$>]\s*)?signoz(?:\s|$)|`signoz(?:\s|`)' README.md crates plugins tests scripts docs
```

Expected: no user-facing old command invocation; remaining matches, if any, are only deliberate legacy-rejection test data or non-command identifiers.

- [ ] **Step 4: Commit documentation and Skill migration**

```bash
git add README.md plugins tests scripts docs/superpowers/specs/2026-07-22-signoz-rust-cli-agent-integration-design.md docs/superpowers/plans/2026-07-22-signoz-rust-cli.md docs/superpowers/plans/2026-07-22-signoz-cli-agent-integration.md
git commit -m "docs: use signoz-cli command everywhere"
```

### Task 4: Full verification and integration

**Files:**
- Verify all modified files

- [ ] **Step 1: Run Rust quality gates**

Run:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release -p signoz-cli
test -x target/release/signoz-cli
```

Expected: every command exits 0; all 51 existing Rust tests plus the updated name assertion pass.

- [ ] **Step 2: Run repository quality gates**

Run the Python validators from Task 3, validate all JSON with `jq empty`, run plugin/Skill validators, run `git diff --check`, and inspect `git status --short`.

Expected: all validators exit 0, JSON parses, no whitespace errors, and only intentional committed changes exist.

- [ ] **Step 3: Review the final diff against the design**

Confirm the repository has exactly one Cargo binary target named `signoz-cli`, no compatibility `signoz` binary, no API/config behavior changes, and no user-facing old command examples.

- [ ] **Step 4: Finish the branch**

Use `superpowers:finishing-a-development-branch`, merge the verified branch back to `main`, rerun the full test suite on the merged result, then remove the owned worktree and feature branch.
