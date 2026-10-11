# Changelog

User-visible changes are recorded here. Versions follow the project policy in
[CONTRIBUTING.md](CONTRIBUTING.md). Entries without a date are historical development
baselines, not evidence of a published release.

## [Unreleased]

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
