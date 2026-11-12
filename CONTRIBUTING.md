# Contributing and releasing

## Version policy

`Cargo.toml`'s `workspace.package.version` is the canonical product version.
`signoz-cli` and `signoz-client` inherit it. Their entries in `Cargo.lock` and both
plugin manifests and `build/npm/package.json` must carry the same release version.

Use plain `MAJOR.MINOR.PATCH` versions for releases:

| Change | Version increment |
| --- | --- |
| Compatible bug fix, security fix, or released Skill correction | Patch |
| New user-visible capability | Minor |
| Incompatible CLI, config, JSON, or public Rust API change while below 1.0 | Minor, with migration notes |
| Incompatible public behavior at 1.0 or later | Major, with migration notes |
| Internal refactor, tests, or documentation with no shipped behavior change | Record under Unreleased when useful; no standalone release is required |

Choose the highest required increment in a batch. Bump once for the completed
batch, not for each intermediate edit or test run. `+codex.<timestamp>` is a local
development cache key, not a substitute for a product version increment; never
use it in a release commit. A changed plain plugin version also selects a fresh
plugin cache during installation.

The JSON envelope's `schemaVersion` is a separate compatibility contract. Do not
change it just because the product version changes. Describe breaking behavior
and required caller changes explicitly in the changelog.

The Claude marketplace's `metadata.version` versions the marketplace descriptor,
not the plugin. Update it only when releasing a descriptor/schema change; plugin
versions belong in the plugin manifests.

## Record changes as you work

For evidence-driven improvement iterations, follow the
[RSI workflow and first experiment](docs/research/2026-11-11-rsi.md): reproduce a
failure before changing behavior, preserve the regression case, check neighboring
valid cases, and accept changes only after the existing validation gates pass.
Static Skill checks establish instruction coverage, not measured agent success.

Add concise, user-facing entries under `CHANGELOG.md`'s `Unreleased` section using
`Added`, `Changed`, `Fixed`, `Removed`, or `Security` as appropriate. Include
migration notes when commands, authentication, defaults, or public APIs change.
Do not rewrite a previously published release's history to hide a later change.

Keep CLI behavior, README examples, and the canonical Skill at
`plugins/signoz-cli/skills/signoz-cli/SKILL.md` in sync. The Skill is embedded in
the executable, so rebuild before updating installed copies.

## Prepare a release

After the batch and its changelog entries are ready, choose the increment:

```sh
python3 scripts/release.py prepare minor
# Or: prepare patch / prepare major
python3 scripts/release.py check
```

`prepare` moves Unreleased notes into a dated release entry, updates the workspace,
lockfile, both plugin manifests, and the npm package, and leaves a fresh Unreleased section. It
uses the local calendar date; `--date YYYY-MM-DD` allows an explicit release date.
It rejects an empty Unreleased section, mismatched versions, and duplicate or
out-of-order release entries before changing files. It does not commit, tag,
push, publish, or install anything.

Use Python 3.9 or newer for the repository scripts. Run the checks before finalizing:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
node --test build/npm/install.test.js
python3 -m unittest discover -s tests -p 'test_*.py'
python3 scripts/validate-agent-integration.py
python3 scripts/release.py check
git diff --check
```

For npm distribution, also build the native executable and run
`npm --prefix build/npm run test:package`. This checks the tarball's file allowlist,
installs it into an isolated directory, and exercises the launcher with a verified
native-binary fixture. Public download verification remains required before publishing.
See [docs/releasing.md](docs/releasing.md) for the first npm publish and CI setup.

Review the version bump and migration notes together with the implementation.
When a release is requested, commit the reviewed changes and create an annotated
`vX.Y.Z` tag on that commit, using the matching CHANGELOG section as release notes.
A request to commit changes alone does not require creating a release tag. Do not
tag an older commit while the release changes are still uncommitted. Push tags or
publish packages only when those actions are requested.

## Refresh local installations

After verification, update the executable and its embedded Skill:

```sh
cargo install --path crates/signoz-cli --locked
signoz-cli version --output json
signoz-cli skills install
```

For an already configured local Codex marketplace, refresh the plugin using its
new release version:

```sh
codex plugin add signoz-cli@signoz-cli
```

Verify that the installed Skill and active plugin cache match the canonical
source. New Codex threads pick up the refreshed plugin instructions. Claude Code
plugin-managed installations use their own plugin update flow; standalone Skills
are updated by `signoz-cli skills install`.

## 第五轮：doctor 包与 Skill 版本诊断

参考 `/Users/caspian/git/jenkins-cli/internal/app/doctor.go` 的诊断规则：更新查询最多 5 秒，
可显式跳过；加载版本与安装版本分开判断，升级信息不参与连接健康或退出码计算。
新检查追加到原报告末尾，保留原有列与输出 envelope；版本信息来自公开发布元数据和本地 Skill 文件，
不复用业务认证。Skill frontmatter 版本随产品版本更新，并由发布检查防止漂移。

## 提交与贡献者

提交正文、验证记录和 AI 署名遵循 [提交规范](docs/commit-conventions.md)。项目维护者及 AI 协作工具见 [CONTRIBUTORS.md](CONTRIBUTORS.md)。

本轮本地安装和 Skill 内容已核验；Rust 两个包的 npm tarball 安装冒烟通过。三个 doctor 的实际表格输出保持原样，维护检查只追加行。

## 第八轮 RSI：新能力与 Skill 工作流优化

现有 doctor 能报告版本概览，installer 能比较文件但会执行写入；缺少升级后、刷新前的只读内容检查。
新增 skills status，复用安装目标、文件清单与版本解析，输出文件级差异，不增加依赖。
接受场景包括未安装、相同版本但被修改、引用丢失、不可读路径，以及损坏业务配置下的离线执行。
只检查 CLI 托管的全局 Codex/Claude 安装位置；不扩展任意目录或项目扫描参数。

Skill 增加任务分流：本地维护直接使用离线命令，数据任务继续选环境并运行 doctor；刷新前审阅自定义内容，
刷新后核对并重载上下文。CLI 测试证明工作流可执行，不把这些通过率当作 Agent 任务成功率。

第八轮验收：0.8.0：161 项 Rust 测试、9 项 Node 安装测试、13 项 Python 测试、fmt/clippy、Agent 集成与发布检查通过。 当前未做模型级 Skill 效果评测或其他平台运行验证。

本机验证：刷新前，三个 CLI 的托管安装都被新命令识别为 different；确认没有本地自定义修改后显式刷新，结果全部转为 current。独立校验 124 个文件与源码一致。两个 Rust npm 包的安装冒烟也验证了只读 skills status 入口。

## RSI 与发布批次

按用户确认，日常 RSI 轮次作为 Unreleased 草稿积累：每轮验证、提交，不再逐轮递增版本。
只有用户要求准备正式发布时，再统一整理待发布变更和版本；已发布的标签、版本与历史不改写。
本轮不调用版本 prepare，也不刷新版本化插件缓存。

## 第九轮 RSI：可审阅的业务请求草稿

本轮作为 Unreleased 草稿，不递增版本、不发布。围绕“离线构造/预览 → 审阅 → 选择环境并诊断 → 授权后执行”完善能力，
分别复用现有查询构造器、契约解析/请求序列化器和文档操作解析器，避免另外维护一套执行语义。
预览不读取业务凭据、不发请求，不表示服务端验证或权限已通过；Skill 将计划步骤和实际执行的前置条件分开。

第九轮验收：163 项 Rust 测试、9 项 Node 安装测试、13 项 Python 测试、fmt/clippy、Agent 集成及发布检查通过；工作区 release 二进制的 npm tarball 安装测试覆盖 query build。固定时间查询在本地模拟服务中原样提交验证通过。
本轮未执行全局重装或插件刷新；草稿使用工作区构建试用。未验证生产服务、其他操作系统或模型级 Skill 效果。

## 第十轮 RSI：请求复用与命令发现

新增 query validate，让手写/保存的 Query Builder v5 文件在无配置环境也能检查结构。复用 preview/run 的读取与校验逻辑，输出规范化 payload，不改写输入。回归覆盖坏配置、文件/stdin、缺省 schemaVersion、非法 JSON/时间以及 validate 后原样发送。

验收：164 项 Rust 测试、9 项 Node 安装测试、13 项 Python 测试、fmt/clippy、Agent 集成、版本检查及工作区 release 二进制 npm tarball 冒烟通过。完整回归曾发现新增补全菜单文案过长，缩短菜单标题并保留长帮助后通过。

本轮新增能力的用例均先在原实现确认失败，再完成实现与验证。保持 Unreleased，不递增版本，不刷新全局安装或插件。通用 Skill 校验器不接受项目既有的 version frontmatter 字段；保留 doctor 需要的字段，并使用仓库的 Skill/发布校验。未验证生产服务、其他操作系统及模型级 Skill 效果。

## 第十一轮 RSI：聚合草稿、按需契约与写入过滤文件

将离线 query build 扩展到聚合、分组和时间序列，复用既有聚合表达式、字段/分组校验和客户端构造器。支持 count/rate 与字段聚合；明确依赖选项，拒绝 raw-only search/offset，避免静默忽略意图。Skill 补充聚合选择、单位和固定时间窗口的复用流程。

验收：166 项 Rust、9 项 Node、13 项 Python 测试、fmt/clippy、Agent 集成、版本检查和 npm tarball 冒烟通过。本地模拟服务对比 logs/traces 的离线构造与直接执行请求，包含时间序列、分组及字段聚合。

新增行为的回归用例先在原实现确认失败，再通过实现和全量验证。保持 Unreleased，不递增版本、不推送发布、不刷新全局安装或插件。未验证生产服务、其他平台和模型级 Skill 效果。

## 第十二轮 RSI：可靠保存审阅结果与聚合预览（2026-11-12）

新增 query build/validate --out，复用现有 tempfile 依赖，采用同目录临时文件和替换，保存原始请求且保持 stdout 格式。校验完成后才保存，因此支持输入输出同路径的规范化；失败不改写旧输入，Unix 文件权限为 0600，符号链接路径替换而不跟随。Skill 区分原地保存和危险的 shell 同路径重定向。

验收：167 项 Rust、9 项 Node、13 项 Python 测试、fmt/clippy、Agent 集成、版本检查及 npm tarball 冒烟通过。覆盖原地规范化、无效输入保留、相对路径、stdout-only、符号链接及临时文件清理。

新增用例均先在原实现确认失败，再完成实现与验证。保持 Unreleased，不递增版本、不推送发布、不刷新全局安装或插件。未验证生产服务、其他操作系统或模型级 Skill 效果。

## 第十三轮 RSI：固定历史窗口与一致的输入来源（2026-11-12）

新增 --since 配合显式 --end 的历史窗口，复用共享时间解析与溢出检查，保留 start/since 互斥及 end 单独使用拒绝。Cost Meter 对显式锚定范围只保留其内部完整小时，不按当前时间移动或扩展窗口。Skill 指导事件复现与固定窗口分页。

验收：170 项 Rust、9 项 Node、13 项 Python 测试、fmt/clippy、Agent 集成、版本及 npm tarball 检查通过。覆盖时区、固定结果、溢出、非法组合、Cost Meter 边界，以及本地模拟服务的 logs search 请求一致性。

新行为回归均先在原实现确认失败，再完成实现和验证。保持 Unreleased，不递增版本、不推送发布、不刷新全局安装或插件。未验证生产服务、其他平台或模型级 Skill 效果。

## 第十四轮 RSI：查询复用与读取范围控制（2026-11-12）

新增 query retime，以显式时间参数更新保存请求的顶层 start/end，复用现有读取、结构校验、时间解析和原子保存。保留过滤、聚合、分组、变量与步长，支持 stdin；未提供新窗口或原输入无效时拒绝。Skill 明确嵌套时间和 SQL 字面量不自动改写，比较前后窗口时保存为独立文件。

验收：171 项 Rust、9 项 Node、13 项 Python 测试、fmt/clippy、Agent 集成、版本及 npm tarball 检查通过。模拟服务验证调整后的完整 payload 原样提交，覆盖变量/规格保留、stdin、输入不变、无效参数不覆盖目标。

新增行为用例先在原实现确认失败，再完成实现和全量验证。保持 Unreleased，不递增版本、不推送发布、不刷新全局安装或插件。未验证生产服务、其他平台或模型级 Skill 效果。
