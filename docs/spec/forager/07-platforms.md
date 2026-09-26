# 7. 平台接入

本章是接入任何内置平台的权威契约。新增或修改平台时先读本章，并保持新平台 checklist 测试（`src/core/platform_checklist.rs`）通过。决策依据见 ADR 0019；arXiv 是参考实现，各节以它举例。

## 概念与规则

- **Platform**：内置的外部内容源，拥有自己的身份空间。它与 Capability Seam 并列，不是 provider，也不是 Vertical Search 的垂直域。只支持随版本发布的内置平台；配置不能定义平台或通用 MCP/CLI route。
- **Platform Route**：接入某个平台的一条路线。route 就是 provider，沿用 provider 身份、凭据池（需要凭据时）、provider 配置段、doctor 探针与 smoke 登记。route id 绝不使用裸平台名（用 `arxiv_api`，不用 `arxiv`）。一个 provider 可以服务多个平台和 seam。
- **链语义**：同平台的 route 按 `platforms.<id>.order` 组成 fallback 链，由共享链执行器运行，沿用 LegitimateEmpty 语义：至少一条 route 返回合法空结果、之后没有 route 被接受时，结果为空成功。结果永不跨平台 fallback。
- **Platform Ref**：平台实体的类型化身份，由平台、平台自有的 kind 和 id 组成。kind 只按「身份空间不同」或「fetch 结果形状不同」划分，不按对话角色划分。字符串形式为 `<platform>:<id>`，例如 `arxiv:2401.01234v2`。
  - ref 解析与 canonical URL 推导是 types 门面中的零 IO 纯函数。每个 kind 都满足往返性质：解析 ref 的 canonical URL，得到同一个 ref。
  - 版本号可选。不带版本的 ref 推导出不带版本的 URL；纯函数不猜测版本，实际版本只在平台返回后确定。
  - 需要联网解析的短链在飞行前退 2。
- **Content Depth**：`snippet`、`abstract`、`full_text` 或 `thread`。每个平台为它支持的每种深度定义含义，每个结果条目都带 `depth`。摘要深度绝不当作全文。

## 参数分层

- **L0 公共参数**：search 为查询词与 `--limit`；fetch 为 ref 或 URL，以及 `--depth`。时间窗和排序不进公共层，因为各平台语义不同。
- **L1 平台选项**：类型化、全部可选、有默认值，定义为 types 门面中平台选项类型（例如 `PlatformSearchOptions`）下按平台划分的封闭变体。CLI 参数是每个平台的静态 clap 定义，再转换到 types 门面，types 不依赖 clap。clap 在飞行前校验枚举和取值范围；跨字段规则（例如日期区间顺序）由 types 中的校验函数负责；两者出错都退 2。
- **L2 平台操作**：凡改变结果种类或必需输入的，就是新操作，不是参数。只有一个 route 实现的操作写成该 route 适配器的 inherent 方法；出现第二个实现它的 route 时，提升为 trait 并在 platform catalog 的 `trait_operations` 中登记其 route 集合。
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
- **正文段**（仅 `full_text`）：答复的 route 在 `PlatformFetchOutcome.content_urls` 中按顺序声明同一版本的正文 URL；route 可以先做受访问策略约束的探测来决定顺序，但不 import Web Fetch provider。core 的 `platform_fetch` 对每个 URL 运行全局 Web Fetch 链（`capabilities.web_fetch.order` 及其凭据、薄正文门、4 MiB 截断诊断），首个成功者即正文；有后续 URL 时本段最多用剩余预算的一半；全部失败时沿用最后一条 Web Fetch 链的终态。不新增平台级正文顺序；`full_text` 时没有已配置的 Web Fetch provider 为飞行前退 3，其他深度不需要 Web Fetch 配置。

### fetch 输出与正文交付

遵循 ADR 0015「默认结果只含下一步决策所需内容，已落盘材料按引用交付」：

- **`full_text`**：正文写入本地 Markdown 文件；stdout 返回 `platform`、`provider`（元数据 route）、条目公共字段与平台字段（`depth` 为 `full_text`），以及 `content_url`（正文实际来自的 URL）、`content_provider`（Web Fetch provider）、`content_path`（可直接读取的文件）与 `content_len`（正文字符数）。正文不出现在 stdout。
- **其他深度**：元数据与该深度的内容直接内联，不写文件。
- **`--format content`**：显式把正文（非全文深度时为该深度的内容）输出到 stdout，不写文件。
- **文件位置**：默认写入系统临时目录下按调用隔离的目录 `forager-platform/<pid>-<时间戳>`（与 research 默认证据目录同一规则），`--content-dir DIR` 可以覆盖；文件名由带版本的 ref 把 `:` 与 `/` 换成 `-` 后加 `.md` 派生。写入方式与 research 证据文件相同；写入失败为 Runtime（退 4），不回退为内联输出。v1 不做跨调用缓存。
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
- 等待窗口的时间计入 attempt 与命令的 Deadline；算法、跨进程协调范围与已知边界见第 4 章「跨进程限速」。
- 不需要凭据的 route 在注册信息中声明 `credentials_required: false`：配置节只有 `url` 与 `timeout`，经 `execute_anonymous` 执行，attempt 的 `credential_index` 与 `rotation_count` 恒为 0。

## skill 与平台词表

- forager skill 的 `SKILL.md` 有一条不列举平台的路由规则：请求点名平台、给出平台 URL 或 ref、或需要平台原生条目时，读取平台 reference（`skills/forager/references/platforms.md`）。
- 平台 reference 写明全文读取方式、分支映射、消费规则（只使用用户给出的或 forager 返回的 ref 与 URL，绝不臆造或拼接）、每个平台的证据语义与恢复规则。
- 平台词表（`skills/forager/references/platform-vocabulary.json`）是机器可读文件，记录每个平台的 id、用途、选择规则、示例与 ref 语法，不写选项和操作；它是以后 classifier 与 `--platforms` 校验的共同来源。新增平台时同步更新词表、平台 reference 与 CLI reference（`skills/forager/references/cli.md`），`tests/skill_contract.rs` 检查词表与 `Platform::ALL` 一致。

## 接入清单

新增一个平台或 route 时逐项完成。新平台 checklist 测试对每个平台操作（search 与 fetch）检查 R1–R8，失败消息写明平台、缺少的登记点与清单编号。

| 编号 | 登记点 | 一致性测试 |
|---|---|---|
| R1 | `catalog::PLATFORMS` 登记平台，search 与 fetch 的 route 集合都非空；`types::Platform` 增加变体 | checklist 测试；`catalog` 单测 `catalogs_project_every_registration_probe_and_smoke_case_consistently` |
| R2 | 每条 route 有 `ProviderId`（全集、解析与名称）和 `ProviderRegistration`；route id 不是裸平台名 | checklist 测试；`catalog` 注册校验 |
| R3 | factory 覆盖每个操作的每条 route：`platform_search_support`／`platform_fetch_support` 有该 route 的分支，`platform_route_config` 返回以该 route 为身份的 `PlatformRouteConfig`（`build_platform_search`／`build_platform_fetch` 按该变体构造）；route 适配器只构造请求、解码响应、声明正文 URL 并提供支持检查 | checklist 测试 |
| R4 | 配置 schema 有 `platforms.<id>.order` 与每条 route 的 `providers.<route>.url`、`.timeout`；`keys` 叶子当且仅当 route 需要凭据；runtime 投影与 `platform_route_config` 覆盖该 route | checklist 测试；`config` schema 单测 |
| R5 | route 的 doctor probe 为 `DoctorProbe::PlatformSearch`（或它所服务的 capability 的 probe） | checklist 测试 |
| R6 | search 与 fetch 各至少有一条 route 登记 smoke 用例，`SPECIFICATION_CASE_IDS` 与第 5 章矩阵同步 | checklist 测试；`tests/smoke.rs` 列表断言 |
| R7 | `tests/acceptance-manifest.json` 为每个操作的每条 route 登记 `(route, platform:<id>:<op>)` fixture 与测试引用 | checklist 测试；`catalog` 单测 `provider_fixture_projection_matches_transport_manifest` |
| R8 | ref 解析与 canonical URL 推导覆盖该平台每个 kind，并在 checklist 的样例表中登记样例 ref | checklist 测试；types 单测 |

同一改动中还须更新：`CONTEXT.md`（新术语）、第 2 章（命令与参数表）、第 3 章（配置键）、第 4 章（模块与依赖）、第 5 章（fixture 与 smoke 用例）、本章的平台示例，以及 skill 的平台词表、平台 reference 与 CLI reference。

## 参考实现：arXiv

- **route**：`arxiv_api`，官方 Query API（默认 `https://export.arxiv.org/api/query`）。不需要凭据；默认 timeout 30 秒；访问策略为每 3 秒 1 个请求、并发 1（arXiv 使用条款）。默认 order 只含这一条 route。
- **ref**：新式 ID（`2401.01234`）与旧式 ID（`hep-th/9901001`），都可以带版本号；字符串形式 `arxiv:<id>[v<n>]`；canonical URL 是 `https://arxiv.org/abs/<id>[v<n>]`；解析也接受 `arxiv.org`、`www.arxiv.org` 与 `export.arxiv.org` 的 abs、pdf（有无 `.pdf` 后缀）与 html 页面 URL，忽略 query、fragment 与末尾 `/`。裸 ID 与其他路径（例如 `list/`、`src/`、HTML 内的图片）不可识别。kind 为 `paper`。
- **search wire 编码**：查询词按空白与双引号切分，每个词写成 `all:"<词>"`，因此布尔运算符、字段前缀与括号都是字面词。arXiv 拒绝带转义双引号的引号词（`all:"a\"b"` 返回 HTTP 400），字面双引号无法发送，所以双引号只作分隔符；分类写成 `cat:<code>`，多个分类写成 `(cat:a OR cat:b)`；作者与标题写成 `au:"<短语>"`、`ti:"<短语>"`；日期区间写成 `submittedDate:[YYYYMMDD0000 TO YYYYMMDD2359]`，缺少的一端用 `199101010000` 或 `999912312359` 补齐；所有条件以 ` AND ` 连接。`sortBy` 为 `relevance`、`submittedDate` 或 `lastUpdatedDate`，`sortOrder` 固定为 `descending`；`--limit` 为 `max_results`，cursor 的页位置为 `start`。
- **解码**：Atom feed 解码为 depth 为 `abstract` 的条目，含完整摘要与元数据；`opensearch:totalResults` 决定是否有下一页，缺少它的成功响应不是 arXiv feed，为 Runtime。cursor 的页位置是 `start` 偏移量，无法解析时在飞行前退 2。id 包含 `arxiv.org/api/errors` 的 entry 是错误，映射为 Parameter 并带上 arXiv 消息；无法解码的 feed 为 Runtime；HTTP 失败沿用共享 status 映射。
- **fetch**：route 支持 `full_text`（默认）与 `abstract`。元数据段请求 Query API 的 `id_list=<id>[v<n>]&max_results=1`；空 feed（`totalResults` 为 0）表示 id 或版本不存在，为 Parameter（`arXiv item not found: arxiv:<ref>`）；返回的条目与请求的 id 或版本不符为 Runtime；429 为 RateLimited，匿名 route 无凭据可轮换，直接成为 route 终态。`full_text` 时 route 对 `<Query API 主机>/html/<id>v<n>` 发一次不重试的 HEAD（经同一访问策略）：404 表示没有 HTML，正文 URL 只有 `https://arxiv.org/pdf/<id>v<n>`；2xx 或其他 HTTP／网络结果（未知）时依次为 `https://arxiv.org/html/<id>v<n>` 与同一版本的 PDF。探测等不到限速窗口时按访问策略终止（预算不足为 Timeout，状态不可用为 Runtime，都退 4、不发送探测、不读正文），保留已完成的 attempts。不按 provider 返回的内容判断有无 HTML，因为 arXiv 的「无 HTML」说明页能通过薄正文门。export 镜像与 arxiv.org 的 HTML 页面返回相同 ETag 与 404 语义（2026-09-26 实测），因此探测跟随 `providers.arxiv_api.url` 的主机，交给 Web Fetch 的正文 URL 始终是 arxiv.org 官方地址。正文由第三方 provider 抓取，不经过本机 arXiv 限速。
