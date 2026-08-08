# smartsearch → forager 迁移逻辑劣化审计

审计日期：2026-08-06

旧行为基线：`/Users/jfmoe/Coder/smartsearch@11ac647`（0.7.1）

当前实现基线：`/Users/jfmoe/Coder/forager@78b3f81`（0.2.0）

## 裁决规则

当前行为与旧基线不同，只有 `docs/spec/forager/` 明确要求该差异时，才是“spec 授权”；否则一律是“逻辑劣化”。旧功能删除、字段缩减和协议收窄同样需要明确授权。issue、ADR、提交记录和现有测试只用于定位旧/当前事实，不能补足授权。

结论：共确认 **36 项逻辑劣化**；另有 **12 个差异族**获 spec 明确授权。主 search、CLI/config/journal、fetch/research、provider wire 与 AnySearch 均已纳入本报告。

## 逻辑劣化

每行均给出旧行为、当前分支、spec 授权定位（没有即“无”）和影响；不存在“待裁决”类别。

### CLI、配置、journal 与 doctor

| ID | 旧行为（path:line；字段/分支） | 当前行为（path:line；字段/分支） | spec 明确授权原文定位 | 裁决 | 影响 |
|---|---|---|---|---|---|
| C1 | search、fetch、research 的飞行前 config/plan 错误构造结构化 dict，再经 `_print_result` 输出 JSON：`cli.py:2480-2506`、`service.py:3462-3494`、`cli.py:2516-2518`、`service.py:1267-1312`、`cli.py:2601-2608`。 | `app.rs:495-533,919-994` 的 load/plan 分支以 `?` 返回 `AppError`，`main.rs:107-110` 只写 stderr；search 在 `app.rs:652-667` 的 journal writer 前退出。 | 无；反向规定：[02-cli.md:36-42](../spec/forager/02-cli.md#L36-L42)、[05-acceptance.md:13-15](../spec/forager/05-acceptance.md#L13-L15)、[04-architecture.md:78](../spec/forager/04-architecture.md#L78)。 | 逻辑劣化 | 默认 JSON 调用在 config/坏 plan 终态没有可解析 stdout；search 同时漏记一次 Err journal。 |
| C2 | `logger.py:7-26` 消费日志级别，旧 `SMART_SEARCH_LOG_LEVEL` 可改变运行时日志输出。 | `log.level` 仅在 schema/config view 出现，没有 runtime/log 调用。 | 无；规格保留 `[log].level` 为 stderr 控制台流：[03-config.md:88-90](../spec/forager/03-config.md#L88-L90)、[04-architecture.md:79-80](../spec/forager/04-architecture.md#L79-L80)。 | 逻辑劣化 | 可配置的日志等级成为 no-op。 |
| C3 | 每日追加 `search_results_YYYYMMDD.jsonl`：`result_journal.py:16,56-72,95-102,163-183`。 | 每次调用另写 `search_result_<nanos>_<pid>_<seq>.json`：`journal.rs:109-127,308-318`。 | 无；规格只规定目录、默认值、双面字段和一次终态写入：[03-config.md:91-94](../spec/forager/03-config.md#L91-L94)、[04-architecture.md:70-80](../spec/forager/04-architecture.md#L70-L80)。 | 逻辑劣化 | 已有按日 JSONL 读取、追加与归档工作流失效。 |
| C4 | retention 只删除名称匹配的旧 JSONL：`result_journal.py:185-203`。 | `cleanup_entry` 删除 journal 目录任意过期普通 `.json`：`journal.rs:266-299`。 | 无。 | 逻辑劣化 | 用户指定的 journal 目录中不属于 forager 的旧 JSON 也会被删除。 |
| C5 | shallow doctor 的 `ok` 由主连接和最低能力状态决定：`service.py:4414-4442`。 | `doctor.rs:62-97` 固定 `ok: true`，即使 provider 不可达。 | 无；规格要求浅检含“可达性”：[02-cli.md:103-105](../spec/forager/02-cli.md#L103-L105)。 | 逻辑劣化 | 机器调用无法用顶层状态发现浅检连接失败。 |

### 主 search 语义

| ID | 旧行为（path:line；字段/分支） | 当前行为（path:line；字段/分支） | spec 明确授权原文定位 | 裁决 | 影响 |
|---|---|---|---|---|---|
| S1 | `--validation` 参与补强和 strict 来源门：`service.py:2557-2565,2813-2848`。 | `app.rs:52-55,921-943` 解析后丢弃 `validation`，runtime 没有该值。 | 无；规格明确保留主搜索 `--validation` 与 `search.validation`：[01-scope.md:29-32](../spec/forager/01-scope.md#L29-L32)、[02-cli.md:44-48](../spec/forager/02-cli.md#L44-L48)、[03-config.md:23-26](../spec/forager/03-config.md#L23-L26)。 | 逻辑劣化 | `fast/balanced/strict` 完全同效；strict 的“无来源失败”消失。 |
| S2 | 选择 web_search 且 `extra_sources=0` 时使用 `max(1, extra_sources or 3)`：`service.py:2813-2824`。 | `app.rs:638-649` 先将数量缩为 `max(1)`，engine 以该值执行。 | 无；规格仅保留 `--extra-sources`，未要求把默认路由数量缩到 1：[01-scope.md:29-32](../spec/forager/01-scope.md#L29-L32)、[02-cli.md:44-48](../spec/forager/02-cli.md#L44-L48)。 | 逻辑劣化 | 未显式数量时的 routed web discovery 从 3 条缩为 1 条。 |
| S3 | 默认配置下，answer 在来源拆分前删除 `<think>` 与开头拒答/策略段：`sources.py:54-94,139-147`；旧开关默认 `true`：`config.py:739`。 | xAI/OpenAI adapter 直接装入 answer，没有等价清理。 | 无；反向规定：[03-config.md:198](../spec/forager/03-config.md#L198) 写明旧开关删除而正文消毒无条件执行。 | 逻辑劣化 | 隐藏推理或开头策略话术可进入 answer、stdout、tee 与 journal。 |
| S4 | `sources.py:139-193,235-343,346-429` 从 sources JSON/Python、Sources block、tail links、details、inline citation 投影 sources，并保留 description。 | 只接受结构化 annotations/citations；`types.rs:412-426` 的 `Source` 无 description。 | 无；规格保留“来源规范化”：[01-scope.md:31](../spec/forager/01-scope.md#L31)。 | 逻辑劣化 | 文本型来源不再进入结构化 sources，且 description 字段丢失。 |
| S5 | Context7 docs 路径至少返回 library candidate：`service.py:2163-2187`。 | `providers/mod.rs:216-270`、`engine.rs:331-370` 在 resolve+docs 后将 candidate/read_sources 置空并回退 Exa。 | 无；每 seam 要有专属 Outcome、MCP adapter 应做结果映射：[04-architecture.md:22-26](../spec/forager/04-architecture.md#L22-L26)、[04-architecture.md:43-45](../spec/forager/04-architecture.md#L43-L45)；C10/C11 要求两个 Context7 操作：[05-acceptance.md:52-54](../spec/forager/05-acceptance.md#L52-L54)。 | 逻辑劣化 | 已成功读取的 Context7 内容不再作为候选/来源暴露，并可能错误回退。 |
| S6 | xAI/OpenAI 成功响应没有 4 MiB 失败门。 | `xai.rs:183-196`、`openai_compatible.rs:445-450,471-495,600-606` 对超过 4 MiB 的成功响应返回 Runtime 失败。 | 无；4 MiB 成功截断只在 Web Fetch 条款中出现：[04-architecture.md:54-58](../spec/forager/04-architecture.md#L54-L58)。 | 逻辑劣化 | 大型正常主回答从可完成变为失败。 |
| S7 | 原 URL 去重，再渲染输出：`sources.py:123-136,346-393`。 | `xai.rs:256-315`、`openai_compatible.rs:580-592` 以脱敏 URL 作去重键。 | 无；规格要求输出 URL 脱敏，但未要求改变内部去重键：[04-architecture.md:84-85](../spec/forager/04-architecture.md#L84-L85)。 | 逻辑劣化 | 仅敏感 query 参数不同的资源会被折叠为一条 source。 |

### fetch、research 与 non-search 命令

| ID | 旧行为（path:line；字段/分支） | 当前行为（path:line；字段/分支） | spec 明确授权原文定位 | 裁决 | 影响 |
|---|---|---|---|---|---|
| N1 | `research --budget` 默认 `deep`：`cli.py:3150-3160`、`service.py:1267-1272`。 | 默认改为 `ResearchBudgetArg::Standard`；在当前实现中 standard 最多保留 4 个子问题、每题 2 条证据，而 deep 为 6/3：`app.rs:67-85`、`research.rs:28-55`。 | 无；规格只列可选值，未要求改默认：[02-cli.md:15-16](../spec/forager/02-cli.md#L15-L16)。 | 逻辑劣化 | 未传参数时不再选择当前实现中覆盖面最大的预算档。 |
| N2 | research 失败至少返回精确 `evidence_dir`；路径可据此恢复制品：`service.py:1267-1529`。 | 初始载荷和三个保留 locator 的压缩档仍超过 4 KiB 时，最终兜底删除 `plan_path`、`unconsumed_candidates.path`、evidence item paths，并截断 `evidence_dir`：`main.rs:191-326`，其中最终兜底在 `main.rs:229-236`。 | 无；成功和 postflight 失败必须返回同一组可读路径：[02-cli.md:90-101](../spec/forager/02-cli.md#L90-L101)、[05-acceptance.md:13-15](../spec/forager/05-acceptance.md#L13-L15)。 | 逻辑劣化 | 最需要从落盘证据恢复时，调用方可能得不到可读 locator。 |

### Provider wire contract

| ID | 旧行为（path:line；字段/分支） | 当前行为（path:line；字段/分支） | spec 明确授权原文定位 | 裁决 | 影响 |
|---|---|---|---|---|---|
| P1 | xAI input 发送 role-array：`xai_responses.py:49-55`。 | `xai.rs:127-144,334-342` 改为 string。 | 无。 | 逻辑劣化 | 改变上游请求 payload 形状。 |
| P2 | xAI SSE 单个 malformed JSON 忽略并继续：`xai_responses.py:98-101`。 | `xai.rs:214-223` 以 Runtime 终止。 | 无。 | 逻辑劣化 | 单坏帧可中断原本可完成的流。 |
| P3 | `response.failed/incomplete` 走 `RemoteProtocolError` 可重试路径：`xai_responses.py:106-111`。 | `xai.rs:234-243` 归 Runtime；`execution.rs:122-149` 不重试 Runtime。 | 无。 | 逻辑劣化 | 上游临时 completed-state 故障不能按旧路径恢复。 |
| P4 | `completed` 缺 `output_text` 产生空成功：`xai_responses.py:147-152`。 | `xai.rs:317-325` 改为失败。 | 无。 | 逻辑劣化 | 空成功的旧调用约定消失。 |
| P5 | OpenAI SSE 自然 EOF 返回已装配内容：`openai_compatible.py:323-360`。 | `openai_compatible.rs:493-537,673-694` 要求 `[DONE]` 或 finish_reason。 | 无。 | 逻辑劣化 | 无终止标记但内容完整的兼容 SSE 会失败。 |
| P6 | OpenAI SSE 单个坏 JSON 忽略：`openai_compatible.py:334-346`。 | `openai_compatible.rs:524-532` 整体失败。 | 无。 | 逻辑劣化 | 一帧格式瑕疵中断完整流。 |
| P7 | Exa `/search` 发送 `useAutoprompt: true`：`exa.py:93-103`。 | `exa.rs:241-275` 删除该字段。 | 无。 | 逻辑劣化 | Exa 查询改为不同的上游检索模式。 |
| P8 | Exa item 缺 `url` 但有 `id` 时用 id fallback：`exa.py:25-42`。 | `exa.rs:300-317` serde 整包失败。 | 无。 | 逻辑劣化 | 一条不完整 item 使全响应失效。 |
| P9 | `include-text/include-highlights` 控制输出字段，image/favicon 保留：`exa.py:34-41`。 | `exa.rs:182-196,308-317` 只要上游返回就输出，`Source` 丢 image/favicon：`types.rs:414-426`。 | 无。 | 逻辑劣化 | 调用方 flag 不再控制字段，媒体元数据丢失。 |
| P10 | Exa 成功 body 没有全局 4 MiB 截断。 | `exa.rs:146-174` 通过 `net.rs:13,40-67` 截断，随后 JSON 可失败。 | 无；4 MiB 仅授权给 web_fetch：[04-architecture.md:54-58](../spec/forager/04-architecture.md#L54-L58)。 | 逻辑劣化 | 大型有效 Exa JSON 变为 parse/runtime 失败。 |
| P11 | Context7 带 `X-Context7-Source: smart-search`；initialize 缺 session 即失败，取得 session 后必须发送 initialized 通知：`context7.py:197-207,235-253`。 | `net.rs:378-405,488-505` 删除 source header，并允许 initialize 无 session 时直接 tools/call。 | 无；规格只要求统一 McpClient：[04-architecture.md:43-45](../spec/forager/04-architecture.md#L43-L45)。 | 逻辑劣化 | 改变 Context7 请求身份与 MCP 会话约束。 |
| P12 | Context7 docs 对纯 structuredContent 回退 JSON，并保留 `code_snippets/info_snippets/results/total`：`context7.py:340-353`。 | `context7.rs:202-208,233-241` 给空 content 且字段丢失，`types.rs:650-659` 无对应字段。 | 无。 | 逻辑劣化 | 结构化 docs 响应不再可消费。 |
| P13 | Context7/AnySearch MCP 正文没有 4 MiB 成功截断：`context7.py:90-134`、`anysearch.py:137-147,433-467`。 | `net.rs:605-697` 对成功正文截断。 | 无；4 MiB 仅授权给 web_fetch：[04-architecture.md:54-58](../spec/forager/04-architecture.md#L54-L58)。 | 逻辑劣化 | 大型 MCP 成功响应可被截断或解析失败。 |
| P14 | 已验证 AnySearch 域执行 schema validation 并返回 fingerprint：`anysearch.py:352-388`。 | `anysearch.rs:94-177` 恒为 `unavailable`。 | 无；域晋升通道须保留：[06-migration.md:32](../spec/forager/06-migration.md#L32)。 | 逻辑劣化 | 已验证域参数不再受本地 schema 校验，fingerprint 消失。 |
| P15 | AnySearch `result.content` 为 string 时可读取：`anysearch.py:137-147`。 | `net.rs:700-711` 仅接受数组。 | 无。 | 逻辑劣化 | 合法 string content 响应被丢弃。 |
| P16 | AnySearch Markdown 解析接受可变空白和无标题 URL fallback：`anysearch.py:150-174,511-521`。 | `anysearch.rs:304-357` parser 更窄；失败后只生成一个 URL 为空的 generic structured result。 | 无。 | 逻辑劣化 | 原先可提取的 URL 级 discovery 结果退化为无 URL 的笼统结果。 |
| P17 | Tavily web_search 请求含 `search_depth=advanced`、`include_raw_content=false`、`include_answer=false`：`service.py:2316-2326`。 | `supplemental.rs:79-95,158-168` 删除三字段。 | 无。 | 逻辑劣化 | Tavily 上游检索/响应契约改变。 |
| P18 | Tavily/Firecrawl web_search 空 results 继续下一家：`service.py:2096-2109`。 | `supplemental.rs:119-154` 的 success predicate 恒 true，`engine.rs:750-793` 停止链。 | 无。 | 逻辑劣化 | 第一家合法空集阻止 fallback provider。 |
| P19 | supplemental 单项缺 url 或 Firecrawl 缺 data 时跳过/视为空：`service.py:558-578,2316-2389`。 | `supplemental.rs:170-197` serde 整包 Runtime。 | 无。 | 逻辑劣化 | 单个异常结果使整个补强响应失败。 |
| P20 | Firecrawl web_fetch 空正文后以 `waitFor=1500/3000…` 再试：`service.py:2413-2444`。 | `web_fetch.rs:240-249` 删除 waitFor 重试；`onlyMainContent` 为新增字段。 | 无；`onlyMainContent` 有授权，但删除 waitFor 没有：[04-architecture.md:56](../spec/forager/04-architecture.md#L56)。 | 逻辑劣化 | 延迟渲染页面不再获得旧的内容恢复机会。 |
| P21 | Tavily map 请求 timeout 原样传递：`service.py:2490-2509`。 | `tavily_map.rs:157-167` 静默上限为 180。 | 无。 | 逻辑劣化 | 调用者给出的更大 map timeout 被忽略。 |
| P22 | Context7 显式 `follow_redirects=False`：`context7.py:310-312`；旧 Exa、Tavily、Firecrawl client 也未开启跟随：`exa.py:197`、`service.py:2328,2372,2421,2511`。 | 所有这些请求共用 `net.rs:290-304` 的 client，未覆盖 redirect policy；锁定的 reqwest 0.12.28（`Cargo.lock:941-944`）在本地依赖源码 `reqwest-0.12.28/src/redirect.rs:160-164` 将默认 policy 定义为跟随最多 10 次。 | 无；规格只要求共享 client：[04-architecture.md:39-42](../spec/forager/04-architecture.md#L39-L42)。 | 逻辑劣化 | Context7、Exa、Tavily、Firecrawl 的 3xx 从可观察响应变为隐式跟随，可能改变错误归因、目标主机和凭据发送边界。 |

## spec 授权差异账

以下差异已在规格中明确要求，故不属于逻辑劣化。

| 差异族 | 授权依据 | 已覆盖的旧→新变化 |
|---|---|---|
| 项目/CLI 面重命名与参数清理 | [01-scope.md:9-14](../spec/forager/01-scope.md#L9-L14)、[02-cli.md:7-34](../spec/forager/02-cli.md#L7-L34) | `smart-search`→`forager`；嵌套 provider 命令；固定可见 alias；移除 `--providers/platform/stream`。 |
| 三层路由、deep 与分类 | [01-scope.md:9-23](../spec/forager/01-scope.md#L9-L23)、[02-cli.md:54-88](../spec/forager/02-cli.md#L54-L88) | rules/embeddings/router/route-calibrate 退休；classifier 与 caller capabilities/Schema v1 plan 继任；deep 并入 research。 |
| Zhipu 与 provider 链序 | [01-scope.md:5-8](../spec/forager/01-scope.md#L5-L8)、[03-config.md:79-86](../spec/forager/03-config.md#L79-L86) | Zhipu 全家删除；web_search 固定 Tavily→Firecrawl；web_fetch 默认 Tavily→Firecrawl→Jina。 |
| Web Fetch 标准化 | [02-cli.md:41-42](../spec/forager/02-cli.md#L41-L42)、[04-architecture.md:54-58](../spec/forager/04-architecture.md#L54-L58) | 统一 FetchOutcome、provider 无关 Markdown、共享 deadline、轮换/重试、薄正文 Quality fallback、Web Fetch 专属 4 MiB 成功截断。 |
| research 文件化证据管线 | [02-cli.md:90-101](../spec/forager/02-cli.md#L90-L101)、[04-architecture.md:66-68](../spec/forager/04-architecture.md#L66-L68) | 不再输出机械 answer/citations；固定计划、evidence、候选、摘要制品，stdout 仅 Evidence Index。 |
| minimum profile 与输出/错误模型 | [01-scope.md:40-43](../spec/forager/01-scope.md#L40-L43)、[02-cli.md:36-42](../spec/forager/02-cli.md#L36-L42)、[04-architecture.md:28-45](../spec/forager/04-architecture.md#L28-L45) | 硬 minimum gate 改 capability gaps；退出码、ErrorKind、整命令 deadline、JSON/tee/verbose 契约重构。 |
| AnySearch acceptance surface | [01-scope.md:12-14](../spec/forager/01-scope.md#L12-L14)、[01-scope.md:33-35](../spec/forager/01-scope.md#L33-L35)、[05-acceptance.md:56-65](../spec/forager/05-acceptance.md#L56-L65) | 只保留 domains 与 search 双形态；extract/batch 完全删除，不入 registry/矩阵。 |
| diagnose、regression、model | [01-scope.md:11-14](../spec/forager/01-scope.md#L11-L14)、[02-cli.md:103-105](../spec/forager/02-cli.md#L103-L105) | diagnose 合并到 `doctor --provider`；regression 改 cargo test/CI；model 命令删除，由 config 覆盖。 |
| TOML/XDG/schema/credential pool/config edit/setup | [03-config.md:5-18](../spec/forager/03-config.md#L5-L18)、[03-config.md:107-126](../spec/forager/03-config.md#L107-L126)、[04-architecture.md:47-52](../spec/forager/04-architecture.md#L47-L52) | TOML/XDG、`FORAGER_` 严格 env、`keys` 真数组、order 权威、config edit 锁与修复通道、统一凭据池。 |
| journal 默认/目录/双面 | [03-config.md:91-94](../spec/forager/03-config.md#L91-L94)、[04-architecture.md:70-80](../spec/forager/04-architecture.md#L70-L80) | journal 默认启用、迁 XDG state、记录结果面加过程面；注意：这不授权 C3/C4 的文件粒度/清理范围变化。 |
| doctor 两档 | [02-cli.md:103-105](../spec/forager/02-cli.md#L103-L105)、[05-acceptance.md:30-33](../spec/forager/05-acceptance.md#L30-L33) | doctor 浅检全体，`--provider` 深探八 provider，OpenAI-compatible 保留双传输判定。 |
| skill 安装同步 | [01-scope.md:9-12](../spec/forager/01-scope.md#L9-L12)、[06-migration.md:11-15](../spec/forager/06-migration.md#L11-L15) | 自研 skills install/status/update/clear、偏好、锁和自动同步删除，转 `npx skills add jfmoe/forager`。 |

## 测试与覆盖性

已完整读取 `docs/spec/forager/README.md` 与第 1–6 章，逐项比对旧仓 `src/smart_search/`、旧行为测试，以及当前 `src/`、`tests/`、skill 资产。重点遍历模块包括：

- 旧：`cli.py`、`service.py`、`logger.py`、`result_journal.py`、`sources.py`、所有 provider adapter、`skill_installer.py`、`skill_sync.py` 与对应测试。
- 当前：`app.rs`、`main.rs`、`engine.rs`、`research.rs`、`journal.rs`、`doctor.rs`、`net.rs`、`types.rs`、`config/`、`providers/`、`skills/forager/` 与 fetch/research/anysearch/skill contract tests。

本轮已执行：

```text
cargo test --test search --test context7 --test exa_search --test doctor
           --test fetch --test firecrawl_fetch --test jina_fetch --test tavily_fetch
           --test research --test research_error --test anysearch --test skill_contract --locked
```

12 个测试目标共 **197 个用例通过，0 失败**。该结果证明报告引用的当前行为面可运行；不会将与规格相反的当前测试期望视为合规证据。另由独立 reviewer 逐项复核 36 项裁决和授权账；其发现的重定向影响面遗漏，以及 N2、S3、P16 的表述边界均已合并。

非迁移差异附注：journal provider-attempt message 未截断为 500 字符属于当前 spec 不合规，但旧版也没有该截断边界，因此不计入上述“旧→新”逻辑劣化总数。
