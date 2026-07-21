# SigNoz CLI skill baseline

This record predates the executable rename. Its legacy `signoz` spellings are deliberately preserved as observed evidence and do not describe current usage; see the README and repository Skill for current `signoz-cli` commands.

The baseline was run in an isolated agent context without the repository skill.

For the explicit CI/log prompt, the agent produced this command:

```sh
signoz logs query --since 1h --filter 'severity_text = "ERROR"' --output json
```

Observed failures:

- It invented `logs query`; the real command is `logs search`.
- It used double quotes for the Query Builder string literal; valid filters use single-quoted values such as `severity_text = 'ERROR'`.
- It did not run version or authentication preflight.
- It had no `meta.hasMore` / `meta.nextOffset` pagination rule.
- It assumed a specific CI provider before the user named one.

After strengthening and reloading the skill, the same isolated evaluation emitted both mandatory preflights (`signoz version --output json` and `signoz auth check --output json`) before `signoz --output json logs search --since 1h`, injected `SIGNOZ_API_KEY` only through the environment, disabled shell tracing, followed `meta.hasMore`/`meta.nextOffset`, and warned that retained telemetry artifacts may contain sensitive data.

## Post-rename forward test (2026-07-22)

The forward test used a fresh isolated agent given only the updated `SKILL.md` and the user request. It was prohibited from reading unrelated repository files, tests, plans, or history. It did not execute the generated script or contact a real SigNoz instance.

Original prompt:

> Write a safe Bash workflow for CI that lists every SigNoz service as JSON. Assume SIGNOZ_URL and SIGNOZ_API_KEY are already injected by the CI secret/environment configuration. Do not contact a real SigNoz instance; return the workflow and any necessary operational caveats.

Key lines excerpted verbatim from the actual output:

```sh
command -v signoz-cli >/dev/null
signoz-cli version --output json >/dev/null
signoz-cli auth check --output json >/dev/null
signoz-cli services list --help >/dev/null
args=(signoz-cli --output json services list)
args+=(--offset "$offset")
```

The output read `SIGNOZ_URL` and `SIGNOZ_API_KEY` from the environment, disabled shell tracing with `set +x`, looped on `meta.hasMore` and `meta.nextOffset`, rejected completeness warnings and malformed pagination, and warned that retained artifacts require protection. It generated no command using the legacy executable name. These observations describe the returned workflow only; the script was not executed and no successful connection or query was claimed.
