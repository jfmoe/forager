# forager

面向 AI Agent 的检索与研究 CLI。forager 按能力组织搜索、网页读取、站点映射、文档检索和深度研究，并统一处理供应方选择、凭据轮换与同能力降级。

默认输出 JSON，适合被 Agent、脚本和 CI 调用；需要人读结果时可选择 Markdown 或纯内容格式。

## 安装

forager 通过 [GitHub Releases](https://github.com/jfmoe/forager/releases) 发布预编译二进制，不要求本机安装 Rust。正式 Release 提供：

- macOS：Apple Silicon、Intel
- Linux：AArch64、x86-64
- Windows：x86-64
- 每个平台归档对应的 SHA-256 校验文件
- Shell 和 PowerShell 安装器
- 构建来源证明与 `release-gate.json`

### macOS / Linux

```sh
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/jfmoe/forager/releases/latest/download/forager-installer.sh | sh
```

安装器默认把二进制放到 `$CARGO_HOME/bin` 或 `$HOME/.cargo/bin`，并在需要时配置 `PATH`。

### Windows PowerShell

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/jfmoe/forager/releases/latest/download/forager-installer.ps1 | iex"
```

希望固定版本或先检查安装内容时，请从 [Releases 页面](https://github.com/jfmoe/forager/releases) 下载对应安装器或平台归档，并用同名 `.sha256` 文件验证归档。

## 初始化

交互式创建配置：

```sh
forager setup
```

检查配置和供应方状态：

```sh
forager doctor --format markdown
```

配置默认写入 `~/.config/forager/config.toml`，同时尊重 `XDG_CONFIG_HOME`。可以用以下命令查看和修改配置：

```sh
forager config path
forager config list
forager config set providers.exa.keys '["YOUR_API_KEY"]'
```

## 基本使用

普通搜索：

```sh
forager search "Rust async drop 的最新进展" --format markdown
```

由调用方明确声明所需能力：

```sh
forager search "OpenAI API 最近有什么变化" \
  --capabilities docs_search,web_search \
  --format markdown
```

多来源研究：

```sh
forager research "对比主流 Rust CLI 分发方式" \
  --budget standard \
  --format markdown
```

直接调用 `research` 时需要配置分类器；通过 Agent Skill 调用时，Agent 会生成计划并通过 `--plan` 注入。

读取已知网页：

```sh
forager fetch https://doc.rust-lang.org/cargo/ --format markdown
```

`fetch` 对 URL 与 PDF 使用同一条默认 `Tavily → Firecrawl → Jina` 链，成功时只返回 provider 无关的 Markdown 正文；诊断与 provider attempts 不会混入正文。

查看全部命令：

```sh
forager --help
```

## 安装 Agent Skill

CLI 负责实际检索，仓库中的 `forager` Skill 负责教 Agent 选择命令、声明能力和组织研究计划。安装二进制并完成配置后，可为当前项目安装 Skill：

```sh
npx skills add jfmoe/forager
```

Skill 要求 `forager >= 0.1.0`。

## 设计边界

- `search` 接受调用方声明的完整能力集合；forager 不擅自扩张该集合。
- `research` 可以接受调用方通过 `--plan` 注入的 Schema v1 研究计划。
- `fetch`、`map` 以及 `exa`、`context7`、`anysearch` 子命令提供显式直连入口。
- 凭据保存在本地配置中；命令输出和持久化 journal 会对敏感 URL 参数脱敏。
- 能力暂时不可用时，结果会明确报告 `capability_gaps`，不会把缺失能力伪装成成功覆盖。

领域术语与约束见 [CONTEXT.md](CONTEXT.md)，完整规格见 [docs/spec/forager/](docs/spec/forager/)。

## 从源码构建

源码构建面向贡献者，不是主要安装方式。项目使用 `rust-toolchain.toml` 固定开发工具链：

```sh
git clone https://github.com/jfmoe/forager.git
cd forager
cargo install --path . --locked
```

提交前运行与 CI 对齐的检查：

```sh
cargo fmt --check
cargo check --all-targets --all-features --locked
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --lib --bin forager --all-features --locked
cargo test --tests --all-features --locked
```

## 发布

版本和 changelog 由 release-plz 管理，跨平台归档、安装器、校验和与 GitHub Release 由 dist 生成。Release 只有在五个目标平台的归档校验、架构检查和真实二进制冒烟测试通过后才会公开。
