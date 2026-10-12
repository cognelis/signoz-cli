# signoz-cli

`signoz-cli` is a Rust command-line client for repeatable, read-only SigNoz queries in terminals, CI jobs, and JSON pipelines. The repository also ships one Agent Skill and plugin manifests so Codex and Claude Code can use the CLI without inventing commands or exposing credentials.

For general agent-led observability work, use the official [SigNoz Agent Skills](https://github.com/SigNoz/agent-skills) with the official [SigNoz MCP server](https://github.com/SigNoz/signoz-mcp-server). This repository's `signoz-cli` skill is deliberately narrow: it activates for explicit CLI, shell, CI, and JSON automation requests.

## Build and install

Install the prebuilt CLI through npm:

```sh
npm install -g @cognelis/signoz-cli
signoz-cli version --output json
```

The npm installer verifies the matching release binary's SHA-256. macOS, Windows,
and glibc-based Linux builds are provided for x64 and ARM64; see the
[npm package requirements](build/npm/README.md). No Rust toolchain is required.

To build from source:

Rust stable is required.

```sh
cargo build --release
cargo install --path crates/signoz-cli
signoz-cli version --output json
```

The binary is named `signoz-cli`.

See [CHANGELOG.md](CHANGELOG.md) for version history and migration notes, and
[CONTRIBUTING.md](CONTRIBUTING.md) for the versioning and release workflow.
The [publishing guide](docs/releasing.md) covers release assets, npm permissions,
and trusted publishing.

## Shell completion

Generate completion scripts for zsh, bash, fish, PowerShell, or elvish from the
same command definitions as the CLI. They cover subcommands, aliases, options,
declared enum values (such as `--output`), and query `--file` paths. Generation
requires no configuration, API key, or SigNoz connection.

For the current zsh session:

```zsh
autoload -Uz compinit && compinit
source <(signoz-cli completion zsh)
```

For persistent zsh completion, save the script in a user-owned directory:

```zsh
mkdir -p ~/.zfunc
signoz-cli completion zsh > ~/.zfunc/_signoz-cli
```

Add `~/.zfunc` to `fpath` **before** the existing `compinit` call in `~/.zshrc`:

```zsh
fpath=("$HOME/.zfunc" $fpath)
autoload -Uz compinit && compinit
```

Start a new shell after configuring it. Other shell examples:

```sh
# bash, current session
source <(signoz-cli completion bash)
# Generate scripts for other shells, then load them using that shell's startup setup
signoz-cli completion fish > signoz-cli.fish
signoz-cli completion powershell > signoz-cli.ps1
signoz-cli completion elvish > signoz-cli.elv
```

Saved scripts are static: regenerate them after upgrading the CLI. Profile names,
metric names, and remote resources are not dynamically queried when pressing Tab.
Menu candidates use short descriptions; detailed explanations remain available
through `--help`. After regenerating a saved zsh script, source that file again
in the current shell or start a new shell to replace its previously loaded functions.
Arbitrary filters and other free-form values do not gain semantic completion.
`completion` always writes shell code to stdout, even with `--output json` or
`--raw`; it must not be parsed as a JSON response. Run `signoz-cli completion --help`
for supported shells and a quick zsh setup example.

## Authenticate

For a workstation, start the guided setup. It creates or repairs a profile, masks the API key while typing, verifies the connection before saving, stores the key in the operating-system credential store, activates the profile, and installs the bundled Skill for Codex and Claude Code by default.

```sh
signoz-cli init
```

The setup is deliberately terminal-only and never starts implicitly. If input or output is redirected it exits without changing configuration and prints the equivalent non-interactive commands.

For CI and short-lived shells, use environment variables. Keep the API key in the CI provider's secret store and disable shell tracing.

```sh
set +x
export SIGNOZ_URL="https://your-signoz.example.com"
export SIGNOZ_API_KEY="...from a secret store..."
signoz-cli doctor --output json
```

For scripts that intentionally persist a profile, profiles store only the instance URL in the operating-system config directory. API keys are verified before they are stored in the operating-system keyring.

```sh
signoz-cli profile add production --url https://your-signoz.example.com --description "Production observability"
signoz-cli profile use production
printf '%s' "$SIGNOZ_API_KEY" | signoz-cli auth login --api-key-stdin
signoz-cli doctor --output json
```

Running `signoz-cli auth login` without `--api-key-stdin` opens a masked prompt. Use `--no-verify` only for an intentional offline setup; the JSON result records a warning. There is intentionally no `--api-key` flag, so the secret does not enter shell history or process listings. `signoz-cli config show` never includes credentials, and `signoz-cli config path` remains usable even if the config file is malformed.

Keep test and production instances in separate profiles. Select a profile for one command with `--profile`, or change the default with `profile use`:

```sh
signoz-cli --profile test init
signoz-cli --profile prod init
signoz-cli profile use test
signoz-cli --profile prod profile show
signoz-cli --profile prod doctor --output json
```

`profile show <name>` selects the positional name first, then `--profile`, then the default profile. Connection URLs follow `--url`, `--profile`, `SIGNOZ_URL`, then the default profile. `SIGNOZ_API_KEY` overrides the selected profile's saved key; unset it to use that profile's credential.

Changing an instance address with `profile update <name> --url <url>` clears its saved API key before saving the new URL. Authenticate again with `signoz-cli --profile <name> auth login`. An equivalent normalized URL preserves the key. If the credential store cannot clear the old key, the URL is not changed.

`profile add` and `profile update` accept `--description` to record what an environment is for; `profile list` and `profile show` return it. A description is trimmed and must be one line of at most 200 characters, `--description ""` clears it, and changing only the description keeps the saved API key. The bundled skill chooses a profile by its name and description: it uses a profile the user names, matches an environment or purpose that the request mentions against `profile list`, asks the user when several profiles or none could match, and uses the default profile only when the request names no environment.

## Diagnose a connection

Run the read-only diagnostic before remote automation or when a connection fails:

```sh
signoz-cli doctor --output json
```

It checks the CLI version, connection configuration, credential availability, API connectivity, and service-account authentication in order. It sends one request to the same identity endpoint used by `auth check` and never emits the API key.

A completed doctor run always writes its ordered `pass`/`fail`/`skipped` check array to stdout. An unhealthy result still exits with the matching nonzero code below, so scripts should capture stdout before testing the status. In JSON and raw modes, stderr stays empty unless `--verbose` is explicitly requested. `auth check` remains available when only the authenticated service-account identity is needed.

## Query SigNoz

```sh
# Discover metrics
signoz-cli --output json metrics list

# Query a metric for the last hour
signoz-cli --output json metrics query http.server.duration \
  --since 1h --filter 'service.name = "checkout"' \
  --group-by service.name --step 60

# Search logs
signoz-cli --output json logs search \
  --since 1h --filter "severity_text = 'ERROR'"

# Find traces for a service
signoz-cli --output json traces search \
  --since 30m --filter 'service.name = "checkout"'

# Group exceptions and inspect infrastructure
signoz-cli --output json exceptions list --service checkout --since 24h
signoz-cli --output json infra list hosts --status inactive --since 1h

# Summarize all five Cost Meter ingestion metrics
signoz-cli --output json meters summary --since 7d --group-by service.name

# Validate, then run a complete Query Builder v5 request
signoz-cli --output json query preview --file request.json
signoz-cli --output json query run --file request.json
printf '%s' "$QUERY_JSON" | signoz-cli --output json query run --file -
```

Use `--since` with `ms`, `s`, `m`, `h`, or `d`, or provide both `--start` and `--end` as RFC3339 timestamps or `@<unix-ms>`. The metric query command reads the metric catalog first and derives safe time/spatial aggregation from its type, temporality, and monotonicity; `--aggregation` only overrides the spatial aggregation. Metrics, log aggregates, and trace aggregates support filtering and grouping; aggregate `--step` switches scalar/grouped output to a time series. Raw searches and ordinary resource lists accept `--limit` and `--offset`; the JSON `meta` reports the resolved time window and pagination state. Alert-rule history uses `--cursor`/`meta.nextCursor` instead of offsets. The time-windowed metrics and field catalogs have a limit but no offset; narrow an incomplete response with their search/context/type/source/time filters. Run `signoz-cli --help` and `signoz-cli <group> <command> --help` for the exact command surface.

`metrics query --source meter` uses the meter source for both catalog metadata and the query. `metrics inspect` defaults to the last 30 minutes and rejects any time range longer than 30 minutes.

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

Table output reports the selected profile, partial-result status, pagination hints, and warnings on stderr. JSON retains these fields in `meta`; `--raw` emits only the data. When a downstream consumer such as `head` closes stdout early, successful commands exit cleanly without a broken-pipe panic.

The terminal display expands nested result collections into rows and sections. Columns follow a predictable order and include fields that appear only in later rows. Configuration uses key/value details; profile lists mark the default with `*`. Column widths account for Chinese and other wide characters. The display follows `COLUMNS` or the terminal width (100 columns when unavailable), and switches to per-record details when columns cannot fit. Multiline text and control characters are escaped for display. Shortened values end in `…` and produce a hint to use `--output json` for the full values; JSON and raw payloads are never shortened by the display.

`--output auto` selects both success and error formatting from whether stdout is a terminal. Explicit `--output table` keeps human-readable output even when captured, while `--output json` keeps structured output. `--raw` always selects JSON data output. Scripts and agents should request `--output json` and read the exit code and metadata instead of parsing table layout or treating all stderr text as a failure.

## Command groups

```text
version
completion <bash|elvish|fish|powershell|zsh>
init
doctor
config path|show
skills install [--target all|codex|claude]
auth login|check|logout
profile add|update|list|show|use|remove
metrics list|query|top|cardinality|usage|metadata|highlights|inspect
fields keys|values
logs search|aggregate
traces search|aggregate|get
services list|operations|dependencies
exceptions list|count|group|get|navigate
infrastructure (infra) check|list
meters summary|query
alerts list
rules list|get|history|history-stats|history-status|history-contributors
dashboards list|get
views list|get
channels list|get
query run|preview
```

The client retries replay-safe requests on transport failures, HTTP 429, 502, 503, and 504, for at most three attempts. Responses are capped at 64 MiB. Feature-specific 404 responses report an unsupported server capability instead of pretending that a resource is missing. Public v2/v5 routes are used for dashboards, metrics, Query Builder, rule history, and infrastructure; services, dependency maps, exceptions, and Explorer views are explicitly maintained as first-party UI/MCP compatibility adapters because SigNoz has not published equivalent stable read contracts for those views.

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
- Metric inspect accepts at most a 30-minute window.
- Cost Meter queries default to seven days, use the documented `meter` source, and enforce a minimum one-hour step.
- Service, dependency, exception, and Explorer-view commands may be unavailable when an internal SigNoz compatibility endpoint changes; the CLI returns exit code 7 in that case.
- The CLI accepts self-hosted HTTP origins but does not provide an insecure TLS bypass.
- Reverse-proxy base paths such as `https://example.com/observability/signoz` are preserved.
- Telemetry can contain secrets or personal data. Review logs and traces before storing them as CI artifacts.

The route map and behavior were derived from current first-party SigNoz documentation, OpenAPI, frontend code, MCP server, and Agent Skills. See the [TUI and API coverage research](docs/research/2026-07-23-tui-and-signoz-api-coverage.md), the main [integration design](docs/superpowers/specs/2026-07-22-signoz-rust-cli-agent-integration-design.md), and the [`doctor` design](docs/superpowers/specs/2026-07-22-signoz-cli-doctor-design.md).
