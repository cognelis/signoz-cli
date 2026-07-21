# `signoz-cli` 构建产物改名设计

## 背景与目标

当前 Cargo 包名是 `signoz-cli`，但 `[[bin]]` 仍命名为 `signoz`，因此 `cargo build` 和 `cargo install` 生成的可执行文件叫 `signoz`。本次变更要求构建产物、程序自报名称、文档示例及 Agent Skill 调用统一为 `signoz-cli`。

验收标准：

- `cargo build -p signoz-cli` 生成 `target/debug/signoz-cli`，不再生成新的 `target/debug/signoz`。
- 安装后的调用命令是 `signoz-cli`。
- `--help`、结构化错误提示、verbose 诊断均显示 `signoz-cli`。
- Codex 与 Claude Code 的共享 Skill 只生成 `signoz-cli ...` 命令。
- Rust 测试、Agent 集成校验和用户文档不再把裸 `signoz` 当作命令名。

## 方案比较

### 方案 A：只改 Cargo 产物名

把 `[[bin]].name` 改为 `signoz-cli`，但保留 clap 与文档中的 `signoz`。改动最小，但帮助信息、错误提示和实际文件名不一致，Agent 也会继续生成不存在的旧命令，因此不采用。

### 方案 B：同时生成 `signoz-cli` 和兼容的 `signoz`

迁移更平滑，但仍会生成用户明确要求移除的旧构建产物，还会增加双入口测试和发布负担，因此不采用。

### 方案 C：原子统一改名（采用）

Cargo 二进制、clap 程序名、运行时提示、测试、README、设计文档、实施计划、插件元数据、Skill 与场景基线全部改为 `signoz-cli`。不提供旧二进制兼容层，使仓库只有一个权威命令名。

## 变更边界

以下内容改名：

- `crates/signoz-cli/Cargo.toml` 中的 `[[bin]].name`。
- clap 命令名和所有面向用户的命令提示。
- `Command::cargo_bin(...)` 测试目标。
- README、设计/计划记录、Agent Skill、插件描述、Skill 场景与集成校验器中的 CLI 调用。

以下内容保持不变：

- 产品名 `SigNoz`。
- Rust 包名 `signoz-cli` 和客户端 crate `signoz-client`。
- `SIGNOZ_URL`、`SIGNOZ_API_KEY` 环境变量。
- 配置目录、Keyring service、API 路由、示例域名和测试夹具中的 `signoz` 字符串。
- 插件名和 Skill 名 `signoz-cli`（它们已经正确）。

## 测试设计

先添加失败断言：clap 根命令必须叫 `signoz-cli`，Agent 校验器必须要求 `signoz-cli version` 与 `signoz-cli auth check`，并拒绝任何行首裸 `signoz` 命令。确认这些测试在旧实现上失败后，再修改生产代码和文档。

最终验证包括格式检查、Clippy、完整 Rust 测试、Agent 集成测试、Skill/插件官方校验、构建产物路径检查，以及仓库范围的旧命令扫描。由于 Wiremock 测试需要本机回环端口，完整测试应在允许绑定临时本机端口的环境运行。

## 发布与兼容性

这是一次有意的命令名破坏性变更。现有脚本中的 `signoz ...` 必须迁移为 `signoz-cli ...`。本项目尚处于 `0.1.0`，不增加兼容别名；明确失败比悄悄保留第二个入口更安全。
