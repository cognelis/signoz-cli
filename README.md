# signoz-cli

`signoz-cli` is a Rust command-line client for repeatable, read-only SigNoz queries in terminals, CI jobs, and JSON pipelines. The repository also ships one Agent Skill and plugin manifests so Codex and Claude Code can use the CLI without inventing commands or exposing credentials.

For general agent-led observability work, use the official [SigNoz Agent Skills](https://github.com/SigNoz/agent-skills) with the official [SigNoz MCP server](https://github.com/SigNoz/signoz-mcp-server). This repository's `signoz-cli` skill is deliberately narrow: it activates for explicit CLI, shell, CI, and JSON automation requests.

## Build and install

Rust stable is required.

```sh
cargo build --release
cargo install --path crates/signoz-cli
signoz-cli version --output json
```

The binary is named `signoz-cli`.

## Authenticate

For CI and short-lived shells, use environment variables. Keep the API key in the CI provider's secret store and disable shell tracing.

```sh
set +x
export SIGNOZ_URL="https://your-signoz.example.com"
export SIGNOZ_API_KEY="...from a secret store..."
signoz-cli doctor --output json
```

For workstation use, profiles store only the instance URL in the operating-system config directory. API keys are stored in the operating-system keyring.

```sh
signoz-cli profile add production --url https://your-signoz.example.com
signoz-cli profile use production
printf '%s' "$SIGNOZ_API_KEY" | signoz-cli auth login --api-key-stdin
signoz-cli doctor --output json
```

There is intentionally no `--api-key` flag, so the secret does not enter shell history or process listings.

## Diagnose a connection

Run the read-only diagnostic before remote automation or when a connection fails:

```sh
signoz-cli doctor --output json
```

It checks the CLI version, connection configuration, credential availability, API connectivity, and service-account authentication in order. It sends one request to the same identity endpoint used by `auth check` and never emits the API key.

A completed doctor run always writes its ordered `pass`/`fail`/`skipped` check array to stdout. An unhealthy result still exits with the matching nonzero code below, so scripts should capture stdout before testing the status. Stderr stays empty unless `--verbose` is explicitly requested. `auth check` remains available when only the authenticated service-account identity is needed.

## Query SigNoz

```sh
# Discover metrics
signoz-cli --output json metrics list

# Query a metric for the last hour
signoz-cli --output json metrics query http.server.duration \
  --since 1h

# Search logs
signoz-cli --output json logs search \
  --since 1h --filter "severity_text = 'ERROR'"

# Find traces for a service
signoz-cli --output json traces search \
  --since 30m --filter 'service.name = "checkout"'

# Send a complete Query Builder v5 request
signoz-cli --output json query run --file request.json
printf '%s' "$QUERY_JSON" | signoz-cli --output json query run --file -
```

Use `--since` with `ms`, `s`, `m`, `h`, or `d`, or provide both `--start` and `--end` as RFC3339 timestamps or `@<unix-ms>`. The metric query command reads the metric catalog first and derives safe time/spatial aggregation from its type, temporality, and monotonicity; `--aggregation` only overrides the spatial aggregation. Raw searches and ordinary resource lists accept `--limit` and `--offset`; the JSON `meta` reports the resolved time window and pagination state. Log/trace aggregate commands return a single ungrouped scalar, do not accept offsets, and require `--aggregate-on` for functions other than `count` and `rate`. Alert-rule history uses `--cursor`/`meta.nextCursor` instead of offsets. The time-windowed metrics catalog has a limit but no offset: narrow it with `--since`, `--search`, or `--source meter`; a full page is marked `hasMore: true` with no `nextOffset`. Run `signoz-cli --help` and `signoz-cli <group> <command> --help` for the complete command surface.

Successful JSON output has a stable versioned envelope:

```json
{
  "schemaVersion": "1",
  "command": "metrics.list",
  "data": {},
  "meta": {
    "status": 200,
    "elapsedMs": 42
  }
}
```

`--raw` emits only the command data payload without the versioned envelope (direct commands keep the upstream shape; composite and client-paginated commands may combine or slice it). Normal diagnostics and errors are written to stderr; stdout remains clean for pipelines. The exception is `doctor`: a completed unhealthy diagnosis writes its report to stdout before exiting nonzero. `--output auto` uses a table only when stdout is a terminal.

## Command groups

```text
version
doctor
skills install [--target all|codex|claude]
auth login|check|logout
profile add|list|show|use|remove
metrics list|query|top|cardinality|usage
fields keys|values
logs search|aggregate
traces search|aggregate|get
services list|operations
alerts list
rules list|get|history
dashboards list|get
views list|get
channels list|get
query run
```

The client retries replay-safe requests on transport failures, HTTP 429, 502, 503, and 504, for at most three attempts. Responses are capped at 64 MiB. Feature-specific 404 responses report the minimum compatible SigNoz version instead of pretending that a resource is missing.

Exit codes are stable:

| Code | Meaning |
| ---: | --- |
| 0 | Success |
| 2 | Invalid arguments, time range, or input |
| 3 | Configuration, authentication, or authorization failure |
| 4 | Network or timeout failure |
| 5 | Non-authentication 4xx or exhausted rate-limit failure |
| 6 | Upstream 5xx or retry exhaustion |
| 7 | Unsupported feature, oversized, or incompatible response |

## Install the agent skill

```sh
# Install for both Codex and Claude Code (default)
signoz-cli skills install
# Install only for Codex
signoz-cli skills install --target codex
# Install only for Claude Code
signoz-cli skills install --target claude
```

Without `--target`, the command installs the skill for both agents. Select `codex` or `claude` to install only for that agent. The operation is local and idempotent: it needs no SigNoz URL or API key, updates embedded managed files when rerun, and preserves unrelated files in the target skill directory.

### Advanced: plugin lifecycle alternatives

The same skill directory is packaged for both agents at `plugins/signoz-cli/skills/signoz-cli`. Use the plugin lifecycle instead when you want the repository marketplace installation.

For Codex, add this repository as a marketplace and install `signoz-cli` in the Plugins UI:

```sh
codex plugin marketplace add /absolute/path/to/signoz-cli
```

For Claude Code:

```text
/plugin marketplace add /absolute/path/to/signoz-cli
/plugin install signoz-cli@signoz-cli
```

The Codex manifest is `plugins/signoz-cli/.codex-plugin/plugin.json`; the Claude Code manifest is `plugins/signoz-cli/.claude-plugin/plugin.json`. Codex may invoke the skill implicitly only when its narrow description matches an explicit CLI, shell, CI, or JSON-automation request, so ordinary SigNoz investigations remain with the official workflows.

For agent-driven investigations, install the official SigNoz integration instead:

```sh
codex plugin marketplace add SigNoz/agent-skills
signoz-mcp-setup <region>
codex mcp login signoz
```

In Claude Code, use `/plugin marketplace add SigNoz/agent-skills` followed by `/plugin install signoz@signoz-skills`.

## Compatibility and safety

- Alert history requires SigNoz 0.118 or newer.
- Alert rules require SigNoz 0.120 or newer.
- Metric usage requires SigNoz 0.131 or newer.
- The CLI accepts self-hosted HTTP origins but does not provide an insecure TLS bypass.
- Telemetry can contain secrets or personal data. Review logs and traces before storing them as CI artifacts.

The route map and behavior were derived from the current official SigNoz MCP server and Agent Skills repositories. See the main [integration design](docs/superpowers/specs/2026-07-22-signoz-rust-cli-agent-integration-design.md) and the [`doctor` design](docs/superpowers/specs/2026-07-22-signoz-cli-doctor-design.md).
