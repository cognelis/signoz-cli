# SigNoz CLI skill baseline

The baseline was run in an isolated agent context without the repository skill.

For the explicit CI/log prompt, the agent produced this command:

```sh
signoz-cli logs query --since 1h --filter 'severity_text = "ERROR"' --output json
```

Observed failures:

- It invented `logs query`; the real command is `logs search`.
- It used double quotes for the Query Builder string literal; valid filters use single-quoted values such as `severity_text = 'ERROR'`.
- It did not run version or authentication preflight.
- It had no `meta.hasMore` / `meta.nextOffset` pagination rule.
- It assumed a specific CI provider before the user named one.

After strengthening and reloading the skill, the same isolated evaluation emitted both mandatory preflights (`signoz-cli version --output json` and `signoz-cli auth check --output json`) before `signoz-cli --output json logs search --since 1h`, injected `SIGNOZ_API_KEY` only through the environment, disabled shell tracing, followed `meta.hasMore`/`meta.nextOffset`, and warned that retained telemetry artifacts may contain sensitive data.
