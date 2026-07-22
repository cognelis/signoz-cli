# SigNoz CLI Skill Installer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a self-contained Rust command that installs the shared `signoz-cli` Skill for Codex and Claude Code, targeting both by default or one through `--target`.

**Architecture:** A focused `skill_install` module embeds the canonical plugin Skill files, resolves official per-user agent directories, and performs idempotent atomic file updates. Clap owns target validation, while the dispatcher treats installation as a local command that bypasses all SigNoz configuration and network setup.

**Tech Stack:** Rust, Clap, serde/serde_json, directories, tempfile, assert_cmd, Python unittest integration validation.

---

### Task 1: Define the command contract

**Files:**
- Modify: `crates/signoz-cli/src/args.rs`
- Modify: `crates/signoz-cli/tests/help.rs`
- Test: `crates/signoz-cli/tests/cli.rs`

- [ ] **Step 1: Write failing help and argument tests**

Add `skills` to the required top-level command list and use this helper for CLI tests against isolated temporary directories:

```rust
fn skill_command(home: &Path, claude_config: &Path) -> assert_cmd::Command {
    let mut command = assert_cmd::Command::cargo_bin("signoz-cli").unwrap();
    command
        .env("HOME", home)
        .env("CLAUDE_CONFIG_DIR", claude_config)
        .env("SIGNOZ_CONFIG", home.join("broken.toml"))
        .env_remove("SIGNOZ_URL")
        .env_remove("SIGNOZ_API_KEY");
    command
}

skill_command(home.path(), claude_config.path())
    .args(["--output", "json", "skills", "install"]);
skill_command(home.path(), claude_config.path())
    .args(["--output", "json", "skills", "install", "--target", "codex"]);
skill_command(home.path(), claude_config.path())
    .args(["--output", "json", "skills", "install", "--target", "claude"]);
```

Also assert that `skills install --target unknown` exits with code 2 and names `all`, `codex`, and `claude` as valid values.

- [ ] **Step 2: Run tests and verify RED**

Run:

```sh
cargo test -p signoz-cli --test help --test cli skills
```

Expected: FAIL because the `skills` subcommand is absent.

- [ ] **Step 3: Add the minimal Clap types**

Add these command shapes in `args.rs`:

```rust
#[derive(Clone, Debug, Subcommand)]
pub enum SkillsCommand {
    Install {
        #[arg(long, value_enum, default_value_t = SkillTarget::All)]
        target: SkillTarget,
    },
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum SkillTarget {
    #[default]
    All,
    Codex,
    Claude,
}
```

Add `Command::Skills { #[command(subcommand)] command: SkillsCommand }` to the top-level enum.

- [ ] **Step 4: Run the help test and verify the command tree is green**

Run `cargo test -p signoz-cli --test help`.

Expected: PASS; the CLI integration tests may still fail because installation is not dispatched yet.

### Task 2: Implement the idempotent installer

**Files:**
- Create: `crates/signoz-cli/src/skill_install.rs`
- Modify: `crates/signoz-cli/src/lib.rs`

- [ ] **Step 1: Write failing unit tests for path resolution and file state**

Cover target resolution inside `skill_install.rs` with exact assertions:

```rust
#[test]
fn default_target_resolves_codex_then_claude() {
    let home = Path::new("/home/tester");
    let targets = resolve_targets(SkillTarget::All, Some(home), None).unwrap();
    assert_eq!(targets[0].agent, Agent::Codex);
    assert_eq!(targets[0].directory, home.join(".agents/skills/signoz-cli"));
    assert_eq!(targets[1].agent, Agent::Claude);
    assert_eq!(targets[1].directory, home.join(".claude/skills/signoz-cli"));
}

#[test]
fn claude_config_directory_overrides_home() {
    let target = resolve_targets(
        SkillTarget::Claude,
        None,
        Some(Path::new("/custom/claude")),
    )
    .unwrap();
    assert_eq!(target[0].directory, Path::new("/custom/claude/skills/signoz-cli"));
}
```

After the installer functions exist, cover state transitions and preservation with:

```rust
#[test]
fn install_reports_installed_updated_then_unchanged() {
    let home = tempfile::tempdir().unwrap();
    let targets = resolve_targets(SkillTarget::Codex, Some(home.path()), None).unwrap();
    assert_eq!(install_targets(&targets).unwrap()[0].status, InstallStatus::Installed);
    fs::write(targets[0].directory.join("SKILL.md"), "stale").unwrap();
    assert_eq!(install_targets(&targets).unwrap()[0].status, InstallStatus::Updated);
    assert_eq!(install_targets(&targets).unwrap()[0].status, InstallStatus::Unchanged);
}

#[test]
fn install_preserves_unmanaged_files() {
    let home = tempfile::tempdir().unwrap();
    let targets = resolve_targets(SkillTarget::Codex, Some(home.path()), None).unwrap();
    fs::create_dir_all(&targets[0].directory).unwrap();
    let notes = targets[0].directory.join("notes.txt");
    fs::write(&notes, "keep me").unwrap();
    install_targets(&targets).unwrap();
    assert_eq!(fs::read_to_string(notes).unwrap(), "keep me");
}
```

Expected paths are `<home>/.agents/skills/signoz-cli` and `<home>/.claude/skills/signoz-cli`. A supplied Claude config directory resolves to `<config>/skills/signoz-cli`.

- [ ] **Step 2: Run the unit tests and verify RED**

Run `cargo test -p signoz-cli skill_install`.

Expected: compilation failure because the installer types and functions do not exist.

- [ ] **Step 3: Add embedded assets and result types**

Define constants against the canonical plugin files:

```rust
const SKILL: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../plugins/signoz-cli/skills/signoz-cli/SKILL.md"
));
const OPENAI_METADATA: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../plugins/signoz-cli/skills/signoz-cli/agents/openai.yaml"
));
```

Serialize each result as camelCase with `agent`, `path`, and `status`, where status is `installed`, `updated`, or `unchanged`.

- [ ] **Step 4: Implement pure target resolution**

Implement:

```rust
fn resolve_targets(
    target: SkillTarget,
    home: Option<&Path>,
    claude_config: Option<&Path>,
) -> Result<Vec<InstallTarget>, String>
```

Select Codex then Claude for `All`, require a home only when its selected path needs one, and use `CLAUDE_CONFIG_DIR` when it is a non-empty path.

- [ ] **Step 5: Implement idempotent atomic writes**

For each selected target, create its directory, compare every managed file byte-for-byte, and use `tempfile::NamedTempFile::new_in` plus `persist` for changed files. Manage `SKILL.md` for both targets and `agents/openai.yaml` only for Codex. Preserve every other file.

- [ ] **Step 6: Run unit tests and verify GREEN**

Run `cargo test -p signoz-cli skill_install`.

Expected: all installer unit tests pass without reading or writing the real home directory.

### Task 3: Dispatch the local command and prove end-to-end behavior

**Files:**
- Modify: `crates/signoz-cli/src/dispatch.rs`
- Modify: `crates/signoz-cli/tests/cli.rs`

- [ ] **Step 1: Verify the existing CLI tests are still RED for installation**

Run:

```sh
cargo test -p signoz-cli --test cli skill_install
```

Expected: FAIL because the command currently falls through to SigNoz configuration or is unreachable in dispatch.

- [ ] **Step 2: Add local dispatch before SigNoz configuration**

Match `Command::Skills` next to `Version` and `Doctor`, call `skill_install::install(*target)`, map filesystem failures through `AppError::config`, and return:

```rust
DispatchResult {
    command: "skills.install".into(),
    data: serde_json::to_value(results).expect("install results are serializable"),
    meta: Meta::default(),
    failure_code: None,
}
```

Mark the `Command::Skills` arm in `prepare` unreachable because local dispatch always handles it first.

- [ ] **Step 3: Complete the integration assertions**

For default installation, assert both `SKILL.md` files exist, Codex has `agents/openai.yaml`, Claude does not, result order is Codex then Claude, and command name is `skills.install`. For each single target, assert the other target directory is absent. Seed stale Skill content and rerun to assert `updated`, then rerun once more to assert `unchanged`. Point `SIGNOZ_CONFIG` at invalid TOML to prove the command stays independent of SigNoz configuration.

- [ ] **Step 4: Run CLI and workspace tests**

Run:

```sh
cargo test -p signoz-cli --test cli skill_install
cargo test --workspace
```

Expected: all tests pass; tests that bind local mock-server ports may require the existing approved test permission.

- [ ] **Step 5: Commit the Rust feature**

```sh
git add crates/signoz-cli/src crates/signoz-cli/tests
git commit -m "feat: install agent skills from the CLI"
```

### Task 4: Document and validate the shared Skill workflow

**Files:**
- Modify: `README.md`
- Modify: `plugins/signoz-cli/skills/signoz-cli/SKILL.md`
- Modify: `scripts/validate-agent-integration.py`
- Modify: `tests/test_agent_integration.py`

- [ ] **Step 1: Record the RED Skill evaluation**

Use an isolated agent with only the current `SKILL.md` and ask for the default, Codex-only, and Claude-only installation commands. Record that it refuses or guesses because `skills install` is absent.

- [ ] **Step 2: Write failing validator tests**

Require these exact tokens in the Skill:

```text
signoz-cli skills install
--target codex
--target claude
```

Add a mutation test that replaces `signoz-cli skills install` with `signoz-cli plugin install` and assert validation fails.

- [ ] **Step 3: Run validator tests and verify RED**

Run `python3 -m unittest tests/test_agent_integration.py`.

Expected: FAIL because the current Skill does not advertise the new installer.

- [ ] **Step 4: Update README and Skill instructions**

Add the command to both command listings and lead README's agent-Skill installation section with:

```sh
signoz-cli skills install
signoz-cli skills install --target codex
signoz-cli skills install --target claude
```

Keep the existing marketplace flows as an advanced alternative and keep the Skill trigger description unchanged.

- [ ] **Step 5: Validate the updated Skill**

Run:

```sh
python3 -m unittest tests/test_agent_integration.py
python3 scripts/validate-agent-integration.py
python3 /Users/caspian/.codex/skills/.system/skill-creator/scripts/quick_validate.py plugins/signoz-cli/skills/signoz-cli
```

Expected: all validators pass.

- [ ] **Step 6: Re-run the Skill scenario with the updated file**

Use the same isolated prompt and verify the response gives exactly the three documented commands without inventing flags.

- [ ] **Step 7: Commit documentation and validation**

```sh
git add README.md plugins/signoz-cli/skills/signoz-cli/SKILL.md scripts/validate-agent-integration.py tests/test_agent_integration.py
git commit -m "docs: teach agents to install the CLI skill"
```

### Task 5: Final quality and plugin refresh

**Files:**
- Modify via helper: `plugins/signoz-cli/.codex-plugin/plugin.json`
- Modify via helper: `plugins/signoz-cli/.claude-plugin/plugin.json`

- [ ] **Step 1: Run formatting, lint, tests, and a release build**

Run:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace --release
git diff --check
```

Expected: every command succeeds, the binary is `target/release/signoz-cli`, and the test count is at least the 65-test baseline plus the new installer coverage.

- [ ] **Step 2: Test the release binary in isolated directories**

Set temporary `HOME` and `CLAUDE_CONFIG_DIR`, run the release binary with `skills install --output json`, verify both Skill locations and the JSON result, then run it again and verify both statuses are `unchanged`.

- [ ] **Step 3: Refresh and validate plugin cachebusters**

Run the plugin-creator cachebuster helper against `plugins/signoz-cli`, keep Codex and Claude manifest versions equal, and run the repository agent-integration validators again.

- [ ] **Step 4: Reinstall both local plugins and inspect installed Skill files**

Use `codex plugin add signoz-cli@personal` and the existing non-interactive Claude plugin installation command, then verify each installed cache contains the updated `SKILL.md`.

- [ ] **Step 5: Review the final diff and merge the feature commit(s) into `main`**

Inspect `git status`, `git diff --stat`, and the full diff; fast-forward `main` to the isolated branch only after all verification succeeds.
