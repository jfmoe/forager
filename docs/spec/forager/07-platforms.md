# 7. 平台接入

本章是接入任何内置平台的权威契约。新增或修改平台时先读本章，并保持新平台 checklist 测试（`src/core/platform_checklist.rs`）通过。决策依据见 ADR 0019；arXiv 是参考实现，各节以它举例；SSRN 是第二个平台，见文末。

## 概念与规则

- **Platform**：内置的外部内容源，拥有自己的身份空间。它与 Capability Seam 并列，不是 provider，也不是 Vertical Search 的垂直域。只支持随版本发布的内置平台；配置不能定义平台或通用 MCP/CLI route。
- **Platform Route**：接入某个平台的一条路线。route 就是 provider，沿用 provider 身份、凭据池（需要凭据时）、provider 配置段、doctor 探针与 smoke 登记。route id 绝不使用裸平台名（用 `arxiv_api`，不用 `arxiv`）。一个 provider 可以服务多个平台和 seam。
- **Route 传输类型**：注册信息的 `transport` 声明 route 到达内容源的方式：`Http`（发 HTTP 请求），或 `OpenCli`（运行本机 OpenCLI 中 forager 自有 adapter 的命令，附 site 与契约版本；这类 route 称为 process route）。配置检查、doctor 与 checklist 测试按传输类型判断，不按 route id 判断。process route 只能由用户手动加入平台 order，永不进入 `default_order`（ADR 0020）。
- **链语义**：同平台的 route 按 `platforms.<id>.order` 组成 fallback 链，由共享链执行器运行，沿用 LegitimateEmpty 语义：至少一条 route 返回合法空结果、之后没有 route 被接受时，结果为空成功。结果永不跨平台 fallback。
- **Platform Ref**：平台实体的类型化身份，由平台、平台自有的 kind 和 id 组成。kind 只按「身份空间不同」或「fetch 结果形状不同」划分，不按对话角色划分。字符串形式为 `<platform>:<id>`，例如 `arxiv:2401.01234v2`。
  - ref 解析与 canonical URL 推导是 types 门面中的零 IO 纯函数。每个 kind 都满足往返性质：解析 ref 的 canonical URL，得到同一个 ref。
  - 版本号可选，由平台决定是否有版本。不带版本的 ref 推导出不带版本的 URL；纯函数不猜测版本，实际版本只在平台返回后确定。
  - 需要联网解析的短链在飞行前退 2。
- **Content Depth**：`metadata`、`snippet`、`abstract`、`full_text` 或 `thread`。`metadata` 只有书目信息，没有摘要。每个平台为它支持的每种深度定义含义，每个结果条目都带 `depth`；深度之间没有全局排序。摘要深度绝不当作全文，片段绝不当作摘要。

## 参数分层

- **L0 公共参数**：search 为查询词与 `--limit`；fetch 为 ref 或 URL，以及 `--depth`。时间窗和排序不进公共层，因为各平台语义不同。
- **L1 平台选项**：类型化、全部可选、有默认值，定义为 types 门面中平台选项类型（例如 `PlatformSearchOptions`）下按平台划分的封闭变体。CLI 参数是每个平台的静态 clap 定义，再转换到 types 门面，types 不依赖 clap。clap 在飞行前校验枚举和取值范围；跨字段规则（例如日期区间顺序）由 types 中的校验函数负责；两者出错都退 2。
- **L2 平台操作**：凡改变结果种类或必需输入的，就是新操作，不是参数。只有一个 route 实现的操作写成该 route 适配器的 inherent 方法；出现第二个实现它的 route 时，提升为 trait，并在 platform catalog 中为该操作登记 route 集合。
- **不提供原样透传。** 新增一个参数需要：一个选项字段、一个 clap flag、每条 route 各自的映射。
- **不支持的选项**：每条 route 提供一个只读请求（选项与页位置）、不联网的支持检查。检查在构造链之前完成：
  - 部分 route 不支持：被跳过的 route 记一条 disposition 为 Skipped、`error_kind` 为空的 attempt，消息写明选项与 route。
  - order 中没有 route 支持：飞行前退 2，不发网络请求，消息写明选项和已配置的 route。
  - route 绝不能静默忽略显式选项。

## 必须提供的操作与输出形状

- 每个平台都提供 **search** 与 **fetch**，命令为 `forager platform <id> search|fetch`。
- **search 结果页**：`{platform, provider, items, next_cursor}`；`provider` 是产出该页的 route；`--verbose` 时附 `provider_attempts`（含 Skipped attempt）。每个 item 含 `ref`、canonical `url`、`depth`、`title`、`authors`、`published`，以及平台自有字段（与公共字段同层）。
- **cursor**：不透明值，格式 `v1.<route>.<payload>`，payload 能完整恢复上一次请求的查询词、选项、limit 与下一页位置。带 cursor 的请求只在产出它的 route 上执行，不 fallback；该 route 必须仍在当前 order 中、支持该操作且已配置。cursor 与显式传入的查询词、L1 选项或 `--limit` 互斥（默认值不算冲突），通用 flag 可以同时使用。
- 平台直连命令不写 Search Result Journal。

### fetch

- **输入**：ref 字符串或可识别的原始平台 URL（L0），以及 `--depth`。平台只接受它定义过含义的深度，由 route 的支持检查判定。
- **元数据段**：fetch route 链（`platforms.<id>.order` ∩ fetch route 集合 ∩ 已配置 route）返回条目元数据；输出的 ref 带平台返回的实际版本。条目不存在是 attempt 级 Parameter；元数据段失败即命令失败，绝不跳过元数据直接取正文。
- **正文段**（仅 `full_text`）：答复的 route 在 `PlatformFetchOutcome.content_urls` 中按顺序声明同一版本的正文 URL；route 可以先做受访问策略约束的探测来决定顺序，但不 import Web Fetch provider。core 的 `platform_fetch` 对每个 URL 运行全局 Web Fetch 链（`capabilities.web_fetch.order` 及其凭据、薄正文门、4 MiB 截断诊断），首个成功者即正文；有后续 URL 时本段最多用剩余预算的一半；全部失败时沿用最后一条 Web Fetch 链的终态；第 4 章的归因总函数只在每条链内部归约，不跨正文 URL 合并。不新增平台级正文顺序；`full_text` 时没有已配置的 Web Fetch provider 为飞行前退 3，其他深度不需要 Web Fetch 配置。

### fetch 输出与正文交付

遵循 ADR 0015「默认结果只含下一步决策所需内容，已落盘材料按引用交付」：

- **`full_text`**：正文写入本地 Markdown 文件；stdout 返回 `platform`、`provider`（元数据 route）、条目公共字段与平台字段（`depth` 为 `full_text`），以及 `content_url`（正文实际来自的 URL）、`content_provider`（Web Fetch provider）、`content_path`（可直接读取的文件）与 `content_len`（正文字符数）。正文不出现在 stdout。
- **其他深度**：元数据与该深度的内容直接内联，不写文件。
- **`--format content`**：显式把正文（非全文深度时为该深度的内容）输出到 stdout，不写文件。
- **文件位置**：默认写入系统临时目录下按调用隔离的目录 `forager-platform/<pid>-<时间戳>`（与 research 默认证据目录同一规则），`--content-dir DIR` 可以覆盖；文件名由带版本的 ref 把 `:` 与 `/` 换成 `-` 后加 `.md` 派生。写入方式与 research 证据文件相同；写入失败为 Runtime（退 4），不回退为内联输出。不做跨调用缓存。
- `--verbose` 时 `provider_attempts` 依次包含 Skipped route、元数据 attempt、探测 attempt 与各 Web Fetch attempt。

## 退出码阶段矩阵

| 情况 | 阶段 | 结果 |
|---|---|---|
| ref 或 URL 无法识别、短链、cursor 冲突或无效、选项取值非法、所有已配置 route 都不支持所请求的选项 | 飞行前（参数） | 退 2，不发网络请求 |
| 平台 order 为空、操作可用 route 集合为空、order 含其他平台的 route；`full_text` fetch 时没有已配置的 Web Fetch provider | 飞行前（配置） | 退 3，不发网络请求 |
| 平台返回参数错误（例如 arXiv Atom error entry）、fetch 的条目不存在 | 飞行后，attempt 级 Parameter | 退 4，带平台消息 |
| fetch 元数据成功，但所有正文 URL 的 Web Fetch 链都失败 | 飞行后 | 沿用最后一条 Web Fetch 链终态，例如全部过薄为 Quality，退 5 |
| 正文文件写入失败 | 飞行后 | Runtime，退 4，不回退为内联输出 |
| 等待限速窗口时剩余预算不足 | 飞行后 | Timeout，退 4，保留已完成的 attempts |
| 限速状态文件或锁不可用 | 飞行后 | Runtime，退 4，不发送请求 |
| 合法零结果检索 | 成功 | `items: []`，退 0 |

attempt 级 Parameter 不映射为退 2（第 4 章）。

## 访问策略与限速

- route 在注册信息中以 `access_policy` 声明最小间隔与最大并发。对该 route endpoint 的**每次发送**都先经过 `RateLimiter::acquire`，包括重试、doctor 的 shallow 可达性探测与 deep 探测。
- process route 的访问策略以一次 OpenCLI 命令为单位：浏览器在一次命令内发出的请求不逐个限速。permit 持有到子进程被回收为止（ADR 0020）。
- 等待窗口的时间计入 attempt 与命令的 Deadline；算法、跨进程协调范围与已知边界见第 4 章「跨进程限速」。
- 不需要凭据的 route 在注册信息中声明 `credentials_required: false`：配置节没有 `keys`（HTTP route 只有 `url` 与 `timeout`，process route 只有 `command` 与 `timeout`），经 `execute_anonymous` 执行，attempt 的 `credential_index` 与 `rotation_count` 恒为 0。

## skill 与平台词表

- forager skill 的 `SKILL.md` 有一条不列举平台的路由规则：请求点名平台、给出平台 URL 或 ref、或需要平台原生条目时，读取平台 reference（`skills/forager/references/platforms.md`）。
- 平台 reference 写明全文读取方式、分支映射、消费规则（只使用用户给出的或 forager 返回的 ref 与 URL，绝不臆造或拼接）、每个平台的证据语义与恢复规则。
- 平台词表（`skills/forager/references/platform-vocabulary.json`）是机器可读文件，记录每个平台的 id、用途、选择规则、示例与 ref 语法，不写选项和操作；它是以后 classifier 与 `--platforms` 校验的共同来源。新增平台时同步更新词表、平台 reference 与 CLI reference（`skills/forager/references/cli.md`），`tests/skill_contract.rs` 检查词表与 `Platform::ALL` 一致。

## 接入清单

新增一个平台或 route 时逐项完成。新平台 checklist 测试对每个平台操作（search 与 fetch）检查 R1–R8，失败消息写明平台、缺少的登记点与清单编号。

| 编号 | 登记点 | 一致性测试 |
|---|---|---|
| R1 | `catalog::PLATFORMS` 登记平台，search 与 fetch 的 route 集合都非空，并声明默认 order（`default_order`，只含该平台的 route）；`types::Platform` 增加变体，平台形状放在新的 `platform_<id>` 类型叶子中 | checklist 测试；`catalog` 单测 `catalogs_project_every_registration_probe_and_smoke_case_consistently` |
| R2 | 每条 route 有 `ProviderId`（全集、解析与名称）和 `ProviderRegistration`；route id 不是裸平台名 | checklist 测试；`catalog` 注册校验 |
| R3 | factory 覆盖每个操作的每条 route：`platform_search_support`／`platform_fetch_support` 有该 route 的分支（process route 在非 Unix 系统上由支持检查拒绝），`PlatformRoutesRuntimeConfig` 有该 route 的字段，`platform_route_config` 返回以该 route 为身份的 `PlatformRouteConfig`（`build_platform_search`／`build_platform_fetch` 按该变体构造）；route 适配器只构造请求、解码响应、声明正文 URL 并提供支持检查 | checklist 测试 |
| R4 | 配置 schema 有 `platforms.<id>.order`，并按 route 的传输类型有配置叶子：HTTP route 为 `providers.<route>.url` 与 `.timeout`，process route 为 `providers.<route>.command` 与 `.timeout`；`keys` 叶子当且仅当 route 需要凭据；runtime 投影与 `platform_route_config` 覆盖该 route | checklist 测试；`config` schema 单测 |
| R5 | route 的 doctor probe 为 `DoctorProbe::PlatformSearch`（或它所服务的 capability 的 probe） | checklist 测试 |
| R6 | search 与 fetch 各至少有一条 route 登记 smoke 用例，`SPECIFICATION_CASE_IDS` 与第 5 章矩阵同步 | checklist 测试；`tests/smoke.rs` 列表断言 |
| R7 | `tests/acceptance-manifest.json` 为每个操作的每条 route 登记 `(route, platform:<id>:<op>)` fixture 与测试引用 | checklist 测试；`catalog` 单测 `provider_fixture_projection_matches_transport_manifest` |
| R8 | ref 解析与 canonical URL 推导覆盖该平台每个 kind，并在 checklist 的样例表中登记样例 ref | checklist 测试；types 单测 |

同一改动中还须更新：`CONTEXT.md`（新术语）、第 2 章（命令与参数表）、第 3 章（配置键）、第 4 章（模块与依赖）、第 5 章（fixture 与 smoke 用例）、本章的平台示例，以及 skill 的平台词表、平台 reference 与 CLI reference。

## 参考实现：arXiv

- **route**：`arxiv_api`，官方 Query API（默认 `https://export.arxiv.org/api/query`）。不需要凭据；默认 timeout 30 秒；访问策略为每 3 秒 1 个请求、并发 1（arXiv 使用条款）。默认 order 只含这一条 route。
- **ref**：新式 ID（`2401.01234`）与旧式 ID（`hep-th/9901001`），都可以带版本号；旧式 ID 的学科子类不属于身份（arXiv 把 `math.GT/0309136` 解析为 `math/0309136`，Query API 只认后者），解析时去掉；字符串形式 `arxiv:<id>[v<n>]`；canonical URL 是 `https://arxiv.org/abs/<id>[v<n>]`；解析也接受 `arxiv.org`、`www.arxiv.org` 与 `export.arxiv.org` 的 abs、pdf（有无 `.pdf` 后缀）与 html 页面 URL，忽略 query、fragment 与末尾 `/`。裸 ID 与其他路径（例如 `list/`、`src/`、HTML 内的图片）不可识别。kind 为 `paper`。
- **search wire 编码**：查询词按空白、双引号与反斜杠切分，每个词写成 `all:"<词>"`，因此布尔运算符、字段前缀与括号都是字面词。arXiv 拒绝带转义双引号的引号词（`all:"a\"b"` 返回 HTTP 400），字面双引号无法发送，所以双引号只作分隔符；反斜杠会转义引号词的闭合引号（`all:"C:\"` 返回 HTTP 400），同样只作分隔符；分类写成 `cat:<code>`，多个分类写成 `(cat:a OR cat:b)`；作者与标题写成 `au:"<短语>"`、`ti:"<短语>"`；日期区间写成 `submittedDate:[YYYYMMDD0000 TO YYYYMMDD2359]`，缺少的一端用 `199101010000` 或 `999912312359` 补齐；所有条件以 ` AND ` 连接。`sortBy` 为 `relevance`、`submittedDate` 或 `lastUpdatedDate`，`sortOrder` 固定为 `descending`；`--limit` 为 `max_results`，cursor 的页位置为 `start`。
- **解码**：Atom feed 解码为 depth 为 `abstract` 的条目，含完整摘要与元数据；`opensearch:totalResults` 决定是否有下一页，缺少它的成功响应不是 arXiv feed，为 Runtime。cursor 的页位置是 `start` 偏移量，无法解析时在飞行前退 2。id 包含 `arxiv.org/api/errors` 的 entry 是错误，映射为 Parameter 并带上 arXiv 消息；无法解码的 feed 为 Runtime；HTTP 失败沿用共享 status 映射。
- **fetch**：route 支持 `full_text`（默认）与 `abstract`。元数据段请求 Query API 的 `id_list=<id>[v<n>]&max_results=1`；空 feed（`totalResults` 为 0）表示 id 或版本不存在，为 Parameter（`arXiv item not found: arxiv:<ref>`）；返回的条目与请求的 id 或版本不符为 Runtime；429 为 RateLimited，匿名 route 无凭据可轮换，直接成为 route 终态。`full_text` 时 route 对 `<Query API 主机>/html/<id>v<n>` 发一次不重试的 HEAD（经同一访问策略）：404 表示没有 HTML，正文 URL 只有 `https://arxiv.org/pdf/<id>v<n>`；2xx 或其他 HTTP／网络结果（未知）时依次为 `https://arxiv.org/html/<id>v<n>` 与同一版本的 PDF。探测等不到限速窗口时按访问策略终止（预算不足为 Timeout，状态不可用为 Runtime，都退 4、不发送探测、不读正文），保留已完成的 attempts。不按 provider 返回的内容判断有无 HTML，因为 arXiv 的「无 HTML」说明页能通过薄正文门。export 镜像与 arxiv.org 的 HTML 页面返回相同 ETag 与 404 语义（2026-09-26 实测），因此探测跟随 `providers.arxiv_api.url` 的主机，交给 Web Fetch 的正文 URL 始终是 arxiv.org 官方地址。正文由第三方 provider 抓取，不经过本机 arXiv 限速。

## SSRN

数据源选择与实测证据见 [SSRN 检索接入方案](../../research/2026-09-26-ssrn-integration.md)（2026-09-26）。

- **route**：search 与 fetch 的 route 集合都是 `[ssrn_crossref, ssrn_browser]`；platform catalog 的默认 order 只含 `ssrn_crossref`，`ssrn_browser` 由用户手动加入 `platforms.ssrn.order` 后才生效。两条 route 共用同一种 ref 与输出形状。
- **`ssrn_crossref`**：匿名 Crossref REST API（默认 `https://api.crossref.org`），只检索 SSRN 的 DOI 前缀 `10.2139`。不需要凭据；默认 timeout 30 秒；访问策略为每秒 1 个请求、并发 1（2026-09-26 公共池响应头为每秒 5 个请求、并发 1）。
- **ref**：`ssrn:<id>`，id 是不带前导零的 ASCII 数字，kind 为 `paper`，不带版本。canonical URL 是 `https://papers.ssrn.com/sol3/papers.cfm?abstract_id=<id>`。不联网即可解析的输入：ref 本身（前缀不区分大小写）；`papers.ssrn.com/sol3/papers.cfm` 摘要页，取 `abstract_id`，忽略其他 query 参数与 fragment；`ssrn.com/abstract=<id>` 与 `www.ssrn.com/abstract=<id>`；DOI `10.2139/ssrn.<id>`（不区分大小写）及其 `doi.org`、`dx.doi.org` URL。主机名不区分大小写，http 与 https 都接受。SSRN 的 `Delivery.cfm` PDF 链接（文件名里的数字不是论文身份）、其他路径、相似域名与短链都在飞行前退 2。
- **search wire 编码**：`GET <url>/prefixes/10.2139/works`，参数为 `query`（查询词原样传入，按上游相关性检索，不要求每个词都出现）、`rows`（`--limit`，1–100）、`offset`、`sort=score`、`order=desc` 与 `select=DOI,title,author,abstract,published,type,created,resource`。查询词必填；第一版没有其他选项。
- **分页**：页位置是绝对 offset，不使用 Crossref 的上游 cursor。只有同时满足以下条件时才签发下一页 cursor：原始页条数等于 `rows`；下一个 offset 小于 `total-results`；下一个 offset 加 `rows` 不超过 10000。Crossref 拒绝 offset 加 `rows` 超过 10000 的请求（HTTP 400，2026-09-26 实测：`rows=20` 时 offset 最大为 9980），因此 route 的支持检查在飞行前以参数错误拒绝越过该上限的 cursor。是否到末页按过滤前的原始条数判断。
- **解码**：响应的 `message-type` 必须是 `work-list`（search）或 `work`（fetch），否则为 Runtime；无法解码的响应为 Runtime。DOI 不符合 `10.2139/ssrn.<数字>` 的记录被跳过，DOI 写入 stderr 诊断；类型为 `journal-article` 的记录保留，因为 SSRN 把部分 DOI 登记成这个类型。条目字段：`title` 取第一个非空标题；`authors` 为 `given family`，机构作者取 `name`；`published` 取 Crossref `published` 的 date-parts，按原始精度写成 `2012`、`2012-04` 或 `2012-04-19`，不补齐；平台字段 `doi` 为 Crossref 记录的 DOI，`crossref_type` 为记录类型，`crossref_created` 为 Crossref 登记 DOI 的时间，永不用来填补 `published`。`snippet`、`posted`、`last_revised`、`date_written` 由读取 SSRN 页面的 route 填写，本 route 恒为 `null`。
- **摘要清理**：去掉 JATS 与 HTML 标记（引号内的属性值不结束标签），保留 CDATA 中的文本，块元素（`p`、`title`、`sec`、`list`、`list-item`、`br`、`div`）之间保留段落换行（空行），解码 XML 实体、`&nbsp;` 与数字字符引用，段内折叠空白。Crossref 摘要是 XML，其他命名实体原样保留。清理后为空的摘要按缺失处理。有摘要的条目深度为 `abstract`，没有摘要的为 `metadata` 且 `abstract: null`。
- **fetch**：请求 `GET <url>/works/10.2139/ssrn.<id>`。route 支持 `metadata` 与 `abstract`；请求的深度是最低要求，`metadata` 在有摘要时一并返回摘要，条目的 `depth` 记录实际拿到的内容。请求 `abstract` 但记录没有摘要时，在 attempt 内报 Quality，链落到下一条 route，全链如此则退 5。`full_text` 不支持，该 route 记为 Skipped；没有其他 route 支持时飞行前退 2。错误映射：HTTP 404 为 Parameter（`SSRN paper not found in Crossref: ssrn:<id>`，只说明 Crossref 没有该 DOI）；返回的 DOI 与请求的不一致为 Runtime；429 为 RateLimited，匿名 route 无凭据可轮换，直接成为 route 终态；其他状态码沿用共享 status 映射，HTTP 400 的消息取 Crossref validation-failure 的首条说明。

### Route `ssrn_browser`

- **传输与配置**：process route，经本机 OpenCLI 驱动用户自己的 Chrome，读取 SSRN 原站。配置项为 `providers.ssrn_browser.command`（OpenCLI 可执行文件，默认 `opencli`，不能为空）与 `.timeout`（一次命令的 attempt 超时，默认 90 秒），没有 `url` 与 `keys`；它是匿名 route，始终视为已配置。命令只能选择可执行文件，参数全部由 route 决定（ADR 0019）。访问策略为每 5 秒一次 OpenCLI 命令、并发 1。route 不重试，失败直接落到下一条 route。
- **OpenCLI 进程传输**（`providers/opencli`，不含站点知识）：输入为可执行文件、adapter（site 与契约版本）、命令、具名参数与 attempt 截止时间。
  - 调用形式：`<command> <site> <命令> --<参数> <值>… --timeout <秒> -f json --window background --site-session ephemeral --keep-tab false`。forager 自有 adapter 的每个命令都接受这些 flag。
  - 截止时间：attempt 截止时间之前预留 5 秒得到工作截止点，剩余整秒数作为 adapter 命令的 `--timeout` 参数传入。OpenCLI 据此设置 daemon 每次操作的截止时间；adapter 在 `--timeout` 之前 3 秒停止读取页面并返回，让 OpenCLI 在工作截止点之前关闭标签页。5 秒预留用于强杀、回收子进程与报告 attempt。子进程放入独立进程组；到达工作截止点或 future 被丢弃时，强杀整个进程组，不做分步终止。access permit 持有到子进程被回收为止。剩余时间不足时 attempt 以 Timeout 结束，不启动进程。
  - 输出上限：stdout 最多 4 MiB（协议上限），超出为 Runtime 并强杀进程组；stderr 只保留前 64 KiB 并做 URL 脱敏。
  - 非 Unix 系统：传输在启动进程之前以 Runtime 拒绝；factory 的支持检查同样拒绝，因此该 route 记为 Skipped。
  - 外壳：stdout 是一个 JSON 对象 `{contract, status, data}`。`contract` 与注册信息的契约版本（`forager-ssrn/1`）不一致为 Runtime，消息附安装提示（把 skill 的 `opencli/<site>` 目录复制为 `~/.opencli/clis/<site>`）；`status` 为 `ok` 或 `no_results`，只有 adapter 核实了站点自己的无结果提示时才是 `no_results`。
  - 退出码映射：

    | OpenCLI 结果 | 映射 |
    |---|---|
    | 0，且外壳有效 | 成功 |
    | 66（EMPTY_RESULT） | Runtime（永远不算合法空结果） |
    | 69，且 stderr 的 `code` 为 `ADAPTER_LOAD` | Runtime，附安装提示 |
    | 其他 69 | Network |
    | 75 | Timeout |
    | 77（含 LOGIN_WALL；adapter 报出的站点验证未通过与访问被拦截也用 77） | Auth |
    | stderr 报告未知命令（未安装 adapter 时 OpenCLI 1.8.6 以 2 退出） | Runtime，附安装提示 |
    | 找不到可执行文件 | Runtime，提示安装 OpenCLI 或设置 `command` |
    | 其他退出码、输出无法解码 | Runtime |

- **search**：adapter 的 `search` 命令直接打开 `https://papers.ssrn.com/searchresults.cfm?term=<查询词>&page=<原生页码>`。SSRN 每个原生页 50 条；页位置是绝对 offset，原生页码 = offset / 50 + 1，页内起点 = offset mod 50。每次最多返回 `--limit` 条且不跨越原生页，因此允许返回不足 limit 的短页；下一个 offset 按实际消费的条数推进。页内还有剩余条目，或页面的下一页链接可用且结果范围未到总数时，才签发下一页 cursor。route 核对页面 URL 的主机、路径、`term` 与 `page`、搜索框中的查询词（空白折叠后相同）、分页标记的当前页，以及结果范围 `Displaying results <first> to <last> of <total>` 与列出的条数一致；任何一项不符，或页面既没有结果范围也不是站点的无结果提示，都为 Runtime，不返回空列表。条目深度为 `snippet`（卡片上的高亮片段，以 ` … ` 连接；没有片段时为 `metadata`），`published` 为卡片 Posted 日期的 ISO 形式，`posted` 保留页面原文。
- **fetch**：adapter 的 `paper` 命令打开规范摘要页。route 从页面的 canonical 链接（或 `citation_doi`）读出 abstract id，与请求的 id 不一致为 Runtime。支持 `metadata` 与 `abstract`：页面有摘要时条目深度为 `abstract`，否则为 `metadata`；请求 `abstract` 但页面没有摘要时在 attempt 内报 Quality。`posted`、`last_revised`、`date_written` 保留页面原文，`published` 为 Posted 日期的 ISO 形式。页面显示论文正在审核或已撤下时，adapter 返回 `no_results`，route 报 attempt 级 Parameter（`SSRN paper not available: ssrn:<id> (<站点提示>)`）。`full_text` 不支持（P3 再接入），该 route 记为 Skipped。
- **route 对比**：两条 route 的召回、重合度、字段覆盖、新鲜度与延迟见 [SSRN 两条 route 的检索对比](../../research/2026-09-26-ssrn-route-comparison.md)（2026-09-26）。
- **route 配合**：order 为 `[ssrn_crossref, ssrn_browser]` 时，`--depth abstract` 而 Crossref 缺摘要的请求在 Crossref attempt 内报 Quality，落到浏览器 route 补齐。
- **站点验证**：SSRN 由 Cloudflare 保护。Chrome 已通过站点验证时，临时站点会话可以直接打开结果页（2026-09-26 实测）；站点显示瞬时验证时 adapter 继续等待它自行通过；站点升级为需要人工勾选的验证时（同日在较多次访问后出现），adapter 等到自己的读取截止点后以 77 结束，attempt 为 Auth。route 永不点击或绕过验证，用户需在 Chrome 中打开 SSRN 手动通过（ADR 0020）。
- **doctor**：shallow 只在 `ssrn_browser` 出现在 `platforms.ssrn.order` 中时检查它，运行 `contract` 命令（不需要浏览器）并核对契约版本；未启用时报告为 `configured: false`，不影响 `ok`。检查失败时状态带 `message`，含安装提示。deep（`doctor --provider ssrn_browser`）同样只在 order 启用该 route 时运行一次真实的平台检索；未启用时以 config 失败退 3，不启动进程。
- **smoke**：C22（search）与 C23（fetch）按 order 门控，只在 `platforms.ssrn.order` 含 `ssrn_browser` 时运行，需要真实的 OpenCLI、Chrome 与已安装的 adapter。
- **adapter 分发**：forager 自有的 SSRN adapter 在 skill 目录 `skills/forager/opencli/ssrn/`（`search.js`、`paper.js`、`contract.js` 与共享的 `shared.js`）。JS 只读取页面事实并返回外壳，校验与归一化都在 Rust 中完成。安装方式是把该目录复制为 `~/.opencli/clis/ssrn/`；步骤与支持的 OpenCLI 版本见 skill 的平台 reference。
