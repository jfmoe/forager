# Rust CLI 生态与发布流水线选型调研

> 调查日期：2026-07-23；对应 issue：[jfmoe/smartsearch#54](https://github.com/jfmoe/smartsearch/issues/54)（父 issue #52）。
>
> 当前仓库基线：`11ac647`（smart-search 0.7.1，Python）。
>
> 性质：为“把约 14K 行 Python 多 provider 网络搜索 CLI 重写为 Rust”所做的选型事实报告。每项给出**推荐 + 理由 + 一手证据 + 取舍**；“推荐”是基于证据的工程判断，不是已确定的架构决策。
>
> 版本口径：以下 crate 版本为调查当日 docs.rs / crates.io / GitHub API 的实际读数，随时间会变；引用时以“调查当日”为准。

## 执行摘要

| 主题 | 推荐 | 一句话理由 |
|---|---|---|
| CLI 框架 | **clap 4（derive API）** | smart-search 有 30+ 子命令与别名，clap 的子命令树、自动 help/补全、生态最成熟；MSRV 与 semver 策略清晰。 |
| async runtime | **tokio（多线程 rt）** | provider 并发/超时/取消/信号处理的事实标准；reqwest、reqwest-eventsource 均以其为基座。 |
| HTTP 客户端 | **reqwest 0.12 + rustls-tls** | 与 tokio 深度集成，`stream` feature 直接给出 `bytes_stream()`；rustls 避免 OpenSSL 交叉编译痛点。 |
| 流式 SSE | **reqwest-eventsource（自动重连）/ eventsource-stream（裸解析）** | 官方 SSE 语义 + 断线重连；provider 若是 OpenAI 兼容 SSE，直接复用。 |
| 序列化 | **serde + serde_json（+ 按需 toml）** | 无争议的事实标准；provider 请求/响应契约即 `#[derive(Serialize/Deserialize)]`。 |
| 配置 | **figment**（首选）或 config（次选） | figment 的来源溯源（provenance）让“某个 key 来自哪层配置/哪个环境变量”可精确报错，契合多 provider 凭据池。 |
| 错误处理 | **thiserror（库/领域层）+ anyhow（顶层编排）+ miette（面向用户 CLI 报告）** | 三者分工互补，可组合；不是三选一。 |
| 发布流水线 | **dist（原 cargo-dist）** 为主，**release-plz** 管版本/changelog | dist 仍在活跃维护（v0.32.0，2026-05），一条命令生成多平台 GitHub Release CI；注意其非“大厂背书”的维护风险。 |
| 架构可借鉴 CLI | **aichat**（provider 抽象）+ **参考其依赖清单** | `register_client!` 宏 + 每 provider 一个模块 + serde tagged enum，是多 provider CLI 的成熟范式。 |

## 背景与范围

- 目标产物形态：一个面向 AI agent 的**多 provider 网络搜索 CLI**，特征是：大量子命令（当前 Python 版有 `search/fetch/map/exa/zhipu*/context7*/anysearch*/research/route/doctor/config` 等，见 `smart-search --help`）、每个 provider 走 HTTP（含 Remote MCP 的 SSE）、需要按 capability 路由与 fallback、需要分发预编译二进制给用户。
- 调查方法：优先一手源——docs.rs（crate 官方文档）、crates.io、目标仓库源码（GitHub raw / API）、GitHub Releases 元数据。二手来源（Reddit PSA、维护者个人页）仅用于交叉印证 dist 的维护状态，并已显式标注。
- 未覆盖：具体性能基准（未跑 benchmark）、Windows/macOS 代码签名与公证细节、MSRV 对具体依赖树的精确约束（需在真实 `Cargo.lock` 上验证）。

## 1. CLI 框架

### 推荐：clap 4，derive API

**理由**

- smart-search 是“一个二进制、多子命令 + 多别名”的结构。clap 原生支持 `Subcommand` 派生枚举、子命令别名（`visible_alias`）、自动 `--help`/`--version`、`ValueEnum`、shell 补全（`clap_complete`）、man page（`clap_mangen`），这些正是当前 Python argparse 手写别名表（`search,s / fetch,f / research,rs …`）要复刻的能力。
- 维护与稳定性有明确承诺：clap 属于 Rust CLI 工作组（WG-CLI），遵循 semver，“大破坏性变更间隔约 6–9 个月”，MSRV 为“最近两个 minor Rust 版本，当前 1.74”（docs.rs/clap，调查当日版本 **4.6.1**）。这对一个要长期维护的重写项目很重要。
- derive API 让子命令定义与结构体一一对应，可读性/可导航性好；需要动态构造时仍可下沉到 builder API（同一 crate，二者可混用）。

**一手证据**：`https://docs.rs/clap/latest/clap/`（“Aspirations”段落写明 WG-CLI、semver、6–9 个月破坏节奏、MSRV 1.74；feature flags：`derive/cargo/env/unicode/wrap_help`）。旁证：aichat 的 `Cargo.toml` 用 `clap = { version = "4.4.8", features = ["derive"] }`。

**备选与取舍**

- **bpaf 0.9.26**（docs.rs/bpaf）：同时提供 derive 与组合子（combinatoric）两套 API，轻量、灵活，适合复杂/不规则参数关系。取舍：生态与文档广度不及 clap，团队熟悉度低。
- **argh 0.1.19**（docs.rs/argh）：Fuchsia 出品，**为二进制体积优化**、仅 derive、help 较朴素。若首要目标是最小体积可考虑；但其功能与生态弱于 clap，不适合子命令多、要补全/彩色 help 的场景。
- **lexopt 0.3.2**（docs.rs/lexopt）：命令式、零依赖、极简，把“怎么解析”留给你自己写。适合追求极致精简/零依赖的小工具；对 30+ 子命令是负担，不推荐。
- pico-args 同属极简派，取舍同 lexopt。

结论：子命令规模 + 生态成熟度 + 长期维护承诺三者叠加，clap 是明确首选；仅当“二进制体积”成为硬约束时才评估 argh。

## 2. async runtime 与 HTTP 客户端

### 推荐：tokio（多线程 runtime）+ reqwest 0.12（rustls-tls）

**理由**

- provider 编排天然是并发 I/O：多 provider 并行发起、按 capability 超时、fallback、取消。tokio 是该类工作负载的事实标准，且 reqwest / reqwest-eventsource / hyper 全部以 tokio 为基座——选 tokio 才能无缝复用这条链。
- reqwest 与 tokio 深度集成，提供 JSON、超时、代理、multipart、重定向策略等开箱能力；`stream` feature 直接给出响应体流式读取（见第 3 节）。
- TLS 选 **rustls**（`rustls-tls`）而非默认 native-tls/OpenSSL，可显著降低跨平台交叉编译与静态链接的痛点——这对第 4 节的多平台二进制发布是关键。aichat 正是这么配的：`reqwest = { version = "0.12.0", default-features = false, features = ["json","multipart","socks","rustls-tls","rustls-tls-native-roots"] }`。

**一手证据**：`https://docs.rs/reqwest/latest/reqwest/struct.Response.html`（`bytes_stream()` 标注 “Available on crate feature `stream` only”，返回 `impl Stream<Item = Result<Bytes>>`，当日 reqwest 0.12.x）。aichat `Cargo.toml`：`tokio = { version = "1.34.0", features = ["rt","time","macros","signal","rt-multi-thread"] }`、reqwest 如上。

**取舍**

- runtime 备选 async-std 已基本退场，社区重心在 tokio；smol 更轻但生态窄。对一个要接大量 HTTP/SSE 库的 CLI，tokio 几乎无争议。
- 若某些子命令是纯同步、短命的（如 `config`），可用 tokio 的当前线程 runtime 局部化，避免为简单命令付出多线程调度成本。
- HTTP 客户端备选：`ureq`（同步、轻、无 tokio）适合纯同步小工具，但与流式 SSE + 并发编排的目标不符。

## 3. 流式 SSE 响应处理

### 推荐：优先 reqwest-eventsource；需要精细控制时用 eventsource-stream

**理由**

- 多数 provider（OpenAI 兼容、xAI Responses、部分 Remote MCP）以 **SSE** 推流。Rust 侧有清晰的两层方案：
  - **eventsource-stream 0.2.3**：在任意 `Stream<Item = Bytes>`（如 `reqwest` 的 `bytes_stream()`）上做 SSE 帧解析，产出 `Event { event, data, id, retry }`。用法：`resp.bytes_stream().eventsource()`。它只做“把字节流解析成 SSE 事件”，不管重连。
  - **reqwest-eventsource 0.6.0**：在 eventsource-stream 之上封装 `reqwest`，提供 `EventSource` 类型，**自动按 SSE 规范重试/重连**，产出 `Event::Open / Event::Message`。用法接近浏览器 `EventSource`。
- 选择准则：想要“开箱即用 + 断线自动重连”选 **reqwest-eventsource**；想要“自己掌握请求构造、鉴权头、错误分类、取消时机、只借用 SSE 解析”选 **eventsource-stream**（直接挂在 `bytes_stream()` 后）。对多 provider、需自定义 header/错误分类的 smart-search，两者会并存：默认走 reqwest-eventsource，特殊 provider 降到 eventsource-stream。

**一手证据**：
- `https://docs.rs/reqwest-eventsource/latest/reqwest_eventsource/`（0.6.0：“wrapper for reqwest to provide an Event Source implementation… uses eventsource_stream to wrap the underlying Bytes stream, and **retries failed requests**”；示例 `EventSource::get(...)` + `Event::Open/Message`）。
- `https://docs.rs/eventsource-stream/latest/eventsource_stream/`（0.2.3：示例 `client.get(...).send().await?.bytes_stream().eventsource()`）。
- 旁证：aichat 用 `reqwest-eventsource = "0.6.0"` + `tokio-stream` + `futures-util` 处理流式补全（`Cargo.toml`）。

**取舍 / 注意**

- 若某 provider 的“流式”其实是**分块 JSON / NDJSON**（不是标准 SSE），不要硬套 SSE 解析——直接用 `bytes_stream()` + 自定义分帧。当前 Python 版对 xAI Responses 的流式处理（见提交 `bb8482f`）需按此判断具体协议。
- 非流式请求无需 `stream` feature；只在流式路径开启，控制依赖面。

## 4. 配置、序列化与错误处理

### 4.1 序列化：serde + serde_json（+ 按需 toml/yaml）

**推荐**：无争议地用 **serde**。provider 的请求/响应契约、配置结构、缓存/日志条目都用 `#[derive(Serialize, Deserialize)]` 表达。JSON 用 `serde_json`；人写配置文件按团队偏好选 `toml`（Rust 生态默认）或 `serde_yaml`（aichat 用 yaml）。一手旁证：aichat `Cargo.toml` 同时依赖 `serde`、`serde_json`（`preserve_order`）、`serde_yaml`。

### 4.2 配置：figment（首选），config（次选）

**推荐 figment 的理由**

- smart-search 的配置是**分层的**：内置默认 < 配置文件 < 环境变量（大量 `*_API_KEY`、`*_BASE_URL`）< 命令行覆盖，且有“provider 凭据池”（见 ADR 0005）。figment 的核心卖点正是**来源溯源（provenance tracking）**：合并多来源后仍能精确指出“某个配置值来自哪一层/哪个环境变量”，从而给出精确的错误位置——对“凭据来自哪个账号/哪层配置”这类排障极有价值。
- API 直观：`Figment::new().merge(Toml::file(...)).merge(Env::prefixed("SMART_")).extract()`；`merge`（覆盖）与 `join`（补充）语义清晰。figment 由 Rocket 作者维护，成熟稳定（docs.rs/figment 调查当日 **0.10.19**）。

**config 次选**：`config` crate（docs.rs/config）同样支持默认值 + 文件 + 环境变量 + 程序化覆盖的分层合并，额外提供**配置文件热监听/重载**和**路径语法深度访问**。若未来需要“运行时监听配置变更热重载”，config 更顺手；但它的来源溯源不如 figment 精细。

**取舍**：CLI 一次性执行为主、极少长驻，热重载价值有限，而“凭据来自哪层”的可诊断性价值高 → figment 更契合。二者都基于 serde，迁移成本可控。

### 4.3 错误处理：thiserror + anyhow + miette（分工组合，非三选一）

**推荐分层用法**

- **thiserror**（dtolnay 出品，docs.rs/thiserror）：给**库/领域层**（provider adapter、路由、配置解析等）定义**具名、结构化**的错误枚举（`#[derive(Error)]` + `#[from]`）。对应当前 Python 版按 capability/provider/redirect/timeout 分类错误的诉求——用类型表达“认证失败/协议错误/超时/上游 4xx”等分类。
- **anyhow**（同作者，docs.rs/anyhow）：在**顶层编排 / main / 子命令处理**用 `anyhow::Result` + `?` + `.context(...)` 快速聚合与传播异构错误，无需为每处都定义枚举。aichat 全程用 anyhow（`use anyhow::{bail, Context, Result}`，见其 `src/client/common.rs`）。
- **miette 7.6.0**（docs.rs/miette）：面向**最终用户的 CLI 诊断报告**——彩色、带源码片段高亮、`help` 提示、错误码，并有 `NO_COLOR`/CI 下的无障碍叙述模式。适合把“配置写错了/缺 API key/URL 非法”渲染成清晰可操作的报错。miette 与 thiserror 组合良好（`#[derive(Error, Diagnostic)]`）。

**取舍**：三者可共存——领域层 thiserror 定义类型，编排层 anyhow 传播，用户边界 miette 渲染。最小可行组合是 thiserror + anyhow；miette 视“是否要打磨用户可读报错”而定（对面向 AI agent 的机器可读 JSON 输出，miette 的价值主要在人读 stderr 路径）。

## 5. 多平台二进制发布流水线（GitHub Releases）

### 推荐：dist（原 cargo-dist）为主线，release-plz 管版本/changelog

**dist 现状（重点核实其维护状态）**

- **仍在活跃维护，未归档、未停摆**。GitHub API（调查当日）：`axodotdev/cargo-dist` `archived=false`，`pushed_at=2026-07-20`，2074 star；最新正式版 **v0.32.0（2026-05-21）**，此前 v0.31.0（2026-02-23）；近期提交以 dependabot 依赖升级 + 维护者 Misty De Meo 的修复为主（如 `deps: downgrade color-backtrace`，2026-06-03）。
- **更名**：约 2024-10（0.24.0 起）从 `cargo-dist` 更名为 **`dist`**，以覆盖 Rust 之外的语言/工具；仍可用 `cargo dist` 或独立 `dist` 调用，仓库仍在 `axodotdev/cargo-dist`。
- **维护风险（须知）**：2024 下半年原作者退出后一度出现“是否停维护”的社区担忧（Reddit “PSA: cargo-dist is not dead”），随后 Misty De Meo 于 2024 末/2025 初接手恢复维护。**它不是大厂长期背书的项目，属小团队/社区维护**，选型时应把“未来维护不确定性”计入风险，并保持可迁移到纯 GitHub Actions 的退路。

**为什么仍推荐 dist**

- 一条命令 `dist init` 生成完整的 GitHub Actions `release.yml`：plan（等 tag）→ build（各平台机器编译二进制 + 安装器）→ host（上传 artifact）→ publish → announce（生成 GitHub Release）。它自动覆盖 tarball/zip、多平台矩阵、机器可读 manifest、shell/npm/Homebrew 安装器等——正是把 Python 版 `pip install` 分发替换为“预编译二进制 + 一键安装脚本”的最短路径。
- 一手证据：`https://raw.githubusercontent.com/axodotdev/cargo-dist/main/README.md`（“it generates its own CI scripts… `dist init` will generate release.yml, which implements the full pipeline of plan, build, host, publish, announce”）；Releases 元数据经 `gh api repos/axodotdev/cargo-dist/releases` 核实。

**配套 / 备选（均在活跃维护，经 GitHub API 核实）**

- **release-plz**（`release-plz/release-plz`，`pushed_at=2026-07-21`，1429 star）：从 CI 自动化版本号提升 + changelog + 发 Release PR + 发布到 crates.io。与 dist 互补（release-plz 管“何时/什么版本发”，dist 管“构建/分发二进制”）。
- **taiki-e/upload-rust-binary-action**（`pushed_at=2026-07-03`，320 star）：**不想引入 dist 时的轻量替代**——纯 GitHub Actions，矩阵构建 + 上传二进制到 Release。可控性最高、依赖最少，代价是自己写矩阵与安装器。**这是 dist 维护若恶化时的迁移退路。**
- **cross-rs/cross**（`pushed_at=2026-07-15`，8273 star）：容器化“零配置”交叉编译，用于覆盖 Linux 各架构 / musl 静态链接等目标三元组，可与上面任一方案组合。
- **cargo-bins/cargo-binstall**（`pushed_at=2026-07-18`，2790 star）：让用户 `cargo binstall smart-search` 直接拉预编译二进制（读取 Release 中约定命名的 artifact），提升安装体验；与 dist 产物命名兼容。

**推荐组合**：`dist`（构建/分发/安装器 + CI 生成）+ `release-plz`（版本/changelog）+ `rustls`（避免交叉编译 TLS 痛点）+ 预留 `taiki-e/upload-rust-binary-action` 作为去 dist 化退路。

## 6. 架构可借鉴的同类 Rust CLI

### 首选参考：aichat（`sigoden/aichat`）

多 provider LLM CLI，10k+ star，`archived=false`（`gh api`，`pushed_at=2026-02-23`）。其 **provider 抽象**对 smart-search 直接可迁移。

**crate/模块组织**（`gh api .../contents/src/client`）：`src/client/` 下每个 provider 一个模块——`openai.rs / claude.rs / gemini.rs / cohere.rs / bedrock.rs / vertexai.rs / azure_openai.rs / openai_compatible.rs`，外加公共件 `common.rs / macros.rs / message.rs / model.rs / stream.rs / mod.rs / access_token.rs`。这与 smart-search 现有 `providers/` 一 provider 一文件的组织同构。

**provider 抽象方式**（一手源码）：

1. **`Client` trait（`src/client/common.rs`）**：定义 provider 的统一接口，带**默认方法**（如 `build_client()` 统一构造 `reqwest::Client`：超时、代理、user-agent；`chat_completions()` 统一处理 dry-run、准备请求数据、调用 `chat_completions_inner`）。每个 provider 只需实现少量 `*_inner`/`*_streaming` 方法。错误用 anyhow（`with_context`）。
2. **`register_client!` 声明宏（`src/client/macros.rs`）**：一处集中登记所有 provider——展开生成：
   - `mod $module;` 引入每个 provider 模块；
   - 一个 **serde tagged 枚举** `ClientConfig`（`#[serde(tag = "type")]`，每个变体 `#[serde(rename = $name)]`，含 `#[serde(other)] Unknown`）——即**用一个枚举承载“配置文件里 `type: openai/claude/...` → 具体 provider 配置”的反序列化**；
   - 每个 provider 的 `$client` struct（持有 `global_config/config/model`）及其 `init/name/list_models`；
   - 顶层 `init_client()`（按 model 依次 `or_else` 尝试各 provider 的 `init`）、`list_client_types()`、`create_client_config()`。
3. **模型注册**：`models.yaml` 经 `include_str!` 内嵌，`ALL_PROVIDER_MODELS` 用 `LazyLock` 懒加载（可被本地配置覆盖）。
4. **依赖栈印证选型**（`Cargo.toml`）：`clap`(derive) + `tokio`(multi-thread) + `reqwest`0.12(rustls) + `reqwest-eventsource`0.6 + `serde/serde_json/serde_yaml` + `anyhow` + `futures-util` + `tokio-stream` + `async-trait` + `indexmap`。**这正好等于本报告第 1–4 节的推荐组合**，是很强的交叉印证。

**对 smart-search 的可借鉴点**：用“`Client`/`Provider` trait（默认方法收敛公共 HTTP/超时/错误逻辑）+ 每 provider 一模块 + 一个声明宏集中登记并生成 serde tagged 配置枚举”这套范式，替换当前 Python 版的 provider 分发；capability 路由/fallback 可叠在 trait 之上。

**注意差异**：aichat 面向交互式 REPL（依赖 `reedline/crossterm/inquire`），smart-search 是一次性、机器可读 JSON 输出为主，UI/REPL 相关依赖不必照搬。

### 次选参考（可选，未逐文件深读）

- 需要“单一二进制、大量子命令、纯 CLI 无 REPL”的组织范式时，可参考 clap 官方 cookbook / rust-cli 工作组的《Command-Line Apps for Rust》书（`https://rust-cli.github.io/book/`，clap 文档中列为 related）。本报告未逐章核实其内容，仅作延伸阅读指针。

## 来源索引

### crate 官方文档（docs.rs，调查当日版本）
- clap 4.6.1：`https://docs.rs/clap/latest/clap/`（Aspirations：WG-CLI / semver / MSRV 1.74；feature flags）。
- bpaf 0.9.26：`https://docs.rs/bpaf/latest/bpaf/`。
- argh 0.1.19：`https://docs.rs/argh/latest/argh/`（Fuchsia、code-size）。
- lexopt 0.3.2：`https://docs.rs/lexopt/latest/lexopt/`（命令式、零依赖）。
- reqwest（0.12.x）：`https://docs.rs/reqwest/latest/reqwest/struct.Response.html`（`bytes_stream()` 需 `stream` feature）。
- reqwest-eventsource 0.6.0：`https://docs.rs/reqwest-eventsource/latest/reqwest_eventsource/`（自动重连、`Event::Open/Message`）。
- eventsource-stream 0.2.3：`https://docs.rs/eventsource-stream/latest/eventsource_stream/`（`.bytes_stream().eventsource()`）。
- figment 0.10.19：`https://docs.rs/figment/latest/figment/`（provenance tracking、`merge/join/extract`）。
- config：`https://docs.rs/config/latest/config/`（分层合并、热监听、路径访问）。
- thiserror：`https://docs.rs/thiserror/latest/thiserror/`（`#[derive(Error)]`）。
- anyhow：`https://docs.rs/anyhow/latest/anyhow/`（trait-object 错误、`?`/`context`）。
- miette 7.6.0：`https://docs.rs/miette/latest/miette/`（诊断报告、无障碍模式）。

### 发布流水线（GitHub API + 官方源，均 `archived=false`）
- dist（原 cargo-dist）：`https://github.com/axodotdev/cargo-dist`，README `https://raw.githubusercontent.com/axodotdev/cargo-dist/main/README.md`；最新 v0.32.0（2026-05-21），`pushed_at=2026-07-20`，维护者 Misty De Meo（`gh api repos/axodotdev/cargo-dist/releases|commits`）。
- 维护状态交叉印证（二手，已标注）：`https://www.reddit.com/r/rust/comments/1noozk7/psa_cargodist_is_not_dead/`、`https://digipres.club/@misty/114813284894018835`。
- release-plz：`https://github.com/release-plz/release-plz`（`pushed_at=2026-07-21`）。
- taiki-e/upload-rust-binary-action：`https://github.com/taiki-e/upload-rust-binary-action`（`pushed_at=2026-07-03`）。
- cross-rs/cross：`https://github.com/cross-rs/cross`（`pushed_at=2026-07-15`）。
- cargo-bins/cargo-binstall：`https://github.com/cargo-bins/cargo-binstall`（`pushed_at=2026-07-18`）。

### 架构参考 CLI（一手源码 / GitHub API）
- aichat：`https://github.com/sigoden/aichat`（`pushed_at=2026-02-23`，10k+ star）。
  - provider 抽象：`https://raw.githubusercontent.com/sigoden/aichat/main/src/client/common.rs`（`Client` trait + 默认方法）、`.../src/client/macros.rs`（`register_client!` + serde tagged `ClientConfig`）。
  - 依赖栈：`https://raw.githubusercontent.com/sigoden/aichat/main/Cargo.toml`（clap/tokio/reqwest-rustls/reqwest-eventsource/serde/anyhow）。
  - 模块清单：`gh api repos/sigoden/aichat/contents/src/client`。
- Rust CLI 工作组书：`https://rust-cli.github.io/book/`（延伸阅读，未逐章核实）。

## 局限性

- 本调查为选型事实报告，未产出代码、未做性能基准、未在真实 `Cargo.lock` 上验证 MSRV/依赖树冲突。
- 版本号为调查当日 docs.rs/crates.io/GitHub API 读数，会随时间变化；落地前应重新核对最新版与 MSRV。
- dist 的“活跃维护”结论基于 GitHub API 元数据 + 发布记录，并用二手社区来源交叉印证其历史维护波动；其未来维护由小团队/社区承担，存在不确定性，报告已建议保留去 dist 化退路。
- aichat 是交互式 LLM CLI，与 smart-search 的“机器可读、一次性执行”定位有差异；借鉴范围限于 provider 抽象与依赖选型，UI/REPL 部分不直接适用。
- 网络检索经由本仓库 smart-search CLI 的 `fetch`（Tavily 通道）与 GitHub `gh api`；未使用通用 WebSearch（调查期间该通道限流）。所有关键结论均落到具体一手 URL 或 API 读数。
