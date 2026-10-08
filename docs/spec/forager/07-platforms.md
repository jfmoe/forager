# 7. 平台接入

本章是接入任何内置平台的权威契约。新增或修改平台时先读本章，并保持新平台 checklist 测试（`src/core/platform_checklist.rs`）通过。决策依据见 ADR 0019；arXiv 是参考实现，各节以它举例；SSRN 是第二个平台，Google Scholar 是第三个平台，小红书是第四个平台，见文末。

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

- **L0 公共参数**：search 为查询词与 `--limit`；fetch 为 ref 或 URL，以及 `--depth`。时间窗和排序不进公共层，因为各平台语义不同。查询词的语法由平台定义：arXiv 把查询词编码成字面词与短语，Google Scholar 原样发送、由谷歌学术解释自己的运算符；平台在本章写明自己的规则。
- **L1 平台选项**：类型化、全部可选、有默认值，定义为 types 门面中平台选项类型（例如 `PlatformSearchOptions`）下按平台划分的封闭变体。CLI 参数是每个平台的静态 clap 定义，再转换到 types 门面，types 不依赖 clap。clap 在飞行前校验枚举和取值范围；跨字段规则（例如日期区间顺序）由 types 中的校验函数负责；两者出错都退 2。
- **L2 平台操作**：凡改变结果种类或必需输入的，就是新操作，不是参数。只有一个 route 实现的操作写成该 route 适配器的 inherent 方法；出现第二个实现它的 route 时，提升为 trait，并在 platform catalog 中为该操作登记 route 集合。在此之前，provider factory 声明它唯一的 route 集合与支持检查，core 按与 search 相同的规划规则选 route（order ∩ 该集合 ∩ 已配置 route），配置错误的消息同样点名 order 或 keys。第一个 L2 操作是 Google Scholar 的 `cited-by`，见文末。
- **不提供原样透传。** 新增一个参数需要：一个选项字段、一个 clap flag、每条 route 各自的映射。这条约束针对结构化选项与上游 API 参数；L0 查询词按平台定义的语法发送，不算透传。
- **不支持的选项**：每条 route 提供一个只读请求（选项与页位置）、不联网的支持检查。检查在构造链之前完成：
  - 部分 route 不支持：被跳过的 route 记一条 disposition 为 Skipped、`error_kind` 为空的 attempt，消息写明选项与 route。
  - order 中没有 route 支持：飞行前退 2，不发网络请求，消息写明选项和已配置的 route。
  - route 绝不能静默忽略显式选项。

## 必须提供的操作与输出形状

- 每个平台都提供 **search** 与 **fetch**，命令为 `forager platform <id> search|fetch`。
- **search 结果页**：`{platform, provider, items, next_cursor}`；`provider` 是产出该页的 route；`--verbose` 时附 `provider_attempts`（含 Skipped attempt）。每个 item 含 `ref`、canonical `url`、`depth`、`title`、`authors`、`published`，以及平台自有字段（与公共字段同层）。
- **cursor**：不透明值，格式 `v1.<route>.<payload>`，payload 能完整恢复上一次请求的查询词、选项、limit 与下一页位置。带 cursor 的请求只在产出它的 route 上执行，不 fallback；该 route 必须仍在当前 order 中、支持该操作且已配置。cursor 与显式传入的查询词、L1 选项或 `--limit` 互斥（默认值不算冲突），通用 flag 可以同时使用。`next_cursor` 为 `null` 表示没有可供下一条命令续页的标识，本身不证明结果已穷尽：小红书 search 从不签发 cursor（见「小红书」）。
- 平台直连命令不写 Search Result Journal。

### fetch

- **输入**：ref 字符串或可识别的原始平台 URL（L0），以及 `--depth`。平台只接受它定义过含义的深度，由 route 的支持检查判定。打开实体需要 Access Token 的平台（小红书）从输入 URL 中拆出 token，放进请求的 `access` 字段，不带 token 时飞行前退 2。
- **元数据段**：fetch route 链（`platforms.<id>.order` ∩ fetch route 集合 ∩ 已配置 route）返回条目元数据；输出的 ref 带平台返回的实际版本。条目不存在是 attempt 级 Parameter；元数据段失败即命令失败，绝不跳过元数据直接取正文。
- **正文段**（仅 `full_text`）：答复的 route 在 `PlatformFetchOutcome.content_source` 中声明同一版本的正文来源，三者之一：一组按序读取的 URL（arXiv）、一个在同一 attempt 内校验过的本地文件（SSRN 浏览器下载的 PDF），或 route 在同一 attempt 内自己读到并核对过的 Markdown 正文（Native，小红书；ADR 0022）。route 可以先做受访问策略约束的探测来决定 URL 顺序，但不 import Web Fetch provider。Native 来源跳过 Web Fetch 链与薄正文门，core 直接用条目的 canonical URL、route id 与正文构造 `PlatformContent`；没有正文时 route 在同一 attempt 内报 Quality，不另记 attempt。core 的 `platform_fetch` 对 URL 与本地文件两种来源用同样的方式运行全局 Web Fetch 链（`capabilities.web_fetch.order` 及其凭据、薄正文门、4 MiB 截断诊断），不含站点知识：URL 来源首个成功者即正文，有后续 URL 时本段最多用剩余预算的一半，全部失败时沿用最后一条 Web Fetch 链的终态；本地文件来源只运行一次链，失败即终态。第 4 章的归因总函数只在每条链内部归约，不跨正文 URL 合并。不新增平台级正文顺序。**Web Fetch 预检**：provider 注册信息为每条 route 声明 fetch 全文是否由 route 自己读取（`native_full_text`）；`plan_fetch` 的计划据此回答是否可能需要 Web Fetch（`full_text` 且计划中有任一 route 不读原生正文）。只有需要时，没有已配置的 Web Fetch provider 才为飞行前退 3；其他深度与只含原生正文 route 的计划不需要 Web Fetch 配置。

### fetch 输出与正文交付

遵循 ADR 0015「默认结果只含下一步决策所需内容，已落盘材料按引用交付」：

- **`full_text`**：正文写入本地 Markdown 文件；stdout 返回 `platform`、`provider`（元数据 route）、条目公共字段与平台字段（`depth` 为 `full_text`），以及 `content_url`、`content_provider`（实际产出 Markdown 的 provider：Web Fetch provider，Native 来源时为 route id）、`content_path`（可直接读取的文件）与 `content_len`（正文字符数）。正文不出现在 stdout。`content_url` 是正文实际来自的 URL；正文由本地文件转换而来或是 Native 来源时，它是条目的 canonical URL（正文来自平台，不是某个可抓取的地址，不承诺可以匿名打开），永不指向有时效的签名下载地址。
- **原始文件**：正文来源是本地文件时，`--keep-pdf` 把它移入内容目录（文件名由 ref 与媒体类型派生，如 `ssrn-<id>.pdf`），输出增加 `pdf_path` 与 `pdf_bytes`；默认在交付成功后删除它；转换失败时一律保留，错误消息写明它的路径。跨文件系统的移动采用先复制再删除。
- **其他深度**：元数据与该深度的内容直接内联，不写文件。
- **`--format content`**：显式把正文（非全文深度时为该深度的内容）输出到 stdout，不写文件。
- **文件位置**：默认写入系统临时目录下按调用隔离的目录 `forager-platform/<pid>-<时间戳>`（与 research 默认证据目录同一规则），`--content-dir DIR` 可以覆盖；文件名由带版本的 ref 把 `:` 与 `/` 换成 `-` 后加 `.md` 派生。写入方式与 research 证据文件相同；写入失败为 Runtime（退 4），不回退为内联输出。不做跨调用缓存。
- `--verbose` 时 `provider_attempts` 依次包含 Skipped route、元数据 attempt、探测 attempt 与各 Web Fetch attempt。

## 退出码阶段矩阵

| 情况 | 阶段 | 结果 |
|---|---|---|
| ref 或 URL 无法识别、短链、cursor 冲突或无效、选项取值非法、所有已配置 route 都不支持所请求的选项 | 飞行前（参数） | 退 2，不发网络请求 |
| 平台 order 为空、操作可用 route 集合为空、order 含其他平台的 route、order 中的 route 都缺凭据（消息点名 `providers.<route>.keys`）；`full_text` fetch 的计划需要 Web Fetch 而没有已配置的 Web Fetch provider | 飞行前（配置） | 退 3，不发网络请求 |
| 平台返回参数错误（例如 arXiv Atom error entry）、fetch 的条目不存在 | 飞行后，attempt 级 Parameter | 退 4，带平台消息 |
| fetch 元数据成功，但正文来源的 Web Fetch 链全部失败；全文的下载校验不满足（下载未完成、文件不存在、不是 PDF、详情页 id 不一致） | 飞行后 | URL 来源沿用最后一条 Web Fetch 链终态，例如全部过薄为 Quality，退 5；下载校验不满足为 Quality，退 5 |
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
- 需要凭据的 HTTP route 声明 `credentials_required: true`：配置节为 `url`、`keys` 与 `timeout`（runtime 投影 `KeyedHttpRouteRuntimeConfig`），经 `execute_v2` 使用 Provider Credential Pool（ADR 0005），额度耗尽与限流时换 key。keys 为空的 route 视为未配置，不进入平台链。

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
| R5 | route 的 doctor probe 为 `DoctorProbe::PlatformSearch`（或它所服务的 capability 的 probe）；需要凭据的 route 也可用 `DoctorProbe::ServiceAccount`，向供应商的账户接口逐 key 查询 | checklist 测试 |
| R6 | search 与 fetch 各至少有一条 route 登记 smoke 用例，`SPECIFICATION_CASE_IDS` 与第 5 章矩阵同步 | checklist 测试；`tests/smoke.rs` 列表断言 |
| R7 | `tests/acceptance-manifest.json` 为每个操作的每条 route 登记 `(route, platform:<id>:<op>)` fixture 与测试引用；只有一条 route 的 L2 操作同样登记，如 `(serpapi, platform:scholar:cited_by)` | checklist 测试（含 `single_route_operations_have_transport_fixtures`）；`catalog` 单测 `provider_fixture_projection_matches_transport_manifest` |
| R8 | ref 解析与 canonical URL 推导覆盖该平台每个 kind，并在 checklist 的样例表中登记样例 ref | checklist 测试；types 单测 |

同一改动中还须更新：`GLOSSARY.md`（新术语）、第 2 章（命令与参数表）、第 3 章（配置键）、第 4 章（模块与依赖）、第 5 章（fixture 与 smoke 用例）、本章的平台示例，以及 skill 的平台词表、平台 reference 与 CLI reference。

## 参考实现：arXiv

- **route**：`arxiv_api`，官方 Query API（默认 `https://export.arxiv.org/api/query`）。不需要凭据；默认 timeout 30 秒；访问策略为每 3 秒 1 个请求、并发 1（arXiv 使用条款）。默认 order 只含这一条 route。
- **ref**：新式 ID（`2401.01234`）与旧式 ID（`hep-th/9901001`），都可以带版本号；旧式 ID 的学科子类不属于身份（arXiv 把 `math.GT/0309136` 解析为 `math/0309136`，Query API 只认后者），解析时去掉；字符串形式 `arxiv:<id>[v<n>]`；canonical URL 是 `https://arxiv.org/abs/<id>[v<n>]`；解析也接受 `arxiv.org`、`www.arxiv.org` 与 `export.arxiv.org` 的 abs、pdf（有无 `.pdf` 后缀）与 html 页面 URL，忽略 query、fragment 与末尾 `/`。裸 ID 与其他路径（例如 `list/`、`src/`、HTML 内的图片）不可识别。kind 为 `paper`。
- **search wire 编码**：查询词中每对双引号括起的部分是一个短语，其余部分按空白切分为词；短语内部按空白切分后以单个空格连接，空短语忽略。每个词或短语写成 `all:"<词或短语>"`，因此布尔运算符、字段前缀与括号都是字面文本。arXiv 拒绝带转义双引号的引号词（`all:"a\"b"` 返回 HTTP 400），字面双引号无法发送，所以查询词中的双引号只界定短语，未配对时飞行前退 2；作者与标题本身就是短语，其中的双引号只作分隔符。反斜杠会转义引号词的闭合引号（`all:"C:\"` 返回 HTTP 400），所以反斜杠只作分隔符；分类写成 `cat:<code>`，多个分类写成 `(cat:a OR cat:b)`；作者与标题写成 `au:"<短语>"`、`ti:"<短语>"`；日期区间写成 `submittedDate:[YYYYMMDD0000 TO YYYYMMDD2359]`，缺少的一端用 `199101010000` 或 `999912312359` 补齐；所有条件以 ` AND ` 连接。`sortBy` 为 `relevance`、`submittedDate` 或 `lastUpdatedDate`，`sortOrder` 固定为 `descending`；`--limit` 为 `max_results`，cursor 的页位置为 `start`。
- **解码**：Atom feed 解码为 depth 为 `abstract` 的条目，含完整摘要与元数据；`opensearch:totalResults` 决定是否有下一页，缺少它的成功响应不是 arXiv feed，为 Runtime。cursor 的页位置是 `start` 偏移量，无法解析时在飞行前退 2。id 包含 `arxiv.org/api/errors` 的 entry 是错误，映射为 Parameter 并带上 arXiv 消息；无法解码的 feed 为 Runtime；HTTP 失败沿用共享 status 映射。
- **fetch**：route 支持 `full_text`（默认）与 `abstract`。元数据段请求 Query API 的 `id_list=<id>[v<n>]&max_results=1`；空 feed（`totalResults` 为 0）表示 id 或版本不存在，为 Parameter（`arXiv item not found: arxiv:<ref>`）；返回的条目与请求的 id 或版本不符为 Runtime；429 为 RateLimited，匿名 route 无凭据可轮换，直接成为 route 终态。`full_text` 时 route 对 `<Query API 主机>/html/<id>v<n>` 发一次不重试的 HEAD（经同一访问策略）：404 表示没有 HTML，正文 URL 只有 `https://arxiv.org/pdf/<id>v<n>`；2xx 或其他 HTTP／网络结果（未知）时依次为 `https://arxiv.org/html/<id>v<n>` 与同一版本的 PDF。探测等不到限速窗口时按访问策略终止（预算不足为 Timeout，状态不可用为 Runtime，都退 4、不发送探测、不读正文），保留已完成的 attempts。不按 provider 返回的内容判断有无 HTML，因为 arXiv 的「无 HTML」说明页能通过薄正文门。export 镜像与 arxiv.org 的 HTML 页面返回相同 ETag 与 404 语义（2026-09-26 实测），因此探测跟随 `providers.arxiv_api.url` 的主机，交给 Web Fetch 的正文 URL 始终是 arxiv.org 官方地址。正文由第三方 provider 抓取，不经过本机 arXiv 限速。

## SSRN

数据源选择与实测证据见 [SSRN 检索接入方案](../../research/2026-09-26-ssrn-integration.md)（2026-09-26）。

- **route**：search 与 fetch 的 route 集合都是 `[ssrn_crossref, ssrn_browser]`；platform catalog 的默认 order 只含 `ssrn_crossref`，`ssrn_browser` 由用户手动加入 `platforms.ssrn.order` 后才生效。两条 route 共用同一种 ref 与输出形状。
- **`ssrn_crossref`**：匿名 Crossref REST API（默认 `https://api.crossref.org`），只检索 SSRN 的 DOI 前缀 `10.2139`。不需要凭据；默认 timeout 30 秒；访问策略为每秒 1 个请求、并发 1（2026-09-26 公共池响应头为每秒 5 个请求、并发 1）。
- **ref**：`ssrn:<id>`，id 是不带前导零的 ASCII 数字，kind 为 `paper`，不带版本。canonical URL 是 `https://papers.ssrn.com/sol3/papers.cfm?abstract_id=<id>`。不联网即可解析的输入：ref 本身（前缀不区分大小写）；`papers.ssrn.com/sol3/papers.cfm` 摘要页，取 `abstract_id`，忽略其他 query 参数与 fragment；`ssrn.com/abstract=<id>` 与 `www.ssrn.com/abstract=<id>`；DOI `10.2139/ssrn.<id>`（不区分大小写）及其 `doi.org`、`dx.doi.org` URL。主机名不区分大小写，http 与 https 都接受。SSRN 的 `Delivery.cfm` PDF 链接（文件名里的数字不是论文身份）、其他路径、相似域名与短链都在飞行前退 2。
- **search wire 编码**：`GET <url>/prefixes/10.2139/works`。`scope=all/title/bibliographic` 分别映射 `query/query.title/query.bibliographic`；查询词必填且原样发送，不承诺精确短语或每个词都出现。作者与机构分别映射 `query.author` 和 `query.affiliation`。`rows` 为 limit（1–100），`offset` 为页位置，`select=DOI,title,author,abstract,published,type,created,resource` 保持不变。
- **过滤条件**：统一在 `filter` 中以逗号连接（不同字段为 AND）。发表、首次登记与元数据更新区间分别映射 `from/until-pub-date`、`from/until-created-date`、`from/until-update-date`；日期含边界，可单端。摘要条件映射 `has-abstract:true`；文献类型、ORCID、资助机构分别映射 `type`、`orcid`、`funder`。类型是当前注册类型的封闭枚举，默认无过滤；ORCID 只接受带正确校验位的裸 ID，funder 只接受 OFR DOI `10.13039/<数字>`。机构与标识符覆盖不足时可能返回空结果，不追加详情读取。
- **排序**：`relevance/published/created/updated/citations` 分别映射 `score/published/created/updated/is-referenced-by-count`，`order=asc/desc`，默认 `score/desc`。更新表示 Crossref 存入或重新存入元数据，不是 SSRN 修订日期；引用量是 Crossref 登记的被引次数。过滤和排序全部由上游执行。
- **支持矩阵与兼容性**：两条 route 支持默认条件、标题范围与作者文本，但匹配规则各自遵循上游。Crossref 额外支持 bibliographic、机构、三类日期区间、摘要、类型、ORCID、funder 与 published/created/updated/citations 排序；browser 额外支持 full-text、显式 mode/date 与 posted/downloads/title 排序。显式 mode（包括 fuzzy）或 date（包括 all-time）限定原站语义，Crossref 不能执行。browser relevance 只支持 desc，其他原站排序支持 asc/desc。不能满足完整条件的 route 记为 Skipped；全链不支持时飞行前退 2，不自动启用 browser。cursor 保存完整条件，旧 cursor 缺少 mode/date 时保持 route 默认匹配与全部时间。
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
  - 外壳：stdout 是一个 JSON 对象 `{contract, status, data}`。`contract` 与注册信息的契约版本（`forager-ssrn/3`）不一致为 Runtime，消息附安装提示（把 skill 的 `opencli/<site>` 目录复制为 `~/.opencli/clis/<site>`）；`status` 为 `ok` 或 `no_results`，只有 adapter 核实了站点自己的无结果提示时才是 `no_results`。
  - 退出码映射：

    | OpenCLI 结果 | 映射 |
    |---|---|
    | 0，且外壳有效 | 传输成功；业务结果由 route 判定（例如小红书 route 的页面事实分类） |
    | 66（EMPTY_RESULT） | Runtime（永远不算合法空结果） |
    | 69，且 stderr 的 `code` 为 `ADAPTER_LOAD` | Runtime，附安装提示 |
    | 其他 69 | Network |
    | 75 | Timeout |
    | 77（含 LOGIN_WALL；adapter 报出的站点验证未通过与访问被拦截也用 77） | Auth |
    | stderr 报告未知命令（未安装 adapter 时 OpenCLI 1.8.6 以 2 退出） | Runtime，附安装提示 |
    | 找不到可执行文件 | Runtime，提示安装 OpenCLI 或设置 `command` |
    | 其他退出码、输出无法解码 | Runtime |

- **search**：adapter 显式构造完整查询：`term`、`text_fields`、`search_mode`、`authors`、`date`、`sort_by` 和 `page`。scope all/title/full-text 对应 `title-abstract-keywords` / `title` / `title-abstract-keywords-fulltext`；mode 省略为 fuzzy；author 省略为空串；date 省略为 all_time，其他预设使用 CLI 名的下划线形式；relevance 的 sort_by 为空串，posted/downloads/title 对应 `approval_date` / `downloads` / `title` 加 `-asc` 或 `-desc`。Boolean 只承诺原站 Help 所列 AND、OR、NOT、括号，不承诺额外 DSL。作者文本直接提交 Author(s)，不是作者身份选择；真实样例中姓氏与全名的召回可能不同。
- **实际状态与刷新**：`forager-ssrn/3` 的 search data 必须含 `search_state`（scope、mode、author、date 显示文本、sort 显示文本或 null、request_url）。adapter 用仅供导航的一次性 `_forager_search` 参数避免 OpenCLI 同 URL 快速复用旧页面；原站在搜索成功后重写 URL 并移除它。adapter 等待这一事实、当前文档 Performance Resource Timing 中已完成的原站搜索请求和结果区域，再读取控件与结果。Rust 同时校验结果页 URL、查询框、已完成请求中的完整条件及页码、radio/作者/日期/排序的实际值。无结果页面不显示排序控件时允许 sort=null，但已完成请求仍须匹配排序。状态缺失、不符或页面不识别均为 Runtime，不能返回假空结果。
- **分页与结果**：SSRN 每个原生页 50 条；页位置为绝对 offset，页码 = offset / 50 + 1，页内位置 = offset mod 50。一页最多 limit 条且不跨原生页；cursor 恢复全部条件并固定 route，不 fallback。页码、范围 `Displaying results <first> to <last> of <total>` 与卡片数须一致。结果深度为 snippet，无片段为 metadata；full-text 搜索只扩大原站匹配范围，不打开详情或下载 PDF。
- **原站能力限制**：2026-09-28 的页面控件、原站脚本与实际请求没有独立机构、文献类型、有摘要或任意起止日期条件，因此这些条件在 browser 支持检查中拒绝，不做本地过滤。证据与仅搜索验收见 [browser 高级搜索验收](../../research/2026-09-28-ssrn-browser-advanced-search.md)。
- **fetch**：adapter 的 `paper` 命令打开规范摘要页。route 从页面的 canonical 链接（或 `citation_doi`）读出 abstract id，与请求的 id 不一致为 Runtime。支持 `metadata` 与 `abstract`：页面有摘要时条目深度为 `abstract`，否则为 `metadata`；请求 `abstract` 但页面没有摘要时在 attempt 内报 Quality。`posted`、`last_revised`、`date_written` 保留页面原文，`published` 为 Posted 日期的 ISO 形式。页面显示论文正在审核或已撤下时，adapter 返回 `no_results`，route 报 attempt 级 Parameter（`SSRN paper not available: ssrn:<id> (<站点提示>)`）。
- **full_text**：route 以 `paper --download true` 在同一次 OpenCLI 命令内读取详情页并点击页面自己的下载链接下载 PDF，adapter 只回报下载状态与本地文件名（签名下载 URL 永不离开浏览器，因此不出现在 stdout、attempts 或日志中）。在同一个 attempt 内，route 依次要求：下载状态为完成且带文件名；文件存在；文件以 `%PDF-` 开头；详情页 id 一致（此处不一致报 Quality，而不是 Runtime）。任何一项不满足为 Quality。全部满足后，route 把这个文件（媒体类型 PDF）作为全文来源返回；不扫描下载目录，也不推导文件名，同一浏览器中同时有其他下载是已接受的限制。下载和转换共用命令的截止时间，PDF 只适用薄正文门的长度线。
- **route 对比**：两条 route 的召回、重合度、字段覆盖、新鲜度与延迟见 [SSRN 两条 route 的检索对比](../../research/2026-09-26-ssrn-route-comparison.md)（2026-09-26）。
- **route 配合**：order 为 `[ssrn_crossref, ssrn_browser]` 时，`--depth abstract` 而 Crossref 缺摘要的请求在 Crossref attempt 内报 Quality，落到浏览器 route 补齐。
- **站点验证**：SSRN 由 Cloudflare 保护。Chrome 已通过站点验证时，临时站点会话可以直接打开结果页（2026-09-26 实测）；站点显示瞬时验证时 adapter 继续等待它自行通过；站点升级为需要人工勾选的验证时（同日在较多次访问后出现），adapter 等到自己的读取截止点后以 77 结束，attempt 为 Auth。route 永不点击或绕过验证，用户需在 Chrome 中打开 SSRN 手动通过（ADR 0020）。
- **doctor**：shallow 只在 `ssrn_browser` 出现在 `platforms.ssrn.order` 中时检查它，运行 `contract` 命令（不需要浏览器）并核对契约版本；未启用时报告为 `configured: false`，不影响 `ok`。检查失败时状态带 `message`，含安装提示。deep（`doctor --provider ssrn_browser`）同样只在 order 启用该 route 时运行一次真实的平台检索；未启用时以 config 失败退 3，不启动进程。
- **smoke**：C22（search）与 C23（fetch）按 order 门控，只在 `platforms.ssrn.order` 含 `ssrn_browser` 时运行，需要真实的 OpenCLI、Chrome 与已安装的 adapter。
- **adapter 分发**：forager 自有的 SSRN adapter 在 skill 目录 `skills/forager/opencli/ssrn/`（`search.js`、`search-state.js`、`paper.js`、`contract.js` 与共享的 `shared.js`）。JS 建立搜索条件、等待对应结果并读取页面事实；Rust 校验协议和实际条件，再归一化结果。安装方式是把该目录复制为 `~/.opencli/clis/ssrn/`；步骤与支持的 OpenCLI 版本见 skill 的平台 reference。

## Google Scholar

设计与实测证据见 [Google Scholar 平台与 SerpApi provider 设计](../../design/2026-10-07-scholar-serpapi-route.md)（2026-10-07）；只经第三方 SERP API 接入的决定见 ADR 0021。

- **route**：search 与 fetch 的 route 集合与默认 order 都是 `[serpapi]`；cited-by 只由 `serpapi` 实现。谷歌学术没有公共 API，`/scholar` 被 robots.txt 禁止，forager 不自建抓取。`serpapi` 按供应商命名：同一账号的额度与吞吐不分引擎，以后接入 SerpApi 的其他引擎时复用同一 key 池与凭据游标。
- **`serpapi`**：SerpApi 搜索端点（默认 `https://serpapi.com/search.json`），HTTP route，需要凭据：`providers.serpapi.keys` 为空时视为未配置，平台命令飞行前退 3，消息点名该键。默认 timeout 30 秒，不设访问策略，每小时吞吐由 429 加 key 轮换处理。key 只能放在查询参数 `api_key` 中，所以 reqwest 错误先去掉 URL 再格式化，route 自己产生的消息先按凭据值脱敏再进入 attempt；输出不投影 `search_metadata`、`search_parameters` 与分页链接。
- **ref**：`scholar:<cluster_id>`，cluster ID 是不带前导零、不溢出 u64 的十进制数，kind 为 `paper`，没有版本；前缀不区分大小写。canonical URL 是 `https://scholar.google.com/scholar?cluster=<cluster_id>`。也接受 `scholar.google.com/scholar` URL（http 或 https，可带末尾 `/`），其查询参数须恰有一个 `cluster` 且没有 `cites`，忽略其他参数与 fragment。被引页（带 `cites`，包括与 `cluster` 同时出现）、`cluster` 重复或溢出、作者主页等其他 URL 都在飞行前退 2。
- **查询语法**：查询词原样作为 `q` 发送，谷歌学术的运算符（`"短语"`、`OR`、`-词`、`author:`、`source:`）照常生效；实测 `author:` 作用于整个查询。去空白后为空的查询词飞行前退 2。
- **search wire 编码**：`GET <url>?engine=google_scholar&hl=en&q=<查询词>&num=<limit>&api_key=<key>`。`--limit` 为 1–20，默认 20：每页无论多少条都计 1 次额度，取满页最省额度。`hl=en` 固定，保证 `publication_info.summary` 的格式稳定。`--year-from`、`--year-to` 映射为 `as_ylo`、`as_yhi`，`--review-only` 映射为 `as_rr=1`，未指定时不发送。第一页不带 `start`，后续页带绝对偏移 `start`。
- **选项值域**：types 的请求校验检查查询词非空、limit 1–20、每个年份 1000–9999、起始年份不晚于结束年份；恢复 cursor 时绕过 clap，所以恢复后运行同一套校验，越界时飞行前退 2。不提供按日期排序（实测 `scisbd=2` 会忽略年份区间）与 `--author`（查询词里的 `author:` 已作用于整个查询）。
- **分页**：页位置是绝对偏移 `start`，cursor 为 `v1.serpapi.<payload>`，payload 是完整的平台检索请求（查询词、选项、limit 与下一页的 `start`）。谷歌学术最多提供 1000 条结果，越界的页返回空集但仍计费，所以只有响应带 `serpapi_pagination.next`（只看是否存在，不使用其链接），且下一页的页尾 `start + limit` 不超过 1000 时才签发 cursor；`--limit` 不整除 1000 时提前结束。下一页的 `start` 为本页 `start + limit`，与身份过滤后剩下的条目数无关。支持检查拒绝页尾越过 1000 或无法解析的页位置，飞行前退 2；cursor 指定的 route 已移出 order 或已无 key 时沿用 pinned route 规则退 2。
- **HTTP 200 成功协议**：HTTP 错误沿用共享 status 映射；429 正文含 `run out of searches`（月额度用尽）为 QuotaExhausted，其他 429 为 RateLimited，两者都换 key。HTTP 200 的响应由 route 在单次发送内判定：

  | HTTP 200 响应 | 结果 |
  |---|---|
  | `search_metadata.status` 为 `Success`，`organic_results` 是非空数组 | 成功 |
  | `Success`，缺少 `organic_results`，且 `search_information.organic_results_state` 恰为 `Fully empty` | 合法空集 |
  | `status` 为 `Error` | Network，按共享策略重试，消息取 SerpApi 的 `error` |
  | `status` 缺失或未知、`organic_results` 为空数组或不是数组、其他状态下缺少结果、JSON 形状不对 | Runtime |

- **身份**：ref 优先取 `inline_links.versions.cluster_id`，其次 `inline_links.cited_by.cites_id`；两者都存在但不一致时跳过该条。两者都没有时解码 `result_id`：无填充 base64url、恰为 9 字节、末字节 `0x09`，取前 8 字节小端序整数。这条规律没有上游文档保证，但实测与显式 ID 全部一致，回查 cluster 也取回原论文；新论文常常没有显式 ID，所以它是常用来源。显式 ID 格式不合法或 `result_id` 不满足上述规律时跳过该条。被跳过的条目在 stderr 汇总为一条诊断（标题与原因）；上游有非空结果但全部被跳过时 attempt 为 Runtime，不当作合法空集。
- **解码**：`depth` 有非空 snippet 时为 `snippet`，否则为 `metadata`（`snippet: null`）；Scholar 的片段永不当作摘要。`authors` 取 `publication_info.authors[].name`（丢弃空名），没有作者数组时取 `summary` 第一个 ` - ` 之前的部分，按 `, ` 切分并去掉 `…`；作者名可能是缩写或截断的。`published` 只取 `summary` 第二段（来源与年份）末尾的 `, YYYY`，或整段恰为 `YYYY` 时的年份，取不到为 `null`，因此 arXiv 编号之类的数字不会被当作年份。平台字段为 `snippet`、`link`（可空）、`source`（`summary` 原文）、`cited_by` 与 `version_count`（整数或 `null`）、`resources`、`result_type`（上游 `type` 原值，可空）。`resources` 形如 `[{title, file_format, url}]`，只保留 HTTP(S) 链接，按 URL 稳定去重，search 与 fetch 共用这条规则。
- **fetch**：只支持 `metadata`（默认），其他深度由支持检查拒绝，飞行前退 2。请求 `engine=google_scholar&hl=en&cluster=<id>&num=20`，只取一页。`ref` 与 `url` 来自请求的 cluster；`title`、`authors`、`published` 取第一个版本；平台字段 `versions` 按谷歌学术的顺序列出本页每个版本的 `{title, link, source, resources}`。第一个版本可能是第三方副本，簇内也可能混入别的论文，所以不把它当作规范出版版本。响应带下一页信号时，stderr 诊断说明版本没有列全并指向 canonical URL；search 的 `version_count` 与 cluster 的条数不是同一计数。`Fully empty` 表示 cluster 不存在，为 attempt 级 Parameter（`Google Scholar has no cluster scholar:<id>`），退 4，且消耗 1 次额度。正文不提供。
- **cited-by（L2 平台操作）**：`platform scholar cited-by REF|URL` 列出谷歌学术统计为引用该论文的文献。它改变了必需输入（被引论文而不是查询词）与结果含义，所以是新操作，不是 search 的参数；目前只有 `serpapi` 实现，写成 `Serpapi::cited_by` inherent 方法，不新增 trait，也不在 platform catalog 登记 route 集合。被引对象沿用 fetch 的 ref 与 URL 解析。请求为 `engine=google_scholar&hl=en&cites=<cluster_id>&num=<limit>`，`--query` 映射为 `q`（在施引文献内筛选，运算符同 search），年份映射同 search，`--sort date` 映射为 `scisbd=2`（按收录时间倒序，`snippet` 带上游的收录时间前缀，原样保留）。实测 `scisbd=2` 会忽略年份区间，所以 `--sort date` 与年份参数同时出现时飞行前退 2；不提供 `--review-only`（`as_rr` 与 `cites` 合用没有实测过）。实测 `cites` 的结果结构与 search 相同（`total_results` 等于 search 条目的 `cited_by`，结果不含被引论文自身），所以输出形状、条目解码、成功协议与分页规则都与 search 共用。上游对无人引用的论文与不存在的 cluster 都返回 `Fully empty`，无法区分，所以 cited-by 的空集是合法结果（`items: []`，退 0），不同于 fetch 的 Parameter。cited-by 的 payload 含被引 cluster、可选查询词、limit、年份、排序与页位置，与 search 的 payload 互不接受对方的字段：把 search 的 cursor 交给 cited-by 或反过来，飞行前退 2，消息写明 cursor 属于哪个操作。响应里的 `citations_per_year` 与 `profiles` 不投影。
- **额度**：成功的搜索计 1 次（含合法空集与不存在的 cluster）；4xx、429 与 1 小时内参数完全相同的缓存命中不计。不持久化配额状态：额度用尽的 key 轮到时先收到一次不计费的 429 再换 key。
- **doctor**：shallow 对端点发一次不带 key 的 GET，任何 HTTP 响应都算可达，不计费；deep（`doctor --provider serpapi`）对每个 key 调用一次 Account API（`/account.json`，与 `providers.serpapi.url` 同源），不计费，报告每个 key 的剩余额度与本小时吞吐（输出字段见第 2 章契约③）；它不运行检索，Scholar 检索的请求与解码由 smoke C24–C26 证明；没有 key 时以 config 失败退 3，不发请求。
- **smoke**：C24（search）、C25（fetch，canary `scholar:18208131694456651388`）与 C26（cited-by，同一 canary，`--limit 3`）只在 `providers.serpapi.keys` 非空且 `platforms.scholar.order` 含 `serpapi` 时运行，各计 1 次额度。

## 小红书

第四个平台，只有一条 process route `xiaohongshu_browser`，经本机 OpenCLI 驱动用户自己已登录的 Chrome，只读取页面自身发出的接口响应与服务端渲染状态。设计依据与实测证据见 [小红书 OpenCLI route 设计](../../design/2026-10-08-xiaohongshu-opencli-route.md)。第一期提供 search 与 fetch；comments 在第二期加入。

- **身份**：kind 只有 `note`。ref 为 `xiaohongshu:<note_id>`，`note_id` 是 24 位十六进制，统一为小写，没有版本。canonical URL 为 `https://www.xiaohongshu.com/explore/<note_id>`，不含查询参数，满足往返性质。可识别的 URL：主机为 `www.xiaohongshu.com` 或 `xiaohongshu.com`（http 或 https），路径为 `/explore/<id>`、`/discovery/item/<id>`、`/search_result/<id>` 或 `/user/profile/<user_id>/<id>`，忽略 fragment 与末尾 `/`。`xhslink.com` 短链需要联网展开、`rednote.com` 适用另一份用户协议，两者都在飞行前退 2，消息分别说明替代做法。
- **Access Token**：笔记 URL 中的 `xsec_token` 是打开该笔记所需的访问参数，不是身份，不进入 ref、canonical URL、cursor 与 journal。解析纯函数把 URL 拆成 `(ref, Option<AccessToken>)`；token 须为 URL 解码后 1–128 个 `A-Za-z0-9_=-` 字符，重复、含 `+` 或转义非法时飞行前退 2。解析错误消息不回显原始输入。`AccessToken` 的 Debug 输出打码，不可序列化。`access_url` 由 ref 与 token 构造：`https://www.xiaohongshu.com/explore/<id>?xsec_token=<token>&xsec_source=pc_search`，不回显用户输入的链接。
- **route 与启用**：传输为 `OpenCli { site: "forager-xhs", contract: "forager-xhs/1" }`；配置为 `providers.xiaohongshu_browser.command`（默认 `opencli`）与 `.timeout`（默认 120 秒）。访问策略为每 10 秒 1 次 OpenCLI 命令、并发 1，跨进程生效；10 秒是实测未触发风控的节奏，不是站点给出的安全阈值。route 不重试（对风控中的账号重复访问会加重风控）。catalog 的默认 order 为空：order 为空时平台命令飞行前退 3、不启动进程，消息给出启用步骤（安装读取模块、在 OpenCLI 驱动的 Chrome 中登录、把 route 加入 `platforms.xiaohongshu.order`）。
- **读取模块**：`skills/forager/opencli/forager-xhs/`（`contract.js`、`search.js`、`note.js` 与共享的 `shared.js`），安装方式是把该目录复制为 `~/.opencli/clis/forager-xhs/`。它只做导航、悬停、点击与滚动；翻页时滚到底后调用两次 `page.screenshot()`，在不可见的后台窗口里强制渲染一帧，页面才会加载下一页。它只读网络抓包（`page.startNetworkCapture` / `page.readNetworkCapture`）与页面状态，不使用 `fetchJson`、store、组件方法或 `installInterceptor`。
- **抓包完成条件**：`readNetworkCapture()` 是消费式读取，读走还没有响应体的条目会让该页永久丢失。因此读取模块以页面 Performance Resource Timing 中搜索请求的完成记录（`responseEnd > 0`，由 `PerformanceObserver` 计数）为信号，再留 1 秒让扩展存下响应体，然后才读抓包。计数属于文档：读取模块每次读取页面状态时都确认当前文档已在计数，没有就装上（`buffered: true`），因此首次加载失败、Chrome 显示自己的错误页并重新加载成新文档时，新文档里的响应照样被计数。第一页的响应通常在导航返回、计数开始之前就已完成，而页面在 load 事件时清空 Resource Timing 缓冲，所以第一页另以页面出现笔记卡片为完成信号（抓包在导航之前已开启，响应已在其中）。在完成信号之后读到的搜索条目仍没有响应体时记为 `body_missing`（命令结束或截止时读到的在途请求不计入）。只保留 GET 与 POST，OPTIONS 预检被过滤。
- **页面事实**：读取模块回报 `{url, title, guest, error_code, notice, blocked_status, load_error}`：`url` 去掉 `xsec_token`；`guest` 来自 `user/me` 响应的 `guest`，或页面没有笔记卡片时 `__INITIAL_STATE__.user.loggedIn` 为 false；`error_code` 取自 `/404` 或 `website-login/error` 跳转的参数；`notice` 是页面没有笔记卡片时的安全限制、访问链接异常、登录后查看、请求太频繁等提示原文；`blocked_status` 是任一小红书接口返回的 461；`load_error` 是 Chrome 因导航失败显示自己的错误页时的错误码（如 `ERR_CONNECTION_CLOSED`），此时 `url` 取导航条目记录的原 URL。遇到前五项终态事实即停止等待；`load_error` 不是终态，Chrome 会自行重新加载错误页，读取模块继续等待。截止点（`--timeout` 之前 3 秒）前没有读到预期响应时同样返回页面事实，带 `timed_out: true`，不以 75 退出。外壳 `status` 恒为 `ok`。
- **分类**：在 route 的 `execute_anonymous` 闭包内、成功 attempt 记录之前，按顺序：未登录（`guest` 或登录提示）为 Auth；461 为 Auth；300031、300017 或安全限制、访问链接异常提示为 attempt 级 Parameter（以上三条 search 与 fetch 共用）；search 接着：`body_missing` 为 Runtime；之后若筛选点击数与请求不符或响应页数不足：`timed_out` 且 `load_error` 非空为 Network（消息给出错误码，fetch 同样适用），`timed_out` 且仍在搜索结果页为 Timeout，停在其他页面为 Runtime（消息带页面标题与去掉 token 的 URL），否则为 Runtime（筛选点击失败时附读取模块报告的原因）；页数足够时进入核对与解码。
- **search**：读取模块打开 `/search_result?keyword=<查询词>&source=web_explore_feed`，只在选项取非默认值时按 sort、note-type、publish-time 的顺序打开筛选面板并点击对应文本（排序依据：综合 / 最新 / 最多点赞 / 最多评论 / 最多收藏；笔记类型：不限 / 图文 / 视频；发布时间：不限 / 一天内 / 一周内 / 半年内），每次点击后等到新的第 1 页响应；之后读取 ⌈limit / 20⌉ 页，`has_more` 为 false 时提前结束。每个响应带上发出前的点击次数，回报 `request: {keyword, page, search_id, filters}` 与 `body`。route 只采用最后一次点击之后的响应，逐页核对：`keyword` 等于去空白后的查询词；`filters` 中 `sort_type`、`filter_note_type`、`filter_note_time` 的取值等于请求（没有 `filters` 时视为全部默认；`general` 与 `不限` 为默认值），`filter_note_range` 与 `filter_pos_distance` 若出现须为 `不限`；`page` 从 1 依次递增；`search_id` 不变；每页都有 `data`。任何不符为 Runtime。请求体顶层的 `sort` 与 `note_type` 不反映筛选，不参与核对。
- **解码**：只解码 `model_type` 为 `note` 的条目（跳过 `hot_query` 等）。`id` 不是 24 位十六进制或 `xsec_token` 不合法的笔记被跳过，并在 stderr 汇总一条诊断；上游有笔记而全部被跳过为 Runtime。跨页按笔记 ID 去重后截到 limit。条目深度恒为 `metadata`；`title` 取 `display_title`（可为空串）；`authors` 为 `[user.nickname]`；平台字段 `note_type`（`normal` 输出为 `image`）、`author_id`、`likes` / `collects` / `comments` / `shares`（`interact_info` 的 `liked_count`、`collected_count`、`comment_count`、`shared_count` 原文）、`published_text`（`corner_tag_info` 中 `publish_time` 的原文）与 `access_url`。`published` 为日历日期，参照请求时的系统时钟与系统本地时区，只精确到日：`YYYY-MM-DD` 原样；`MM-DD` 补上参照日期的年份，得到的日期晚于参照日期时改用上一年；`刚刚` 为参照日期；`N分钟前`、`N小时前` 为参照时刻减去该时长后的日期；`N天前` 为参照日期减 N 天；`昨天` 与 `昨天 HH:MM` 为参照日期的前一天；其他形式（如 `前天`、`N周前`）为 `null`。原文总在 `published_text`。
- **合法空集与未列全**：第一个采用的响应没有笔记且 `has_more` 为 false 才是空成功；没有笔记而 `has_more` 为 true 为 Runtime。search 不签发 cursor，`next_cursor` 恒为 `null`，CLI 没有 `--cursor`，支持检查拒绝任何非首页的页位置：小红书每次访问的排序都不同，续页既不能复现也无法去重。最后一页 `has_more` 仍为 true，或去重后的条目截到 limit 时有余项，stderr 输出"未列全"诊断，提示加大 `--limit`（上限 100）。
- **fetch**：输入必须带 Access Token（search 条目的 `access_url`，或从浏览器复制的带 `xsec_token` 的笔记链接）；只给 ref 或不带 token 的链接时飞行前退 2，消息提示改用 `access_url`，不启动进程。支持 `metadata` 与 `full_text`（默认），其他深度由支持检查拒绝、飞行前退 2。读取模块的 `note` 命令（`--id`、`--xsec-token`）打开 `/explore/<id>?xsec_token=<token>&xsec_source=pc_search`，回报 `{page, note, timed_out}`：`note` 取自 `__INITIAL_STATE__.note.noteDetailMap[<id>].note`，只保留 route 解码的字段（视频只保留时长与各档尺寸，带签名的 `masterUrl` 与页面自带的 token 不离开页面）；页面渲染出笔记时清空 `notice`，正文或评论里的字样不算站点提示。route 先按「分类」处理页面事实，300031/300017 的消息为 `Xiaohongshu note unavailable: xiaohongshu:<id> (<code>: <notice>); the access token may be stale, the note restricted or removed, or the account rate-limited`；没有笔记时，`timed_out` 且 `load_error` 非空为 Network，`timed_out` 且仍停在该笔记页为 Timeout，否则为 Runtime（消息带页面标题与去掉 token 的 URL）；`noteId` 与请求不符为 Runtime。命令内不重试。
- **fetch 输出**：`ref` 与 `url` 来自请求；`title`（可为空串）；`authors` 为 `[user.nickname]`；`published` 由 `time`（毫秒）转成北京时间 ISO 8601 时间戳。平台字段：`updated`（`lastUpdateTime`，同一格式）、`note_type`（`normal` 输出为 `image`）、`author_id`、`likes` / `collects` / `comments` / `shares`（`interactInfo` 计数原文）、`tags`（`tagList[].name`）、`images`（`[{url, width, height}]`，`urlDefault` 只保留 HTTP(S)）、`video`（`{duration_seconds, width, height}`，取 `video.capa.duration` 与第一个非空视频流档位的尺寸；图文笔记为 `null`；永不输出视频地址）、`ip_location`（可空）与 `access_url`。
- **Native 正文**：`full_text` 时 route 在同一 attempt 内构造 Markdown：`# <title>`（标题为空时省略），空行，`desc` 原文（话题写法 `#话题名[话题]#` 原样保留），空行，`标签：` 加逗号分隔的标签名，然后每张图一行 `![](<图片 URL>)`；视频笔记再加一行 `（视频笔记，时长 <秒> 秒，视频文件未下载）`。标题、正文与图片全为空时为 Quality（退 5）。正文作为 Native 来源交付（见「fetch」），`content_provider` 为 `xiaohongshu_browser`，不需要配置任何 Web Fetch provider；文件名为 `xiaohongshu-<id>.md`。没有 `--keep-pdf`。
- **token 脱敏**：route 在记录 attempt 之前，按本次请求的 token 值把来自 OpenCLI stderr（例如回显 `--xsec-token` 参数）、外壳解码错误与页面事实的消息中的 token 换成 `********`。token 只出现在 OpenCLI 的进程参数与成功输出的 `access_url` 中（ADR 0022）。
- **doctor**：shallow 只在 order 启用该 route 时运行 `contract` 并核对契约版本，否则报告 `configured: false`；deep（`doctor --provider xiaohongshu_browser`）沿用 `PlatformSearch` 探针执行一次真实检索（同时证明登录态），未启用时以 config 失败退 3。
- **smoke**：只在 `platforms.xiaohongshu.order` 含该 route 时运行，每个用例只执行一次、不重试（第 5 章）。C27 为 `platform xiaohongshu search 咖啡 --limit 25 --sort latest --publish-time week`；C28 在内存中接收 C27 第一条的 `access_url`，以 `--depth full_text` 运行 fetch，通过时不要求 `title` 非空。任一小红书用例以 Auth 或 attempt 级 Parameter 失败，本轮其余小红书用例不启动，记为失败（`attempts: 0`，消息说明原因）；C27 没有返回 `access_url` 时 C28 同样不启动。
- **待交付**：comments、smoke C29 与对应的规格和 skill 更新，见设计文档的分期。
