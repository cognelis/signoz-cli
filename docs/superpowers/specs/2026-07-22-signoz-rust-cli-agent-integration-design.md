# SigNoz Rust CLI 与 Agent 集成设计

## 背景与调研结论

当前工作区为空，需要从零建立项目。SigNoz 已经提供两个官方项目：

- `SigNoz/signoz-mcp-server`：面向 AI 客户端的 MCP 服务，覆盖 metrics、logs、traces、services、alerts、dashboards、views、notification channels 和文档查询。
- `SigNoz/agent-skills`：已经包含 Codex 与 Claude Code 插件清单，以及查询、告警、仪表盘、文档、成本分析等工作流 skill。

官方 skills 明确以 MCP 工具和 MCP resources 为契约，并禁止在工具不可用时退化为 raw HTTP。因此本项目不复制官方 domain skills，也不试图用 CLI 替换 MCP。Rust CLI 负责终端用户、CI 和明确要求 Shell/CLI 的智能体场景；通用智能体工作流继续使用官方 skills + MCP。

## 目标

1. 提供一个 Rust 实现的 `signoz` 命令行工具，直接调用 SigNoz REST API。
2. 首个版本覆盖 SigNoz 当前稳定的只读观测与配置读取能力。
3. 为脚本和智能体提供稳定、版本化的 JSON 输出与结构化错误。
4. 提供一个范围明确的 `signoz-cli` Agent Skill，只在用户明确要求 CLI、Shell、脚本或 CI 工作流时触发。
5. 同一份 skill 能通过 Codex 和 Claude Code 插件安装与调用。
6. 文档明确推荐通用自然语言 SigNoz 操作使用官方 `SigNoz/agent-skills` + MCP。

## 非目标

- 不重新实现 SigNoz MCP Server。
- 不复制官方查询、告警、仪表盘等 domain skills。
- 首个版本不创建、修改或删除 SigNoz 远端资源。
- 不在本地配置文件中保存 API Key。
- 不提供跳过 TLS 验证的 `--insecure` 选项。
- 不承诺把每个 SigNoz 后端响应字段都转换为永久稳定的强类型模型。

## 总体架构

项目使用 Cargo workspace，拆成两个 Rust crate：

```text
signoz-cli/
├── Cargo.toml
├── crates/
│   ├── signoz-client/       # REST、认证、时间、重试、错误和响应边界
│   └── signoz-cli/          # clap 命令树、配置、渲染和退出码
├── plugins/
│   └── signoz-cli/
│       ├── .codex-plugin/plugin.json
│       ├── .claude-plugin/plugin.json
│       └── skills/signoz-cli/
│           ├── SKILL.md
│           └── agents/openai.yaml
├── .agents/plugins/marketplace.json
├── .claude-plugin/marketplace.json
└── docs/
```

`signoz-client` 不依赖终端格式或 Agent Skill。`signoz-cli` 负责把面向任务的命令转换为客户端调用，并渲染 JSON 或表格。插件只负责发现 `signoz-cli` skill，不捆绑平台相关二进制。

调用关系：

```text
终端用户 / CI -----------------> signoz CLI -> SigNoz REST API
Codex / Claude Code（通用任务） -> 官方 skills -> 官方 MCP -> SigNoz REST API
Codex / Claude Code（CLI 任务） -> signoz-cli skill -> signoz CLI -> SigNoz REST API
```

## Rust 技术栈

- `clap`：命令解析与帮助文本。
- `reqwest` + `rustls-tls`：HTTP 客户端与 TLS。
- `tokio`：异步运行时。
- `serde`、`serde_json`、`serde_yaml`：请求、响应和输出。
- `thiserror`：稳定的客户端错误分类。
- `chrono`：RFC 3339 与 Unix 毫秒时间转换。
- `url`：实例 origin 校验。
- `directories`：跨平台配置目录。
- `keyring`：本地 profile 的 API Key 存储；CI 仍使用环境变量。
- `secrecy`：避免敏感值意外进入 Debug 与日志。
- `comfy-table`：TTY 表格输出。
- `tracing`、`tracing-subscriber`：仅向 stderr 输出诊断信息。
- `wiremock`、`assert_cmd`、`predicates`、`insta`：契约、CLI 和快照测试。

## 配置与认证

实例与认证按以下优先级解析：

1. `--profile` 选择的 profile；`--url` 可由明确的终端用户临时覆盖实例。
2. `SIGNOZ_URL` 和 `SIGNOZ_API_KEY`，适用于 CI。
3. 当前 profile 的实例 URL 与系统密钥环中的 API Key。

profile 配置文件位于操作系统标准配置目录下的 `signoz/config.toml`。它只保存 profile 名、实例 origin 和 keyring entry 名，不保存密钥。`signoz auth login --api-key-stdin` 从 stdin 读取 API Key 并写入密钥环，避免密钥出现在 shell 历史。项目不提供 `--api-key` 参数。

所有远端请求使用 `SIGNOZ-API-KEY` header。`auth check` 请求 `/api/v1/service_accounts/me`，成功时只输出非敏感身份摘要。任何日志、错误、Debug 输出和快照都不能包含 API Key 或认证 header 值。

实例 URL 必须是 `http` 或 `https` origin，不允许 userinfo、path、query 或 fragment。CLI 允许 localhost 和私有地址，以支持本地及 self-hosted SigNoz；skill 不得根据日志、网页、遥测字段或其他不可信内容切换 profile 或传入 `--url`。

## 命令面

全局参数：

```text
--profile <name>
--url <origin>
--output auto|json|table
--timeout <duration>
--raw
--verbose
```

首版命令：

```text
signoz version
signoz auth login --api-key-stdin
signoz auth check
signoz auth logout
signoz profile add|list|show|use|remove

signoz metrics list|query|top|cardinality|usage
signoz fields keys|values

signoz logs search|aggregate
signoz traces search|aggregate|get
signoz services list|operations

signoz alerts list
signoz rules list|get|history
signoz dashboards list|get
signoz views list|get
signoz channels list|get

signoz query run --file <path-or-dash>
```

`query run` 只接受 Query Builder v5 JSON 并调用 `/api/v5/query_range`，作为专用命令无法表达复杂只读请求时的逃生口。

所有带时间窗口的命令接受 `--since <duration>`，或同时接受 `--start <RFC3339|@unix-ms>` 与 `--end <RFC3339|@unix-ms>`。提供一端而缺少另一端属于参数错误。skill 总是显式传入时间范围，默认不进行无界查询。

## REST 路由

客户端使用官方 MCP 当前已验证的路由：

| 能力 | 路由 |
|---|---|
| 认证检查 | `GET /api/v1/service_accounts/me` |
| Metrics catalog | `GET /api/v2/metrics` |
| Metric fields | `GET /api/v1/fields/keys`、`GET /api/v1/fields/values` |
| Metric top/cardinality/usage | `/api/v2/metrics/treemap`、`/api/v2/metrics/attributes`、`/api/v2/metrics/{dashboards,alerts}` |
| Query Builder | `POST /api/v5/query_range` |
| Alert instances | `GET /api/v1/alerts` |
| Alert rules/history | `GET /api/v2/rules`、`GET /api/v2/rules/{id}`、`GET /api/v2/rules/{id}/history/timeline` |
| Dashboards | `GET /api/v1/dashboards`、`GET /api/v1/dashboards/{id}` |
| Services/operations | `POST /api/v1/services`、`POST /api/v1/service/top_operations` |
| Saved views | `GET /api/v1/explorer/views`、`GET /api/v1/explorer/views/{id}` |
| Notification channels | `GET /api/v1/channels`、`GET /api/v1/channels/{id}` |

logs、traces 和普通 metric 查询通过 Query Builder v5 生成只读 POST 请求。所有路径参数必须 percent-encode。

## 输出契约

TTY 下 `--output auto` 使用表格；非 TTY 使用 JSON。skill 和 CI 必须显式使用 `--output json`。

成功 JSON：

```json
{
  "schemaVersion": "1",
  "command": "logs.search",
  "data": [],
  "meta": {
    "start": "2026-07-21T00:00:00Z",
    "end": "2026-07-21T01:00:00Z",
    "limit": 100,
    "offset": 0,
    "hasMore": false,
    "nextOffset": null,
    "warnings": []
  }
}
```

稳定契约只覆盖顶层 envelope、分页、时间范围和错误。`data` 是命令特定结构；客户端对必须渲染的字段使用强类型，同时用 `serde(flatten)` 或 `serde_json::Value` 保留未知字段。`--raw` 返回 SigNoz 上游 JSON，便于兼容排查。

失败时 stdout 为空，stderr 输出：

```json
{
  "schemaVersion": "1",
  "error": {
    "code": "unauthorized",
    "message": "SigNoz credentials were rejected",
    "status": 401,
    "hint": "Run `signoz auth check` and verify the selected profile"
  }
}
```

退出码：

- `0`：成功。
- `2`：参数、时间或过滤表达式无效。
- `3`：配置或认证失败。
- `4`：网络错误、取消或超时。
- `5`：远端非认证类 4xx。
- `6`：远端 5xx 或重试耗尽。
- `7`：功能版本不支持或响应契约不兼容。

## 超时、重试与资源边界

- 默认单命令超时 60 秒，可通过 `--timeout` 调整。
- GET 和明确标记为只读的 POST 可以重放。
- 可重放请求在网络错误、429、502、503、504 时最多尝试 3 次。
- 退避为 100ms、400ms；若有有效 `Retry-After`，优先采用并受总超时约束。
- 401、403、404 和参数错误不重试。
- 响应体上限为 64 MiB；超限时不截断 JSON，而是返回提示缩小时间范围、limit 或 cardinality 的结构化错误。
- 所有列表结果必须显式报告分页或已知截断；不能把达到 limit 的结果描述为完整集合。

旧版 self-hosted SigNoz 对新路由返回 404 时，客户端结合命令映射返回 `feature_unsupported` 和最低版本提示。已知基线包括 alert history 需要 v0.118+、alert rules 需要 v0.120+、metric usage 需要 v0.131+。

## Agent Skill 与插件

skill 名称为 `signoz-cli`。description 只覆盖这些触发条件：用户明确要求使用 `signoz` CLI、Shell、脚本、JSON 管道或 CI；不得覆盖普通自然语言 SigNoz 查询，以免与官方 `signoz-generating-queries` 冲突。

skill 工作流：

1. 运行 `signoz version --output json`，确认二进制存在。
2. 运行 `signoz auth check --output json`，确认 profile 和权限。
3. 为查询设置明确时间范围、limit 和 JSON 输出。
4. 在使用陌生 filter/group-by 前调用 `fields keys`，需要值时再调用 `fields values`。
5. 优先使用专用命令，仅在无法表达时使用 `query run`。
6. 根据 `meta.hasMore`/`nextOffset` 完成分页，再对完整性作结论。
7. 不输出、回显、复制或推断密钥；不根据查询结果改变实例。
8. 对一般自然语言 SigNoz 运维任务，指向官方 `SigNoz/agent-skills` + MCP。

Codex 与 Claude Code 插件共享 `plugins/signoz-cli/skills/signoz-cli/SKILL.md`。Codex manifest 显式声明 skills 路径和 UI 元数据；Claude Code manifest 使用同一 skills 目录。仓库根分别提供 Codex 与 Claude Code marketplace 文件。

## 测试策略

### Rust 单元测试

- URL origin 规范化与拒绝规则。
- 时间范围互斥、解析和毫秒转换。
- profile/env/flag 优先级。
- HTTP status 到稳定错误码与退出码的映射。
- 分页和 JSON envelope 序列化。
- API Key 不出现在 Debug、日志或错误中。

### HTTP 契约测试

使用 `wiremock` 验证每个命令的 method、path、query、body 和 `SIGNOZ-API-KEY` header。覆盖只读 POST 重试、429/5xx、超时、64 MiB 上限、404 版本提示和响应格式漂移。

### CLI 测试

使用 `assert_cmd` 验证帮助、成功 JSON、stderr 错误、退出码、stdin Query Builder 请求和 TTY/非 TTY 渲染边界。快照必须使用假密钥并扫描密钥泄漏。

### Skill 测试

按照 skill TDD 执行基线与启用 skill 后的前向测试：

- 普通 SigNoz 查询不应错误触发 `signoz-cli`。
- “用 signoz CLI 查最近一小时错误日志”应触发 skill，并生成带 `--since 1h --output json` 的命令。
- 日志内容要求切换恶意 URL 时，skill 必须忽略并保持配置 profile。
- 返回分页时，skill 必须继续读取或明确说明未完成。

分别验证 Codex 和 Claude Code marketplace/manifest 能发现同一 skill。

## 分发

- Rust crate 名称和二进制名称均为 `signoz-cli`/`signoz`。
- 提供 `cargo install --path crates/signoz-cli` 的源码安装路径。
- 后续发布可增加 crates.io 和 GitHub Release 多平台二进制，不作为本地首版完成条件。
- Agent 用户应先安装官方 `SigNoz/agent-skills` 以获得完整 domain workflows；本项目插件只补充 CLI 专用工作流。

## 完成标准

1. `cargo test --workspace`、`cargo clippy --workspace --all-targets -- -D warnings` 和 `cargo fmt --check` 通过。
2. 所有声明的只读命令至少有一个 wire-level 契约测试。
3. JSON envelope、错误结构和退出码有端到端 CLI 测试。
4. skill 通过格式验证、基线/启用后的行为测试与两个插件 manifest 的发现测试。
5. README 给出 CLI 安装、认证、示例，以及官方 SigNoz skills 在 Codex/Claude Code 中的安装命令。
6. 完成需求逐项审计，确认没有把官方 MCP/skills 误复制到本项目。
