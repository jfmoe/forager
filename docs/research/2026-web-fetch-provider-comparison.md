# 单 URL Web Fetch Provider 对比：Jina Reader、Tavily Extract、Firecrawl Scrape

> 调研日期：2026-08-04（社区指标观测于 00:27–00:29 UTC）。本文为 GitHub issue #89 提供事实输入，范围严格限于一次请求读取一个 URL；crawl、search、map 等只作为相邻能力提及。未发起任何会消耗 Jina、Tavily 或 Firecrawl 账户额度的 API 调用。
>
> 证据口径：产品事实只采用官方 API 文档、官方定价/隐私页面、官方 GitHub 仓库与 npm 官方下载 API。仓库旧研究中的 MDN 实测单列为“既有实测”，**不能外推为产品普遍表现**。文中“判断/建议”是工程推断，不是厂商承诺。

## 执行摘要

- **功能控制面优先时，Firecrawl Scrape 是最强候选；但本文不足以据此改动 forager 当前 `Jina → Tavily → Firecrawl` 链头。**它在单页接口上同时公开了正文提取、标签筛选、浏览器动作/会话、代理、缓存、批量和 JSON 抽取；响应也有 metadata/warning。代价是每页计费、请求失败（包括目标 403/404）仍会耗基础 credit，且更多参数会扩大不稳定面。是否将它设为链头仍需代表性、多 URL 的成功率、延迟、正文质量和成本基准。[Scrape API](https://docs.firecrawl.dev/api-reference/endpoint/scrape) [账单说明](https://docs.firecrawl.dev/billing)
- **最强页面控制/浏览器抓取：Firecrawl；轻量文档读取：Jina Reader；已有 Tavily 搜索工作流：Tavily Extract。**这不是“星数最高者最好用”：三者公开代码范围不同，GitHub star 不可横向当作服务质量或市场份额。
- **#89 的关键边界：provider-first 能稳定的是“有界、可判定的载荷契约”，不能稳定保证同一正文边界。**三家原生精简的力度与失败形态不同；因此本地质量门控应拒绝空/过薄/超限或无法解析的结果并保留诊断，而不应试图把 Markdown 再做语义降噪。

## 1. 可比能力矩阵（官方事实）

“支持”表示当前官方单 URL API 明确公开；“未证实”表示本次未在该 API 的官方材料中找到，**不是**能力不存在。所有链接均于 2026-08-04 读取。

| 维度 | Jina Reader | Tavily Extract | Firecrawl Scrape |
|---|---|---|---|
| 请求与返回形态 | URL 前缀 GET 或 POST；可返回 Markdown、HTML、text、screenshot/pageshot、JSON 封装。[README](https://github.com/jina-ai/reader/blob/main/README.md) | `POST /extract`；`raw_content` 只公开 `markdown`（默认）或 `text`。[API](https://docs.tavily.com/documentation/api-reference/endpoint/extract) | `POST /v2/scrape`；Markdown、HTML、raw HTML、links、images、screenshot、JSON 等 format。[API](https://docs.firecrawl.dev/api-reference/endpoint/scrape) |
| PDF / Office 输入 | 任意 PDF URL 由 PDF.js 解析并返回 Markdown；PDF 与 Word/Excel/PowerPoint 也可经 `file` body 直接上传。[README](https://github.com/jina-ai/reader/blob/main/README.md) | Extract API 未列 PDF 输入/输出保证。 | 明确有 PDF parsing，按 PDF 页另收 credit；所以 PDF 是支持的输入类型而非返回格式。[账单](https://docs.firecrawl.dev/billing) |
| JS 渲染与等待 | `X-Engine` 可选 browser/direct/Cloudflare browser rendering，并有 timeout、等待选择器与响应时机控制。[README](https://github.com/jina-ai/reader/blob/main/README.md) | `advanced` 说明会抓更多表格/嵌入内容且延迟更高；文档未把渲染引擎或页面动作作为 Extract 参数公开。 | 支持 actions、wait、移动端、location、浏览器代理等高级抓取配置。[高级抓取](https://docs.firecrawl.dev/advanced-scraping-guide) |
| 正文/选择器/标签精简 | CSS `X-Target-Selector`、`X-Remove-Selector`；另有链接/图片/媒体保留模式。 | 无 CSS selector/tag 参数；仅 `query` + `chunks_per_source` 做相关性重排后的有损片段（每片最多 500 字符）。[API](https://docs.tavily.com/documentation/api-reference/endpoint/extract) | `onlyMainContent`、`includeTags`、`excludeTags`；本次不把未见于当前文档的同类参数名写入契约。[API](https://docs.firecrawl.dev/api-reference/endpoint/scrape) |
| 链接、图片、媒体 | 链接可 all/none/text/gpt-oss；图片、媒体与 links/images summary 皆可控制。[README](https://github.com/jina-ai/reader/blob/main/README.md) | `include_images`、`include_favicon`；未公开链接/媒体过滤。 | 可请求 `links`、`images`、screenshot 等独立 format，另有图片与广告控制。[API](https://docs.firecrawl.dev/api-reference/endpoint/scrape) |
| 缓存/新鲜度 | `X-No-Cache` / `X-Cache-Tolerance`；cookie、storage-state 等会影响缓存。 | Extract 参考页未公开缓存或 freshness 参数。 | 高级抓取页公开 `maxAge`、`storeInCache` 等缓存控制。[高级抓取](https://docs.firecrawl.dev/advanced-scraping-guide) |
| 认证、cookie、origin header、动作 | 支持 `X-Set-Cookie`、storage state、代理、locale；未把任意目标站 `Authorization` header 作为稳定通用参数承诺。 | API 自身须 Bearer key；本次未找到传入目标站 cookie/header/动作的 Extract 参数。 | API key 外，可配 headers、cookies、actions；适合必须登录后或交互后才出现内容的页面。[高级抓取](https://docs.firecrawl.dev/advanced-scraping-guide) |
| 反爬/代理 | `X-Proxy` country/auto/none 和 `X-Proxy-Url`；效果并非绕过保证。[Jina Docs](https://docs.jina.ai/) | Extract 文档未公开代理/反爬选项。 | 有 proxy、Enhanced/Lockdown mode 等选项；它们是额外产品能力，不能把“可访问任何站点”当保证。[高级抓取](https://docs.firecrawl.dev/advanced-scraping-guide) |
| 单 URL、批量与异步 | 本次核实的 Reader 请求以单 URL 为中心；未确认同一 Reader endpoint 的批量异步契约。 | 同步 `urls` 接受 URL 或数组，最多 20；返回成功 `results` 与 `failed_results`。[API](https://docs.tavily.com/documentation/api-reference/endpoint/extract) | 单 URL scrape 外还有 batch scrape（异步）；这是相邻能力，不应混入单 fetch 成功语义。[批量抓取](https://docs.firecrawl.dev/features/batch-scrape) |
| 结构化抽取 | OpenAPI 0.5.0+805b083 列出 `instruction` 与 object 型 `jsonSchema` 请求参数；但该文件未说明端到端返回形态、schema 保证或计费，故只能确认“参数存在”，不能等同已证实的 schema 驱动抽取契约。[OpenAPI](https://r.jina.ai/openapi.json) | Extract 未公开 schema 驱动结构化抽取。 | `json` format 可带 JSON schema / prompt，属于 LLM 抽取，基础 scrape 之外加 4 credits/页。[JSON mode](https://docs.firecrawl.dev/features/llm-extract) [账单](https://docs.firecrawl.dev/billing) |
| 响应元数据、警告、错误 | JSON 模式的 `FormattedPageDto` 含 url/content/html/text/links/images/warning/metadata/httpStatus 等；OpenAPI 还定义认证、超时、断言、限流等错误。[OpenAPI](https://r.jina.ai/openapi.json) | 返回 `url`、`raw_content`，可选 images/favicon/usage，顶层有 `failed_results`、`response_time`、`request_id`；文档列 400/401/429/432/433/500。 | 返回 content 与页面 metadata/status；文档定义错误目录，且账单页明确建议据 `metadata.statusCode` 分辨目标 403/404，避免盲重试。[API](https://docs.firecrawl.dev/api-reference/endpoint/scrape) [账单](https://docs.firecrawl.dev/billing) |
| 超时、限额、延迟 | `X-Timeout` 最大 180 秒；公开 Reader 限流因 key/tier 而异，响应时机/浏览器模式会影响延迟。[README](https://github.com/jina-ai/reader/blob/main/README.md) | timeout 1–60 秒；basic 默认 10 秒、advanced 默认 30 秒，advanced 可能提高成功率并增加延迟。[API](https://docs.tavily.com/documentation/api-reference/endpoint/extract) | plan 决定 RPM 与并发浏览器；排队时间也计入 timeout，超限为 429。[限流](https://docs.firecrawl.dev/rate-limits) |
| 计费 | Reader 以输出 token 计，输出裁剪会直接改变成本；公开匿名与带 key 的不同速率档。[Reader](https://jina.ai/reader) | basic 每 5 个成功 URL=1 credit，advanced 每 5 个成功 URL=2 credits；失败 URL 不收费。[定价](https://docs.tavily.com/documentation/api-credits) | base scrape 1 credit/页；PDF/JSON/Enhanced/ZDR 各有加价。即使目标 403/404，基础处理已发生仍收费。[账单](https://docs.firecrawl.dev/billing) |
| 隐私/留存 | 可用 `DNT: 1` 要求不缓存、不追踪请求 URL；`eu.r.jina.ai` 宣称处理留在 EU。常规请求留存期限本次未证实。[Jina Docs](https://docs.jina.ai/) | 隐私政策称会收集 query 和上传文档来提供服务；除合同另定外，可用部分 query 改善未来响应，并按“提供/改进服务所需期间”保留，未给 Extract 请求的固定天数。[隐私政策](https://www.tavily.com/privacy) | 可选 ZDR：文档称请求外不持久化数据，+1 credit/页；常规 scrape 的具体留存期限本次未证实。[Scrape ZDR](https://docs.firecrawl.dev/features/scrape#zero-data-retention-zdr) [账单](https://docs.firecrawl.dev/billing) |

### 既有仓库实测（仅供校准，不作外推）

commit `5e925c7` 的 2026-08-02 MDN 单页实验显示：forager 当时的调用下，Jina 输出 64,076 B、Firecrawl 11,488 B、Tavily 7,539 B；Firecrawl 的 `onlyMainContent` 默认开启，Tavily 仍有少量导航噪声，Jina 可经 `X-Retain-Links: text` 或 CSS remove selector 大幅缩短。该实验只有一个 URL，且涉及实际 provider 调用，不能推出总体成功率、延迟或正文质量排名；原始细节在 Git 历史的 `docs/research/2026-fetch-provider-trimming-capabilities.md`（`5e925c7`）。

## 2. 社区采用度与公开生态

下表是可复核的“采用信号”，不是同一口径的市场份额。Reader 与 Firecrawl 的旗舰服务端仓库是开源项目，Tavily 这里能观察到的是官方 SDK；因此不能拿 160,098 vs 11,794 vs 1,343 stars 做“谁更流行”的简单排序。stars/forks 也会受仓库年龄、许可证和产品覆盖面影响；npm 下载会包括 CI、转依赖与旧版本。

| 观察对象（官方） | 2026-08-04 观测值 | 可得出的有限结论 |
|---|---:|---|
| [jina-ai/reader](https://github.com/jina-ai/reader) | 11,794 stars、863 forks；最后 push 2026-05-22；Apache-2.0；GitHub contributors API 分页至第 7 页（每页 1） | Reader 本身有可观的公开社区痕迹，但仓库并不等同于托管 API 的全部采用量。 |
| [tavily-ai/tavily-python](https://github.com/tavily-ai/tavily-python) | 1,343 stars、174 forks；最后 push 2026-07-30；MIT；contributors API 至第 27 页（每页 1） | 这是官方 Python SDK，反映 SDK 社区而非整个 Tavily 服务。官方文档还列 Python、JavaScript SDK 和 MCP。[SDK/API 文档](https://docs.tavily.com/documentation/api-reference/introduction) |
| [firecrawl/firecrawl](https://github.com/firecrawl/firecrawl) | 160,098 stars、9,077 forks；最后 push 2026-08-03；AGPL-3.0；contributors API 至第 167 页（每页 1）；最新 release `v2.11.0` 发布于 2026-06-19 | 在公开 GitHub 可见度和近期代码活动上明显强；但这是包含 search/crawl/agent 等完整平台的旗舰仓库，不能归因给 scrape 单项。 |
| [@tavily/core](https://www.npmjs.com/package/@tavily/core) | npm 官方 API 2026-07-04 至 08-02：1,495,498 downloads | Tavily JS SDK 有高安装/CI 使用信号；不等于独立用户数。[下载 API](https://api.npmjs.org/downloads/point/last-month/%40tavily%2Fcore) |
| [@mendable/firecrawl-js](https://www.npmjs.com/package/@mendable/firecrawl-js) | 同期：3,754,592 downloads | Firecrawl JS SDK 的使用信号更高；区间和统计方式与前项相同，才可做非常粗的相对观察。[下载 API](https://api.npmjs.org/downloads/point/last-month/%40mendable%2Ffirecrawl-js) |

Reader 的官方入口以 HTTP URL/headers 为核心，本次未找到可与上述两项同口径、且明确属于 Reader 的官方 SDK 下载数，故不以猜测数字补齐。GitHub API 的 contributor 分页值应理解为“至少该页仍有条目”的可复核下界近似，不应当作精确人工贡献者数。

## 3. 分别适合什么、不适合什么

| Provider | 适合场景（事实→判断） | 关键优点 | 不适合场景 / 关键缺点 |
|---|---|---|---|
| Jina Reader | 公开页面、PDF/Office 文档、临时读取，或想用一个 URL 前缀和原生 headers 做精简/渲染控制时。**判断：**低集成摩擦与 token 计费使它适合轻量 fallback。 | 输出/渲染/选择器/链接图片媒体参数丰富，且 PDF URL 与 PDF/Office 直传都有正式支持；缓存、代理、cookie 和 EU endpoint 也有官方入口。[README](https://github.com/jina-ai/reader/blob/main/README.md) | provider 原生正文边界可与另两家明显不同；`target selector` 等选择性参数会把站点结构写进调用方；公开的单 fetch 批量、`instruction`/`jsonSchema` 的端到端结构化响应契约、常规留存期未证实。 |
| Tavily Extract | 已使用 Tavily Search/Research，想用相同 key、credit 和返回结构补取已知 URL；或接受整页/相关片段两种模式。 | API 小而直接；最多 20 URL 同步请求；成功才收费，且 `failed_results`/request id 易于调用方观测。[API](https://docs.tavily.com/documentation/api-reference/endpoint/extract) | 没有 selector/tag、cookie/header、代理、浏览器动作或 schema extraction 的公开 Extract 参数；`query + chunks_per_source` 是有损片段，不是稳定的“全文”。 |
| Firecrawl Scrape | JS 重、反爬、需要主体选择/标签/动作/cookie/代理、需后续批量或 JSON extraction 的 fetch。**判断：**最适合作为功能优先的默认。 | 单 URL 控制面最完整，正文、缓存、响应 metadata、异步 batch 和 ZDR 能力连贯；开源/SDK/MCP 生态公开可见。 | 基础 scrape 就按页收费，目标 HTTP 失败也可能收费；动作、代理、JSON、Enhanced、ZDR 都会增成本或复杂性；多开关组合更容易让跨 provider 契约漂移。 |

## 4. “哪家最好用”：必须按权重分场景回答

| 场景 | 推荐 | 理由（工程判断，依据见上表） |
|---|---|---|
| a) forager 的通用 Web Fetch fallback | **暂不以本文决定链头；Firecrawl 是功能优先候选** | `onlyMainContent`/tag 控制、metadata/status、cookie/actions/proxy、缓存和可扩展路线最完整；但没有代表性多 URL 的成功率、延迟、正文质量和成本基准，不能证明它应替换当前 `Jina → Tavily → Firecrawl` 顺序。成本敏感或不需重 JS 时，Jina 仍是强候选。 |
| b) 最强页面控制/浏览器抓取 | **Firecrawl Scrape** | 公开的 actions、headers/cookies、location/proxy、Enhanced/Lockdown、并发队列语义比另两者更完整。 |
| c) 简单、轻量的文档读取 | **Jina Reader** | URL-prefix 入口、匿名限流访问和 Markdown/text/HTML 输出更适合“读一页”；但应接受其精简结果与 Firecrawl 不同，不能以它的单点 MDN 输出代表所有站点。 |
| d) 已有 Tavily 搜索工作流 | **Tavily Extract** | 复用凭据、credit 模型、SDK 和错误/usage 结构，少一个供应商；若需要 selector/cookie/actions，应切 Firecrawl，而不是把 Tavily 的 query chunk 当完整网页。 |

若默认权重改为“零/极低引入成本和 token 价格”，结论会转向 Jina；若改为“复用现有 Tavily 预算与观测”，会转向 Tavily。因此“总体最好”不应脱离权重宣布唯一胜者，更不能以 GitHub 可见度代替服务质量基准。

## 5. 对 #89 的具体建议：provider-first 的稳定边界

### 应保证的最小稳定契约

1. 调用结果要有统一的 `success | failure` 判定；success 至少带请求 URL、最终/来源 URL（provider 给出时）、**非空且在字节上限内**的 agent-consumable Markdown/text 载荷，以及 provider、HTTP/应用状态、request id、warning/metadata 的可选诊断。
2. 本地门控只处理可判定的安全/质量事实：HTTP/应用错误、空内容、过薄内容、响应截断/超限、无法解码。遇到空内容即使 HTTP 200 也应视为失败并让 fallback 继续；不要把“返回了某段文字”误报为合格正文。
3. 保留 provider 原生 warning、status 与计费相关用量（若返回），并区分“目标站失败”“provider 失败”“质量门控拒绝”。这比把三家错误硬压成同一个字符串更有利于回退和诊断。

### 不应承诺的事

- 不承诺相同 URL 在三家得到同一 Markdown、同一标题/链接/图片集或相同的 main-content 边界；Jina selector、Tavily query chunk、Firecrawl `onlyMainContent` 本来就是不同语义。
- 不承诺无导航、无广告、无过期内容、可绕过登录/反爬，或“完整正文”；这些都是网站、渲染时间、缓存、proxy 与产品版本共同决定的。
- 不以本地的 readability/正则二次过滤来伪造一致性。旧研究已显示此类处理在索引页和链接列表有误伤风险；#89 的既定方向“上游原生精简优先，本地只包装与门控”更安全。

### 参数分层

| 固定为 provider profile（可测试） | 只由调用者选择 | 仅站点定制/显式高风险选择 |
|---|---|---|
| 明确请求 Markdown；有限 timeout；不在默认路径传 cookie/header；Firecrawl 保持 `onlyMainContent: true`；Tavily 用 `basic` + Markdown 且不传 `query`（完整内容优先）；Jina 用明确 Markdown 输出。 | freshness/cache（是否接受旧内容）、图片/链接是否需要、Tavily `advanced`、Firecrawl 是否 JSON/ZDR/Enhanced、Jina 链接保留模式。 | CSS target/remove selector、浏览器 actions、目标站 cookie/headers/storage state、国家/自带 proxy、注入脚本、Tavily `query + chunks_per_source`。这些都会让结果变成站点或任务特定。 |

特别地，Jina `X-Retain-Links: text` 对上下文瘦身有效但会移除 URL；它只能是“文本阅读”选项，不能作为默认同时声称“可继续导航”。Firecrawl 的 JSON mode 也不应混入通用 fetch 默认，因为它把网页 fetch 改成付费 LLM 抽取。缓存、ZDR 和复杂浏览器动作同理，应让调用方为新鲜度、隐私、成本和成功率取舍负责。

## 6. 局限性与未证实项

- 本文没有做三家同 URL、同时间、同参数的大样本基准，因此没有成功率、P50/P95 延迟、正文准确率或 token 数的排名；旧 MDN 实测只作历史校准。
- Tavily Extract 的 PDF、JS 引擎、缓存、目标站认证/代理和 selector 支持在本次官方 API 阅读中均未证实；不能据此称“不支持”。
- Jina Reader 的 PDF URL 与 PDF/Office 直传已获 README 明确支持；但同 endpoint 异步批量、`instruction`/`jsonSchema` 的端到端返回与计费契约、常规数据留存期限未证实。DNT/EU endpoint 也不等于完整的企业数据处理协议。
- Firecrawl 常规 scrape 的具体数据留存期限未在本次阅读中证实；只有 ZDR 的“请求外不持久化”有明确产品说明。所有隐私结论都应在采购/合规前以当期 DPA、子处理者和合同复核。
- 价格、RPM、并发、免费额度和 package 下载都会变化；表中数字仅对观测时间成立。npm 下载数据覆盖 2026-07-04 至 08-02，而不是完整自然月。

## 方法与来源

- 官方网页资料用本机 `forager fetch` 读取，未调用三家目标 fetch/scrape/extract API；GitHub 指标用已登录的 `gh repo view` / GitHub REST contributors 分页读取。`gh` 凭据有效，故未退回匿名网页抓取。
- 核心官方来源：Jina [Reader README](https://github.com/jina-ai/reader/blob/main/README.md)、[OpenAPI](https://r.jina.ai/openapi.json)、[Reader 价格页](https://jina.ai/reader)；Tavily [Extract](https://docs.tavily.com/documentation/api-reference/endpoint/extract)、[Credits](https://docs.tavily.com/documentation/api-credits)、[Privacy](https://www.tavily.com/privacy)；Firecrawl [Scrape](https://docs.firecrawl.dev/api-reference/endpoint/scrape)、[Advanced Scraping](https://docs.firecrawl.dev/advanced-scraping-guide)、[Billing](https://docs.firecrawl.dev/billing)、[Rate limits](https://docs.firecrawl.dev/rate-limits)。
- 社区源：官方 GitHub REST/仓库页及 npm Registry downloads API，具体链接紧贴表中指标。没有使用二手评测、SEO 比较或厂商营销数字支撑事实结论。
