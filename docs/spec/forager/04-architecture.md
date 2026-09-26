# 4. Rust 架构骨架

权威来源：[#58 Resolution](https://github.com/jfmoe/smartsearch/issues/58) 及其[补充决议（F1–F10）](https://github.com/jfmoe/smartsearch/issues/58#issuecomment-5078828791)。选型输入：#54（clap 4 derive、tokio + reqwest(rustls)、figment + serde、thiserror/anyhow/miette、dist + release-plz；aichat 为架构参考）。

## 工程形态：单 crate `forager`，bin + lib

不上多 crate 工作区（Rust 模块私有性已在编译期阻止跨私有面 import；单 crate → 工作区是机械重构）。两条纪律：① 模块默认私有，`lib.rs` 对外只开放 `app`、`config`、`types` 三个必要门面；跨模块共享必须显式 `pub(crate)` 且只能上移共享层，provider adapter 之间禁止横向 import；② `main.rs`/`lib.rs` 分离，bin 只负责 clap 解析、输出渲染与退出码映射。

## 六分组物理结构与五层单向依赖

`src/` 以职责形成六个物理分组；`lib.rs` 用显式 `#[path]` 声明保持既有 crate 内模块名，不把目录本身变成新的公共 API 层级。

- **`cli/`**：CLI 参数定义、应用分发与 `app` 公共门面；参数树在 `args.rs`，分发在 `dispatch.rs`；`forager platform` 命令组的静态参数树与分发在 `platform.rs`。
- **`core/`**：engine（各 seam 的 provider 链）、platform_chain（平台 route 规划、cursor 与 search 链）、platform_fetch（平台 fetch 的元数据链与正文段）、platform_checklist（仅测试构建：新平台接入清单一致性检查）、search_fanout（普通 search 的辅助能力 fan-out 与结果合并）、chain、classifier 与 Attempt Trace。
- **`capabilities/`**：Capability Catalog 与 platform catalog、Provider Credential Pool、跨进程限速（`rate_limit`）、Provider HTTP Read Contract（`net`）及 provider adapter（含平台 route adapter）。
- **`evidence/`**：Research Evidence Pipeline、Search Result Journal 与 stderr attempt log。
- **`infra/`**：config、secure filesystem、共享私有状态文件（`state_file`）、redaction 与零 IO 的 `types` 基底。
- **`ops/`**：doctor 与 smoke 运维入口。

物理分组表达职责归属，调用关系仍遵守五层从上到下的单向纪律：

```
入口（main）
  → 应用（cli/app、args、dispatch）
    → 能力编排（engine、platform_chain、platform_fetch、search_fanout、research、classifier、doctor、smoke、journal）
      → 能力基础设施（catalog、providers、credentials、rate_limit、net、config、secure_fs、state_file、redact）
        → 类型基底（types）
```

上层可以依赖下层，下层不得反向依赖上层；同层共享行为必须放到该职责的唯一拥有模块，再以最窄的 crate 内可见性提供。`catalog` 独立于 config 与 providers，二者只单向消费它；`catalog` 只从 `rate_limit` 取访问策略类型，`rate_limit` 不依赖 `catalog`。provider adapter 只消费 `providers/shared`、`providers/execution` 等共享拥有模块，不互相 import；平台 route adapter 同样不 import 任何 capability provider。平台 fetch 的正文段由编排层的 `platform_fetch` 调用 `engine::fetch`，route adapter 只声明候选正文 URL。

- **应用组合层**（F1）：`cli/app.rs` 只公开参数与分发门面；`dispatch.rs` 先构造共享 `NetworkDependencies`，再按命令建立 `AppContext<P>`、`FetchContext`、`SearchContext` 或 `ResearchContext`，各自持有所需的 runtime、配置与网络依赖。provider 实现与路由策略留在下层模块，Search Result Journal 仍由分发层在命令终态统一落笔。
- **`types` 类型基底**：零 IO 纯类型层——ErrorKind、ProviderError、Capability、`PlanCapability`（plan 语境独立三值枚举）、各 Outcome、ProviderAttempt、Source、ResearchPlan Schema v1、Deadline、薄正文阈值常量，以及平台形状（Platform、Platform Ref 与 canonical URL 推导、Content Depth、平台选项、条目、结果页与 fetch 结果）。所有跨层形状的唯一定义点。`infra/types/` 是目录模块，按职责分为 `capability`、`research`、`error`、`attempt`、`search`、`outcome`、`platform`、`deadline` 私有子模块，由 `mod.rs` 统一再导出，公共路径保持 `forager::types::*`。后续平台只在 `platform` 叶子中增加形状；公开的平台身份类型不引用 crate 私有的 `ProviderId`。
- **`net` 网络边界**：共享 HTTP client 构造、RetryPolicy、SSE 解析、status→ErrorKind 唯一映射、McpClient。
- 输出格式化保留在 bin 侧，出现第二个消费者再提升为独立共享模块。

## Provider 契约与 registry

- **每 seam 一个 trait + 专属返回类型**：`WebSearch`/`DocsSearch`/`WebFetch`（supplemental 与主搜索共用 WebSearch 签名，registry 区分链序归属）；`SearchOutcome`/`DocsOutcome`/`FetchOutcome` 共享 ProviderAttempt/Source 构件。一个 provider＝一个 struct，同一 `Arc` 实例登记进多条 seam 链。
- **seam 支持矩阵**＝「谁 impl 了哪个 trait」的编译期事实；`order` 校验查 registry。**`map` 命令**＝tavily 直连操作（`site_map`），不设独立 seam trait（唯一 provider，需要时提升为 trait 是纯增量）；registry 在 tavily 描述内登记该操作。
- **凭据要求**：registry 的 `credentials_required` 是 provider 是否需要凭据的唯一来源。执行路径、doctor 与 smoke 用同一判定（`ProviderRegistration::is_configured`）决定「已配置」：需要凭据的 provider 要求 keys 非空，不需要凭据的 provider 恒为已配置。registry 校验与 smoke 的注册完整性检查都不要求 provider 需要凭据。
- **匿名执行**：不需要凭据的 provider 经 `execute_anonymous` 执行：不 claim、不轮换、不注入认证；每个 attempt 的 `credential_index` 为 0，`rotation_count` 恒为 0，这两个字段对匿名 provider 没有凭据含义。需要凭据的执行入口 `execute_v2` 遇到空凭据池时在发送前以 Auth 失败，不 panic。共享构造器不断言 provider 需要凭据。
- **单次发送契约**：每次发送返回可选的 status 与解码值。现有调用方照常传入 status：多数 HTTP 调用方传入响应的实际 status，Exa 以及 Context7、AnySearch 的 MCP 调用成功时记为 200；非 HTTP 传输可以不带 status，attempt 的 `http_status` 随之为空。
- **registry 最小职责**（F10）：唯一登记 `ProviderId`、支持 seam、凭据要求、访问策略、doctor probe、构造入口；注册合法性为「属于某个 Capability Catalog、某个 platform catalog，或拥有 operation 之一」，启动期校验、catalog 单测与 smoke 注册完整性检查共用 `catalog::has_owner`；config/doctor/capability status 从同一描述读取身份，不各设 allowlist；engine 只调用 seam trait 并聚合 `ProviderAttempt`，禁止按 provider id/model 分支；openai-compatible 的 model 候选、断路器、transport fallback 全部封装在 provider 内。不引入宏、不生成 clap 树。

## Platform 维度

Platform 与 Capability Seam 并列（ADR 0019），完整接入契约见第 7 章。

- **platform catalog**：`catalog::PLATFORMS` 为每个平台登记平台 id、search 与 fetch 的 route 集合，以及已提升为 trait 的平台操作的 route 集合。它是平台 route 隶属关系的唯一出处；Capability Catalog 集合保持原含义，不塞入平台 route。
- **配置**：`platforms.<id>.order` 以 `Rule::PlatformOrder` 对照 platform catalog 校验，拒绝重复项与不属于该平台的 route，允许为空（禁用平台）；文件加载与 `config set` 走同一规则，env 按既有公式派生。runtime 投影为 `PlatformRuntimeConfig`，每项是 `SeamEntry<PlatformRouteConfig>`。
- **平台 seam trait**：`PlatformSearch` 与 `PlatformFetch` 不使用泛型，形状与兄弟 seam 相同，分别返回 `ProviderError` 或 `PlatformSearchOutcome`／`PlatformFetchOutcome`。factory 的 `build_platform_search` 与 `build_platform_fetch` 按 `PlatformRouteConfig` 的变体构造 route（route 身份由配置变体决定，不另传 `ProviderId`），`platform_search_support` 与 `platform_fetch_support` 调用 route 的纯函数支持检查；支持检查只读取请求（search 为选项与页位置，fetch 为深度），不联网。
- **平台链**：`platform_chain::plan_routes` 为每个平台操作做纯规划——可用 route＝order ∩ 该操作的 route 集合 ∩ 已配置 route；集合为空为 Config（退 3）；不支持显式选项的 route 记为 Skipped attempt（`error_kind` 为空），全部不支持为参数错误（退 2）；cursor 的 route 不能运行该请求（例如页位置不是它签发的）同样为参数错误。这些判定不经过链执行器的 gate。然后以共享链执行器按 `SlicedEven` 运行 route 链，沿用 LegitimateEmpty 语义，永不跨平台 fallback。
- **平台 fetch**：`platform_fetch` 以同一规划与链执行器运行 fetch route 链取得元数据；元数据失败即命令终态。`full_text` 深度下，答复的 route 在 `PlatformFetchOutcome.content_urls` 中按顺序声明该版本的正文 URL（arXiv：HEAD 探测为 404 时只有 PDF，否则 HTML 在前、PDF 在后），`platform_fetch` 对每个 URL 运行 `engine::fetch`，首个成功者即正文；有后续 URL 时本段最多使用剩余预算的一半；全部失败时以最后一条链的 `ProviderError` 为终态，attempts 按元数据、探测、正文的顺序合并。正文落盘与 `--format content` 的交付由 `cli/platform.rs` 负责，写入失败为 Runtime。
- **cursor**：`v1.<route>.<payload>`，payload 是 base64url（无填充）编码的 JSON 请求（查询词、limit、选项与 route 自有的页位置）。带 cursor 的请求只在该 route 上执行；route 已不可用、版本未知、无法解码、route 未知或属于其他平台均为飞行前参数错误。
- **attempt target**：平台操作的 attempt 以 `AttemptTarget::Platform { platform, operation }` 序列化为 `{"platform", "operation"}`；链执行器的合成 attempt 使用调用方传入的 `AttemptTarget`。

## 错误模型

- **`ErrorKind` 10 变体**：Auth / RateLimited / QuotaExhausted / Parameter / Config / Timeout / Network / Quality / Evidence / Runtime。三方法：`is_retryable()`；`rotates_credential()`（RateLimited|QuotaExhausted——轮换优先于重试，429 不重试）；`family() → Transport|Content`（Quality/Evidence 为 Content 族）。**无 `exit_code()` 方法**。
- 「empty」从错误分类法除名：直连命令空结果＝`Ok(空 Outcome)` 退 0；证据管线的证据不足＝Evidence 退 5（域切分见第 1 章）。
- **退出码两阶段**：飞行前（argv→2、config/未知 env→3）只由预检产生；飞行后由**归因总函数**产生。attempt 级 Parameter 不映射退 2。
- **归因总函数**（F4 + #59 B3）：作用于单条 provider 链，只按每个 provider 的**最终 attempt** 归约（重试不参与计数），对各 kind 按**优先级全序**取最大，与重试次数、失败顺序无关。已有成功响应进入质量/证据阶段且终局失败＝Content 优先退 5，不被后续网络失败覆盖；所有可用 provider 均未产生可验证响应才退 4；同质失败顶层透传原 kind（如全 401 报 auth_error，退出码仍按族）；attempts 永远带原始 kind。
  - **全序表定稿**（低 → 高；Content 族恒高于 Transport 族）：`Network < Timeout < RateLimited < QuotaExhausted < Auth < Parameter < Runtime < Quality < Evidence`。定义域为**飞行后 kind**（ErrorKind ∖ {Config}）：`Config` 只在飞行前预检产生（退 3），**`ProviderAttempt` 不得携带 Config**——此为类型不变量，进 unit 真值表。族间关系与全序存在性为契约（真值表穷举验证）；族内排布编码期可微调，调整须同步更新真值表。
- `ProviderError`（thiserror）定义在 types 门面：kind + 脱敏消息 + attempts（每个 attempt 带 provider、status 与耗时）+ 非致命诊断；`forager::app::ProviderError` 作为再导出保留，types 下的新可达路径是预期结果。status→kind 映射只在 net 一份。
- **分类器已配置但失败**：降级继续 + stderr 警告 + journal 落痕，不影响退出码；research 裸调用下采用**固定最小降级 plan**（单步 web_search）继续执行（#59 H8）。
- miette 只渲染 text 人类报错；契约路径（JSON）不经 anyhow/miette。

## net 层

1. 全进程一个 `reqwest::Client`（rustls），构造参数单点。`net::build_client` 统一拥有 TLS、connect/read timeout、pool、User-Agent 与 `redirect::Policy::none()`；任何 provider endpoint 的 3xx 保留原 status，不请求 `Location`，经共享 status mapper 形成不可重试、不可轮换的 Runtime。当前不设 same-origin、provider 或状态码例外，也不构造第二个 client。
2. **`Deadline` 贯穿调用链**；main search 的主 backend、主 model 与 SSE 首次尝试可使用全部剩余预算，fallback 只消费失败后的残余。classifier 与辅助 seam 的 attempt 上限＝`min(层上限, 剩余预算 / 剩余必要 fallback 槽位数)`（F3）；耗尽→Timeout、保留已完成 attempts。辅助 seam 的槽位可执行定义见第 5 章 M17，main search 的边界与已接受后果见 ADR 0007。
3. SSE 只用 **eventsource-stream 裸解析**（不用 reqwest-eventsource——其自动重连绕过预算与 attempt 记录）；NDJSON 同入口换分帧器。
4. **`McpClient`** 统一 HTTP、Deadline、可选 session、JSON/SSE、JSON-RPC 与错误映射；只有 server 返回 session 时才发送 initialized notification 与后续 session header，缺 session 时直接 tools/call。session 过期重握手只服务实际发放 session 的 server，受同一 Deadline 约束。共享 client 接收 provider-owned 静态 header map：AnySearch 每个请求发送 `X-Anysearch-Client: mcp/1.0.0`，Context7 不发送旧 `X-Context7-Source`。`result.content` 存在但非标准数组时为 Runtime；structuredContent-only 路径继续可消费。**语义错误解码**（F5）：统一识别 JSON-RPC `error` 与 `result.isError`；可识别限流/配额文本映射 RateLimited/QuotaExhausted（触发轮换），未知 tool error 映射 Runtime 且不重试；归类先于轮换/重试决策。
5. `RetryPolicy` 参数化；可重试集合由 `is_retryable()` 决定；执行序固定：轮换 → 重试 → 上抛。
6. 4 MiB 成功截断仅属于 Web Fetch。xAI/OpenAI-compatible SSE、Exa JSON、Context7/AnySearch MCP 保留 4 MiB 协议硬上限，超限整体为 Runtime，不从不完整协议包装合成成功；所有非 2xx 错误正文继续限制为 64 KiB。

## 主搜索流式终态与规范化

- xAI Search 与 ModelProbe 都发送 role-array input；xAI 必须看到 `response.completed`，completed 无 `output_text` 为 Runtime。failed/incomplete 按 payload code/status/reason 映射现有 ErrorKind，只有映射为 Network/Timeout 的终态重试，RateLimited/QuotaExhausted 轮换，其他保持非重试。
- OpenAI-compatible 以 `[DONE]` 或非空 `finish_reason` 完成；底层 transport 干净 EOF 且已累计非空 answer 也可成功。transport error、空 EOF 和任一非空 SSE data 的畸形 JSON 都为失败，不跳过坏帧继续组装；xAI 不复用 clean EOF。
- 两类主搜索在完整响应组装后、attempt 判定成功前进入同一个 normalizer：先删除完整闭合、大小写不敏感且可跨行的 `<think>...</think>`，再投影末尾显式 Sources/References/Citations/来源/参考资料/引用标题块与 `[[N]](HTTP(S) URL)`。不做策略/prompt-injection 关键词过滤，不解析 `sources(...)`、任意 `<details>` 或尾链猜测；规范化后 answer 为空为 Runtime。provider 引用注释中仅由数字构成的标题是引用序号而非页面标题，按无标题处理。来源按脱敏后的公开 URL 稳定去重。

## 凭据池与断路器

分界原则：跨进程必须共享的落盘，策略性短时效的留进程内。

- **池**：游标落盘 `$XDG_STATE_HOME/forager/credential_pool_state.json`，fs2 有界文件锁，「锁内取号推进 / 拿不到锁乐观降级」语义保持。**状态文件不变量**（F6）：带 schema version、只存非敏感索引；同目录 `0600` 临时文件 + fsync + 原子 rename；解析失败只复位受影响 provider 的游标并发非致命诊断，不升 config_error、不阻断搜索。`CredentialPool` struct 作为显式参数传入，无 reset 钩子。`classifier.keys` 走同一实现。
- **共享状态文件**：`state_file` 统一拥有状态目录解析（`XDG_STATE_HOME`，否则 `HOME/.local/state`，均须为绝对路径）、私有目录与有界文件锁、原子写，以及进程内串行化（咨询文件锁不能可靠排斥同进程内的其他句柄，因此同一状态文件的进程内读写经该文件的互斥逐个执行；调用方可以先限时等待轮次，再执行阻塞工作）。凭据池与跨进程限速共用它，各自决定锁不可用时的策略。
- **model 断路器**：进程内显式 `ModelBreakers` struct（阈值 2 / 冷却 600s），不落盘。

## 跨进程限速

`rate_limit` 统一负责跨进程请求节奏。访问策略包括最小间隔与最大并发，由 provider 注册信息的 `access_policy` 声明（`arxiv_api`：每 3 秒 1 个请求、并发 1）；对受限 endpoint 的每次发送（包括重试、doctor 的 shallow 可达性探测与 deep 探测）都必须先调用 `RateLimiter::acquire` 申请时间窗口。`providers::route_limiter` 按注册信息构造限速器；route adapter 在单次发送内先取窗口再发请求，窗口的等待计入该 attempt 与命令的 Deadline。

- **算法**：先在进程内按最大并发取得 permit，再取得跨进程连接槽位：状态目录下每个 route 有最大并发个槽位锁文件（`rate_limit_<route>.<n>.lock`），异步轮询到第一个空闲槽位；两段等待都计入 Deadline，超时为 Timeout。然后在 Deadline 内等到状态文件的进程内轮次（此时尚未预留，超时不会占用窗口），再在共享状态锁内读取该 route 上次预留的时刻，计算下一个窗口 = max(现在, 上次预留 + 最小间隔)。需要等待的时间不小于剩余预算时，直接以 Timeout 结束，不写入预留；否则写入新的预留时刻，释放锁，然后在锁外等待到该绝对时刻。permit 与槽位锁持有到发送结束，因此共享状态目录的所有进程同时在途的请求不超过最大并发；进程退出时操作系统释放槽位锁。
- **时钟**：预留时刻是墙钟毫秒，从构造时的墙钟起按 Tokio 时钟推进，因此预留、等待与 Deadline 在同一时间线上。
- **失败语义**：状态目录无法解析、锁在有界等待内拿不到、状态文件不可读或不可写时，不发送请求，以 Runtime 结束；不复用凭据池拿不到锁时「乐观降级」的策略。状态文件损坏或 schema 不符时，视为该 route 在当前时刻已有一次预留，因此修复后的第一个窗口仍间隔完整的最小间隔。
- **已知边界**：间隔按预留时刻保证，某个进程在预留之后被调度延迟时，实际发送间隔可能短于最小间隔，不做发送后复核；协调范围是共享同一状态目录的进程。

## web_fetch 薄正文质量门控

Web Fetch 成功值是 **Normalized Fetch Content**：从成功 provider 响应解码出的 provider 无关 Markdown 正文，不含传输包装、attempts 或 diagnostic。默认链为 `Firecrawl → Tavily → Jina`（ADR 0018），直接 fetch、research 取证、search-side Web Fetch 与 PDF 都消费同一个 `engine::fetch`，不存在内容类型专属顺序。Tavily 固定请求完整 basic Markdown 且不传 query chunks；每个 Firecrawl provider attempt 只发送一次 `/scrape`，显式请求 Markdown、`onlyMainContent: true`、`timeout: 60000` 与 PDF parser `{"type": "pdf", "mode": "auto", "pageMarkers": true}`（只对 PDF 生效，HTML 输出不变；ADR 0018），`providers.firecrawl.timeout` 默认 60 秒与之对齐；不发送 `waitFor`、actions 或第二套 rendering probe；Jina 通过官方结构化 JSON 响应的 `data.content` 读取正文，不启用链接删除或通用 selector。Firecrawl 以 HTTP 403 表示拒绝抓取目标站点（凭据失效为 401）；net 的共享 status mapper 把正文声明 `support this site` 的 403 记为 Parameter 而非 Auth，该 attempt 照常落到下一家，错误消息取 provider 响应的 `error` 文本。

薄正文门只对 HTTP 成功并完成 provider 解码后的正文生效。两线命中任一 → `Quality`（Content 族）落下一家：**长度线**正文 < 200 字符；**密度线**唯一行数 ≤ 3 且总长 < 500。PDF 只适用长度线。全链皆薄 → 终态 Quality 退 5，attempts 带实测字符数。阈值为 types 具名常量，**不设配置键**。4 MiB 截断继续是成功加 diagnostic；只有截断后正文仍薄才 fallback。64 KiB 错误响应上限不变。

## Search Candidate 与 provider request contract

- `sources` 只保存 Primary Search Source；所有非主候选统一进入 `extra_sources`，不再公开独立 `vertical_results`。Search Candidate 固定包含必填 provider/capability/provider_data 与可空 title/url/summary；URL 只能是真实 HTTP(S)，summary 只复制 provider-native 描述、摘要或片段，provider_data 是 snake_case 强类型白名单。
- Documentation Search 默认顺序为 Exa → Context7（ADR 0017）：Exa 返回可直接抓取的 URL 候选；Context7 只在 Exa 没有可消费候选时被自动调用，直连 `context7` 命令不受影响。research 在每个子问题的发现名额内按能力轮转分配候选，先声明的能力不能占满名额。
- Context7 Documentation Search 只做 library resolve，使用 `url: null` 与 typed library locator；Research 通过现有 Documentation Search seam 的 provider-owned query-docs 读取它。有 URL candidate 走 Web Fetch。Evidence Index 对 Context7 保留 `library_id`、`path`、`url: null`，Citation Binding 使用非链接 `[eN]`；URL evidence 使用 `[eN](URL)`。不建立通用 provider registry。
- Exa direct search 的 text/highlights 按请求 flag 精确投影并保留 image/favicon；Documentation Search 保留 highlights 与媒体选择信号但不读取全文。不强制 `useAutoprompt`，不以 `id` 代替必填 URL。AnySearch Candidate 的 summary 复制 description；URL-less structured result 仅投影 `evidence_type=structured`。
- AnySearch 当前没有 verified manifest entry，显式未验证域继续报告 `schema_validation.status=unavailable` 并原样透传参数；不交付 test-only validator、fingerprint 或运行时 schema 依赖。Domain Discovery 将参数名后的 `(required)` 投影到 `parameter_schema.required`，但不从自然语言猜测 type/enum/default。Markdown decoder 只容忍编号标题与 `- **URL**:` 内的可变 ASCII 空白；没有编号标题时仅提取带 host 的 HTTP(S) URL 并按出现顺序去重，仍无 URL 时保留 structured result。
- Supplemental Tavily 请求显式发送 `search_depth: "advanced"`、`chunks_per_source: 1`（每条候选摘要至多一个约 500 字符的片段）、`include_raw_content: false`、`include_answer: false`；只有规范化后非空候选停止链。合法 `results: []` / `data.web: []` 继续 fallback，全链有合法空集且无非空结果时 `Ok(empty)`，`fallback=off` 只执行链头。仅跳过缺失/null/空白 URL 单项；非 HTTP(S) URL 单项同样跳过，其余合法候选保留；非字符串 URL、非对象条目、缺失或错误 container 为 Runtime。
- Tavily map 是 direct operation：CLI 校验 timeout `10..=150`、depth `1..=5`、breadth `1..=500`、limit > 0；合法 timeout 原样进入命令 Deadline 和 body，每个 attempt 再受 `providers.tavily.timeout` 与剩余预算较小值限制。
- 普通 search 在分类器决定能力集合后，让主搜索与辅助能力 fan-out 并发执行并共享同一绝对 Deadline（ADR 0016）；主搜索结束后按「分类器 attempts → 主搜索 attempts → 各能力分支（词汇表顺序）」合并。主搜索失败时，辅助 attempts 只追加用于诊断，已取得的候选与 capability gaps 由 `search_fanout::SearchFailure` 携带交付。
- search-side Web Fetch 成功结果以实际 provider 和 Normalized Fetch Content 的 300 字 preview 进入 `extra_sources`；抓取失败由 attempts、capability gap 和既有终态表达。Markdown 明确渲染 `Primary Sources` 与 `Extra Sources`；content 只返回主 answer；JSON、verbose 和 journal 消费同一结果角色。

## research 文件化交付

Research Evidence Pipeline 默认使用 standard 预算，将正文逐条写入 evidence Markdown，并只在成功 stdout 返回 Research Evidence Index。计划、未消费候选与 Research Recovery Manifest 分别写入 `00-plan.json`、`candidates.json` 与 `summary.json`；manifest 包含无正文的 evidence identity/metadata/path、coverage、gap、capability gaps、终态、attempts 与 `synthesis_policy`。未消费候选 artifact 以 `is_evidence: false` 明确其角色。失败 stdout 使用稳定小形状并以可空 `summary_path` 指向已成功写入且可直接读取的 manifest；locator 永不截断，极端长合法路径可超过 4 KiB 目标。

## journal

定位：结果面 + 过程面双记录。

- **结果面**：search 保存 query、answer 全文、仅属于主回答的 sources[] 与独立 supplemental candidates（含 search-side Web Fetch preview，URL 经统一脱敏器）；主搜索失败时保存 error_kind、message、已取得的完整 `extra_sources` 与 `capability_gaps`；research 保存 Evidence Index、coverage、artifact 路径与 capability gaps，不保存机械 answer/citations，也不重复 evidence 正文。Vertical Discovery Result 不复制到其他来源集合。
- **过程面**：plan 摘要（capabilities 终集 + 来源 + 分类器是否降级）、provider_attempts[]（provider、seam、error_kind、http_status、duration_ms、credential_index、retry/rotation 计数（匿名 provider 的 credential_index 与 rotation 计数恒为 0）、脱敏截断 500 字符错误消息、model、endpoint_host、断路器事件）、终态归因、budget 视图 `{total_ms, consumed_ms, exhausted}`、分类器耗时、capability_gaps。
- **字段白名单排除项**：请求/响应头、请求体、原始响应体、key 任何形式（含掩码）、分类器 prompt 原文。
- `capability_gaps` 形状：`[{capability, reason: no_configured_provider|partial_failure|all_attempts_failed, providers_skipped[]}]`，空则省略；结果 JSON 顶层 + stderr 警告 + journal 三出口。
- **落笔机制**（F7 修订）：`app` 层唯一终态写入器落笔一次，Ok/Err 皆写；panic hook 只做最小 stderr 诊断、**不写 journal**；孤儿任务 panic 与 kill -9 丢记录为已接受限制。
- **路径规则**（F8）：`journal.dir` 只支持前导 `~/` 展开；相对路径统一相对 config 目录解析（不依赖 cwd）；`FORAGER_CONFIG_DIR` 改变 config 目录时同步改变该基准。
- 深钻只投影现有 attempts：debug 在命令终结时至多一条脱敏摘要，trace 再输出逐 attempt 安全字段；error/warn/info 不新增可选日志。日志不持久化，也不输出 attempt message、model、endpoint、请求/响应、正文、header、credential、prompt 或 tool trace。
- journal 每次调用写独占 `search_result_<nanos>_<pid>_<seq>.json` 并 `sync_all`；retention 只删除完整匹配该命名且过期的普通文件。其他 JSON/JSONL、相近名称、目录和链接不属于 forager 所有权。

## 照办件（上游已定契约汇总）

- 具名 request class 预留（v1 空）；env 数组＝TOML 字面量、按 schema 目标类型解析（figment Env 层自定义）；`FORAGER_CONFIG_DIR` 豁免、未知 `FORAGER_*` 退 3。
- **统一 URL 脱敏器**居 `config` 模块：去 userinfo/fragment、掩 token/key/secret/signature/authorization 类 query 参数；config list / doctor / 错误消息 / journal 四出口共用。
- config 目录 0700 / 文件与临时文件 0600（替换后重申）；`config set KEY -` stdin 语义（第 3 章）；坏配置双通道（严格加载 vs 修复）；toml_edit 承诺以 #57 原文为准，不扩大。
- Schema v1 严格解析细则全套（第 2 章）；`reason` 必填非空；`PlanCapability` 独立三值枚举；plan→执行纯函数只读 `required_capabilities`。

## #55 痛点 13 条消解落点

| 痛点 | 落点 |
|---|---|
| 1 service.py God 模块 | 模块拆分 + app 组合层 |
| 2 虚假基类契约 | 每 seam trait + 专属 Outcome |
| 3 HTTP/重试样板漂移 | 单 Client、RetryPolicy、唯一 status 映射 |
| 4 多套 MCP transport | McpClient 统一（Zhipu 已砍） |
| 5 error_type 无单一真相 | ErrorKind + 归因总函数 |
| 6 结果 dict 多处拼装 | types + 专属 Outcome |
| 7 配置 God 单例 | 强类型 schema + AppContext 持有 Config |
| 8 两套凭据体系 | 唯一凭据池 |
| 9 monkeypatch/全局 reset | 显式 struct 参数传入，无全局 |
| 10 双向耦合 | 私有性纪律 + 单向层 |
| 11 新 provider 多点登记 | registry 唯一描述 |
| 12 路由分裂 | classifier 独立模块，app 唯一组合点 |
| 13 engine 中 provider 特判 | F10 封装边界 |
