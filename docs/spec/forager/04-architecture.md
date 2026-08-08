# 4. Rust 架构骨架

权威来源：[#58 Resolution](https://github.com/jfmoe/smartsearch/issues/58) 及其[补充决议（F1–F10）](https://github.com/jfmoe/smartsearch/issues/58#issuecomment-5078828791)。选型输入：#54（clap 4 derive、tokio + reqwest(rustls)、figment + serde、thiserror/anyhow/miette、dist + release-plz；aichat 为架构参考）。

## 工程形态：单 crate `forager`，bin + lib

不上多 crate 工作区（Rust 模块私有性已在编译期阻止跨私有面 import；单 crate → 工作区是机械重构）。两条纪律：① 模块默认私有，跨模块共享必须显式 `pub(crate)` 且只能上移共享层，provider 之间禁止横向 import；② `main.rs`/`lib.rs` 分离，bin 只做 clap 解析与退出码映射。

## 顶层模块（12 个，五层单向依赖）

```
main → app → {engine, research, classifier, doctor, journal}
                → {net, credentials, config, providers} → types
```

- **`app` 组合层**（F1）：极薄；持有显式 `AppContext`（Config、共享 Client、CredentialPool、ModelBreakers、Deadline、journal 写入器的唯一所有权），只做具名输入/输出的顺序组合；禁止 provider 分支、路由规则、结果拼装。持有关键任务 `JoinHandle`，`JoinError` 归 Runtime。
- **`types`**：零 IO 纯类型层——ErrorKind、Capability、`PlanCapability`（plan 语境独立三值枚举）、各 Outcome、ProviderAttempt、Source、ResearchPlan Schema v1、Deadline、薄正文阈值常量。所有跨层形状的唯一定义点。
- **`net`**：共享 HTTP client 构造、RetryPolicy、SSE 解析、status→ErrorKind 唯一映射、McpClient。
- `research`/`classifier`/`doctor`/`journal` 各自一格、互不依赖、不被 engine 依赖。
- 输出格式化先放 bin 侧，出现第二个消费者再提升为 `output/`。

## Provider 契约与 registry

- **每 seam 一个 trait + 专属返回类型**：`WebSearch`/`DocsSearch`/`WebFetch`（supplemental 与主搜索共用 WebSearch 签名，registry 区分链序归属）；`SearchOutcome`/`DocsOutcome`/`FetchOutcome` 共享 ProviderAttempt/Source 构件。一个 provider＝一个 struct，同一 `Arc` 实例登记进多条 seam 链。
- **seam 支持矩阵**＝「谁 impl 了哪个 trait」的编译期事实；`order` 校验查 registry。**`map` 命令**＝tavily 直连操作（`site_map`），不设独立 seam trait（唯一 provider，需要时提升为 trait 是纯增量）；registry 在 tavily 描述内登记该操作。
- **registry 最小职责**（F10）：唯一登记 `ProviderId`、支持 seam、凭据要求、doctor probe、构造入口；config/doctor/capability status 从同一描述读取身份，不各设 allowlist；engine 只调用 seam trait 并聚合 `ProviderAttempt`，禁止按 provider id/model 分支；openai-compatible 的 model 候选、断路器、transport fallback 全部封装在 provider 内。不引入宏、不生成 clap 树。

## 错误模型

- **`ErrorKind` 10 变体**：Auth / RateLimited / QuotaExhausted / Parameter / Config / Timeout / Network / Quality / Evidence / Runtime。三方法：`is_retryable()`；`rotates_credential()`（RateLimited|QuotaExhausted——轮换优先于重试，429 不重试）；`family() → Transport|Content`（Quality/Evidence 为 Content 族）。**无 `exit_code()` 方法**。
- 「empty」从错误分类法除名：直连命令空结果＝`Ok(空 Outcome)` 退 0；证据管线的证据不足＝Evidence 退 5（域切分见第 1 章）。
- **退出码两阶段**：飞行前（argv→2、config/未知 env→3）只由预检产生；飞行后由**归因总函数**产生。attempt 级 Parameter 不映射退 2。
- **归因总函数**（F4 + #59 B3）：只按每个 provider 的**最终 attempt** 归约（重试不参与计数），对各 kind 按**优先级全序**取最大，与重试次数、失败顺序无关。已有成功响应进入质量/证据阶段且终局失败＝Content 优先退 5，不被后续网络失败覆盖；所有可用 provider 均未产生可验证响应才退 4；同质失败顶层透传原 kind（如全 401 报 auth_error，退出码仍按族）；attempts 永远带原始 kind。
  - **全序表定稿**（低 → 高；Content 族恒高于 Transport 族）：`Network < Timeout < RateLimited < QuotaExhausted < Auth < Parameter < Runtime < Quality < Evidence`。定义域为**飞行后 kind**（ErrorKind ∖ {Config}）：`Config` 只在飞行前预检产生（退 3），**`ProviderAttempt` 不得携带 Config**——此为类型不变量，进 unit 真值表。族间关系与全序存在性为契约（真值表穷举验证）；族内排布编码期可微调，调整须同步更新真值表。
- `ProviderError`（thiserror）：kind + provider + status + 脱敏消息 + 耗时；status→kind 映射只在 net 一份。
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
- 两类主搜索在完整响应组装后、attempt 判定成功前进入同一个 normalizer：先删除完整闭合、大小写不敏感且可跨行的 `<think>...</think>`，再投影末尾显式 Sources/References/Citations/来源/参考资料/引用标题块与 `[[N]](HTTP(S) URL)`。不做策略/prompt-injection 关键词过滤，不解析 `sources(...)`、任意 `<details>` 或尾链猜测；规范化后 answer 为空为 Runtime。来源按脱敏后的公开 URL 稳定去重。

## 凭据池与断路器

分界原则：跨进程必须共享的落盘，策略性短时效的留进程内。

- **池**：游标落盘 `$XDG_STATE_HOME/forager/credential_pool_state.json`，fd-lock 有界文件锁，「锁内取号推进 / 拿不到锁乐观降级」语义保持。**状态文件不变量**（F6）：带 schema version、只存非敏感索引；同目录 `0600` 临时文件 + fsync + 原子 rename；解析失败只复位受影响 provider 的游标并发非致命诊断，不升 config_error、不阻断搜索。进程内轮换状态为显式 `CredentialPool` struct（`Arc<Mutex>`）参数传入，无全局、无 reset 钩子。`classifier.keys` 走同一实现。
- **model 断路器**：进程内显式 `ModelBreakers` struct（阈值 2 / 冷却 600s），不落盘。

## web_fetch 薄正文质量门控

Web Fetch 成功值是 **Normalized Fetch Content**：从成功 provider 响应解码出的 provider 无关 Markdown 正文，不含传输包装、attempts 或 diagnostic。默认链为 `Tavily → Firecrawl → Jina`，直接 fetch、research 取证、search-side Web Fetch 与 PDF 都消费同一个 `engine::fetch`，不存在内容类型专属顺序。Tavily 固定请求完整 basic Markdown 且不传 query chunks；每个 Firecrawl provider attempt 只发送一次 `/scrape`，显式请求 Markdown、`onlyMainContent: true`、`timeout: 60000`，不发送 `waitFor`、actions 或第二套 rendering probe；Jina 通过官方结构化 JSON 响应的 `data.content` 读取正文，不启用链接删除或通用 selector。

薄正文门只对 HTTP 成功并完成 provider 解码后的正文生效。两线命中任一 → `Quality`（Content 族）落下一家：**长度线**正文 < 200 字符；**密度线**唯一行数 ≤ 3 且总长 < 500。PDF 只适用长度线。全链皆薄 → 终态 Quality 退 5，attempts 带实测字符数。阈值为 types 具名常量，**不设配置键**。4 MiB 截断继续是成功加 diagnostic；只有截断后正文仍薄才 fallback。64 KiB 错误响应上限不变。

## Search Candidate 与 provider request contract

- `sources` 只保存 Primary Search Source；所有非主候选统一进入 `extra_sources`，不再公开独立 `vertical_results`。Search Candidate 固定包含必填 provider/capability/provider_data 与可空 title/url/summary；URL 只能是真实 HTTP(S)，summary 只复制 provider-native 描述、摘要或片段，provider_data 是 snake_case 强类型白名单。
- Context7 Documentation Search 只做 library resolve，使用 `url: null` 与 typed library locator；Research 通过现有 Documentation Search seam 的 provider-owned query-docs 读取它。有 URL candidate 走 Web Fetch。Evidence Index 对 Context7 保留 `library_id`、`path`、`url: null`，Citation Binding 使用非链接 `[eN]`；URL evidence 使用 `[eN](URL)`。不建立通用 provider registry。
- Exa direct search 的 text/highlights 按请求 flag 精确投影并保留 image/favicon；Documentation Search 保留 highlights 与媒体选择信号但不读取全文。不强制 `useAutoprompt`，不以 `id` 代替必填 URL。AnySearch Candidate 的 summary 复制 description；URL-less structured result 仅投影 `evidence_type=structured`。
- AnySearch 当前没有 verified manifest entry，显式未验证域继续报告 `schema_validation.status=unavailable` 并原样透传参数；不交付 test-only validator、fingerprint 或运行时 schema 依赖。Domain Discovery 将参数名后的 `(required)` 投影到 `parameter_schema.required`，但不从自然语言猜测 type/enum/default。Markdown decoder 只容忍编号标题与 `- **URL**:` 内的可变 ASCII 空白；没有编号标题时仅提取带 host 的 HTTP(S) URL 并按出现顺序去重，仍无 URL 时保留 structured result。
- Supplemental Tavily 请求显式发送 `search_depth: "advanced"`、`include_raw_content: false`、`include_answer: false`；只有规范化后非空候选停止链。合法 `results: []` / `data.web: []` 继续 fallback，全链有合法空集且无非空结果时 `Ok(empty)`，`fallback=off` 只执行链头。仅跳过缺失/null/空白 URL 单项；非字符串 URL、非对象条目、缺失或错误 container 为 Runtime。
- Tavily map 是 direct operation：CLI 校验 timeout `10..=150`、depth `1..=5`、breadth `1..=500`、limit > 0；合法 timeout 原样进入命令 Deadline 和 body，每个 attempt 再受 `providers.tavily.timeout` 与剩余预算较小值限制。
- search-side Web Fetch 成功结果以实际 provider 和 Normalized Fetch Content 的 300 字 preview 进入 `extra_sources`；抓取失败由 attempts、capability gap 和既有终态表达。Markdown 明确渲染 `Primary Sources` 与 `Extra Sources`；content 只返回主 answer；JSON、verbose 和 journal 消费同一结果角色。

## research 文件化交付

Research Evidence Pipeline 默认使用 standard 预算，将正文逐条写入 evidence Markdown，并只在成功 stdout 返回 Research Evidence Index。计划、未消费候选与 Research Recovery Manifest 分别写入 `00-plan.json`、`candidates.json` 与 `summary.json`；manifest 包含无正文的 evidence identity/metadata/path、coverage、gap、capability gaps、终态、attempts 与 `synthesis_policy`。未消费候选 artifact 以 `is_evidence: false` 明确其角色。失败 stdout 使用稳定小形状并以可空 `summary_path` 指向已成功写入且可直接读取的 manifest；locator 永不截断，极端长合法路径可超过 4 KiB 目标。

## journal

定位：结果面 + 过程面双记录。

- **结果面**：search 保存 query、answer 全文、仅属于主回答的 sources[] 与独立 supplemental candidates（含 search-side Web Fetch preview，URL 经统一脱敏器）；research 保存 Evidence Index、coverage、artifact 路径与 capability gaps，不保存机械 answer/citations，也不重复 evidence 正文。Vertical Discovery Result 不复制到其他来源集合。
- **过程面**：plan 摘要（capabilities 终集 + 来源 + 分类器是否降级）、provider_attempts[]（provider、seam、error_kind、http_status、duration_ms、credential_index、retry/rotation 计数、脱敏截断 500 字符错误消息、model、endpoint_host、断路器事件）、终态归因、budget 视图 `{total_ms, consumed_ms, exhausted}`、分类器耗时、capability_gaps。
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
