# Changelog

User-visible changes are recorded here. Versions follow the project policy in
[CONTRIBUTING.md](CONTRIBUTING.md). Entries without a date are historical development
baselines, not evidence of a published release.

## [Unreleased]

### Added

- Time-window commands support `--since DURATION --end TIMESTAMP` for a
  reproducible historical lookback. `--since` still conflicts with `--start`;
  `--end` alone remains invalid. Cost Meter keeps only complete hours inside
  explicitly anchored windows, as it does for absolute start/end ranges.

- `query build` and `query validate` accept `--out PATH` to atomically save
  the raw payload while retaining normal stdout output. Validation completes before
  replacement, so validate can safely normalize an input file in place. Unix output
  files are owner-only; symlink destinations are replaced without following them.

- `query build` now supports `--aggregation`, `--aggregate-on`, repeated
  `--group-by`, and `--step` for offline scalar, grouped, and time-series log/trace
  requests. It reuses execution builders and validation; aggregation refuses
  raw-only `--search`/`--offset`, and dependent flags require `--aggregation`.

- `query validate --file <path|->` checks Query Builder v5 structure offline,
  using the same validation as preview/run, and prints the normalized payload.
  Missing `schemaVersion` becomes `v1`; input files are never rewritten.

- `query build --signal logs|traces` builds a local Query Builder v5 search
  payload using the same builders as execution. Time windows are resolved once;
  `--raw` output can be saved and passed to `query preview` or `query run`.
  No connection configuration, credentials, or network access is needed.


## [0.8.0] - 2026-11-11

### Added

- `skills status [--target all|codex|claude]` performs an offline, read-only
  inspection of managed Skill installations. It reports installed/bundled
  versions and changed, missing, or unreadable files, including same-version
  edits and missing references, without modifying user files.
- The core Skill routes local maintenance directly to offline commands and
  reserves profile/doctor preflight for data workflows. It describes the
  inspect, review, refresh, verify, and context-reload sequence.

## [0.7.2] - 2026-11-11

### Fixed

- Views and channels lists reject non-array `data` as an incompatible response
  (exit 7), instead of silently replacing it with an empty list and reporting
  zero items. Missing, null, and empty-array legacy responses remain supported.

## [0.7.1] - 2026-11-11

### Fixed

- Offset-paginated responses no longer advertise a `nextOffset` that repeats the
  current page after an empty response or integer overflow. `hasMore` is preserved
  and a warning asks callers to narrow the time window or filter.

## [0.7.0] - 2026-11-11

### Changed

- Record the maintainer as `dnvyrn <dnvyrn@gamil.com>` and document Codex/Claude
  collaboration. Future commits include meaningful rationale and validation,
  with AI co-author trailers only for actual participants; history is unchanged.

### Added

- Doctor now appends informational package-release and companion-Skill version
  checks, preserving connection health, exit codes, existing check order, and
  display layout. Release lookup is bounded to five seconds and can be skipped
  with `doctor --no-update-check`.
- Bundled Skills carry synchronized version metadata. Doctor distinguishes
  loaded and installed versions, detects stale or legacy unversioned copies,
  and suggests CLI upgrade, Skill refresh, and agent-context reload in order.
  Refresh legacy installations with `skills install` after upgrading.

## [0.6.5] - 2026-11-11

### Fixed

- Service and exception compatibility requests preserve exact millisecond-to-
  nanosecond conversion across the full input range instead of silently clamping
  timestamps to signed 64-bit limits. Server-side range restrictions still apply.

## [0.6.4] - 2026-11-11

### Fixed

- Resource identifiers encode spaces as `%20` in URL paths, keeping spaces
  distinct from literal `+` characters for rules, dashboards, views, and channels.
- Update the locked transitive `chacha20` dependency from the yanked 0.10.1 to
  0.10.2, matching the compatible dependency fix validated in mongodb-cli.

## [0.6.3] - 2026-11-11

### Fixed

- Replay-safe requests now retry interrupted response body reads within the
  existing three-attempt limit and total timeout. Non-replay-safe requests,
  oversized responses, and complete but invalid JSON responses are not retried.

## [0.6.2] - 2026-11-11

### Fixed

- Relative time windows reject integer underflow instead of panicking or wrapping.
  Cost Meter queries safely align extreme timestamps to complete hours and return
  validation errors when hourly boundaries cannot be represented, rather than
  overflowing or silently clamping the requested range.

## [0.6.1] - 2026-10-12

### Changed

- `alerts list --help` says it returns the alert instances Alertmanager holds now,
  not rule definitions or past alerts.
- The bundled skill explains how to answer questions about recent alerts with
  `rules list` and `rules history-stats`, and that `services list` shows only
  services that send traces.

## [0.6.0] - 2026-10-12

### Added

- A choice of credential store: `config set credential-store keychain|security|file`,
  or `SIGNOZ_CLI_CREDENTIAL_STORE`. `security` uses macOS `/usr/bin/security`, so
  keychain approvals survive CLI upgrades. `file` keeps API keys in an owner-only
  file and needs confirmation in an interactive terminal. `--migrate` moves existing
  API keys to the new store.

### Changed

- `doctor` reports where the API key came from.
- The bundled skill triggers for SigNoz investigations unless the SigNoz MCP server
  is configured, with a description short enough for agents' skill lists. It uses a
  production profile only when the request names production, answers questions
  about a whole environment from each of its profiles, runs `doctor` once per
  profile in a session, matches both spellings of log severities, and documents the
  result paths of common commands and the units of `services list`.

## [0.5.0] - 2026-10-12

### Added

- Profile descriptions: `profile add|update --description` records what an
  environment is for, and `profile list` and `profile show` return it. `profile
  update` now needs `--url`, `--description`, or both, and a description change
  keeps the saved API key. The bundled skill chooses a profile by its name and
  description, and asks the user when several profiles or none could match.

## [0.4.2] - 2026-10-12

### Changed

- Release binaries no longer include a symbol table, which makes them about a
  quarter smaller. Panic messages still name the source file and line; only
  `RUST_BACKTRACE` call stacks lose their function names.

### Fixed

- The npm installer no longer fails on slow connections. A binary download now
  fails only after 30 seconds without receiving data or 10 minutes in total,
  instead of having to finish within 30 seconds.

## [0.4.1] - 2026-10-12

### Fixed

- On a terminal, usage errors and the help shown for a missing subcommand, such as
  `signoz-cli` alone, keep their line breaks instead of printing as one line with
  escaped newlines.

## [0.4.0] - 2026-09-06

### Added

- npm distribution as `@cognelis/signoz-cli`, with a native launcher, strict
  SHA-256 verification, bounded HTTPS downloads, and cache-corruption detection.
- Native release builds and smoke checks for macOS, Linux, and Windows on x64
  and ARM64, plus a complete-assets gate before npm publishing.
- GitHub Actions validation and release workflows, a first-publish guide, and
  version synchronization for the npm package alongside the Rust crates and plugins.

## [0.3.1] - 2026-09-06

### Fixed

- Completion menus now give every command and option a concise description, so
  zsh no longer packs undescribed commands into an uneven grid or pushes long
  explanations off-screen. Detailed help remains available through `--help`.
- Shortened generated help-command descriptions and documented how to reload
  saved zsh scripts after upgrading.

## [0.3.0] - 2026-09-06

### Added

- `completion <shell>` generates zsh, bash, fish, PowerShell, and elvish completion
  scripts for subcommands, aliases, options, declared enum values, and query file
  paths. It works without configuration, credentials, or network access.
- Documented current-session and persistent zsh setup, static completion limits,
  and the need to regenerate saved scripts after upgrades.

### Changed

- Completion generation always emits shell code directly on stdout, including
  with `--output json` or `--raw`. Other commands retain their output contracts.

## [0.2.0] - 2026-09-06

### Added

- Human-readable terminal tables with nested result sections, predictable column
  ordering, Unicode-aware alignment, narrow-terminal record details, and visible
  shortening hints. JSON and raw data remain complete at the display layer.
- Default-profile markers in profile lists and key/value configuration displays.
- Release preparation and consistency checks, with a documented versioning and
  changelog workflow for contributors and coding agents.

### Changed

- Success and error formatting now both use stdout's terminal status in automatic
  mode. Explicit `--output table` remains human-readable when captured;
  `--output json` and `--raw` provide structured output for automation.
- Table mode shows the selected profile, time window, pagination status, and
  warnings on stderr. Multiline and terminal control characters are escaped.
- `metrics inspect` defaults to the last 30 minutes, matching its maximum window.
- The bundled Skill documents environment selection, credential overrides,
  output parsing, fixed time windows for pagination, and display shortening.

### Fixed

- Changing a profile's normalized URL clears its saved API key before saving the
  new destination. Credential-store failure prevents the URL change, while an
  equivalent URL preserves the credential.
- `profile show` honors selection in the order positional name, `--profile`, then
  persisted default.
- `metrics query --source meter` passes the source to both metadata discovery and
  the final query.
- Downstream consumers closing stdout early no longer cause a broken-pipe panic.
- Tables retain fields appearing only in later rows and display completeness
  warnings instead of silently omitting them.

### Migration notes

- After changing a profile URL, run `signoz-cli --profile <name> auth login` to
  store a key for the new instance. `SIGNOZ_API_KEY` still explicitly overrides
  saved credentials.
- Scripts should use `--output json`, inspect the exit code, and retain `meta`
  when completeness matters. Human-readable table layout is not a parsing API.
- Rust callers must adapt to `Operation::metrics_search`'s new source argument,
  `ConfigFile::update_profile`'s credential-store argument and result type, and
  `emit_success` returning `io::Result<()>`. The onboarding-only `upsert_profile`
  helper is now crate-private.
- JSON envelope `schemaVersion` remains `1`; it is versioned independently from
  the executable and plugin release number.

## [0.1.0]

- Historical development baseline: authenticated read-only SigNoz commands,
  named profiles, guided setup, connection diagnostics, JSON envelopes, and
  bundled Codex/Claude Code Skill installation. No release tag or publication
  date was recorded for this baseline.
