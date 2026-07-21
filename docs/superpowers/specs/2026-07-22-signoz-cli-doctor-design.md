# `signoz-cli doctor` Design

**Date:** 2026-07-22

## Goal

Add a deterministic diagnostic command that Codex, Claude Code, humans, and CI can use to distinguish local configuration, credential, network/API, and authentication failures without exposing secrets.

## Chosen interface

```console
signoz-cli doctor [GLOBAL OPTIONS]
```

`doctor` accepts the existing global connection and output flags. It uses the same connection precedence and timeout as every remote command and makes one read-only request to the existing, supported `/api/v1/service_accounts/me` endpoint.

The JSON data payload is an ordered array of exactly five checks:

1. `version`
2. `configuration`
3. `credentials`
4. `api`
5. `authentication`

Each row has stable `name`, `status`, and `message` fields. Status is one of `pass`, `fail`, or `skipped`. The array shape also produces a useful table with the existing renderer.

## Execution model

Checks run in dependency order. A failed prerequisite marks dependent checks `skipped`; independent earlier checks remain visible.

- `version` always passes and identifies the running CLI version.
- `configuration` loads the config, applies connection precedence, and validates the selected URL/profile without reading an API key.
- `credentials` resolves a non-empty key from the environment or keyring without ever serializing it.
- `api` constructs the client and sends the authentication probe. A received HTTP response proves connectivity; an HTTP/API failure is reported here.
- `authentication` validates a successful identity response. HTTP 401/403 fails authentication while API connectivity passes. A malformed successful response also fails authentication as an incompatible response.

Configuration and credential resolution will be split internally into two stages. The current combined `ConfigFile::resolve` remains as the compatibility wrapper, so all existing commands retain their exact precedence and behavior.

## Output and exit contract

A completed diagnostic always writes its report to stdout using the normal versioned success envelope (`command: "doctor"`) or raw/table projection, even when a check fails. Stderr remains empty unless serializing the report itself fails or the caller explicitly requests the existing global `--verbose` diagnostic summary. This deliberate exception lets automation inspect every completed check.

After writing the report, the process exits using the existing stable failure categories:

- `0`: all checks pass
- `3`: configuration, missing credentials, unauthorized, or forbidden
- `4`: network or timeout
- `5`: rejected/not-found/rate-limited request
- `6`: upstream failure
- `7`: unsupported or incompatible response

The `DispatchResult` carries an optional failure category so normal commands continue to return exit 0 after successful output while `doctor` can emit a report and then exit nonzero.

## Safety

- The API key is represented only by `Credentials`; it is never included in report data, metadata, debug output, or check messages.
- Server error bodies keep the transport's existing API-key redaction.
- Doctor uses fixed, conservative messages rather than copying local parser/keyring errors that might include sensitive source text.
- The probe is read-only, has the existing redirect protection, retry policy, response limit, and timeout.
- No new or inferred SigNoz endpoint is introduced.

## Agent integration

The shared Agent Skill will prescribe `signoz-cli doctor --output json` as the preferred preflight/troubleshooting command, explain its stdout/nonzero-exit behavior, and keep `version` plus `auth check` available for narrow checks. The deterministic integration validator will require the doctor workflow token, covering both the Codex plugin and Claude Code plugin because they share the same Skill source.

## Alternatives considered

1. **Alias `doctor` to `auth check`.** Small, but cannot distinguish missing configuration, credentials, connectivity, or incompatible responses.
2. **Return at the first ordinary error.** Preserves the normal error envelope but prevents users and agents from seeing the full diagnostic state.
3. **Probe several telemetry endpoints.** Broader, but adds load and makes the command depend on version-specific APIs and data availability. The existing identity endpoint is enough to validate connection and authentication deterministically.

The staged single-probe report is selected because it provides the best diagnostic signal with the smallest stable API surface.
