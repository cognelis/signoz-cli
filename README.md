# signoz-cli

`signoz-cli` is a Rust command-line client for repeatable, read-only SigNoz queries in terminals, CI jobs, and JSON pipelines. The repository also ships one Agent Skill and plugin manifests so Codex and Claude Code can use the CLI without inventing commands or exposing credentials.

For general agent-led observability work, use the official [SigNoz Agent Skills](https://github.com/SigNoz/agent-skills) with the official [SigNoz MCP server](https://github.com/SigNoz/signoz-mcp-server). This repository's `signoz-cli` skill is deliberately narrow: it activates for explicit CLI, shell, CI, and JSON automation requests.

## Build and install

Rust stable is required.

```sh
cargo build --release
cargo install --path crates/signoz-cli
signoz version --output json
```

The binary is named `signoz`.

## Authenticate

For CI and short-lived shells, use environment variables. Keep the API key in the CI provider's secret store and disable shell tracing.

```sh
set +x
export SIGNOZ_URL="https://your-signoz.example.com"
export SIGNOZ_API_KEY="...from a secret store..."
signoz auth check --output json
```

For workstation use, profiles store only the instance URL in the operating-system config directory. API keys are stored in the operating-system keyring.

```sh
signoz profile add production --url https://your-signoz.example.com
signoz profile use production
printf '%s' "$SIGNOZ_API_KEY" | signoz auth login --api-key-stdin
signoz auth check --output json
```

There is intentionally no `--api-key` flag, so the secret does not enter shell history or process listings.

## Query SigNoz

```sh
# Discover metrics
signoz --output json metrics list

# Query a metric for the last hour
signoz --output json metrics query http.server.duration \
  --since 1h

# Search logs
signoz --output json logs search \
  --since 1h --filter "severity_text = 'ERROR'"

# Find traces for a service
signoz --output json traces search \
  --since 30m --filter 'service.name = "checkout"'

# Send a complete Query Builder v5 request
signoz --output json query run --file request.json
printf '%s' "$QUERY_JSON" | signoz --output json query run --file -
```

Use `--since` with `ms`, `s`, `m`, `h`, or `d`, or provide both `--start` and `--end` as RFC3339 timestamps or `@<unix-ms>`. The metric query command reads the metric catalog first and derives safe time/spatial aggregation from its type, temporality, and monotonicity; `--aggregation` only overrides the spatial aggregation. Raw searches and ordinary resource lists accept `--limit` and `--offset`; the JSON `meta` reports the resolved time window and pagination state. Log/trace aggregate commands return a single ungrouped scalar, do not accept offsets, and require `--aggregate-on` for functions other than `count` and `rate`. Alert-rule history uses `--cursor`/`meta.nextCursor` instead of offsets. The time-windowed metrics catalog has a limit but no offset: narrow it with `--since`, `--search`, or `--source meter`; a full page is marked `hasMore: true` with no `nextOffset`. Run `signoz --help` and `signoz <group> <command> --help` for the complete command surface.

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

`--raw` emits only the command data payload without the versioned envelope (direct commands keep the upstream shape; composite and client-paginated commands may combine or slice it). Diagnostics and errors are written to stderr; stdout remains clean for pipelines. `--output auto` uses a table only when stdout is a terminal.

## Command groups

```text
version
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

The same skill directory is packaged for both agents at `plugins/signoz-cli/skills/signoz-cli`.

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

The route map and behavior were derived from the current official SigNoz MCP server and Agent Skills repositories. See the design record in [`docs/superpowers/specs/2026-07-22-signoz-rust-cli-agent-integration-design.md`](docs/superpowers/specs/2026-07-22-signoz-rust-cli-agent-integration-design.md).
