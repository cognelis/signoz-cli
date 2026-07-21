# SigNoz Rust CLI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a tested Rust `signoz-cli` binary that exposes the designed read-only SigNoz API surface with stable JSON output, safe authentication, retries, and explicit compatibility errors.

**Architecture:** A Cargo workspace separates the reusable `signoz-client` HTTP library from the `signoz-cli` command and rendering layer. The client owns URL/time validation, request replay policy, response bounds, API route mapping, and typed errors; the binary owns profiles, keyring integration, clap arguments, output envelopes, and process exit codes.

**Tech Stack:** Rust stable, Tokio, clap, reqwest/rustls, serde, chrono, url, directories, keyring, secrecy, tracing, wiremock, assert_cmd, predicates, insta.

---

## File Map

- `Cargo.toml`: workspace members and shared dependency versions.
- `.gitignore`: local Rust toolchain, Cargo output, snapshots pending review, and editor files.
- `crates/signoz-client/src/lib.rs`: public client API exports.
- `crates/signoz-client/src/url.rs`: strict SigNoz origin parsing.
- `crates/signoz-client/src/time.rs`: duration/RFC3339/Unix-ms time windows.
- `crates/signoz-client/src/error.rs`: stable error codes, status mapping, and retry classification.
- `crates/signoz-client/src/http.rs`: authenticated bounded HTTP transport and replay policy.
- `crates/signoz-client/src/api.rs`: endpoint-specific read-only operations.
- `crates/signoz-client/src/query.rs`: Query Builder v5 payload constructors for metrics/logs/traces.
- `crates/signoz-client/tests/contracts.rs`: wire-level route, header, retry, and size tests.
- `crates/signoz-cli/src/main.rs`: runtime, diagnostics, dispatch, and process exit.
- `crates/signoz-cli/src/args.rs`: complete clap command tree.
- `crates/signoz-cli/src/config.rs`: profile storage, precedence, and credential provider.
- `crates/signoz-cli/src/dispatch.rs`: command-to-client translation.
- `crates/signoz-cli/src/output.rs`: stable JSON/error envelopes and human tables.
- `crates/signoz-cli/tests/cli.rs`: end-to-end process behavior.
- `README.md`: installation, authentication, examples, and official skill guidance.

### Task 1: Bootstrap the Cargo workspace

**Files:**
- Create: `Cargo.toml`
- Create: `.gitignore`
- Create: `crates/signoz-client/Cargo.toml`
- Create: `crates/signoz-client/src/lib.rs`
- Create: `crates/signoz-cli/Cargo.toml`
- Create: `crates/signoz-cli/src/main.rs`

- [ ] **Step 1: Add workspace manifests and a compile smoke test**

Create the workspace with resolver 2 and two members. Define `signoz-client` as a library and `signoz-cli` with `[[bin]] name = "signoz-cli"`. In `main.rs`, parse no behavior yet and print the package version only when invoked with `--version` through clap derive.

```rust
#[derive(clap::Parser)]
#[command(name = "signoz-cli", version, about = "Query SigNoz from the command line")]
struct Cli {}

fn main() {
    let _ = <Cli as clap::Parser>::parse();
}
```

- [ ] **Step 2: Run the initial compile checks**

Run: `cargo check --workspace`

Expected: both crates compile without warnings.

- [ ] **Step 3: Commit the workspace bootstrap**

```bash
git add Cargo.toml .gitignore crates/signoz-client crates/signoz-cli
git commit -m "build: bootstrap signoz-cli rust workspace"
```

### Task 2: Validate origins and time windows with TDD

**Files:**
- Create: `crates/signoz-client/src/url.rs`
- Create: `crates/signoz-client/src/time.rs`
- Modify: `crates/signoz-client/src/lib.rs`

- [ ] **Step 1: Write failing origin tests**

```rust
#[test]
fn accepts_http_origins_and_removes_trailing_slash() {
    assert_eq!(SigNozOrigin::parse("http://localhost:3301/").unwrap().as_str(), "http://localhost:3301");
}

#[test]
fn rejects_paths_userinfo_queries_and_non_http_schemes() {
    for value in [
        "https://user@example.com",
        "https://example.com/api",
        "https://example.com?q=1",
        "file:///tmp/signoz",
    ] {
        assert!(SigNozOrigin::parse(value).is_err(), "accepted {value}");
    }
}
```

- [ ] **Step 2: Run tests and verify RED**

Run: `cargo test -p signoz-client url -- --nocapture`

Expected: FAIL because `SigNozOrigin` does not exist.

- [ ] **Step 3: Implement `SigNozOrigin`**

Use `url::Url`; require `http` or `https`, no username/password, path exactly `/`, no query/fragment, and a non-empty host. Preserve localhost/private addresses and strip the trailing slash from the canonical string.

- [ ] **Step 4: Write failing time-window tests**

```rust
#[test]
fn resolves_relative_and_absolute_windows() {
    let now = DateTime::parse_from_rfc3339("2026-07-22T01:00:00Z").unwrap().to_utc();
    assert_eq!(TimeWindowArgs::since("1h").resolve(now).unwrap().start_ms, now.timestamp_millis() - 3_600_000);
    assert!(TimeWindowArgs::absolute(Some("@1000"), None).resolve(now).is_err());
}
```

- [ ] **Step 5: Run tests and verify RED**

Run: `cargo test -p signoz-client time -- --nocapture`

Expected: FAIL because `TimeWindowArgs` does not exist.

- [ ] **Step 6: Implement time parsing**

Accept positive `ms`, `s`, `m`, `h`, and `d` durations. Accept RFC3339 or `@<unix-ms>` absolute timestamps. Require both absolute endpoints, prohibit mixing them with `since`, and require `start < end`.

- [ ] **Step 7: Run tests and commit**

Run: `cargo test -p signoz-client`

Expected: all origin and time tests PASS.

```bash
git add crates/signoz-client
git commit -m "feat(client): validate origins and time windows"
```

### Task 3: Build stable errors and bounded retrying HTTP transport

**Files:**
- Create: `crates/signoz-client/src/error.rs`
- Create: `crates/signoz-client/src/http.rs`
- Modify: `crates/signoz-client/src/lib.rs`
- Create: `crates/signoz-client/tests/http.rs`

- [ ] **Step 1: Write failing status and secret-redaction tests**

```rust
#[test]
fn maps_auth_and_transient_statuses() {
    assert_eq!(ClientError::from_status(401, "denied", None).code(), ErrorCode::Unauthorized);
    assert!(ClientError::from_status(503, "down", None).retryable());
    assert!(!ClientError::from_status(404, "missing", None).retryable());
}

#[test]
fn credentials_are_redacted_from_debug() {
    let credentials = Credentials::new("never-print-this");
    assert!(!format!("{credentials:?}").contains("never-print-this"));
}
```

- [ ] **Step 2: Run and verify RED**

Run: `cargo test -p signoz-client error -- --nocapture`

Expected: FAIL because error and credential types are missing.

- [ ] **Step 3: Implement stable errors and credentials**

Define `ErrorCode::{Validation, Config, Unauthorized, Forbidden, NotFound, RateLimited, Network, Timeout, Upstream, FeatureUnsupported, ResponseTooLarge, IncompatibleResponse}`. Store secrets in `SecretString`; implement neither `Display` nor an exposing `Debug` for the raw value.

- [ ] **Step 4: Write failing wire tests for auth, retries, and response caps**

Use `wiremock` to assert `SIGNOZ-API-KEY`, return 503 twice then 200, and return a body one byte over 64 MiB. Assert three total attempts for replay-safe calls and `ResponseTooLarge` without truncated JSON.

- [ ] **Step 5: Run and verify RED**

Run: `cargo test -p signoz-client --test http -- --nocapture`

Expected: FAIL because `HttpTransport` is missing.

- [ ] **Step 6: Implement `HttpTransport`**

Build reqwest with rustls and a 60-second default timeout. Add authentication and user-agent headers. Buffer at most 64 MiB plus one byte. Retry replay-safe requests on transport failures, 429, 502, 503, and 504 for a total of three attempts, using 100ms and 400ms waits or a bounded `Retry-After` value.

- [ ] **Step 7: Run tests and commit**

Run: `cargo test -p signoz-client`

Expected: unit and transport tests PASS.

```bash
git add crates/signoz-client
git commit -m "feat(client): add safe retrying http transport"
```

### Task 4: Implement every read-only API route

**Files:**
- Create: `crates/signoz-client/src/api.rs`
- Create: `crates/signoz-client/src/query.rs`
- Modify: `crates/signoz-client/src/lib.rs`
- Create: `crates/signoz-client/tests/contracts.rs`

- [ ] **Step 1: Write a failing table-driven route contract test**

Define cases for auth, metrics, fields, top/cardinality/usage, alerts, rules/history, dashboards, services/operations, views, channels, and raw query. Each case specifies method, exact path, expected query/body fragment, and whether POST is replay-safe.

```rust
struct RouteCase {
    operation: Operation,
    method: &'static str,
    path: &'static str,
    body_contains: Option<&'static str>,
}
```

- [ ] **Step 2: Run and verify RED**

Run: `cargo test -p signoz-client --test contracts -- --nocapture`

Expected: FAIL because `SigNozClient` and `Operation` are missing.

- [ ] **Step 3: Implement endpoint methods**

Expose one method per designed command. Percent-encode IDs with URL path-segment mutation. Return `serde_json::Value` plus response metadata, preserving unknown upstream fields. For metric usage, make both dashboards and alerts requests and return a combined object. Map route-specific 404 responses to minimum-version `FeatureUnsupported` errors.

- [ ] **Step 4: Write failing Query Builder constructor tests**

Assert log search uses signal `logs`, trace search uses signal `traces`, metric query includes discovered aggregation fields, all generated specs include a positive limit and non-empty ordering, and all outer timestamps are Unix milliseconds.

- [ ] **Step 5: Run and verify RED**

Run: `cargo test -p signoz-client query -- --nocapture`

Expected: FAIL until constructors produce the exact v5 JSON shapes.

- [ ] **Step 6: Implement Query Builder constructors**

Create typed request structs for log search/aggregate, trace search/aggregate/get, and metric query. Combine shortcuts and explicit filters with `AND`; escape literal string values before insertion. Keep `query_raw(Value)` as the only arbitrary payload path.

- [ ] **Step 7: Run full contract tests and commit**

Run: `cargo test -p signoz-client`

Expected: every route case and Query Builder case PASS.

```bash
git add crates/signoz-client
git commit -m "feat(client): cover SigNoz read api"
```

### Task 5: Implement profiles and authentication precedence

**Files:**
- Create: `crates/signoz-cli/src/config.rs`
- Modify: `crates/signoz-cli/src/main.rs`
- Create: `crates/signoz-cli/tests/config.rs`

- [ ] **Step 1: Write failing precedence and persistence tests**

Use a temporary config root and an in-memory `CredentialStore` fake. Assert explicit URL plus environment key wins, selected profile is next, current profile is last, profile TOML contains no secret, and removing a profile deletes its keyring entry.

- [ ] **Step 2: Run and verify RED**

Run: `cargo test -p signoz-cli --test config -- --nocapture`

Expected: FAIL because profile types are missing.

- [ ] **Step 3: Implement profile config and keyring adapter**

Define `ProfileConfig`, `ConfigFile`, `CredentialStore`, `SystemKeyring`, and `ResolvedConnection`. Write config atomically using a same-directory temporary file and rename. Read API keys from `SIGNOZ_API_KEY` or keyring; never serialize them.

- [ ] **Step 4: Run tests and commit**

Run: `cargo test -p signoz-cli --test config`

Expected: all precedence, atomic-write, and no-secret tests PASS.

```bash
git add crates/signoz-cli
git commit -m "feat(cli): add profiles and secure credentials"
```

### Task 6: Define the complete clap command tree

**Files:**
- Create: `crates/signoz-cli/src/args.rs`
- Modify: `crates/signoz-cli/src/main.rs`
- Create: `crates/signoz-cli/tests/help.rs`

- [ ] **Step 1: Write failing help-surface tests**

Use `Cli::command().debug_assert()` and snapshots that require every designed group and subcommand: version, auth, profile, metrics, fields, logs, traces, services, alerts, rules, dashboards, views, channels, and query.

- [ ] **Step 2: Run and verify RED**

Run: `cargo test -p signoz-cli --test help -- --nocapture`

Expected: FAIL because the command tree is incomplete.

- [ ] **Step 3: Implement args with shared time/pagination groups**

Use flattened `ConnectionArgs`, `TimeArgs`, `PaginationArgs`, and `OutputArgs`. Enforce clap conflicts/requires for `since` versus `start/end`, restrict enums with `ValueEnum`, and use kebab-case long flags.

- [ ] **Step 4: Run help tests and commit**

Run: `cargo test -p signoz-cli --test help`

Expected: command debug assertions and snapshots PASS.

```bash
git add crates/signoz-cli
git commit -m "feat(cli): define read-only command surface"
```

### Task 7: Dispatch commands and render stable output

**Files:**
- Create: `crates/signoz-cli/src/dispatch.rs`
- Create: `crates/signoz-cli/src/output.rs`
- Modify: `crates/signoz-cli/src/main.rs`
- Create: `crates/signoz-cli/tests/cli.rs`

- [ ] **Step 1: Write failing JSON and error envelope tests**

```rust
#[test]
fn success_envelope_is_versioned() {
    let value = SuccessEnvelope::new("metrics.list", json!([]), Meta::default());
    assert_eq!(serde_json::to_value(value).unwrap()["schemaVersion"], "1");
}

#[test]
fn errors_use_stderr_and_stable_exit_codes() {
    // Launch against a mock 401 and assert stdout empty, stderr JSON code
    // "unauthorized", and exit code 3.
}
```

- [ ] **Step 2: Run and verify RED**

Run: `cargo test -p signoz-cli --test cli -- --nocapture`

Expected: FAIL because dispatch and output modules are missing.

- [ ] **Step 3: Implement output envelopes and tables**

Serialize camelCase JSON exactly as designed. `auto` chooses table only for a TTY. `raw` emits only the upstream JSON body. Errors always use stderr JSON when JSON/raw mode is selected, otherwise concise human text. Map client errors to exit codes 2–7.

- [ ] **Step 4: Implement command dispatch**

Resolve local-only profile commands without constructing a client. For remote commands, resolve connection, time window, and pagination, call the exact client method, then attach command name and meta. Continue to keep stdout free of diagnostics.

- [ ] **Step 5: Add parameterized end-to-end command tests**

For every remote subcommand, launch `signoz-cli` with fake `SIGNOZ_URL`/`SIGNOZ_API_KEY`, serve the expected wire response, and assert exit 0 plus `schemaVersion=1`. Add invalid-time, missing-config, unauthorized, timeout, feature-unsupported, raw-query-stdin, and secret-leak tests.

- [ ] **Step 6: Run tests and commit**

Run: `cargo test --workspace`

Expected: all client and CLI tests PASS.

```bash
git add crates/signoz-cli
git commit -m "feat(cli): dispatch commands with stable output"
```

### Task 8: Document and harden the CLI

**Files:**
- Create: `README.md`
- Modify: workspace Rust files as required by lint findings

- [ ] **Step 1: Write README examples against real help output**

Document local installation, environment authentication, profile/keyring authentication, JSON examples for metrics/logs/traces, raw Query Builder stdin, exit codes, self-hosted support, and why general agent workflows should install `SigNoz/agent-skills`.

- [ ] **Step 2: Run formatting, lint, tests, and help smoke checks**

Run:

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p signoz-cli -- --help
```

Expected: all checks return 0; help lists every command group.

- [ ] **Step 3: Commit CLI documentation and hardening**

```bash
git add README.md Cargo.toml Cargo.lock crates .gitignore
git commit -m "docs: document signoz-cli workflows"
```

## Plan Self-Review

- Spec coverage: workspace boundaries, all read-only commands, authentication, JSON/error contracts, retries, response cap, version compatibility, and documentation each have an implementing task.
- Placeholder scan: no incomplete implementation markers are present.
- Type consistency: `SigNozOrigin`, `TimeWindowArgs`, `Credentials`, `ClientError`, `HttpTransport`, `SigNozClient`, `SuccessEnvelope`, and `Meta` are introduced before downstream use.

