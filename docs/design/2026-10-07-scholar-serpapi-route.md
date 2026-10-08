# Google Scholar 平台与 SerpApi provider 设计

状态：search、fetch 与 skill 的 Scholar 指引均已实现。日期：2026-10-07。规格 issue 见 GitHub（`ready-for-agent`）。

## 目标与边界

为 forager 新增第三个平台 `scholar`，首条 route 为 provider `serpapi`：经 SerpApi 的 Google Scholar API 检索，凭据沿用现有 Provider Credential Pool（ADR 0005），以 SerpApi 免费计划为默认使用场景。

- **要解决的问题**：arXiv 与 SSRN 只覆盖各自的预印本；Google Scholar 覆盖期刊、会议与各类预印本，并提供被引数与版本聚合，适合跨出版方的文献发现。
- **不做**：自建 Scholar 抓取（官方无公共 API，robots.txt 禁止 `/scholar`，见 `docs/research/2026-09-25-scholarly-agent-access.md`）；全文读取（Scholar 只是索引，正文交给目标平台或 Web Fetch）；跨调用的配额状态持久化；第一期不做被引列表、作者主页与引文导出。
- **与浏览器路线的关系**：OpenCLI 的 `google-scholar` 适配器可作为以后的 process route（`scholar_browser`），不在本设计范围。

## 已核实的外部事实

来源：[Scholar API](https://serpapi.com/google-scholar-api)、[状态与错误码](https://serpapi.com/api-status-and-error-codes)、[Account API](https://serpapi.com/account-api)、[价格](https://serpapi.com/pricing)，均于 2026-10-07 读取。2026-10-07 用一个免费计划 key 做了 16 次计费搜索，以及若干次不计费的请求（缓存命中、400、无效 key、Account API），结果见下文「真实请求验证」。

| 事实 | 内容 |
|---|---|
| 端点与认证 | `GET https://serpapi.com/search.json?engine=google_scholar&...&api_key=KEY`；key 只能放在查询参数，不支持 `Authorization` 头 |
| 免费计划 | 每账号每月 250 次搜索；价格页写每小时吞吐 50 次，免费账号的 Account API 实测报告 `account_rate_limit_per_hour: 250`；付费计划从 $25/月 1,000 次起 |
| 计费口径 | 只计成功搜索，合法空结果也算成功；缓存命中、出错和失败的搜索不计；`num` 为 1 或 20 都算 1 次（均已实测，见下表） |
| 缓存 | 查询与全部参数完全相同时 1 小时内命中缓存，免费；`no_cache` 默认 `false` |
| 检索参数 | `q`（支持 `author:`、`source:` 等运算符）、`as_ylo`／`as_yhi`（年份区间）、`scisbd`（`0` 相关性；`1`／`2` 为近一年新增并按日期排序，`1` 只含摘要条目）、`as_rr=1`（仅综述）、`start`（偏移）、`num`（1–20，默认 10）、`hl`、`cites`（被引检索）、`cluster`（全部版本，不能与 `q`、`cites` 同用） |
| 结果字段 | `organic_results[]`：`position`、`title`、`result_id`、`link`、`snippet`、`type`、`publication_info.summary`（形如 `作者 - 来源, 年份 - 域名`）与可选 `publication_info.authors[]`、`resources[]`（`title`、`file_format`、`link`）、`inline_links.cited_by.{total,cites_id}`、`inline_links.versions.{total,cluster_id}` |
| 状态 | `search_metadata.status` 为 `Success` 或 `Error`；搜索引擎无结果仍是 `Success`，同时顶层 `error` 给出说明 |
| HTTP 错误 | 400 缺参数；401 无效 key；403 账号无权限（通常已删除）；429 超出每小时吞吐**或**本月额度用尽（后者正文为 `Your account has run out of searches.`）；500／503 服务端错误 |
| Account API | `GET https://serpapi.com/account.json?api_key=KEY`，免费且不计额度，返回 `plan_searches_left`、`this_hour_searches`、`account_rate_limit_per_hour` 等；响应同时含 `api_key` 与 `account_email` |

### 真实请求验证（2026-10-07）

| 请求 | 观察 | 设计结论 |
|---|---|---|
| `q=time series momentum`，`num=20` | 20 条都有 `versions.cluster_id`，且与 `cited_by.cites_id`、`result_id` 解码值（cluster ID 的 8 字节小端序加 `0x09` 后的 base64url）一致；`position` 从 1 开始；`summary` 一律为 `作者 - 来源, 年份 - 域名`；15 条带 `link: null`、`title` 为空的资源，资源会重复；部分条目没有 `type`；首条 `versions.total` 为 45 | ref 以显式 cluster ID 为主，`result_id` 只作最后回退；资源丢弃空链接并去重 |
| 同查询加 `scisbd=2`、`as_ylo=2020`、`as_yhi=2021` | 年份区间被忽略；结果是 3 天前收录、与主题弱相关的书籍（2026、2027 年） | 第一期不提供按日期排序 |
| `cluster=18208131694456651388` | 11 个版本，没有下一页（search 却报 45 个版本）；第一条是第三方托管的 PDF，标题带 `$` 等杂字符；其中一条是另一篇复现论文；有版本没有 `link`；版本的 `result_id` 解码值是各版本自己的 ID，不等于父 cluster | fetch 不假设第一条是规范版本，以 `versions` 列表交付；`ref` 用请求的 cluster；不承诺列全所有版本 |
| 无结果的引号查询 | `status: Success`，`organic_results` 缺失，`search_information.organic_results_state: "Fully empty"`，顶层 `error: "Google hasn't returned any results for this query."` | 判为合法空集 |
| 无效 key | HTTP 401，`{"error": "Invalid API key. ..."}` | Auth |
| 全部响应 | 均不包含 key | 不投影 `search_metadata`、`search_parameters` 与分页链接 |
| 1 小时内重复第一次搜索的完全相同请求 | Account API 的 `this_month_usage` 不变 | 缓存命中免费，已确认 |
| 缺少 `q` | HTTP 400，`{"error": "Missing query \`q\`, \`cites\` or \`cluster\` parameter."}`；不计入月额度，但计入 `this_hour_searches` | 400 免费；飞行前校验仍有必要，它省的是吞吐和往返 |
| `num=50` | HTTP 200，自动截为 20 条，照常计费 | forager 自己把 limit 限制在 1–20 |
| `start=980`、`start=1000` | 980 返回 19 条且无下一页；1000 返回 `Fully empty`，**计费** | 1000 条上限确认；页尾规则避免为越界页付费 |
| `cluster=1`（不存在） | `Success` 加 `Fully empty`，**计费** | fetch 不存在的 ref 为 attempt 级 Parameter，且消耗 1 次额度 |
| `trend OR reversal author:"Moskowitz"` | 20/20 条的作者含 Moskowitz | `author:` 作用于整个查询，包括 `OR` 两侧 |
| `"time series momentum" author:"Pedersen"` | 3/3 条的作者含 Pedersen | 短语加作者正常 |
| `momentum crash hedging portfolio`，`as_ylo=2026` | 年份过滤生效；20 条中 11 条没有 `versions`，其中 8 条连 `cited_by` 也没有（新论文没有被引和多版本） | 显式 ID 对新论文经常缺失，`result_id` 是必要的身份来源 |
| 用两条无显式 ID 条目的 `result_id` 解码值请求 `cluster=` | 2/2 各返回 1 条，正是原论文 | `result_id` 解码值可以回查到原论文，作为身份来源可靠 |
| `Attention is all you need` 的 cluster（search 报 26 个版本） | cluster 只返回 9 条，没有下一页；第一条是 NeurIPS 正式版 | 与 `time series momentum`（45 对 11）一致：`version_count` 与 cluster 条数不是同一计数；第一条有时是正式版，有时不是 |
| `as_rr=1` | 返回综述类论文，且有下一页 | `--review-only` 可用 |
| `cluster=18208131694456651388`，`num=20`（2026-10-08） | 20 个版本，且有下一页 | 簇的版本可能多于一页；「未列全」诊断会实际触发 |
| 按设计投影 20 条 search 结果 | JSON 约 19 KB，约 4.8k token；10 条约 2.5k token | 默认 `--limit 20` 的上下文成本可接受 |
| 不带 key 的 `GET /search.json` | HTTP 401 | 普通 doctor 的可达性探测可用，不计费 |

## 平台 `scholar`

- **id**：`scholar`。kind 只有 `paper`。
- **ref**：`scholar:<cluster_id>`，cluster ID 为不溢出的十进制 u64，无版本号。
- **canonical URL**：`https://scholar.google.com/scholar?cluster=<cluster_id>`。
- **URL 解析**：接受 `scholar.google.com/scholar` 且查询参数恰有一个 `cluster`、没有 `cites` 的 URL，忽略其他查询参数、fragment 与末尾 `/`；`cluster` 重复、带 `cites`（被引页，含与 `cluster` 同时出现）、作者主页等不可识别，飞行前退 2。往返性质：解析 canonical URL 得到同一个 ref。
- **Content Depth**：search 条目有 snippet 时为 `snippet`，否则为 `metadata`；fetch 只支持 `metadata`。Scholar 的片段永不当作摘要。
- **查询语法**：QUERY 属于 L0，原样发送，谷歌学术的运算符（`"短语"`、`OR`、`-词`、`author:`、`source:`）照常生效。第 7 章「不提供原样透传」约束的是结构化选项与 API 参数出口，第 7 章需写明 L0 查询语法由平台定义。

## Route `serpapi`

**命名**：provider 按供应商命名为 `serpapi`，不叫 `scholar_serpapi`。SerpApi 的同一端点、同一账号还提供 Google、Google Patents 等其他引擎，额度和每小时吞吐按账号计算，与引擎无关。`providers.<id>` 本来就按供应商分节（tavily、jina 同时服务 web_search 与 web_fetch），所以 key 池 `providers.serpapi.keys` 和凭据游标（键 `serpapi`）可以直接给以后的其他 SerpApi 平台复用，一个账号的轮换状态也只有一份。目前它只出现在 `scholar` 平台的 route 集合中；以后接入其他引擎时，把 `serpapi` 加进对应平台的 route 集合，并按平台分派请求构造与解码。

| 注册项 | 取值 |
|---|---|
| `credentials_required` | `true` |
| `transport` | `Http` |
| `access_policy` | 不设，与其他需要凭据的 SaaS provider 一致；吞吐上限由 429 加凭据轮换处理 |
| `probe` | `DoctorProbe::PlatformSearch { platform: Scholar, name: "search", transport: "http" }` |
| 配置 | `providers.serpapi.url`（默认 `https://serpapi.com/search.json`）、`.keys`、`.timeout`（默认 30 秒） |
| 默认 order | `platforms.scholar.order = ["serpapi"]` |

它不是 process route，可以进入默认 order。默认 order 中有它不等于已配置，也不会让普通 search 自动调用它。

配置有两个消费路径，都要覆盖新 route 的 keys：平台链与 smoke 用的 `PlatformRouteConfig::configured()`（匿名 route 目前传空切片，新变体传自己的 keys），以及 doctor 用的 `provider_runtime`。两边都调用注册信息的 `is_configured`。

| 情况 | 结果 |
|---|---|
| keys 为空 | 飞行前 Config，退 3，零请求；消息额外点名 `providers.serpapi.keys`（现有消息只写 order 键） |
| order 为空 | 飞行前退 3，消息只提示 order |
| cursor 指定的 route 已移出 order 或已无 key | 沿用现有 pinned route 规则，退 2 |
| keys 非空，上游 401/403 | attempt 级 Auth，不换 key、不重试，退 4（飞行后传输错误） |

### 凭据池复用

执行走 `execute_v2`，不新增凭据机制：

- 每次请求在 XDG 状态文件锁内领取 `next_index` 并立即推进，所以多个 key 按轮询平摊用量。
- 遇到 RateLimited 或 QuotaExhausted 时换下一个 key，同一次请求里轮换次数不超过 key 数；Network、Timeout 在同一个 key 上按共享重试策略重试，轮换预算与 `retry.max_attempts` 分开计算。
- 所有 key 都返回额度耗尽时终态为 QuotaExhausted；失败原因混合时按现有归因规则取终态。
- 某个 key 额度耗尽后，轮到它时会先收到一次 429 再换下一个 key。出错不计费，代价只是一次往返的延迟。因此不做配额状态的持久化。

### 错误归因

HTTP 错误归因只由 net 的共享读取边界负责（GLOSSARY「Provider HTTP Read Contract」；第 4 章「status→kind 映射只在 net 一份」），route 不自建状态码映射，也不复制读取上限与脱敏逻辑。

- **共享层的唯一改动**：429 的额度嗅探在现有 `quota` 之外，再识别 SerpApi 的 `run out of searches`（大小写不敏感）。于是额度耗尽为 QuotaExhausted，其他 429 为 RateLimited，都换 key。
- 其余状态码全部继承共享规则：400 为 Parameter，401/403 为 Auth，408/504 为 Timeout，其他 5xx 为 Network，3xx 为不可重试的 Runtime。
- **route 只解码 HTTP 200 的成功协议**，并在 `execute_v2` 的单次发送闭包内判定，这样 status 为 200 的失败 attempt 也能如实记录：

| HTTP 200 响应 | 结果 |
|---|---|
| `search_metadata.status` 为 `Success`，`organic_results` 是非空数组 | 成功 |
| `Success`，缺少 `organic_results`，且 `search_information.organic_results_state` 恰为 `Fully empty` | 合法空集 |
| `status` 为 `Error` | Network（重试），消息取 SerpApi 的 `error` |
| `status` 缺失或未知、JSON 形状不对 | Runtime |

**凭据不外泄**：key 只能放在 URL 查询参数里。现有 `Secret` 的 Debug 固定打码，net 的错误消息先脱敏再截断，`redact_url` 会把 `api_key=` 打码；但 `execute_v2` 不替失败消息脱敏，所以 route 自己产生的消息（200 加 `status: Error`、解码错误）必须在进入 `AttemptFailure` 前按凭据值脱敏，reqwest 错误先去掉 URL 再格式化。debug/trace 只投影安全的 attempt 字段，平台直连命令不写 journal，这两处都不需要新增机制。需要一个测试来保证：用一个可识别的假 key 走完 401、429、200 加 Error 和网络错误，覆盖 `--verbose` 与 trace 输出，断言都不出现这个 key。不用响应里的 `serpapi_pagination` 链接，也不把 `search_metadata`、`search_parameters` 投影到输出。

**多个免费账号**：key 池技术上支持任意多个 key。SerpApi 的条款页面里没有找到明确禁止一人注册多个免费账号的条文，但这种做法可能被认定为滥用免费计划。文档只写"每个 key 对应一个账号额度"，不建议用多注册账号的方式扩大额度。

## search

### 参数

| 层 | 参数 | 线上映射 | 校验 |
|---|---|---|---|
| L0 | `QUERY` | `q`，原样发送 | 去空白后不能为空 |
| L0 | `--limit` | `num` | 1–20，默认 20（每页无论多少条都计 1 次，默认取满页最省额度） |
| L1 | `--year-from YYYY`、`--year-to YYYY` | `as_ylo`、`as_yhi` | 各自 1000–9999；from 不能晚于 to |
| L1 | `--review-only` | `as_rr=1` | 无 |
| 固定 | `engine=google_scholar`、`hl=en` | 固定 `hl`，保证 `publication_info.summary` 的格式稳定 | 不提供 flag |

上表的值域由 types 的请求校验负责（clap 也做同样的限制），因为恢复 cursor 时会绕过 clap、直接调用请求校验；被改动的 cursor 带着越界的 limit、年份或页位置时，零请求退 2。

不提供的参数：

- **`--sort date`**：实测 `scisbd=2` 会忽略年份区间。
- **`--author`**：实测查询词里的 `author:"Name"` 作用于整个查询（含 `OR` 两侧与短语），直接写在 QUERY 里即可；单独的 flag 只会多一条把作者拼进查询词的规则。skill 用实测过的写法举例。
- **`--cites`、`--cluster`**：前者改变结果类型，后者属于 fetch，都按 L2 规则处理。

### 解码

SerpApi 响应的 DTO 与解码属于 route，不进 types 门面。

| 输出字段 | 来源 |
|---|---|
| `ref` | 优先用 `inline_links.versions.cluster_id`，其次 `inline_links.cited_by.cites_id`；两者都存在但不一致时跳过该条并记 diagnostic。两者都没有时用 `result_id`：严格校验 base64url、9 字节长度和末字节 `0x09`，取前 8 字节小端序整数。这条规律没有上游文档保证，但实测有显式 ID 的 40 条全部一致，无显式 ID 的条目用解码值回查 cluster 2/2 取回原论文；新论文约四成没有显式 ID，所以它是常用来源，不是罕见兜底。只适用于搜索结果，cluster 响应里的版本 `result_id` 是各版本自己的 ID。仍无法确定时跳过该条并记 diagnostic；上游有非空结果但全部被剔除时，attempt 为 Runtime，不当作合法空集 |
| `url` | 由 ref 推导出的 canonical URL |
| `depth` | 有 `snippet` 为 `snippet`，否则为 `metadata` |
| `title` | `title` |
| `authors` | `publication_info.authors[].name`；没有时取 `summary` 第一个 ` - ` 之前的部分，按 `, ` 切分并去掉 `…`。Scholar 显示的作者名可能是缩写或截断后的，这一点在 skill 的证据规则里要写明 |
| `published` | 只从 `summary` 中间一段（来源和年份所在的那段）取年份：该段末尾的 `, YYYY`，或整段恰为 `YYYY`（实测有 `Authors - 2024 - domain` 这种无来源的形式）；取不到时为 `null`。这样避免把 arXiv 编号这类数字误当年份（OpenCLI 适配器就有这个 bug） |
| 平台字段 | `snippet`、`link`（可空）、`source`（`summary` 原文）、`cited_by`（整数或 `null`）、`version_count`（整数或 `null`）、`resources`、`result_type`（`type` 原值，可空） |
| `resources` | 允许上游资源的 `link` 缺失或为 null；只输出带有效 HTTP(S) URL 的资源，形状 `[{title, file_format, url}]`，`url` 非空，按 URL 稳定去重，空 `title` 合法。search 与 fetch 共用这条规则 |

### 分页

- 页位置为绝对偏移 `start`。cursor 是 `v1.serpapi.<payload>`，payload 复用现有 `PlatformSearchRequest`，保存查询词、L1 选项、`limit` 和下一页的 `start`，无需另写编解码。
- 沿用 SSRN Crossref 的页尾规则：只有响应带 `serpapi_pagination.next`（只看是否存在，不使用其链接），且下一页的页尾 `start + limit` 不超过 1000 时才签发 cursor。limit 不整除 1000 时会提前结束。下一页按原始偏移推进，不按身份过滤后的条目数推进。
- route 的支持检查拒绝页尾越界的页位置，退 2。
- 每翻一页都是一次计费搜索；用 cursor 重放同一页时，如果在 1 小时缓存期内则不计费。

## fetch

- 输入：`scholar:<id>` 或 cluster URL；只接受 `--depth metadata`（默认），其他深度在支持检查中拒绝，退 2。
- 请求：`cluster=<id>`，`num=20`，只请求一页，不自动翻页，不签发 fetch cursor。
- 输出：
  - `ref` 和 `url` 来自请求的 cluster；`depth` 固定为 `metadata`，不带 snippet。
  - `title`、`authors`、`published` 是第一个版本的代表性书目信息。
  - 平台字段 `versions` 按谷歌学术的顺序列出本页每个版本的 `{title, link, source, resources}`。
  - 响应带下一页信号时，用 diagnostic 说明版本没有列全，并指向 canonical cluster URL。2026-10-08 实测 cluster 18208131694456651388 在 `num=20` 下返回 20 个版本并带下一页，所以这条诊断会实际触发。
  - 不把 search 的 `version_count`、cluster 的 `total_results` 和本页条数当作同一个计数。
- 实测第一个版本可能是第三方托管的副本、标题带杂字符，簇内也可能混入别的论文，所以 skill 要求按 `versions` 挑选来源，不把第一个版本当作规范出版版本。
- 返回空集时为 attempt 级 Parameter（条目不存在），退 4。
- 正文：不提供。skill 引导 agent 在 `link` 或 `versions[].link` 指向 arXiv、SSRN 时改用 `platform arxiv|ssrn fetch`（直接传 forager 返回的 URL），其他情况用 `forager fetch`。

## 额度预算

| 操作 | 计费次数 |
|---|---|
| `platform scholar search`（每页，含零结果） | 通常 1；1 小时内参数完全相同的重复请求为 0 |
| `platform scholar fetch`（含 ref 不存在） | 通常 1 |
| HTTP 4xx、429、缓存命中 | 0（4xx 仍计入每小时吞吐） |
| `forager doctor` | 0（只对端点发一次不带 key 的 GET，任何 HTTP 响应都算可达） |
| `forager doctor --provider serpapi` | 0（每个 key 调用一次 Account API） |

"通常"的含义：一次逻辑检索成功且未命中缓存时计 1 次；重试、超时等故障下，本机拿不到结果并不能证明上游没有处理，因此不保证精确账单。

以 250 次/月计，大约够每个工作日 10 次检索。skill 应写明：先用一次 search 拿 20 条候选，只有在需要版本链接时才 fetch，不要用翻页来凑数量。

`platforms.scholar.order` 设为空会禁用平台命令及其 smoke，但不影响显式的 `doctor --provider`：HTTP route 的 provider 深探按 key 是否存在决定能否运行，不看 platform order。

provider 深探调用 Account API，不计费，并报告每个 key 的剩余额度与本小时吞吐。Account API 的响应里有 `api_key` 和 `account_email`，因此只解码数字字段。

## 实现改动清单

按 07-platforms.md 接入清单逐项完成：

1. **types**：`Platform::Scholar`；`platform_scholar` 叶子模块包含 `ScholarRef`（解析、canonical URL）、`ScholarSearchOptions`、`ScholarItemData`；请求校验覆盖上文的值域；`PlatformRef`、`PlatformSearchOptions`、`PlatformItemData` 各加一个变体。
2. **config**：新增需要凭据的 HTTP route 配置形状 `KeyedHttpRouteRuntimeConfig { url, keys, timeout_seconds }`，作为现有 `HttpRouteRuntimeConfig` 的姊妹类型；schema 增加 `providers.serpapi.*`（`keys` 用现有 Secrets 叶子）与 `platforms.scholar.order`；`PlatformRoutesRuntimeConfig`、`PlatformRouteConfig`（含 `configured()`）、`platform_route_config`、`provider_runtime` 覆盖新 route。
3. **net**：429 额度嗅探增加 `run out of searches`。
4. **route**：`providers/serpapi` 模块。SerpApi 通用部分（端点、`api_key` 参数、200 成功协议判定、凭据脱敏）与 Google Scholar 引擎部分（请求参数、DTO 解码、search 与 fetch 的支持检查）分开，以后加引擎只新增引擎部分；`factory` 中补齐 build 与 support 分支。
5. **catalog**：`ProviderId::Serpapi`、注册信息、`PLATFORMS` 条目（search 与 fetch 的 route 集合、默认 order）、search 与 fetch 各一个 smoke 用例。
6. **CLI**：`ScholarSearchArgs`、`ScholarFetchArgs`；缺 key 时的退 3 消息点名 keys 配置键。
7. **测试与清单**：`tests/acceptance-manifest.json` 的 `(serpapi, platform:scholar:search)` 与 `(serpapi, platform:scholar:fetch)`、smoke 用例 ID 与第 5 章矩阵、checklist 的样例 ref 与 keyed route 配置夹具。
8. **文档**：`GLOSSARY.md`、规格第 2、3、4、5、7 章；skill 的 `platform-vocabulary.json`、`references/platforms.md`、`references/cli.md`；新增 ADR，记录"Scholar 只经第三方 SERP API 接入、需用户自备 key、免费额度优先"这一决定。

## 测试接缝

| 接缝 | 测什么 |
|---|---|
| `forager platform scholar search\|fetch` 进程级测试，对本地 HTTP fixture（主接缝） | 线上参数编码（含 cursor 第二页）；条目解码（显式 ID 缺失时的 `result_id` 回退、显式 ID 冲突、无法确定身份、无作者数组、summary 里带 arXiv 编号、资源含 null 链接与重复、`link` 缺失）；恰为 `Fully empty` 的合法空集与"全部剔除"的 Runtime；200 加 Error 重试；两个 key 时额度耗尽触发轮换；非空无效 key 恰好发送一次、退 4；空 key 池零请求退 3；cursor 往返、互斥与被改动 cursor 的零请求退 2；fetch 的 `versions`、metadata 深度与"未列全"诊断；凭据不外泄 |
| types 公开函数 | ref 解析与 canonical URL 往返（含溢出、重复 `cluster`、带 `cites`）；请求校验的值域与年份顺序 |
| net 共享层 | 429 额度嗅探真值表加入 `run out of searches` |
| 规格 checklist 测试 | R1–R8；需补 keyed route 的配置夹具。checklist 只证明登记点齐全，keys 能否流到执行器由进程级测试证明 |

期望值用字面量或真实样本，不用生产解码公式重新计算。真实 SerpApi 只在 smoke 中调用，并且要求已配置 key。

## 分期

1. **第一期**（已实现）：上述 search 与 fetch。
2. **第二期**：
   - L2 操作 `cited-by REF`（`cites=<cluster_id>`）**已实现**，含 `--query`、年份与 `--sort date`（`scisbd=2`，与年份互斥）。2026-10-08 的实测与决定见 #183，现行契约见规格第 7 章「Google Scholar」。
   - provider 深探改用 Account API **已实现**：逐 key 报告剩余额度，不计费，见 #184 与规格第 2 章契约③。
3. **按需**：`scholar_browser` process route（复用 OpenCLI，需用户手动开启）。

## 仍未实测的部分

- **429**（额度耗尽、每小时吞吐超限）与 **HTTP 200 加 `status: Error`**：要把免费额度用完或碰上上游故障才能触发，按官方文档设计，由 fixture 测试覆盖。
- **`as_yhi`** 单独使用的效果没有测；`as_ylo` 已验证。
