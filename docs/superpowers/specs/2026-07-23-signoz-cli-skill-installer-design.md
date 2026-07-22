# SigNoz CLI Skill Installer Design

## Goal

Add a local `signoz-cli skills install` command that installs the repository's shared `signoz-cli` Skill for Codex and Claude Code. With no target option it installs both; `--target codex` and `--target claude` install only one.

## Chosen approach

Embed the canonical files from `plugins/signoz-cli/skills/signoz-cli` in the Rust binary at compile time and copy them into each agent's user Skill directory.

The alternatives were rejected for these reasons:

- Shelling out to `codex plugin` and Claude Code `/plugin` commands would require both clients to be installed, configured, and possibly authenticated. It would also install namespaced plugin Skills instead of the requested standalone user Skill.
- Symlinking the repository Skill would make installations depend on the source checkout remaining at the same path and would not work from a distributed binary.

Direct installation keeps one canonical Skill source in the repository while making the released `signoz-cli` binary self-contained.

## Command interface

```text
signoz-cli skills install [--target <all|codex|claude>]
```

- `--target all` is the default and installs Codex first, then Claude Code.
- `--target codex` installs only Codex.
- `--target claude` installs only Claude Code.
- The command is local and must not load SigNoz configuration, credentials, or make network requests.
- Existing global output controls continue to apply. JSON output uses command name `skills.install` and returns one result object per selected agent.

Each result contains:

```json
{
  "agent": "codex",
  "path": "/home/user/.agents/skills/signoz-cli",
  "status": "installed"
}
```

`status` is `installed` when the Skill was absent, `updated` when embedded content replaced older content, and `unchanged` when every managed file already matched. Re-running the command is therefore safe and observable.

## Target discovery

- Codex: `$HOME/.agents/skills/signoz-cli`, matching current Codex user-Skill discovery.
- Claude Code: `$CLAUDE_CONFIG_DIR/skills/signoz-cli` when `CLAUDE_CONFIG_DIR` is non-empty; otherwise `$HOME/.claude/skills/signoz-cli`.
- Home discovery checks `HOME` first so CI and tests can isolate writes, then uses the platform home directory returned by the Rust `directories` crate.
- A missing home directory or an unusable target is a configuration error with exit code 3.

## Managed files and write behavior

Both targets receive `SKILL.md`. Codex also receives `agents/openai.yaml`, which carries Codex presentation and invocation metadata. The installer creates missing directories and changes only those managed files; it preserves unrelated files in the target Skill directory.

For each managed file, the installer compares bytes before writing. Changed content is written through a temporary file in the destination directory and persisted to the final path, avoiding a partially written `SKILL.md`. Paths reported in errors identify the failed target but never include credentials.

## Failure behavior

Target paths are resolved before writes begin. If a selected target cannot be resolved, no target is changed. Filesystem failures use the existing structured configuration error and exit code 3. If a filesystem error occurs after another target was installed, the earlier installation remains valid and the error identifies the path that needs attention.

## Testing

- Clap tests prove that the `skills` group exists, the default target is `all`, and unsupported targets fail as validation errors.
- CLI integration tests set an isolated `HOME` and `CLAUDE_CONFIG_DIR`, then verify default dual installation, Codex-only and Claude-only selection, file contents, JSON results, and independence from broken SigNoz configuration.
- Installer unit tests cover `installed`, `updated`, `unchanged`, preservation of unrelated files, and Claude config-directory precedence.
- Agent integration validation requires the shared Skill to advertise the new installer command and confirms that the embedded source files remain the canonical plugin files.

## Documentation

README command listings and installation guidance will lead with the new one-command flow while retaining the marketplace instructions for users who prefer plugin lifecycle management. The shared Skill command reference will include `signoz-cli skills install` without changing its narrow trigger boundary for remote SigNoz automation.
