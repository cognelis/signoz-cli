# SigNoz CLI: onboarding TUI and API-coverage research

**Date:** 2026-07-23

**Scope:** read-oriented `signoz-cli` for SigNoz Cloud and self-hosted SigNoz

**Decision horizon:** the current CLI plus SigNoz v0.133.0, the latest version listed by the official API reference when this report was written

**Implementation status:** this report is the pre-implementation baseline. The accompanying change set implements its guided `init`, dashboard v2 migration, query preview, rule-history extensions, dependency/exception compatibility adapters, all listed infrastructure resources, Cost Meter commands, metric inspection, field discovery filters, and grouped metric/log/trace queries. Tables below retain the original baseline wording so the evidence and prioritization remain auditable.

## Executive recommendation

Build one deliberately small TUI: `signoz-cli init`. It should create or repair one profile, collect an API key through a masked prompt, validate the URL and identity in memory, commit configuration and credentials only after validation, select the profile, and finish by running the same checks as `doctor`. Do not turn missing configuration on an ordinary command into a surprise full-screen UI. Do not invent browser authentication: SigNoz's documented direct API authentication is an API key, while the browser/OAuth flow documented for the hosted SigNoz MCP server is a different product boundary.

Keep every operational command script-first. In a non-interactive terminal, `init` must fail without writing anything and print an exact command recipe using `profile add`, `auth login --api-key-stdin`, and `doctor`. `SIGNOZ_URL` plus `SIGNOZ_API_KEY` remains the zero-persistence CI path.

For API breadth, first complete two end-to-end jobs rather than adding isolated list commands:

1. **Incident triage:** alert/rule/history → service/operation → trace/log correlation → exception group/detail and dependency edges.
2. **Estate and spend:** infrastructure checks and host/Kubernetes inventory → Cost Meter ingestion summary.

Prefer endpoints present in the released, versioned SigNoz OpenAPI document. Put first-party but unpublished UI endpoints behind a named compatibility adapter and a capability check. Never imply that `/api/v1` or `/api/v2` in a path makes an endpoint public; publication in the versioned OpenAPI specification or an official API guide is the contract boundary.

## Evidence and confidence model

This report uses only vendor documentation and first-party source repositories. The SigNoz API reference is versioned and currently lists v0.133.0; SigNoz announced auto-generated OpenAPI publication in v0.105.1. Those are the strongest sources for a supported HTTP contract ([versioned API reference](https://signoz.io/api-reference/), [OpenAPI announcement](https://signoz.io/changelog/2025-12-17-openapi-specification-auto-generation-m59rm12dvggh3k32p1zj4e6y/)).

The labels below mean:

- **Public:** present in the released [SigNoz v0.133.0 OpenAPI file](https://github.com/SigNoz/signoz/blob/v0.133.0/docs/api/openapi.yml) or described as an API in official product documentation.
- **Internal:** an exact route and shape are used by the SigNoz frontend or the first-party SigNoz MCP server, but the route is absent from that released OpenAPI file. The route is confirmed; its compatibility is not promised.
- **Unknown:** the product capability is documented, but no public route and no sufficiently clear first-party route were confirmed. No route should be guessed.

An endpoint being confirmed in v0.133.0 means only “present by v0.133.0,” not “introduced in v0.133.0.” Minimum versions are stated only where an official source states them.

## What strong CLIs do on first run

### Source-backed comparison

| CLI | First-run and target selection | Authentication and secret handling | Validation, profiles, recovery, and automation | Lesson for `signoz-cli` |
|---|---|---|---|---|
| GitHub CLI (`gh`) | `gh auth login` is explicit. It defaults to `github.com` and accepts `--hostname` for GitHub Enterprise. Interactive login defaults to a browser/device-code flow (`--clipboard` copies the one-time code), with pasted-token input as an alternative ([official manual](https://cli.github.com/manual/gh_auth_login)). | `--with-token` reads from standard input; `GH_TOKEN`/`GH_ENTERPRISE_TOKEN` are recommended for headless automation. Credentials go to the system store when available, with an explicit plaintext fallback warning and `--insecure-storage` choice ([manual](https://cli.github.com/manual/gh_auth_login)). The pasted-token prompt is secret input in the [official login flow](https://github.com/cli/cli/blob/ae66a1c02e08366858f3070664f493afbe0cdf18/pkg/cmd/auth/shared/login_flow.go#L150-L204). | Token scope and current identity are checked before `authCfg.Login` persists the token ([source](https://github.com/cli/cli/blob/ae66a1c02e08366858f3070664f493afbe0cdf18/pkg/cmd/auth/login/login.go#L189-L218)). `gh auth switch` refuses an ambiguous non-TTY selection and requires an explicit user ([source](https://github.com/cli/cli/blob/ae66a1c02e08366858f3070664f493afbe0cdf18/pkg/cmd/auth/switch/switch.go#L123-L148)); `gh auth status` checks accounts and supports structured JSON ([manual](https://cli.github.com/manual/gh_auth_status), [source](https://github.com/cli/cli/blob/ae66a1c02e08366858f3070664f493afbe0cdf18/pkg/cmd/auth/status/status.go)). | Validate identity before persistence; keep host/profile selection explicit; ambiguous non-interactive state is an error, not a prompt.
| Google Cloud CLI (`gcloud`) | `gcloud init` detects an existing configuration and offers re-initialization, switching, or creating a new named configuration. A fresh install creates `default`; the flow chooses an account and project, and may set region/zone ([initialization guide](https://docs.cloud.google.com/sdk/docs/initialize)). `--console-only`/`--no-launch-browser` prints an authorization URL instead of opening it ([command reference](https://docs.cloud.google.com/sdk/gcloud/reference/init)). | Human credentials are stored under the gcloud configuration directory; Google recommends service accounts, credential files, or workload identity for automation rather than reusing local human credentials ([authentication guide](https://docs.cloud.google.com/sdk/docs/authenticate)). | Named configurations can be created, activated, and selected per command or through `CLOUDSDK_ACTIVE_CONFIG_NAME` ([configuration guide](https://docs.cloud.google.com/sdk/docs/configurations)). Scripts can use `--quiet`, structured `--format`, and exit status instead of parsing prose ([scripting guide](https://docs.cloud.google.com/sdk/docs/scripting-gcloud)). `gcloud config list` provides a direct post-setup check ([initialization guide](https://docs.cloud.google.com/sdk/docs/initialize)). Screen-reader mode replaces Unicode spinners and tables with textual status, percentages, and flattened records ([accessibility guide](https://docs.cloud.google.com/sdk/docs/enabling-accessibility-features)). | Treat a profile as a whole operating context, offer reconfigure/switch/new on repeat runs, and make accessibility a behavior mode rather than merely removing color.
| Fly.io CLI (`flyctl`) | `fly auth login` defaults to browser login and also supports interactive email/password/OTP ([official reference](https://fly.io/docs/flyctl/auth-login/)). If browser opening fails, it prints the URL for manual use; in a non-TTY it fails with a recipe to set `FLY_API_TOKEN` ([first-party source](https://github.com/superfly/flyctl/blob/b74c9391409b3e443383a5f4d928cef007825ddc/internal/command/auth/webauth/webauth.go#L78-L106)). | `FLY_API_TOKEN` overrides cached authentication and is the supported CI path; Fly recommends narrow, expiring, or read-only tokens where possible ([integration guide](https://fly.io/docs/flyctl/integrating/)). | Browser polling has a 15-minute expiry and returns a retry message, but the current source notes that its older poll loop does not break on interrupts ([source](https://github.com/superfly/flyctl/blob/b74c9391409b3e443383a5f4d928cef007825ddc/internal/command/auth/webauth/webauth.go#L112-L143)). More importantly, Fly persists the token before fetching the current user, so a validation failure can follow a saved credential ([source](https://github.com/superfly/flyctl/blob/b74c9391409b3e443383a5f4d928cef007825ddc/internal/command/auth/webauth/webauth.go#L23-L48)). `fly auth whoami` is the verification command ([reference](https://fly.io/docs/flyctl/auth/)). | Copy the good manual-browser and CI fallback, but not save-before-validate or cancellation-insensitive polling.
| Stripe CLI | `stripe login` presents a pairing code and browser URL; the documented `--interactive` mode accepts an API key for machines without browser access or for CI ([installation/login guide](https://docs.stripe.com/stripe-cli/install)). Current first-party source also provides a two-stage non-TTY protocol: `--non-interactive` emits JSON containing `browser_url`, `verification_code`, and `next_step`; `--complete` performs polling ([source](https://github.com/stripe/stripe-cli/blob/b1427863c0aff41a5d33b7486eb8455b80dea3e3/pkg/cmd/login.go#L45-L88), [implementation](https://github.com/stripe/stripe-cli/blob/b1427863c0aff41a5d33b7486eb8455b80dea3e3/pkg/login/login.go#L40-L84)). | Browser login creates restricted keys with a finite lifetime; the CLI accepts an explicit API key ([key guide](https://docs.stripe.com/stripe-cli/keys)). Manual key entry uses terminal password input and restores terminal state on interruption ([source](https://github.com/stripe/stripe-cli/blob/b1427863c0aff41a5d33b7486eb8455b80dea3e3/pkg/login/interactive_login.go#L96-L128)). Secure keyring storage is preferred; the CLI warns when using an unencrypted fallback, and its config file is forced to mode `0600` ([source](https://github.com/stripe/stripe-cli/blob/b1427863c0aff41a5d33b7486eb8455b80dea3e3/pkg/login/login.go#L16-L25), [config source](https://github.com/stripe/stripe-cli/blob/b1427863c0aff41a5d33b7486eb8455b80dea3e3/pkg/config/config.go#L102-L145)). | The browser completion response is saved through a profile configurer; current source supports listing and switching named profiles ([login persistence](https://github.com/stripe/stripe-cli/blob/b1427863c0aff41a5d33b7486eb8455b80dea3e3/pkg/login/keys/configurer.go), [profile source](https://github.com/stripe/stripe-cli/blob/b1427863c0aff41a5d33b7486eb8455b80dea3e3/pkg/config/config.go#L254-L361)). Manual login does an account lookup, but its current implementation can continue to profile creation after that lookup errors ([source](https://github.com/stripe/stripe-cli/blob/b1427863c0aff41a5d33b7486eb8455b80dea3e3/pkg/login/interactive_login.go#L36-L54)). | A machine-readable handoff is excellent when a browser protocol really exists; masked manual input and terminal restoration are mandatory. Do not copy best-effort validation or add a browser façade without server support.

### Cross-cutting findings

1. **The entry point is explicit.** These tools use `auth login` or `init`; ordinary commands do not unexpectedly take over the terminal.
2. **Target and identity are verified before success.** GitHub's ordering is the strongest model. Fly's save-before-current-user ordering is a concrete failure mode to avoid.
3. **Human and machine authentication are separate contracts.** A browser can be convenient for a person; stdin/environment/credential files are deterministic for CI.
4. **Profiles are first-class and selectable.** A named profile should include the endpoint and credential reference, and any per-command override must be visible in diagnostics.
5. **Cancellation is a state transition.** Ctrl+C must restore the terminal, stop polling/network work, and leave no partially committed profile.
6. **Accessibility changes information presentation.** Text status, percentages, flattened records, `NO_COLOR`, and no spinner-only state are more useful than an “accessible colors” claim.

## Recommended `signoz-cli init` experience

### State machine

```text
START
  ├─ stdin/stdout not interactive ──> NON_TTY_ERROR (exit 2, no writes, recipe)
  └─ interactive
       -> SELECT_INTENT (create | repair current | choose another profile)
       -> PROFILE_NAME
       -> INSTANCE_URL
       -> AUTH_METHOD
            └─ masked API key only today
       -> VALIDATE_IN_MEMORY
            ├─ malformed URL/base path -> back to INSTANCE_URL
            ├─ 401/403 -> re-enter key | change URL | cancel
            ├─ network/TLS/timeout -> retry | change URL | cancel
            └─ valid identity -> REVIEW
       -> COMMIT (config + credential as one recoverable transaction)
       -> SELECT_CURRENT_PROFILE
       -> OPTIONAL_SKILL_INSTALL
       -> DOCTOR
       -> SUCCESS (identity, URL, profile, next commands; never the secret)

From every pre-commit screen: Esc goes back, Ctrl+C cancels, and no state is saved.
```

### Detailed behavior

- **Repeat-run detection:** read the current profile and list existing profiles first. Offer “repair current,” “switch/reconfigure another,” and “create new.” Default a first profile to `default`; never silently overwrite a profile.
- **URL/environment selection:** accept SigNoz Cloud and self-hosted HTTPS URLs in one URL field. Normalize only safe syntax (scheme and trailing slash); preserve a deployment base path because SigNoz supports serving on an external base URL from v0.121.0 ([official configuration guide](https://signoz.io/docs/manage/administrator-guide/configuration/serving-on-external-url/)). If `SIGNOZ_URL` is set, show where it sits in the active precedence; when it would override the current saved profile, ask whether the user wants to copy it. Do not silently persist an environment value.
- **Authentication:** default to a masked API-key prompt. The official self-hosted MCP setup likewise instructs users to create an API key in SigNoz Settings ([first-party MCP README](https://github.com/SigNoz/signoz-mcp-server/blob/66af322c02d9a804f11c5184753a97031b9cd1f7/README.md#L204-L211)). The hosted MCP server's OAuth flow does not establish a direct SigNoz CLI OAuth/device endpoint, so browser/device auth remains **unknown and out of scope**.
- **Validation:** call `GET /api/v1/service_accounts/me` with the candidate URL and key, require a successful envelope containing a non-empty identity, and display the returned identity before saving. That route is in the v0.133.0 public OpenAPI specification ([OpenAPI route](https://github.com/SigNoz/signoz/blob/v0.133.0/docs/api/openapi.yml#L13500)). A “skip verification” option belongs only to the explicit command-line escape hatch, not the TUI happy path.
- **Commit:** stage configuration in memory, write the API key to the OS keyring, write configuration atomically, and roll back the new keyring entry if configuration fails. If the platform keyring is unavailable, fail safely and offer a separately confirmed `--insecure-storage` mode; if implemented, name the file, require user-only permissions, and print a persistent warning. Never silently downgrade to plaintext.
- **Back/retry:** moving back from authentication clears the secret buffer. Retry is user-triggered and bounded; changing the URL invalidates prior validation. Preserve non-secret form values.
- **Cancellation:** handle Ctrl+C/SIGINT in every screen and during network calls, restore terminal modes, remove temporary files, clear the in-memory secret, and exit 130. No screen before `COMMIT` writes persistent state.
- **Verification:** run `doctor` after commit and show each check as text: CLI version, config, credential-store lookup, API connectivity, and authenticated identity. The same checks and exit classification must be available as `doctor --output json` outside the TUI.
- **Accessibility:** visible step names and counts, keyboard-only operation, stable focus order, no meaning conveyed only by color, honor `NO_COLOR`, use text such as “Checking credentials…” rather than spinner-only feedback, keep errors on screen until acknowledged, and provide a plain prompt mode (`--no-tui` or `SIGNOZ_CLI_SCREEN_READER=1`). This follows gcloud's documented replacement of spinners and boxed tables for screen readers ([official accessibility guide](https://docs.cloud.google.com/sdk/docs/enabling-accessibility-features)).

### Non-TTY contract

`signoz-cli init` must never try to draw, prompt, or launch a browser when either stdin or stdout is not a terminal. It should exit 2, make no writes, and return a stable error object under `--output json`, for example:

```json
{
  "error": {
    "code": "INTERACTIVE_TERMINAL_REQUIRED",
    "message": "signoz-cli init requires an interactive terminal",
    "next": [
      "signoz-cli profile add prod --url https://example.signoz.cloud",
      "printf '%s\\n' \"$SIGNOZ_API_KEY\" | signoz-cli --profile prod auth login --api-key-stdin",
      "signoz-cli --profile prod doctor --output json"
    ]
  }
}
```

The supported machine paths are:

```sh
# Stateless CI: nothing is persisted.
SIGNOZ_URL=https://example.signoz.cloud \
SIGNOZ_API_KEY="$SIGNOZ_API_KEY" \
signoz-cli services list --output json

# Persistent profile without placing the key in argv or shell history.
signoz-cli profile add prod --url https://example.signoz.cloud
printf '%s\n' "$SIGNOZ_API_KEY" | signoz-cli --profile prod auth login --api-key-stdin
signoz-cli --profile prod doctor --output json
```

`--api-key-stdin` should reject a terminal stdin, accept one bounded line, trim only the line ending, never echo or log the value, validate before persistence by default, and distinguish authentication, connectivity, and storage failures with stable exit codes. `--no-verify` should be visibly unsafe and opt-in. No API-key value flag should be added.

### What the TUI should and should not do

**It should:** complete one setup/repair transaction; expose profile context; preserve base-path URLs; mask and clear secrets; give deterministic back/retry/cancel behavior; validate before commit; run `doctor`; degrade to a linear, screen-reader-friendly prompt presentation.

**It should not:** auto-launch on an ordinary command; act as a log/trace explorer; place secrets in argv, clipboard, logs, crash reports, or configuration; silently fall back to plaintext; claim browser/device authentication without a documented SigNoz endpoint; edit alerts or dashboards; retry forever; hide an environment override; or require the TUI for any automatable operation.

## Current CLI coverage versus the SigNoz product/API

The current command surface inspected for this report is:

```text
init, doctor, config path/show, skills install
auth login/check/logout
profile add/update/list/show/use/remove
metrics list/query/top/cardinality/usage
fields keys/values
logs search/aggregate
traces search/aggregate/get
services list/operations
alerts list
rules list/get/history
dashboards list/get
views list/get
channels list/get
query run
```

### Coverage matrix

| Product surface | Confirmed API contract and shape | Contract class / known minimum | Current CLI mapping | Coverage judgment and recommended next move |
|---|---|---|---|---|
| Logs | `POST /api/v5/query_range`; v5 bodies contain `schemaVersion`, millisecond `start`/`end`, `requestType`, `compositeQuery`, optional formatting and variables. Official guides cover log Query Builder requests ([logs API guide](https://signoz.io/docs/logs-management/logs-api/overview/), [OpenAPI route](https://github.com/SigNoz/signoz/blob/v0.133.0/docs/api/openapi.yml#L24075)). | **Public**; confirmed present by 0.133.0. | `logs search`, `logs aggregate`, `query run`. | Good row/scalar base. Add group-by, time-series interval, ordering, and cursor/limit semantics so aggregation answers operational questions without raw JSON.
| Traces | The same public v5 query endpoint supports trace builder and trace-operator queries; the product documents trace API usage ([traces API guide](https://signoz.io/docs/apm-and-distributed-tracing/traces-api/), [Query Builder v5](https://signoz.io/docs/userguide/query-builder-v5/)). | **Public**; confirmed present by 0.133.0. | `traces search`, `traces aggregate`, `traces get`, `query run`. | Strong baseline. Add group-by/time-series and an explicit correlation output that exposes trace ID, service, operation, duration, status, and deep link consistently.
| Metrics | Public v2 routes cover listing, alert/dashboard references, attributes, highlights, inspect, metadata, onboarding, stats, and treemap; ordinary values use v5 query range ([OpenAPI metrics block](https://github.com/SigNoz/signoz/blob/v0.133.0/docs/api/openapi.yml#L17382-L18082), [metrics query guide](https://signoz.io/docs/metrics-management/query-range-api/)). | **Public**; confirmed present by 0.133.0. Metric reference/usage calls require v0.131.0+ according to the first-party MCP server ([compatibility note](https://github.com/SigNoz/signoz-mcp-server/blob/66af322c02d9a804f11c5184753a97031b9cd1f7/README.md#L332-L338)). | `metrics list/query/top/cardinality/usage`. | Broad but uneven. Add `metrics metadata`, `highlights`, and bounded `inspect`; allow grouping/filtering/source on query. Clarify that current `metrics usage` means “which dashboards and alerts reference this metric,” not ingestion spend.
| Fields | `GET /api/v1/fields/keys` and `/values`; the public spec includes signal plus filters such as metric name, search text, context/type, and source ([OpenAPI keys](https://github.com/SigNoz/signoz/blob/v0.133.0/docs/api/openapi.yml#L10993), [values](https://github.com/SigNoz/signoz/blob/v0.133.0/docs/api/openapi.yml#L11082)). | **Public**; confirmed present by 0.133.0. | `fields keys/values` exposes signal and key only. | Partial. Expose search, metric/source, context/type, and pagination where the schema supports them; this unlocks discoverable query composition.
| Services and operations | The first-party MCP client uses `POST /api/v1/services` with nanosecond-string `start`/`end` and `POST /api/v1/service/top_operations` with `start`, `end`, `service`, and `tags` ([MCP client](https://github.com/SigNoz/signoz-mcp-server/blob/66af322c02d9a804f11c5184753a97031b9cd1f7/internal/client/client.go)). Current SigNoz frontend main already calls analogous `/api/v2/services` and `/api/v2/service/top_operations`, showing contract drift ([service source](https://github.com/SigNoz/signoz/blob/cbc8b5424b8f4a5b044e8ec5dc590c1177d7dfa1/frontend/src/api/metrics/getService.ts), [operation source](https://github.com/SigNoz/signoz/blob/cbc8b5424b8f4a5b044e8ec5dc590c1177d7dfa1/frontend/src/api/metrics/getTopOperations.ts)). | **Internal**. No public service-list route was confirmed in the v0.133.0 OpenAPI; minimum unknown. | `services list/operations` uses the v1 MCP-compatible paths. | Valuable but fragile. Keep behind a service adapter, detect 404/shape mismatch, state server compatibility, and avoid silently guessing v2. A future published service endpoint should replace it.
| Exceptions/errors | The product exposes grouped exception lists and detail with stack trace, attributes, and trace linkage ([exceptions guide](https://signoz.io/docs/userguide/exceptions/)). Frontend routes are `POST /api/v1/listErrors`, `GET /api/v1/errorFromErrorID`, `GET /api/v1/errorFromGroupID`, `POST /api/v1/countErrors`, and `GET /api/v1/nextPrevErrorIDs` ([list source](https://github.com/SigNoz/signoz/blob/cbc8b5424b8f4a5b044e8ec5dc590c1177d7dfa1/frontend/src/api/errors/getAll.ts), [detail source](https://github.com/SigNoz/signoz/blob/cbc8b5424b8f4a5b044e8ec5dc590c1177d7dfa1/frontend/src/api/errors/getById.ts)). | **Internal** exception-specific routes; no public exception endpoint confirmed; minimum unknown. | None beyond raw trace queries. | High-value gap. Add `exceptions list` and `exceptions get` only through a capability-gated internal adapter, clearly reporting unsupported servers. Do not call the internal path “stable.” Preserve trace IDs so the user can continue with `traces get`.
| Alert instances | `GET /api/v1/alerts` is in the released OpenAPI ([route](https://github.com/SigNoz/signoz/blob/v0.133.0/docs/api/openapi.yml#L8905)). Alert instances are distinct from configured rules. | **Public**; confirmed present by 0.133.0. | `alerts list`. | Basic coverage. Add state/severity/search filters and `alerts get` only if a public get-by-ID contract is confirmed; otherwise return enough identifiers and labels from list output.
| Rules and history | Public `GET /api/v2/rules`, `GET /api/v2/rules/{id}`, and history endpoints for filter keys/values, overall status, stats, timeline, and top contributors ([rules](https://github.com/SigNoz/signoz/blob/v0.133.0/docs/api/openapi.yml#L18449), [history block](https://github.com/SigNoz/signoz/blob/v0.133.0/docs/api/openapi.yml#L21133-L21652)). The first-party MCP states v0.120.0+ for v2 rule list/get and v0.118.0+ for v2 history ([compatibility note](https://github.com/SigNoz/signoz-mcp-server/blob/66af322c02d9a804f11c5184753a97031b9cd1f7/README.md#L204-L211), [history detail](https://github.com/SigNoz/signoz-mcp-server/blob/66af322c02d9a804f11c5184753a97031b9cd1f7/README.md#L600-L610)). Older official alert documentation still mentions `/api/v1/rules`, so treat that as documentation drift, not an instruction to regress ([alerts management guide](https://signoz.io/docs/userguide/alerts-management/)). | **Public**. Known minimums: history 0.118.0+, v2 rule list/get 0.120.0+. | `rules list/get/history` covers only timeline history. | Add `history stats`, `overall-status`, and `top-contributors`; these make history diagnostic rather than merely enumerable. Keep mutations out of the read-only roadmap.
| Dashboards | Public v2 endpoints list/get dashboards and also expose clone/lock; list supports query, sort/order, limit, and offset ([OpenAPI](https://github.com/SigNoz/signoz/blob/v0.133.0/docs/api/openapi.yml#L14847-L15379)). | **Public**; confirmed present by 0.133.0. The first-introduced version is not established here. | `dashboards list/get` currently targets legacy `/api/v1/dashboards` behavior shared with the first-party MCP client. | Migrate list/get to public v2 with explicit response adapters and tests. Do not infer that the v0.106.0 “public dashboard APIs” changelog establishes the first version of every v2 dashboard route; it describes public sharing APIs ([changelog](https://signoz.io/changelog/2026-01-07-public-dashboard-apis-jjxoiek4n3cba3czq7n1n11p/)).
| Saved views | Explorer saved views are used through `/api/v1/explorer/views?sourcePage=...` and `/{id}` by the first-party MCP client ([client source](https://github.com/SigNoz/signoz-mcp-server/blob/66af322c02d9a804f11c5184753a97031b9cd1f7/internal/client/client.go)). Public `/api/v2/dashboard_views` is a different resource: saved dashboard-list presentation state, not an Explorer query view ([OpenAPI](https://github.com/SigNoz/signoz/blob/v0.133.0/docs/api/openapi.yml#L14622-L14846)). | Explorer views: **Internal**, minimum unknown. Dashboard views: **Public**, present by 0.133.0. | `views list/get` means Explorer views and supports traces/logs/metrics/meter. | Retain the name but document the distinction. Add name/category filters only against the internal adapter. Do not “migrate” Explorer views to dashboard views—they are not equivalent.
| Notification channels | `GET /api/v1/channels` and `GET /api/v1/channels/{id}` are public; test/create/update/delete also exist in the spec ([OpenAPI](https://github.com/SigNoz/signoz/blob/v0.133.0/docs/api/openapi.yml#L8991-L9288)). The first-party MCP links the channel envelope routes to specific SigNoz PRs but gives no released minimum ([compatibility note](https://github.com/SigNoz/signoz-mcp-server/blob/66af322c02d9a804f11c5184753a97031b9cd1f7/README.md#L332-L338)). | **Public**; confirmed present by 0.133.0; minimum unknown. | `channels list/get`. | Adequate read coverage. Add type/status filters only if evidenced in the schema. Channel test and mutations are explicit non-goals for a read-only CLI.
| Service map / dependencies | Frontend sends `POST /api/v1/dependency_graph` with `start`, `end`, and `tags`; the response is a set of parent/child edges with call count/rate, error rate, and p99 ([first-party source](https://github.com/SigNoz/signoz/blob/cbc8b5424b8f4a5b044e8ec5dc590c1177d7dfa1/frontend/src/store/actions/serviceMap.ts)). | **Internal**; no public route confirmed; minimum unknown. | None. | Add `services dependencies` only behind the internal compatibility adapter. It is a natural incident-triage bridge, but must not be presented as a stable API.
| Infrastructure and hosts | Public `POST /api/v2/infra_monitoring/{resource}` covers clusters, daemonsets, deployments, hosts, jobs, containers, namespaces, nodes, pods, PVCs, and statefulsets. `GET /checks?type=...` reports whether required telemetry exists. List bodies use a time window and support resource-dependent limit/offset, filter, group-by, ordering, and host status filtering; responses distinguish ordinary and grouped lists ([OpenAPI block](https://github.com/SigNoz/signoz/blob/v0.133.0/docs/api/openapi.yml#L15921-L16792)). Official docs define the infrastructure product and required host/Kubernetes attributes ([overview](https://signoz.io/docs/infrastructure-monitoring/overview/), [host metrics](https://signoz.io/docs/infrastructure-monitoring/hostmetrics/), [telemetry requirements](https://signoz.io/docs/user-guides/telemetry-data-requirements/)). | **Public**; confirmed present by 0.133.0; first-introduced version unknown. | None. | Largest stable read-only gap. Add `infra check` plus `infra <resource> list` for hosts, clusters, nodes, pods, and deployments using one schema-driven command family, not twelve unrelated implementations.
| Cost Meter / ingestion usage | Official Cost Meter docs use `POST /api/v5/query_range` with builder source `meter`. Documented metrics are `signoz.meter.log.count`, `.log.size`, `.span.count`, `.span.size`, and `.metric.datapoint.count`; data is Delta/Sum, hourly is the minimum useful aggregation, and partial edge buckets require care ([official query guide](https://signoz.io/docs/cost-meter/query-api/)). | **Public** through v5 query; confirmed by current official docs. Retention and availability depend on the Cost Meter product. | `metrics list --source meter`, `views ... --source-page meter`, and raw `query run`; there is no ergonomic meter query/summary. `metrics usage` is a different concept. | Add `meters summary` and `meters query`, default to complete hourly buckets, identify/drop or mark `partial` buckets, and use read-only API keys. Never label metric-reference usage as billing usage.
| Query Builder and validation | `POST /api/v5/query_range` executes builder/formula/PromQL/ClickHouse SQL requests. `POST /api/v5/query_range/preview` validates/previews, and `/substitute_vars` resolves variables ([OpenAPI execute](https://github.com/SigNoz/signoz/blob/v0.133.0/docs/api/openapi.yml#L24075), [preview](https://github.com/SigNoz/signoz/blob/v0.133.0/docs/api/openapi.yml#L24473), [guide](https://signoz.io/docs/userguide/query-builder-v5/)). | **Public**; confirmed present by 0.133.0. | `query run --file`. | Add `query preview --file` before adding more raw power. It is a high-value safety and debugging command. Preserve structured errors and never print rendered SQL unless the server response and verbosity contract intentionally expose it.
| Other high-value read-only | Public routes include authenticated service-account identity, installation stats, health/live/ready checks, current user, user dashboard pins, metric onboarding status, and infra checks ([identity](https://github.com/SigNoz/signoz/blob/v0.133.0/docs/api/openapi.yml#L13500), [stats](https://github.com/SigNoz/signoz/blob/v0.133.0/docs/api/openapi.yml#L14037), [health](https://github.com/SigNoz/signoz/blob/v0.133.0/docs/api/openapi.yml#L15886), [current user](https://github.com/SigNoz/signoz/blob/v0.133.0/docs/api/openapi.yml#L23384)). | **Public**; confirmed present by 0.133.0. | `auth check` and `doctor` cover identity/connectivity; other routes are absent. | Fold health/identity into `doctor`; consider `server info` for version/stats only if its output is stable and non-sensitive. User pins and onboarding status are low priority. Exclude service-account/key administration.
| External/third-party APIs | The frontend confirms `POST /api/v1/third-party-apis/overview/list` with time range, IP-display flag, and a filter expression ([first-party source](https://github.com/SigNoz/signoz/blob/cbc8b5424b8f4a5b044e8ec5dc590c1177d7dfa1/frontend/src/api/thirdPartyApis/listOverview.ts)). | **Internal**; no public route confirmed; minimum unknown. | None. | Useful later, after the two complete jobs above. Keep it capability-gated and do not guess detail routes.

## Prioritized gap table

| Priority | Gap | Why it matters | Contract choice | Definition of useful completion |
|---|---|---|---|---|
| P0 | Transactional `init` and strict non-TTY setup | Users cannot trust broader API coverage if setup can save an invalid key, expose a secret, or hang in CI. | Public identity route plus local config/keyring only. | Create/repair/switch a profile; masked input; validate-before-save; rollback; cancellation; accessibility/plain mode; `doctor`; JSON non-TTY error and exact recipe.
| P0 | Public dashboard v2 migration | Current list/get behavior depends on an unpublished legacy route even though a public replacement exists. | Public `/api/v2/dashboards`. | List/search/sort/page and get work against released schema; old-server failure is explicit; no writes.
| P1 | Query preview | Prevents costly or invalid raw query execution and makes CI validation possible. | Public `/api/v5/query_range/preview`. | `query preview --file` shares parsing, timeouts, JSON errors, and output conventions with `query run`.
| P1 | Complete incident-triage bundle | Exceptions and dependency edges are the missing links between existing alerts, services, traces, and logs. | Public APIs wherever possible; internal exception/map adapters only where necessary. | One documented workflow can start from a firing alert, inspect rule history, identify service/operation, view exception group/detail or dependency edges, then fetch a trace and related logs. Unsupported internal routes fail as capability errors, not generic JSON errors.
| P1 | Rich rule history | Timeline alone does not explain duration, contributors, or overall status. | Public v2 history endpoints; history 0.118+, rules 0.120+. | Stats, overall status, filter keys/values, and top contributors with stable pagination/filter semantics.
| P2 | Infrastructure inventory bundle | It is the largest current product area with a released read-only API and no CLI coverage. | Public `/api/v2/infra_monitoring/*`. | `infra check`, hosts, and core Kubernetes resources share filters/grouping/order/output; docs show required telemetry when checks fail.
| P2 | Cost Meter summary | Users need a clear ingestion/cost view; current “usage” name is misleading. | Public v5 meter queries. | Complete hourly summaries for all five documented meter metrics, units, partial-bucket marking, JSON/table output, and a raw meter query escape hatch.
| P2 | Field and aggregation ergonomics | Discoverability and grouping turn existing generic endpoints into usable investigations. | Public fields and v5 query. | Search/context/source for fields; group-by/time-series/order/top-N for logs, traces, and metrics.
| P3 | Metric metadata/highlights/inspect | Useful for instrumentation quality and metric debugging. | Public v2 metrics. | Metadata and last-received/highlight summaries; inspect enforces the server's bounded time window and returns clear cardinality warnings.
| P3 | Internal API stabilization | Services, Explorer views, exceptions, maps, and external APIs can drift independently. | Named internal adapters and capability matrix. | Central endpoint/version mapping, fixture tests per observed shape, `UNSUPPORTED_SERVER_CAPABILITY`, and one place to migrate when SigNoz publishes replacements.

## Delivery phases by complete user value

### Phase 1 — Trustworthy connection

Ship the full `init` state machine, validated stdin login, transaction/rollback behavior, profile repair/switch, screen-reader/plain mode, and a machine-readable `doctor`. Migrate dashboard reads to v2 in the same phase because setup success should lead to at least one fully public resource path.

**Exit criterion:** a new human can configure Cloud or self-hosted SigNoz without exposing a key, and a CI job can reach the same verified state without any prompt or persistent secret.

### Phase 2 — Incident from symptom to evidence

Add query preview and richer rule history, then the capability-gated exception and dependency commands. Improve grouped log/trace queries and consistent correlation fields. Keep the existing services adapter but make its compatibility status visible.

**Exit criterion:** a user can begin with a firing alert and reach concrete trace/log evidence in one documented CLI session, with explicit messaging if their server lacks an internal-only capability.

### Phase 3 — Estate health and ingestion spend

Add infrastructure checks, hosts, and the highest-value Kubernetes lists through the public v2 contract. Add Cost Meter summaries and raw meter queries, with correct bucket and unit semantics.

**Exit criterion:** an operator can answer “what infrastructure is unhealthy?” and “what telemetry is driving ingestion?” without copying Query Builder JSON from the UI.

### Phase 4 — Discovery and instrumentation quality

Expand fields, metric metadata/highlights/inspect, and remaining infra resources. Add dashboard search/sort and deliberately distinguish Explorer views from dashboard views.

**Exit criterion:** users can discover valid query fields, diagnose missing/stale/high-cardinality metrics, and navigate assets without raw API work.

### Phase 5 — Optional internal surfaces

Consider external/third-party API summaries and other UI-only reads only after adapter telemetry and maintenance cost are understood. Promote an internal adapter to public only when a released OpenAPI or official API document confirms it.

## Security boundaries and non-goals

- Read-only is the product boundary for this roadmap. Alert-rule, channel, dashboard, service-account, key, role, invite, and user mutations are excluded even where the public API exposes them.
- Prefer least-privilege/read-only API keys. Never print the key, request `SIGNOZ-API-KEY` header, secret-bearing environment, or credential-store value under `--verbose`, JSON errors, or crash reports.
- Do not persist an unvalidated credential by default. Treat configuration and keyring writes as a transaction, and prevent symlink/permission surprises for any explicit plaintext fallback.
- Validate URL scheme and authority, preserve supported base paths, reject embedded credentials, and do not follow cross-origin redirects while carrying `SIGNOZ-API-KEY`.
- Bound response size, stdin size, retries, and polling; apply timeouts and cancellation to every network request.
- Raw Query Builder is read-only at the SigNoz endpoint, but PromQL/ClickHouse SQL can still be expensive. `query preview`, explicit time windows, server timeouts, and clear limits are required guardrails.
- Internal routes are not hidden “public APIs.” Their commands must identify themselves as compatibility features, emit a stable unsupported-capability error on 404/shape drift, and never silently try speculative route variants.
- A full-screen telemetry explorer, browser-based SigNoz auth, automatic plaintext credential storage, and general administrative CRUD are explicit non-goals.

## Final decision record

The recommended shape is a **small setup TUI plus a broad, composable, non-interactive CLI**. The TUI earns trust by making one risky transaction—endpoint plus credential setup—safe and understandable. API growth earns trust by prioritizing released public contracts, grouping work into complete operator outcomes, and honestly labeling first-party internal routes. This produces more useful coverage than either a large terminal dashboard or a command-per-endpoint inventory, while leaving a clear path to adopt new public SigNoz APIs as they are released.
