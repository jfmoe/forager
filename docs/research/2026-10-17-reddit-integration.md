# Reddit 平台接入可行性

核验日期：**2026-10-17**。仓库基线：`b3e0d456bfa8f7310e3e88cf22b1a49069f6e0e8`；本机 `forager 0.13.0`、OpenCLI `1.8.6`、Browser Bridge `1.0.24`。本文是调研与接入提案，**没有实现 Reddit Platform，也没有修改 ADR 或 spec**。版本、原文、命令结果及源码快照保存在 [证据目录][E-dir]，逐文件来源和校验和见 [manifest][E-manifest]。本机 HTTP 经过现有代理出口，结论仅适用于本次出口与会话，不代表所有住宅或机房 IP。[环境][E-env]、[HTTP 实测][E-http]。

## 建议

**最优工程方案：新增 `reddit` Platform，优先验收用户自备 key 的 HTTP route `scrapecreators`，以实时原生搜索、帖子读取和有界讨论串交付为目标；`reddit_browser` 只作为个人本机、显式启用的替代。** 推荐理由是 ScrapeCreators 已文档化全站/版块搜索、帖子和评论读取、续页及版块列表，适配现有 keyed HTTP route 的成本最低；browser 则能使用用户已有 Chrome 的页面事实。两者都不等于获得 Reddit 授权：供应商明确不隶属于源站，并把第三方条款责任留给客户；未找到它持有 Reddit 数据再许可的公开证明。商业、学术研究、AI 下游和删除同步要求应先核清，不能把“API 能用”写成“用途已获许可”。[供应商端点][SC-index]、[搜索][SC-search]、[评论与帖子][SC-comments]、[供应商条款 §§2、7–10][SC-terms]、[Reddit 条款][OFF-developer]。

**最小可行方案：只交付一条自有 OpenCLI route `reddit_browser` 的 post 搜索与 `fetch --depth metadata`，默认 `platforms.reddit.order = []`。** 搜索首期只取一屏、最多 7 条，`next_cursor: null`，明确“没有可续页标识，不代表穷尽”；不提供评论独立搜索、`full_text`、`thread` 或 L2 操作。已有实测覆盖搜索结果与帖子/评论页的 SSR 节点，但还须验证 post 根身份和元数据抽取、匿名/已有登录会话差异及真正空集。这个最小范围满足平台必须同时提供 search/fetch 的契约，避免先建设 OAuth 生命周期、档案合并或评论展开。[平台契约][F-platform]、[浏览器结构][E-browser-structure]、[帖子与评论页][E-browser-comment]。若用户接受付费，**同样收缩为 search + metadata 的 `scrapecreators` 是更适合无浏览器/CI 环境的 MVP**，但本次没有使用 key 实测，须先做少量带 key 验收。

这里的“最优”指当前 Rust CLI 的工程匹配度，不是已证明的授权或可用性。若目标是有授权保障的长期商业数据服务，最优前置动作是向 Reddit 确认可覆盖该用途的书面许可/数据合同；本次没有查到可自助购买、明确适用于 forager 的 Reddit 官方商业套餐。若不接受未授权抓取风险、供应商费用或浏览器依赖，保持现有 Web Search 发现能力、暂缓新增平台，是可行的零实现选择。[官方准入][OFF-access]、[Responsible Builder Policy][OFF-responsible]。

**不建议把新的 public OAuth Data API 或 RSS 当成长线基础。** 官方迁移公告列出：2026-10-31 停收新的 public API 访问申请；2027-01-12 开始关闭未登记/未响应迁移的访问；2027-03 关闭剩余 public 访问，未给出三月的具体日期。官方十月通讯同时确认 RSS 于 2026-11-13 停止支持。它们是公告中的未来计划，不能写成今天已经全部关闭；也不应外推为独立商业数据合同一并终止。[迁移公告][OFF-migration]、[原文证据][E-announcements]、[官方 RSS 公告][OFF-rss]、[RSS 原文证据][E-rss]。

## 路线对比

“文档覆盖”与“本次跑通”分列理解；未测试任何付费请求，未创建 app、申请授权、登录或更改账号。

| 路线 | 能力与本次证据 | 优势 | 额外成本、风险 | 结论 |
|---|---|---|---|---|
| 官方 public Data API + OAuth | 技术文档有 search/info/listing/comments；没有可用凭据实测 | 原生结构、fullname、服务端筛选与限额响应头 | 先审批；public 退役时间表；OAuth 凭据与 token 生命周期；删除同步 | 只为已有获批应用/有后续许可时评估，不作为新默认基础 [OFF-api]、[OFF-migration] |
| 官方 Devvit / 合同数据服务 | Devvit 在 Reddit 托管应用内自动鉴权，围绕已安装社区；没有找到供任意本地 CLI 使用的通用外部读取替代端点 | 官方支持与授权路径 | 托管架构、应用审查、用途/社区范围；合同条件未知 | 值得询问，不把 Devvit API 直接当 Rust HTTP endpoint [OFF-devvit]、[OFF-responsible] |
| 匿名 `.json` / old Reddit | www 搜索/讨论串均 403；old 均 302 去登录 | JSON 易适配 | 本机不可读；旧站不是稳定匿名后门 | 排除首期 [E-http] |
| 匿名 RSS | www 搜索一回 200 Atom、一回 429；old 302 | 免 key、可发现帖子 | 能力与计数有限，且临近官方停止支持日期 | 不建 route [E-http]、[OFF-rss] |
| 匿名 Shreddit `/svc` HTTP | 四种页面片段两种 UA 均 200，有结果/cursor、分数、评论 | 免 key、原生 HTML 字段、本机最轻量 | 未公开契约、robots 禁止、抓取条款、A/B 与解析漂移、正文与评论完整性 | 技术可行；不默认发布为基础 route [E-content]、[OFF-user] |
| 自有 OpenCLI `reddit_browser` | 真实搜索页 7 结果、lazy cursor；详情页 1 post/97 comment 节点 | 只读站点已加载内容，复用现有 process transport | Chrome/OpenCLI/已有会话；DOM 漂移；翻页未成功；账号和条款风险 | 个人本机 MVP，opt-in [E-browser-structure]、[E-browser-comment] |
| OpenCLI 内置 `reddit` | 21 个命令，既有读也有写；search/read 主动浏览器 fetch `.json` | 现成原型、筛选、递归评论 | 超出只读页面自身响应的边界；有截断、重排且不是 forager 外壳 | 只参考源码，不直接接为 route [E-oc-help]、[E-oc-source] |
| ScrapeCreators | 文档有搜索、版块、帖子/评论、cursor；未带 key 测试 | JSON、用户自备 key、供应商维护抓取 | 预付 credits；无可用性保证；非 Reddit 授权证明；数据发给供应商 | 最优工程候选，验收和用途确认后显式启用 [SC-search]、[SC-comments]、[SC-terms] |
| Arctic Shift | 列表样本 200；版块内 keyword 样本 422；文档有 ID/tree | 免费、历史数据、批量 ID 和树 | 存档非实时、keyword 有限制、删除同步与授权不明、无 SLA | 历史补充候选，不默认补实时分数 [AS-api]、[E-arctic]、[E-arctic-keyword] |
| PullPush | 文档站可读；两条查询路径均 429，正文明确不供 agent 免费抓取 | 传统 Pushshift 式查询 | 当前出口无数据、维护/覆盖与许可不明；单独删除流程 | 本轮不可用，不继续重试 [PP-doc]、[E-pullpush]、[E-pullpush-topic] |
| Web Search `site:reddit.com` | 复用已有 discovery；不是平台原生检索 | 无新增运行时，可找旧帖与相关 URL | 索引延迟、覆盖/排序不可控，不能保证正文/评论和准确计数 | 保留为显式补充，不能作为 Platform 链的隐式 fallback [F-platform]、[E-orientation] |

## 官方 API：准入、OAuth 与端点

本节各项均在 **2026-10-17** 核验。帮助中心与官方旧 wiki 不同层级：当前政策决定能否访问，旧 wiki 只说明技术协议；Data API Wiki 自己警告旧资料可能过时。[OFF-wiki]。

### 新应用如何访问

| 问题 | 已核实的现状 | 来源与局限 |
|---|---|---|
| 可以先自行建 app 就开始用吗 | 不可以据此推定。Responsible Builder Policy 要求先申请并获明确批准；开发者优先用 Devvit，不被支持的用途可走 developer ticket | [当前政策][OFF-responsible]；没有提交申请，也未登录 `prefs/apps` 验证按钮是否对某个账号仍可用 |
| `prefs/apps` 是否仍存在 | 官方归档 OAuth 教程仍链接该页面并描述 Create app；这不证明 2026 新账号能自助得到可用授权 | [旧 quick start][OFF-script]；界面存在性和审批资格分开处理 |
| 申请入口 | Data API Wiki 链到 help ticket `14868593862164`；政策另给 developer/enterprise/researcher 分类；不支持 Devvit 的用途可以提出，但批准时长、概率及本 CLI 是否符合条件均查不到 | [Wiki][OFF-wiki]、[政策][OFF-responsible] |
| 最新时间限制 | public API 新申请截止 2026-10-31；后续关闭按上文公告。未找到新 Rust CLI 在 Devvit 外持续使用 public API 的豁免承诺 | [迁移公告][OFF-migration]；不能由旧教程推出长期保证 |
| 免费额度 | **符合免费访问资格者每 OAuth client id 100 QPM**，当前以 **10 分钟窗口平均**，允许突发；不是每个终端用户独享 100，也不是无限日/月额度承诺 | [当前 Wiki][OFF-wiki]；没有查到统一日/月请求额度或新应用审批后的专用额度 |
| 限额响应头 | `X-Ratelimit-Used`、`X-Ratelimit-Remaining`、`X-Ratelimit-Reset`，分别是本周期已用、剩余、到周期结束的秒数 | [当前 Wiki][OFF-wiki]；无 token，本次未观测 OAuth 响应头 |
| 匿名额度 | 当前 Wiki 说未使用 OAuth 或登录凭据的流量会被拦截，默认额度不适用；不沿用旧的“匿名 10 QPM” | [当前 Wiki][OFF-wiki]；少数网站 HTML/RSS 返回 200 不构成 API 免费授权 |

### 适用 OAuth 流程

以下是**仍被当前 Wiki 链接的官方归档技术文档**，不是对新应用批准或 2027 后可用性的保证。[OAuth2][OFF-oauth]、[script 示例][OFF-script]。

| 流程 | 旧官方技术定义 | 对 forager 的建议 |
|---|---|---|
| `script` + `grant_type=password` | client id/secret 走 HTTP Basic；POST `/api/v1/access_token`，表单含用户名密码；只用于登记为 app developer 的账号。示例 `expires_in: 3600` | 公开 CLI 不收集或分发用户 Reddit 密码；只有用户自管、已批准的个人脚本才可能适用 |
| Application-only `client_credentials` | 无用户上下文；confidential web/script client 可用；Basic client id/secret，POST `https://www.reddit.com/api/v1/access_token` | 若已有适用批准，这是只读公共内容最简单的技术流；不是免审批匿名访问 |
| Application-only `https://oauth.reddit.com/grants/installed_client` | installed app 无法保密 secret，不能用 `client_credentials`；使用该扩展 grant，并提供持久、随机的每设备 `device_id`（20–30 ASCII 字符）；无 secret 的 Basic 密码为空 | 分发本机 CLI 更符合该 app 类型，但需要实际批准和 token 端点验收；不能内置共享 secret |
| User authorization code | 用户同意 scope、校验 `state`、redirect URI；`duration=permanent` 可得 refresh token | 如确需私有/用户授权内容再评估；本提案只读公开内容，不增加这个范围 |

app-only 不返回 refresh token，应按响应的有效期重新取 token；持 bearer 的资源请求发到 `https://oauth.reddit.com`。不要把示例有效期、旧 wiki 中歧义的 `expires_in` 描述写死成新的服务承诺。[OFF-oauth]。OAuth route 需要 client id、可能的 secret/device id、token 更新及诚实 UA 联系人，**现有 `keys: [string]` 的长期 API-key 投影不能直接完整表达它**；单放一小时 bearer 只能临时验证，不能称完成集成。若决定做官方 route，先提出满足 R4 的凭据配置和生命周期方案，再实施。[F-platform]。

### 相关只读端点与参数

以下来自官方自动生成 API 文档；这套文档与现行准入政策共同使用。`read` 是相关端点的 OAuth scope。[OFF-api]。

| 端点 | 能力与参数 | 接入时注意 |
|---|---|---|
| `GET /search`、`GET /r/{sub}/search` | 搜索 links；`q` 最长 512 字符；`sort=relevance\|hot\|top\|new\|comments`；`t=hour\|day\|week\|month\|year\|all`；`restrict_sr` boolean；`type=sr,link,user` 的子集；listing 参数 | post 搜索固定 `type=link`；官方此端点未文档化全站 comment 搜索，不能把新版网页 Comments tab 当同一 API 能力 |
| `GET /comments/{article}`（可带 `/r/{sub}`） | `article` 为 post ID36；`comment` 为聚焦 comment ID36；`context=0..8`；`depth`、`limit` 为可选整数；`sort=confidence\|top\|new\|controversial\|old\|random\|qa\|live` | 返回根帖与评论视图；本页未给 comments `limit` 的统一最大值，**查不到，不套用 listing 的 100** |
| `GET /api/info?id=t3_...,t1_...` | 按 fullname 返回 listing；允许 links、comments、subreddits；也有 `url`、`sr_name` 查询 | metadata/身份核对候选；本页未给批量 id 数量上限，不能声称无限或假定 100 |
| `GET /by_id/{names}` | 逗号/空格分隔的 link fullnames | 只对 posts，不替代 comment info |
| `GET /r/{sub}/hot\|new\|rising\|top\|controversial`，以及 `/best` | 各类 listing；top/controversial 有 `t`；通常默认 `limit=25`，最大 **100** | 页面排名是当前切片，不是可复现历史快照；best 涉及会话语义，不列首期 |
| `GET /api/morechildren` | `link_id`、`children`（comment ID36 列表）展开 omitted comments，至多 100 个一次；另有 sort/depth 等 | 官方明确同一时刻只能一个该端点请求；若加入 thread 展开必须串行且有总预算 |

listing 不用页码：`after` 或 `before` 只取一个，值为 fullname anchor；`count` 是已见数量，响应提供下一组 `after/before`。`limit` 是最大希望条数，不保证填满。上述 `type` 枚举是 Data API 的枚举，不是 Shreddit 的 `type=posts`，也不是第三方的 `filter=posts|comments`。[API overview/search/info/comments][OFF-api]。

## 条款、缓存与内容生命周期

**这是条款原文的工程含义梳理，不是对某种用途的法律许可判断。** 核验日期均为 2026-10-17；原文快照分别见 `official-*.json` 和对应 `.txt`。

| 约束 | 原文要点 | 对接入的影响 |
|---|---|---|
| Data API Terms | 页面标注 2023-06-19 生效、2026-07-20 修订；§2.2 用户拥有内容，复制/展示限于应用必要范围；§3.1 商业、超限研究或未明确允许用途需单独协议；§3.2 不得保留超出获批用途的数据；§7 终止时删除缓存/存储及相关派生物 | 不能因免费额度或供应商可抓取而假定商业或转售许可 [OFF-data] |
| Developer Terms | 页面标注 2024-09-24 生效、2026-03-24 修订；§4.1 企业或代表企业使用、货币化产品等需批准；§4.2 禁止规避限制、隐藏用途、同用途多 app 扩额等 | 不以多 client/key/account 轮换规避每 client 限额；不得把 existing provider pool 的换 key 机制用于绕过 Reddit 限制 [OFF-developer] |
| AI/ML | Data API Terms §2.2/§3.2 要求内容权利人对训练用途明确许可；Developer Terms §4.2/§5.1 与帮助中心亦限制未经允许训练模型 | **没有查到“临时给 LLM 做问答/RAG 一律允许”或“一切推理均禁止”的明确通用许可**；forager 的 Agent 下游、外发供应商、企业用途需申明后确认，训练与检索推理分开判断 [OFF-data]、[OFF-developer]、[OFF-access] |
| 学术研究 | 当前帮助中心与 Responsible Builder Policy 将 Reddit for Researchers（RFR）列为官方授权研究路径；政策说从 RFR 外收集数据用于研究违反该政策 | 本文的接入可行性核查不构成获批的 Reddit 内容研究项目；不能将第三方 archive 标作“官方认可研究通道” [OFF-access]、[OFF-responsible] |
| User-Agent | 当前 Wiki 要求可行时用独特、描述性的 UA，含 platform/app/version/Reddit username 联系人，且不得撒谎 | 正式 HTTP route 应提供真实 app/version/contact；本次诚实 UA 说明了研究目的，未冒认任何 Reddit 用户；浏览器 UA 只是用户指定的有限对比试验，不能据此建议长期伪装 [OFF-wiki]、[E-http] |
| 删除与修改 | Developer Terms §3.3 要求内容删除、变私密、暂停、撤回、修改后尽快删除/修改副本；Wiki 要求删除标题、正文、内嵌 URL 等，账号删除时移除作者身份信息 | `content_path` 文件也是副本，零跨调用缓存并不自动满足义务；讨论串、日志/测试样本、用户另存的文件均须纳入生命周期判断 [OFF-developer]、[OFF-wiki] |
| 缓存时限 | Wiki **强烈建议**例行在 48 小时内清理存储的用户数据和内容；Data API Terms 禁止超获批用途保留，终止时立即删除。没有查到适用于所有缓存的单一硬性“24h/48h TTL” | 48h 不是授权永久留存的安全窗口，也不是站点删除后可等 48h；MVP 不做跨调用缓存，输出应说明读取时间及保存责任，正式发布前决定删除同步与交付文件政策 [OFF-wiki]、[OFF-data] |
| 网页抓取 | User Agreement 禁止未经事先书面许可 scraping，允许的 crawl 受 robots 参数约束；本机 www/old robots 均 `User-agent: *` / `Disallow: /` | `/svc` 200 或已有登录不能推导为许可；browser opt-in 只控制用户选择，不改变源站条款 [OFF-user]、[E-robots-www]、[E-robots-old] |
| 供应商与存档 | ScrapeCreators 把源站合规留给客户且可能保存请求/响应；Arctic Shift 有独立移除申请；PullPush 也要求单独申请删除 | 采购与接入都要核对授权、删除传递与数据保存，不能以它们来恢复原站已删除内容 [SC-terms]、[AS-readme]、[PP-doc] |

建议不要默认把 Arctic Shift 的旧内容或计数与实时网页混合：它可能把当前不可见的数据重新带回结果；“0 分”也可能是真实值，不能直接当缺失后补。[存档规则][AS-data]、[参考实现补分逻辑][L-keyless]。这些是本提案的风险控制取舍，不是已实现机制。

## 匿名访问实测

### 方法与边界

使用本机 `curl`，不带 Cookie/OAuth、不保存/回送站点 Cookie、不重试、不执行挑战 JavaScript、不切换出口。每个代表端点两次：诚实研究 UA 与 Chrome 风格 UA 各一次，串行、前一个请求完成后等待 **2.2 秒**。重定向先不自动跟随，保存 `Location`；`redd.it` 和分享短链的已知目标各追加一次手动首跳观察。初始矩阵 22 次，加 robots 2 次、分享链接 2 次及其目标 2 次，共 **28 次直接 Reddit HTTP 观测**；没有发起翻页抓取。浏览器自身网络请求及经 forager 的文档检索另计，无法逐条按这个间隔控制。[可复核脚本][E-probe]、[原始状态][E-http]、[分享链接][E-share]。

诚实 UA：`forager-reddit-integration-research/0.13.0 (local read-only feasibility check; no OAuth; 2026-10-17)`；browser UA 的完整值见各记录。只报告实际首跳，不把 301/302 当目标正文成功。状态码之外检查内容类型和结构，区分 403 拦截、200 挑战页、真实数据与页面中普通安全组件。[E-probe]、[E-content]。

| 代表入口 | 诚实 UA | browser UA | 结构和挑战判断 |
|---|---:|---:|---|
| `www /search.json?q=rust&limit=1` | 403 | 403 | HTML “blocked by network security”，不是 JSON |
| `www /comments/1wubcvf.json?limit=1&depth=1` | 403 | 403 | 同上 |
| `www /search.rss?q=rust&limit=1` | 200 | 429 | 第一次 `application/atom+xml`，第二次限流；UA 与顺序/共享额度混杂，不能归因为 UA 优劣 |
| `old /search.json`（同查询） | 302 | 302 | `Location` 指向 old `/login/?reason=lor2&dest=...` |
| `old /comments/1wubcvf.json`（同参数） | 302 | 302 | 同上 |
| `old /search.rss`（同查询） | 302 | 302 | 同上；未跟进登录页 |
| `www /svc/shreddit/search/?q=rust&type=posts&t=month&sort=relevance` | 200 | 200 | 各 7 个 `search/view/post` 结果；都有下一页 lazy partial/cursor |
| `www /svc/shreddit/r/rust/search/?q=async&type=posts&t=month&sort=relevance` | 200 | 200 | 各 7 个结果与 cursor |
| `www /svc/shreddit/community-more-posts/new/?name=rust` | 200 | 200 | 分别 84/87 个 `<shreddit-post>` 起始标签，有 fullname/标题/score/时间；含页面额外内容，数量不是经过去重和广告过滤的交付数 |
| `www /svc/shreddit/comments/r/redditdev/t3_1wubcvf` | 200 | 200 | 分别 104/124 个 `<shreddit-comment>` 起始标签；不证明读全、顺序稳定或根帖全文可用 |
| `https://redd.it/1wubcvf` | 301 | 301 | 同向 `https://www.reddit.com/comments/1wubcvf`；目标另测为 **200 JS challenge**，在 hidden form 中有 `js_challenge`/`jsc_token`；未执行 |
| `www /r/running/s/3TzXiyxaMD` | 301 | 301 | 同向帖子 `17snc7d` 的 comment `k8so54m`，带 share/utm 参数；样例来源为 Arctic Shift 官方项目 API 文档；目标不带 tracking 参数另测仍 301，未继续追链 |
| www/old `/robots.txt` | 200 / 200 | 未重复 | 都禁止通用爬虫访问全部路径 |

矩阵证据：[状态/头/正文][E-http]、[结构验证][E-content]、[redd.it 目标][E-redd-target]、[分享首跳][E-share]、[分享目标][E-share-target]、[robots][E-robots-www] [E-robots-old]。

`/svc` 页面对两种 UA 都有 `recaptcha` 字符串，但真实结果/评论同时存在，不能只匹配这个词就判 Auth；`redd.it` 的目标才是有 hidden challenge form、没有已验证帖子结果的明确反例。[E-content]、[E-redd-target]。本轮没有测 old 的 `/svc`、不同地区/出口、私有/隔离/NSFW 社区、真实空集、删除帖或长时间稳定性；没有通过换 UA 反复试探挑战。

## OpenCLI browser route 可行性

### 已安装实现与小红书边界的差异

OpenCLI doctor 显示扩展已连接；内置 Reddit 有 21 个命令，包括 search/read/hot 和发帖评论、投票等写操作。安装源码快照固定在 [opencli-source.json][E-oc-source]；它代表本机 1.8.6，不代表上游最新版本。[doctor][E-oc-doctor]、[帮助][E-oc-help]。

- `clis/reddit/search.js:66–81` 构造 `/search.json` 或 `/r/{sub}/search.json`，在 `evaluate` 内主动 `fetch(..., {credentials:'include'})`；不是读取页面自己发出的搜索响应。后续投影数据并切 limit，没有 forager cursor 外壳。
- `clis/reddit/read.js:1–9` 描述 Top-K 评论、递归、截断和可选 morechildren；`:108–125` 定义这些开关，`:216` 与 `:322` 主动 fetch `.json` 与展开接口。输出形状不能直接充当无截断 thread 或平台身份契约。
- 内置 adapter 不能按名称整体授权给 forager。自有 `forager-reddit` adapter 应只注册 `contract/search/post` 等只读命令，由 Rust 固定构造 argv，沿用本站点自有 adapter 的契约版本/错误分类模式。[源码][E-oc-source]、[OpenCLI 规则][F-opencli]、[小红书设计][F-xhs-design]。

### 页面实际加载了什么

本次通过专用临时浏览器会话打开一次正常搜索页和一次 comment permalink，读取 DOM、Resource Timing 与 OpenCLI 网络形状，最后释放标签页。没有执行登录，也没有读取/导出 Cookie、localStorage 或 token。**这是用户已有 Chrome 会话；页面呈现 logged-in drawer/inbox 等迹象，因此不能称为干净匿名浏览器实验。**[浏览器打开][E-browser-open]、[观察][E-browser]、[释放][E-browser-close]。

| 场景 | 实测 | 能支持的判断 |
|---|---|---|
| `/search/?q=rust&type=posts` 首屏 | DOM 7 条结果，`search-telemetry-tracker`、`a[data-testid=post-title]`、`faceplate-timeago` 等；没有 `shreddit-post`；存在 `/svc/shreddit/search/?...&cursor=...` 的 lazy partial；没有独立 `application/json` 状态 script | 搜索内容可从 SSR HTML/DOM 读取；不能用“找不到 shreddit-post”等同于没结果 [E-browser-structure] |
| 搜索网络 | 主要是 `/svc/shreddit/events`、安全/consent/drawer 等；未捕获首个主文档正文，也没有已证实的搜索 JSON API 响应 | 不应为首屏硬等 `.json`/GraphQL 才宣告完成；DOM 已包含搜索事实 [E-browser-network] |
| 一次普通滚动和一次滚到底 | 页面仍 7 条结果；后续 network 返回空 entries | 只验证了 cursor 元素存在，**没有验证页面实际完成第二页加载或独立续页**；不能宣称翻页跑通 [E-browser-scroll]、[E-browser-after] |
| `https://www.reddit.com/comments/17snc7d/_/k8so54m/` | 页面正常，目标 `t1_k8so54m` 出现，1 个根 post、97 个 comment 节点 | 不带 subreddit/slug 的、含 post/comment id 的 URL 在此样本能定位 comment，可用于后续 canonical 方案验收 [E-browser-comment] |
| 详情网络 | 同时存在 `/svc/shreddit/graphql` 的 `application/json` 响应；形状为 `redditorInfoByName/trophyCase`、`createCaptchaToken`、`exposeExperiment` 等 | 新版页面是 **SSR HTML/片段与 JSON GraphQL 并存**；本次 GraphQL 不是已经证明可读取完整帖子/评论树的接口 [E-browser-comment-network] |

与小红书相同的可行实现边界是：导航正常页面、读取已渲染事实/页面原生响应、必要时点击页面已有的只读筛选和“更多评论”，不拼 GraphQL query、不自行请求未由页面触发的 `/svc`、不注入 token 或替用户处理验证。[F-xhs-design]、[F-opencli]。建议读取模块只返回必需字段及页面状态，不回传完整 HTML、遥测、用户菜单、鉴权与安全接口响应。

首屏完成信号应依赖身份匹配的结果容器或明确空集，详情依赖目标 fullname；分页/筛选要等待新目标响应完成并核对查询/游标。页面有实际内容时不因通用安全脚本存在而失败；真正登录墙/挑战为 Auth，结构漂移或无响应体为 Runtime，停在浏览器网络错误页为 Network。这些是**待实现、待验证的提案**，不能由本轮网络形状直接宣布已实现。

## last30days 参考实现剖析

只读本地浅克隆，未执行它的任何代码。基线 **`0341304f8ecb307ca0a939e48ecd002761ef23b2`**（HEAD 提交日期 2026-10-15）；以下 GitHub 链接均固定该提交，完整原文按行数组保存在 [源码证据][E-last-source]。fixtures 是固定测试样本；搜索样本文件头自述来自 2026-09-30 的页面并经裁剪，不能作为 2026-10-17 活站可用性的证明。

| 文件/行 | 实际职责和细节 | 迁入 forager 前的判断 |
|---|---|---|
| [`reddit_keyless.py:1–16, 113–194`][L-keyless] | 默认发现调用站内搜索；定向社区另做 top/hot/new listing，按 URL 去重，优先实时 listing 分数，再对 falsey score 用 Arctic Shift 补分；没有实际调用 public search 的 Tier 0 | 是多来源发现/排序流水线，不能直接等同一页原生搜索；真实 0 分会被当缺失 |
| [`reddit_search.py:1–18, 81–97, 100–160`][L-search] | 全站 `/svc/shreddit/search/`、社区 `/svc/shreddit/r/{sub}/search/`；`q,type=posts,t,sort=relevance,cursor`；解析 tracker、标题链接、timeago、counter；从 lazy partial 取 cursor | 只证明 relevance 参数实现；不能据此宣称多 sort 已实现 |
| [`reddit_listing.py:1–15, 130–185`][L-listing] | `community-more-posts/{sort}/?name={sub}`，读 `<shreddit-post>` 的 score、comment-count、作者、创建时间、permalink | 候选 L2 榜单来源；需去广告/去重/空页和挑战识别 |
| [`reddit_shreddit.py:1–42`][L-shreddit] | `/svc/shreddit/comments/r/{sub}/t3_{id}` 读取带 score/author/created/thingid 的 comments；最多保留 12 条，按研究深度只增强部分帖子 | 研究摘要用的 top comments，不是完整 thread；不能照搬后标为全串 |
| [`reddit_arctic.py:1–29, 57–108`][L-arctic] | `/api/posts/ids` 批量补分：每批 50、最多 3 批、0.4 秒间隔、运行内 memo；另有 `/api/posts/search` listing fallback | 0.4 秒是该实现经验，不是 Arctic Shift 保证额度；旧分数不是实时点赞 |
| [`http.py:1136–1180, 1246–1277, 1363–1478`][L-http] | browser UA；共享 keyless 令牌桶默认 1 req/s、burst 2；运行内相同 URL memo/合并在途请求；429 经过 limiter 再试一次，短暂停顿带 jitter | 只是单进程共享；forager 须用现有跨进程 limiter；不照抄 UA 伪装和短于 reset 的重试策略 |
| [`reddit_public.py:1–18`][L-public] | 文件头仍说 `.json` 为 keyless Tier 0、可能住宅 200，并保留兼容入口 | 与实际 `reddit_keyless` 及 changelog 不一致，是文档残留；“永久 403/429”只可归属为项目判断，不能当 Reddit 官方事实 |
| [`reddit.py:32, 322–440`][L-paid] | ScrapeCreators base；global/subreddit search 与 post/comments，`SCRAPECREATORS_API_KEY` 进请求头 | 源码参数与当前供应商文档已有差异：全站 sort/time 枚举、社区 cursor/filter 要独立核验 |
| [`CONFIGURATION.md:195, 606–607`][L-config] | pin `LAST30DAYS_REDDIT_BACKEND=scrapecreators` 可优先付费；当前默认免费结果少于 5 条时付费 backfill；设阈值 0 恢复仅空结果补充；一次 backfill 可有约 10 次 API 调用 | forager 不应悄悄把薄结果扩成多次计费检索，也不沿用本地融合/重排语义 |

**tracker 存在两层，不能只看外层判断解析兼容性。** 该提交 `reddit_search.py:109–116` 从所有 tracker 中按 `action_info.type == "post"` 接受上下文。实测外层 `search/view/post` tracker 只有 position，但内层 `search/click/post` 带 `type=post`；对全部 tracker 逐个套用该谓词，四份搜索样本均匹配 7 个唯一 post，与外层结果数一致。这里没有执行参考代码，也没有验证完整 `parse_page` 的所有字段；证据支持身份筛选谓词在这些样本上匹配，**不支持据外层缺字段宣称解析已失效**。[结构核对][E-parser]。

其 fixtures 覆盖分工也值得保留：`fixtures/reddit_search_page1.html:1,40–43` 是有 7 条结果和 cursor 的裁剪页，已同时包含这两层 tracker；`reddit_search_last_page.html` 覆盖末页；`reddit_search_no_results.html:40` 有明确 `search/view/no_results`，`reddit_search_challenge.html:15` 有 challenge form；`reddit_listing_cards_sample.html` 和 `reddit_shreddit_comments_sample.html` 分别覆盖卡片与评论属性，`reddit_thread_sample.json` 是 JSON thread 形状参考。它们的原文与逐行定位都在 [源码证据][E-last-source]，但正文截断、完整评论树和当前页面动态加载仍须活站验收。

### 路线演变及查证边界

| 时间/版本 | 已有一手记录 | 可以下的结论 |
|---|---|---|
| 2026-01-15 v1.0.0 | CHANGELOG:1313–1317 写 Reddit + X 经 OpenAI Responses/xAI；`docs/how-search-works.md:30–75` 明确 Reddit 用 OpenAI `web_search`，域限定 reddit.com，再用 public JSON 补互动数 | **曾有 OpenAI Reddit discovery 路径**；这份 how-search-works 留着旧实现叙述，不能代表当前默认行为 [L-changelog]、[L-how] |
| 2026-03-05 v2.9.0 | CHANGELOG:1211–1235 新增 ScrapeCreators Reddit，改 `primaryEnv`，Reddit 搜索不再需要 OpenAI key；强调搜索、社区发现、评论和互动 | 替代为结构化第三方读取；记录没有提供可比较的可靠性/成本实测，不能补写成“因 OpenAI 不准” [L-changelog] |
| 2026-06-25 v3.8.2/3.8.3 | CHANGELOG:608–626：付费 pin、Arctic 补分、删除“permanently-403”的 search.json Tier 0；免费为 RSS + listing + shreddit | 免 key 与付费两路并存；避开匿名 JSON 是项目观察/决策 [L-changelog] |
| 2026-10-01 v3.26.0 | CHANGELOG:147–163：因 11-13 RSS 停止支持而删除 RSS，改站内 `/svc` 搜索，并把付费薄结果补充阈值改为 5 | 改动原因有明确文档，且 RSS 日期被 Reddit 官方通讯独立确认 [L-changelog]、[OFF-rss] |

没有找到 **Reddit 通过 xAI 专属检索** 的可证实历史；早期总述把 Reddit/X 和 OpenAI/xAI 并列不能推出四种组合。没有在当前浅克隆的 Python、Markdown、配置或 changelog 找到官方 Reddit OAuth client/secret/refresh token 路线；这是**检索范围内未发现**，不是“历史上从未存在”。浅克隆没有完整历史，未额外取旧版本；当前 skill 里还留有 OpenAI legacy fallback 描述，与实际 keyless 路径不应混为一谈。[历史文档][L-how]、[skill:232][L-skill]、[搜索范围与结果][E-last-audit]。

## 第三方能力、价格与风险

### ScrapeCreators

一手目录在 [llms.txt][SC-index]，基础 URL `https://api.scrapecreators.com`，`x-api-key`。本轮核验以下实际文档，**没有带 key 调用**：

| 端点 | 已文档化映射 | 边界 |
|---|---|---|
| `GET /v1/reddit/search` | `query` 必填；`filter=posts\|comments`；`sort=relevance\|new\|top\|comment_count`（comment 搜索不支持最后一种）；post `timeframe=all\|day\|week\|month\|year`；`after`；每请求 1 credit | 全站未列 hot/hour；不能照抄 last30days 参数枚举 [SC-search] |
| `GET /v1/reddit/subreddit/search` | `subreddit`；`query`；posts/media 的 sort `relevance\|hot\|top\|new\|comments`，comments 则 relevance/top/new；`timeframe` 另含 hour；不透明 `cursor`；1 credit | 文档示例混合 posts/comments/media，但未列显式类型 selector，不能凭示例假设 filter 可用；必须带 key 确认形状与分页 [SC-subsearch] |
| `GET /v1/reddit/post/comments` | `url` 必填；一次一个 opaque `cursor`；返回帖子信息和评论；cursor 太长可用同参数的 POST 版本；1 credit | 这是本轮核实的 fetch 候选；目录里没有证明独立 `/v1/reddit/post` 是所需通用详情端点，不据搜索摘要臆造 [SC-comments]、[SC-index] |
| `GET /v1/reddit/subreddit` | `subreddit`；sort `best\|hot\|new\|top\|rising`；timeframe `all\|day\|week\|month\|year`；`after`；1 credit | 候选 L2 列表；`cache_max_age=1d\|3d\|7d\|14d\|30d` 可用缓存 0 credit，但可能很旧 [SC-listing] |
| `/subreddit/details`、`/post/comments` POST | 目录存在 | 非首期必需，未逐一验收 [SC-index] |

**价格（2026-10-17 官网）**：100 初始免费 credits，宣传另可申领 bonus；$47/25,000、$497/500,000 credits，一次性预付、非订阅。若请求均为 1 credit，分别约 **$1.88/1,000 次**、**$0.994/1,000 次**；一页 search + 一次 fetch 约 2 credits，额外评论页继续增加。广告式 bonus 不是默认免费额度，不沿用 last30days 配置文档里“10K 免费”的描述。[价格原文][SC-home]、[保存副本][E-sc-home]。

官网宣称无账号级每分钟/套餐并发上限，建议低于 500 并发；这不保证 Reddit 源站不封锁。[限流说明][SC-rate]。官网宣传 credits 不过期，**条款 §4 却保留使其过期/停止提供的裁量**；§8/§11 不保证端点连续可用，§7 客户自行遵守第三方条款，§9 可保留响应/请求元数据/日志，§10 终止可没收未用 credits。[SC-home]、[SC-terms]。没有找到可核验的 Reddit 授权、删除传播 SLA、长期数据保留硬期限或请求失败计费的完整承诺；实施前应询问。

doctor 可考虑 `/v1/account/credit-balance`，但本轮文档把它也标为 **1 credit/request**，不是已证实免费的账户探针。应告知 probe 成本，或先向供应商确认；不能套用 SerpApi 的“doctor 免费”。[账户文档][SC-account]。

### Arctic Shift

维护方是 GitHub `ArthurHeitmann/arctic_shift`，自称 Project Arctic Shift，使用 photon-reddit 域名托管。README 称有限 API、提供 dumps 和删除请求表单，API README 明确没有 uptime/performance 保证；它不是 Reddit 官方 API。[AS-readme]、[AS-api]。

- ID 查询：`/api/posts/ids`、`/api/comments/ids`（最多 500 IDs）；搜索：`/api/posts/search`、`/api/comments/search`；树：`/api/comments/tree?link_id=...`，可用 parent id、limit（文档最大 25,000）、breadth/depth，超过限制以 `kind:more` 折叠；另有汇总、short_links 和 time_series。[AS-api]
- keyword 能力不是 Reddit 全站 relevance 搜索：post `title/selftext/query`、comment `body` 有范围/高活跃社区限制，API 文档还明确说全文搜索说明已过时、能力可能改变；sort 为 `created_utc` 的 asc/desc，没有官方 hot/top 语义。[AS-api]
- 文档说先即时归档，约 **36 小时后更新分数/评论数等**，早期值可能 0/1；这是计数刷新延迟，**不是“所有数据必须等 36 小时才可查询”**。数据说明说不含 private/quarantined 社区，较新数据做第二次获取并记录 `retrieved_on/_meta.retrieved_2nd_on/was_deleted_later` 等。[AS-api]、[AS-data]
- 没有在已核验一手资料找到全球覆盖率、固定最早/最新完整日期或有保障的入库延迟。不同数据集时期处理不同，不能把旧 Pushshift、月 dump 与在线 API 覆盖范围混写。[AS-data]、[AS-readme]
- 速率动态按负载和复杂度计算；文档说正常每秒几次无需担心，但没有固定 numeric QPM 保证；429 用 `X-RateLimit-Reset`（秒）或 `X-RateLimit-Reset-At` 等待。[AS-api]

**本机两次小样本**：r/rust 最新 1 条列表返回 200，帖创建 `2026-10-17T09:47:14Z`、`retrieved_on=09:47:37Z`，说明这条记录入库相隔 23 秒；查询发生在约 12:14Z，不能据此认定全站新鲜度，尤其 score=1/comments=0 可能尚未更新。随后一次限定 r/rust 的 `query=async&limit=1` 返回 **422**，正文 `Timeout. Maybe slow down a bit`；不能当合法空集，也不能认定 keyword 完全不能用。[列表证据][E-arctic]、[关键词证据][E-arctic-keyword]。

**Reddit 对它的立场：查不到针对 Arctic Shift 的明确授权、认可或专门禁止声明。** 检索到的官方约束是通用政策与条款；不能把对 Pushshift 的历史处理等同 Arctic Shift，也不能把 archive 自称为 researchers 服务当 RFR 授权。[检索记录][E-position]、[OFF-responsible]、[OFF-user]。这使它适合作为另行评估的历史资料来源，暂不适合作为默认实时 route 或透明分数增强器。

### PullPush 与 Web Search

PullPush 当前文档将 `/comment`、`/topic`、`/link_id` 列为 legacy API；参数包括 q/title/selftext、author/subreddit、after/before、size≤100、sort/score、ids/exclude_ids。文档自己说明删除请求不会自动从 Pushshift 传播到 PullPush，须单独提交。[PP-doc]。

本机对常见 `/reddit/search/submission/` 与文档直接列出的 `/topic` 各请求一次，两者均 **429 JSON**，原文表示该网站不为 agents 提供免费抓取资源，可联系管理员询问付费服务。这比一般“暂时超限”更明确，因此不重试、不换 UA，**本轮未获得可用数据**。不能由此宣称网站全球下线；也没有核实它当前完整时间覆盖、固定限流额度或商业授权。[E-pullpush]、[E-pullpush-topic]。它的优势只保留为既有查询模型和存档线索，当前不列推荐接入。

Web Search 用来找平台 URL、冷门/旧帖和官方公告；结果是搜索索引/模型所见的 snippet，不保证全站、server-side subreddit/time/sort、实时计数或完整评论。本轮几次 discovery 的自动答案含无法直接证实的历史额度、日期和第三方端点推断，均只作线索，本文以读到的原文/源码/实测为准。[E-orientation]、[E-third-discovery]。它沿用既有 search/research，不冒充 `reddit` Platform 的成功页，也不为 Reddit fetch 跨平台或跨能力静默兜底。[F-platform]。

## 映射到 forager 接入契约：提案

以下均为**计划或待定方案**，不是当前行为；事实依据是[第 7 章][F-platform]、ADR [0019][F-adr19]、[0020][F-adr20]、[0021][F-adr21]、[0022][F-adr22]、[0026][F-adr26]、[0027][F-adr27]。

### 平台、route、凭据与访问策略

平台 id 为 `reddit`；route id 不用裸平台名。发布初期统一建议 `default_order = []`，用户明确选择并配置后才启用；即使 HTTP route 日后可以像 Scholar 进入默认 order，也不在授权、价格、删除及 paid smoke 未验证前默认启用。process route 永不进入 default_order。[F-platform]、[F-adr20]、[F-adr21]。

| route 提案 | transport / 凭据 / 配置 | `access_policy` 建议值与依据 |
|---|---|---|
| `scrapecreators`（最优工程候选） | `Http`；`credentials_required: true`；`providers.scrapecreators.url=https://api.scrapecreators.com`、`keys`、`timeout=30s`；key 只在 `x-api-key` header | **1 秒/发送，并发 1**；供应商没有固定上限，这是 CLI 控费、串行续页的保守初值，非供应商额度；每次发送/重试/doctor 均 acquire。401/403 鉴权、402/明确 credits exhausted 配额、429 限流按真实响应验收，不盲目因源站 403 换 key [SC-rate]、[F-platform] |
| `reddit_browser`（MVP） | `OpenCli {site: forager-reddit, contract: forager-reddit/1}`；不设 `keys`（会话留 Chrome）；`command/timeout`；Unix 支持检查 | **10 秒/命令，并发 1**，命令不重试、permit 持到回收。依据小红书的保守实践与 ADR 0020；不是 Reddit 承诺的安全阈值，也不能逐个限制页面内请求 [F-adr20]、[F-platform] |
| `reddit_oauth`（仅有持久适用授权才考虑） | `Http`；需凭据；`url=https://oauth.reddit.com`、token URL 独立配置；client/device/token 模型尚待设计，不假装一串 keys 足够 | **1 秒/发送，并发 1**，即最多 60 QPM，低于 100 QPM；还须按响应头进一步等候。每 client 的服务端限额可能被其他程序共用，本机 limiter 不足以保证总体合规 [OFF-wiki] |
| `reddit_shreddit`（不建议首期发布） | `Http`；无 `keys`；`url=https://www.reddit.com`、`timeout`；不保存 Cookie，不伪装 browser UA | 若用户另行接受并有适用许可，建议 **2 秒/发送，并发 1**；来自本次受限探测节奏，不是稳定额度；429 不急重试，挑战终止 [E-http]、[OFF-user] |
| `arctic_shift`（历史路线候选） | `Http`、无 `keys`；单独 URL/timeout，不与实时 route 自动合并 | **2 秒/发送，并发 1**，动态配额可更严，遵守 reset。422 timeout 不按空集；全站 relevance 等明确不支持 [AS-api]、[E-arctic-keyword] |

主推的 `scrapecreators` 与 browser 也不自动互加 fallback；由用户 order 决定。若将来确有“实时主源 + 存档增强”需求，应像 HN 在**同一 route 内明确增强关系、端点配置和降级诊断**，但第 7 章当前举例是同平台官方端点，混入第三方存档还涉及来源与删除语义，不是本轮可直接套用的已批准设计。[F-adr27]、[F-platform]。

### Platform Ref、kind 与 URL

**首期采用 `kind=post`，不把 comment 当 post。** 官方 fullname 给 post/comment 独立 `t3_`/`t1_` 类型，fetch 的 root 和父子关系不同，后续支持独立评论时应增加 `kind=comment`，不以 root/reply/quoted 等对话角色拆 kind。[OFF-api]、[OFF-devvit]、[F-platform]。

| 输入/输出提案 | 规则及往返 |
|---|---|
| post ref | 规范字符串 **`reddit:t3_<id36>`**；接受 `reddit:<id36>` 作为无歧义 post 简写，`t3_` 只去一次；输出统一保留 fullname。base36 `[0-9a-z]+`、不转 u64、不从 id 推算时间；大小写归一，拒绝空 id、前导零的非规范形式和其他 fullname 类型。大小/长度上限应依据真实样本而非臆造固定 7 位 |
| post canonical URL | **`https://www.reddit.com/comments/<id36>/`**，无 subreddit、slug、tracking；解析它直接回到同一 post ref，无 IO。样本 `1wubcvf` 的 HTTP 目标遇挑战，只影响可读性，不影响身份往返；browser 另有 comment 样本证明同类路径可导航 [E-redd-target]、[E-browser-comment] |
| 完整帖子 URL | 严格主机 allowlist `reddit.com`、`www.reddit.com`、`old.reddit.com`、`new.reddit.com`、`np.reddit.com`、`m.reddit.com`，http/https，拒绝凭据/相似域/额外可疑路径；识别 `/r/{sub}/comments/{post}[/{slug}]/` 和 `/comments/{post}/`；尾 `/`、query/fragment 不参与 post 身份。host 接受是离线语法支持，不承诺该 host 当日可访问 |
| comment permalink | `/r/{sub}/comments/{post}/{slug}/{comment}/` 或 `/comments/{post}/_/{comment}/` 必须识别为 comment；首期尚不支持该 kind 则飞行前退 2，提示改取根帖，不静默丢掉 comment id |
| comment 的未来方案 | 单独 `reddit:t1_<comment>` 无法离线得到所属 post，不能用它凭空生成完整网页 canonical。推荐延后首期；若必须支持，候选 ref **`reddit:t1_<comment>@<post>`**，类型中同时保留 `comment_id` 和 routing `post_id`，canonical 为 `https://www.reddit.com/comments/<post>/_/<comment>/`，纯解析可往返；服务端核对 comment 的 `link_id` 等于 post，不同为 attempt Parameter。这个上下文携带语法尚需用户接受，不能悄悄把 t1 当 t3 [OFF-api]、[E-browser-comment] |
| `redd.it/{id36}` | 本次 301 样本直接对应 `/comments/{id36}`，**可提议离线等价映射为 post**，不需要先联网展开。如果选择保守地把所有短域都归为需解析短链，则飞行前退 2；推荐只接受严格的单段 ID，不接受任意 redd.it 路径。这是局部实测支持的提案，非官方长期保证 [E-http] |
| `/r/{sub}/s/{code}`、`/s/{code}`、未知短链 | opaque 分享码须联网才能知 post 还是 comment；**飞行前退 2，不发网络**，提示用户提供展开后的 permalink。分享码区分大小写，不尝试 base36 解码；本次样本确实落到 comment [E-share] |
| 其他 URL | 首页、社区页、用户页、media CDN、outbound link、未定义路径飞行前退 2。`?comment=<id>` 若出现，必须处理为 comment 或拒绝，不能当普通 tracking 丢弃；待核验前采用拒绝 |

public 文档以 ID36 描述身份，但没有承诺它永远单调或可由 ID推算日期。因此只把它当不透明 base36 标识，不借用 X snowflake 或 HN ID 排序。comment 独立 fetch 的最终语法、是否先仅接受带 post context 的 URL，是实施前的明确决策项。[OFF-api]、[F-adr26]。

### depth、条目与正文交付

| depth | 含义提案 | 交付与首期范围 |
|---|---|---|
| `metadata` | title、author（删除账号为 null/空作者集合）、发布时间、subreddit、fullname、permalink、score、num_comments、`is_self/over_18/spoiler/locked`、外链/媒体 URL（有才填）；没有帖子正文 | 内联；MVP fetch 默认值。score 未知用 null，不用 0；隐藏/模糊/存档计数不得当实时精确点赞 |
| `snippet` | 搜索页展示的 selftext excerpt/评论片段；没有内容则 metadata | search 内联；不冒充 `abstract`，不补造正文。browser MVP 只 metadata，后续确认可辨识 excerpt 后再开放 |
| `full_text` | **Reddit 条目自身的可见原文**：post 的 selftext/caption；以后 comment 为其 body。不自动下载图片视频、不 OCR，也不自动读取 post 外链文章 | route 核对身份后声明 `Native(Markdown)`，`native_full_text=true`，按 ADR 0022 写 `reddit-t3_<id>.md`、stdout 给 `content_*`；缺失/删除正文为 Quality，不能只拿标题凑全文。纯链接帖读外链是显式 Web Fetch，保持平台原文含义清楚 |
| `thread` | root post（将来可为聚焦 comment）加本次实际读到、保持父子结构的评论，附来源、排序、统计、截断/未展开节点说明 | 参照 HN 返回 Native 讨论串文件，stdout 保持 `depth: thread`、`comments_delivered`、`order`，并提议 `has_more/truncated/stop_reason`。初始有界读取，不承诺“整棵树”，MVP 明确不支持 |

`published` 用 `created_utc` 或页面明确的绝对时间转 UTC 秒，只有相对时间时注明精度/保留 `published_text`，不能由 base36 推算。平台字段可含 `post_type`、`subreddit`、`score`、`comments`、`link_url`、`parent_ref`、`post_ref`、`retrieved_at`；存档路线另标 `archived_at/counts_as_of`。命名最终应沿用仓库现有同义字段，本文不提前锁成 schema。[F-platform]、[F-adr22]、[F-adr27]、[AS-data]。

推荐完整方案的 thread 初始预算提案为**根请求加最多两次续页/展开、并发 1、至多 100 条交付评论**，同时服从命令 deadline 与现有 4 MiB 上限。预算是产品取舍而非上游额度；评论树有 more、缺页或预算耗尽时明确部分交付，不输出 complete=true。删除/移除节点不恢复文本，可保留用于父子定位的无正文占位；广告/推荐帖不得混入 root 或评论统计。先验收 cursor/父子关系，再决定是否把 thread 设为完整方案的默认；首期一律 metadata。[OFF-api]、[SC-comments]、[F-platform]。

### L0/L1 映射与不支持检查

L0 保持必填 query、`--limit`；fetch 为 ref/URL、`--depth`。推荐首期 limit 1–7（browser 一屏），HTTP 完整方案 1–100 但必须以供应商实测页容量/分页能力确定最终上限；实际一页可能不足 limit，不自动多页填满而偷偷增加费用。[F-platform]、[E-browser-structure]、[SC-search]。

| L1 选项提案 | 官方 OAuth（条件路线） | ScrapeCreators | browser / 匿名 Shreddit 候选 |
|---|---|---|---|
| `--subreddit SUB`（单值） | 路径 `/r/SUB/search` + `restrict_sr=true` | 改走 `/subreddit/search`，传 `subreddit` | 正常 `/r/SUB/search/` 页面；MVP 先验证这一条后开放。多 sub 聚合改变排序/cursor，首期不做 |
| `--sort relevance\|hot\|top\|new\|comments` | 同名映射 | 全站 comments→`comment_count`，hot 不支持；社区文档有同名五项，需验收 | 通过正常 URL/页面筛选触发，逐项验收才开放；MVP 只允许默认 relevance |
| `--time hour\|day\|week\|month\|year\|all` | `t` | 全站 timeframe 无 hour，社区有 hour；comments 的时间语义未核实则拒绝 | `t`/页面控件；MVP 只默认 all，不能把支持 month 的单样本当全部枚举已跑通 |
| `--author USER` | 可考虑编译为查询 `author:USER`，但须先核实 Reddit 搜索语法及与原 q 的组合 | 当前文档未给独立 author 参数；不能静默 post-filter 凑近似 | 尚无独立 author 实测；**首期不暴露该 flag**，若后续加入需验证查询组合语义 |
| `--type post\|comment` | 原 search 只支持 post，comment 拒绝 | 全站 `filter` 已有 posts/comments，但独立 comment fetch/canonical 还没定；社区显式 selector 未文档化 | 首期只 post；Comment tab 是后续独立能力，不凭 DOM 有这个 tab 就宣布支持 |
| 精确 `--from/--to`、min-score 等 | 本轮没有对应已核验服务端契约 | 本轮未核实 | 不提供“本地过滤就算支持”；若以后提供，须有明确语义和 route 映射 |
| comment 排序/展开（fetch） | `/comments` 的 confidence/top/new 等，与 search sort 分开 | comments 接口未给统一 sort 参数 | 后续正常控件操作；MVP 不暴露，避免 search/fetch 共用一枚语义不同的 sort |

原始 query 按平台已记录的语法传递，不提供任意 API 参数透传；typed option 合并必须防止 query 中条件互相冲突，不能声称“用户 query 含 author 就等于 typed author 已支持”。明确选项与 cursor 的支持检查均为零 IO：部分 route 不支持记 Skipped、全部不支持退 2，绝不先发请求再忽略筛选。[F-platform]。

### cursor、L2、doctor 与 smoke

**cursor 提案**：沿用 `v1.<route>.<payload>`，payload 恢复 query、全部 L1、limit、上游 endpoint 模式及下一页位置。OAuth/global ScrapeCreators 的 `after` 与社区 ScrapeCreators/Shreddit 的 opaque cursor 是不同变体，不混用。它只绑定原 route，route 不在 order/未配置/不支持时飞行前失败，cursor 请求不 fallback，不含 key、Cookie、token、临时 HTML 或任意可执行上游 URL。用户不得同时显式给 query/L1/limit。[F-platform]、[SC-search]、[SC-subsearch]、[E-content]。

当 upstream 返回大于 limit 的一页，不能截断后直接用页末 cursor 跳过未交付条目：应验证能按最后交付 id 续页，或把 limit 约束为实际可消费页、明确不签发 cursor。不同排序的结果会随时间变化，不承诺无重复快照。browser MVP 无独立续页令牌；后续若页面自己的 lazy cursor 不能在新命令稳定恢复，则一直 `next_cursor:null` 并诊断，不保存浏览器 tab/session handle 当持久 cursor。[F-platform]、[E-browser-after]。

**候选 L2：** `platform reddit posts SUB [hot|new|top] [--time ...] [--limit ...]`，不需要查询词，改变必需输入，类比 HN stories。用官方 `/r/SUB/{sort}`、SC `/subreddit` 或正常网页，保持各来源原排序并输出 1-based rank；首期不做个性化 best/home，也不把 archive date 排序冒充 hot/top。另一个候选是 `comments POST --limit --replies` 有界读取：只在用户确需独立分页时引入，与 fetch thread 分工，避免同时交付两个等价操作。单 route 时 inherent 方法及 factory route 集合；第二实现才提升 trait。[F-platform]、[F-adr27]、[SC-listing]。

| 探针/验收项 | 推荐行为 | 计费与限制 |
|---|---|---|
| browser shallow doctor | 仅 order 启用后检查 command/bridge/`forager-reddit/1`；非 Unix 支持检查拒绝 | 禁用不启动浏览器，不读取会话私密数据 |
| browser deep doctor | 单次 `search rust --limit 1`，检查结果 fullname/permalink 与结构，不以 200 或网页 title 作为成功 | 一条命令、不重试；挑战 Auth；明确空集须有空集信号 |
| SC doctor | `DoctorProbe::ServiceAccount` 候选 `/account/credit-balance` 逐 key；确认费用后才能决定是否 deep 默认执行；替代 `PlatformSearch` 有搜索费用 | 文档标 1 credit；shallow 无 key 可达性/401 只能证明网络，不等于凭据健康 [SC-account] |
| OAuth doctor | 若路线被选择，取 token 后单次最小 read 检查，校验限额响应头；不要以匿名 oauth URL 200 代替有效授权 | 先明确 token 预算和审批；不滥用 pool 换 client |
| search smoke 候选 | `rust --limit 3`；输出身份唯一、URL 往返、深度准确；参数边界用本地 fixture | browser/SC 仅启用并具备条件时运行，不在本轮执行 |
| metadata fetch canary | `reddit:t3_17snc7d`，r/running 的 2023-11-11 日常帖；另可用官方迁移帖 `t3_1wubcvf` 检查近期结构 | 旧帖正文/评论在 browser 可见，但 canary 根 metadata 和各 route 均须验收；本次非“所有 route 已通过” |
| thread/comment canary | `17snc7d` 中 `t1_k8so54m`（分享样例、browser 已定位）；若增加 kind，验证 composite ref 与 canonical 往返；thread 文件写临时目录 | 不断言固定评论数/分数；续页需真的出现额外唯一 ID 才记覆盖，否则 not verified/skipped |
| L2 canary | r/rust new，limit 3，rank 单调且身份有效 | 动态页面不固定 title/count；只在实现该 L2 后登记 |

### 输出、错误矩阵与 R1–R8

search 返回 `{platform, provider, items, next_cursor}`，每条有 `ref/url/depth/title/authors/published` 和平台字段；fetch metadata 内联，Native 正文/讨论串按文件引用交付；`--verbose` 有真实 attempts；不写 Search Result Journal。即使通用 Web Search 能找到同一个 URL，也不替平台 route 伪造成功。[F-platform]。

| 情形 | 阶段与建议结果 |
|---|---|
| malformed ref/URL、需联网分享链、cursor 冲突、未支持 kind/depth/L1 | 飞行前退 **2**，不发请求 |
| order=[]、全缺凭据、缺可用操作 route、缺 OpenCLI adapter 的配置条件 | 配置类飞行前退 **3**，消息点名 order/keys/安装步骤；安装/启动后的运行错误按 process 契约判定，不全塞进配置错误 |
| 请求后 404/条目不存在/目标 fullname 与结果不符 | attempt 级 Parameter，退 **4**；不能改为飞行前 2 |
| 401、源站拒绝/挑战、429、网络/超时、结构漂移、文件写失败 | 按 Auth/RateLimited/Network/Timeout/Runtime 分类，退 **4**；429 不伪装合法空集，也不尝试规避挑战 |
| metadata 成功但请求的原生正文不可取得 | 同一 route attempt Quality，退 **5**；不把 snippet/标题当全文 |
| 真正的零结果 search | 明确上游空集结构，`items:[]`，退 **0**；“解析到 0 条”本身不是空集证据 |

推荐后续实施逐项使用下面的契约清单，本轮不修改这些文件：

| 登记 | 需要完成的映射 |
|---|---|
| **R1** | catalog `PLATFORMS` 加 `reddit`，search/fetch route 集合非空，明确 `default_order=[]`；`Platform` 及 `platform_reddit` 类型叶子拥有 ref、选项、条目 |
| **R2** | 每 route 独立 `ProviderId/ProviderRegistration`、transport、credentials_required、access_policy；不用裸 `reddit` provider |
| **R3** | factory 的 search/fetch support、runtime/route config 与 build 分支；process 的非 Unix 拒绝；route 只负责请求/读取、解码、核对与正文来源声明 |
| **R4** | `platforms.reddit.order`；HTTP url/keys/timeout 或 process command/timeout；OAuth 若做必须先补足凭据契约设计，不绕过 schema |
| **R5** | 登记适用 `PlatformSearch` 或 `ServiceAccount`，明确哪些 probe 发真实请求/计费，默认禁用 browser 不跑 |
| **R6** | search/fetch 各有 smoke；更新 `SPECIFICATION_CASE_IDS` 与第 5 章，编号实施时分配，不在研究稿占用 |
| **R7** | 每 route/operation 的 transport fixture 和 `acceptance-manifest.json`，新 L2 同登记；HTML fixtures 覆盖真实结构、明确空集、200 challenge、403、429、删除/缺失、广告混入及部分树 |
| **R8** | post 及未来 comment kind 的 ref/canonical 往返样例，严格 host/path、t1/t3、大小写、分享码和 query comment 边界 |

未来代码归属应沿既有架构：`infra/types/platform_reddit` 负责纯类型/身份，`capabilities/providers` 的各 route adapter 负责 IO 与解码，browser 页面读取放 `skills/forager/opencli/forager-reddit`，CLI 静态选项放平台 CLI，编排复用 core 平台链，不在 types 做网络、不让 provider 互相 import。实现时同时更新 GLOSSARY、第 2–5/7 章、架构模块登记、skill 平台词表/reference/CLI reference；本稿只列工作清单，不提前写 spec/ADR。[架构][F-architecture]、[F-platform]、[F-opencli]。

## 实施前待决事项与待验证项

**需要用户选择：**

1. 用途是个人阅读、企业产品、学术研究还是面向 Agent 的自动分析；是否需要向 Reddit 申请用途许可/合同或 RFR。没有确认前不能声称推荐路线已获授权。[OFF-responsible]
2. 优先本机免费 browser MVP，还是接受自备 ScrapeCreators key/费用、先做 HTTP MVP；是否接受该供应商无源站授权证明与无连续性保证。建议 paid 路线先用最小免费额度验收，不默认开启 backfill 或 auto-recharge。[SC-terms]
3. comment 独立身份首期是否必需。建议首期仅 post，评论放后续 thread；若必需则明确接受带 post context 的 ref，或先提出别的可离线往返 canonical 设计。
4. 是否接受有界、明确部分交付的 thread；最大评论数/请求数/时限、排序和文件保存/删除责任如何设定。建议 root + 2 页、100 条作为讨论起点，metadata 仍是 MVP 默认。
5. 是否确有历史 archive 需求。建议独立 opt-in，不能默认用 Arctic Shift 补实时分数、恢复删除正文或代替 native sort。

**建议实施前按顺序进行的低流量验证：**

1. 自有 browser 原型只读 search/post 各一个：核对 root fullname、作者/绝对时间/计数，确认不同卡片变体、广告与明确空集；在用户自选会话中对比登录/未登录，不替用户登录。
2. 验证页面自身触发的第二页与筛选：本轮一次到底没有发出续页，需先确认触发条件和结果/游标绑定；未跑通就维持无 cursor，不主动 fetch 未由页面触发的 API。
3. ScrapeCreators 带 key 最小验收：全站一页、社区一页、一个 metadata fetch；确认 `url` 可接受无 slug canonical、filter/返回形状、真正空集和 404。再决定是否开放 comment search/hour/hot 等差异项。未核实前纯支持检查拒绝。
4. 只有完整方案需要时再各测一页续页与评论展开，验证没有跳页、重复、父子关系丢失或后台多请求；记录真实每次 credits 消耗，账户 probe 是否收费一并核实。
5. 向供应商询问 Reddit 授权/再许可、删除同步、日志/响应保留、源站关闭后的计划；公开文档目前没有足够证明。独立复核 public API/RSS 时间表是否更新。
6. 若坚持官方 OAuth，先确认获批资格和 2027 后适用路径，再设计 client/device/token 配置；不要先实现即将退役的接口、再赌审批或迁移豁免。

已知缺口：没有新 app/OAuth 实测、付费 API 实测、全新匿名浏览器对照、可用的第二页、完整评论树或长期风控测量；没有找到所有第三方的源站授权证明、完整覆盖率及删除 SLA。没有用二手文章补齐这些未知项。以上缺口分别限制准入可用性、费用/字段契约、会话依赖、分页/完整性及长期维护判断，不妨碍本轮比较路线与确定下一步验收边界。

## 来源与证据索引

网页事实以 2026-10-17 取回的原文为准；参考实现以固定提交为准。本地 `.json` 通常包含 URL、命令/方法、退出状态与正文；`.txt` 是同一 JSON 中正文的可读投影。第三方 discovery 和模型答案只用于定位来源，未当作独立已核实事实。

[OFF-wiki]: https://support.reddithelp.com/hc/en-us/articles/16160319875092-Reddit-Data-API-Wiki
[OFF-access]: https://support.reddithelp.com/hc/en-us/articles/14945211791892-Developer-Platform-Accessing-Reddit-Data
[OFF-responsible]: https://support.reddithelp.com/hc/en-us/articles/42728983564564-Responsible-Builder-Policy
[OFF-data]: https://redditinc.com/policies/data-api-terms
[OFF-developer]: https://redditinc.com/policies/developer-terms
[OFF-user]: https://redditinc.com/policies/user-agreement
[OFF-api]: https://www.reddit.com/dev/api/
[OFF-oauth]: https://github.com/reddit-archive/reddit/wiki/OAuth2
[OFF-script]: https://github.com/reddit-archive/reddit/wiki/OAuth2-Quick-Start-Example
[OFF-devvit]: https://developers.reddit.com/docs/capabilities/server/reddit-api
[OFF-migration]: https://www.reddit.com/r/redditdev/comments/1wubcvf/moving_data_api_apps_to_the_developer_platform/
[OFF-rss]: https://www.reddit.com/r/modnews/comments/1x6rjnd/mod_monthly_your_october_2026_newsletter_is_here/
[SC-index]: https://docs.scrapecreators.com/llms.txt
[SC-search]: https://docs.scrapecreators.com/v1/reddit/search
[SC-subsearch]: https://docs.scrapecreators.com/v1/reddit/subreddit/search
[SC-comments]: https://docs.scrapecreators.com/v1/reddit/post/comments
[SC-listing]: https://docs.scrapecreators.com/v1/reddit/subreddit
[SC-home]: https://scrapecreators.com/
[SC-terms]: https://scrapecreators.com/terms
[SC-rate]: https://scrapecreators.com/no-rate-limits
[SC-account]: https://docs.scrapecreators.com/v1/account/credit-balance
[AS-api]: https://github.com/ArthurHeitmann/arctic_shift/blob/master/api/README.md
[AS-readme]: https://github.com/ArthurHeitmann/arctic_shift/blob/master/README.md
[AS-data]: https://github.com/ArthurHeitmann/arctic_shift/blob/master/file_content_explanations.md
[PP-doc]: https://pullpush.io/
[F-platform]: ../spec/forager/07-platforms.md
[F-architecture]: ../spec/forager/04-architecture.md
[F-opencli]: ../agents/opencli.md
[F-xhs-design]: ../design/2026-10-08-xiaohongshu-opencli-route.md
[F-adr19]: ../adr/0019-platforms-as-a-dimension-separate-from-capability-seams.md
[F-adr20]: ../adr/0020-opt-in-browser-routes-paced-per-operation.md
[F-adr21]: ../adr/0021-scholar-only-through-a-user-keyed-serp-api.md
[F-adr22]: ../adr/0022-platform-native-full-text.md
[F-adr26]: ../adr/0026-x-platform-through-xai-x-search-with-relayed-content.md
[F-adr27]: ../adr/0027-hn-platform-combines-algolia-search-with-the-official-firebase-api.md
[L-keyless]: https://github.com/mvanhorn/last30days-skill/blob/0341304f8ecb307ca0a939e48ecd002761ef23b2/skills/last30days/scripts/lib/reddit_keyless.py#L113-L194
[L-search]: https://github.com/mvanhorn/last30days-skill/blob/0341304f8ecb307ca0a939e48ecd002761ef23b2/skills/last30days/scripts/lib/reddit_search.py#L81-L160
[L-listing]: https://github.com/mvanhorn/last30days-skill/blob/0341304f8ecb307ca0a939e48ecd002761ef23b2/skills/last30days/scripts/lib/reddit_listing.py#L130-L185
[L-shreddit]: https://github.com/mvanhorn/last30days-skill/blob/0341304f8ecb307ca0a939e48ecd002761ef23b2/skills/last30days/scripts/lib/reddit_shreddit.py#L1-L42
[L-arctic]: https://github.com/mvanhorn/last30days-skill/blob/0341304f8ecb307ca0a939e48ecd002761ef23b2/skills/last30days/scripts/lib/reddit_arctic.py#L1-L108
[L-http]: https://github.com/mvanhorn/last30days-skill/blob/0341304f8ecb307ca0a939e48ecd002761ef23b2/skills/last30days/scripts/lib/http.py#L1136-L1478
[L-public]: https://github.com/mvanhorn/last30days-skill/blob/0341304f8ecb307ca0a939e48ecd002761ef23b2/skills/last30days/scripts/lib/reddit_public.py#L1-L18
[L-paid]: https://github.com/mvanhorn/last30days-skill/blob/0341304f8ecb307ca0a939e48ecd002761ef23b2/skills/last30days/scripts/lib/reddit.py#L322-L440
[L-config]: https://github.com/mvanhorn/last30days-skill/blob/0341304f8ecb307ca0a939e48ecd002761ef23b2/CONFIGURATION.md#L606-L607
[L-changelog]: https://github.com/mvanhorn/last30days-skill/blob/0341304f8ecb307ca0a939e48ecd002761ef23b2/CHANGELOG.md
[L-how]: https://github.com/mvanhorn/last30days-skill/blob/0341304f8ecb307ca0a939e48ecd002761ef23b2/docs/how-search-works.md#L30-L75
[L-skill]: https://github.com/mvanhorn/last30days-skill/blob/0341304f8ecb307ca0a939e48ecd002761ef23b2/skills/last30days/SKILL.md#L232
[E-dir]: reddit-2026-10-17/
[E-manifest]: reddit-2026-10-17/source-manifest.json
[E-env]: reddit-2026-10-17/environment.json
[E-http]: reddit-2026-10-17/anonymous-summary.json
[E-content]: reddit-2026-10-17/anonymous-content-validation.json
[E-probe]: reddit-2026-10-17/probe-anonymous.py
[E-redd-target]: reddit-2026-10-17/redd-it-target.json
[E-share]: reddit-2026-10-17/share-honest.json
[E-share-target]: reddit-2026-10-17/share-target.json
[E-robots-www]: reddit-2026-10-17/www.reddit.com-robots.json
[E-robots-old]: reddit-2026-10-17/old.reddit.com-robots.json
[E-announcements]: reddit-2026-10-17/official-announcements-web.json
[E-rss]: reddit-2026-10-17/official-rss-announcement-web.json
[E-orientation]: reddit-2026-10-17/orientation.json
[E-third-discovery]: reddit-2026-10-17/third-party-discovery.json
[E-last-source]: reddit-2026-10-17/last30days-source.json
[E-last-audit]: reddit-2026-10-17/last30days-history-audit.json
[E-parser]: reddit-2026-10-17/reference-parser-compatibility.json
[E-oc-source]: reddit-2026-10-17/opencli-source.json
[E-oc-help]: reddit-2026-10-17/opencli-reddit-help.json
[E-oc-doctor]: reddit-2026-10-17/opencli-doctor.json
[E-browser-open]: reddit-2026-10-17/browser-open.json
[E-browser]: reddit-2026-10-17/browser-observation.json
[E-browser-structure]: reddit-2026-10-17/browser-search-structure.json
[E-browser-network]: reddit-2026-10-17/browser-network.json
[E-browser-scroll]: reddit-2026-10-17/browser-scroll-bottom.json
[E-browser-after]: reddit-2026-10-17/browser-network-after-scroll.json
[E-browser-comment]: reddit-2026-10-17/browser-comment-observation.json
[E-browser-comment-network]: reddit-2026-10-17/browser-comment-network.json
[E-browser-close]: reddit-2026-10-17/browser-close.json
[E-sc-home]: reddit-2026-10-17/sc-home.json
[E-arctic]: reddit-2026-10-17/arctic-live.json
[E-arctic-keyword]: reddit-2026-10-17/arctic-keyword-live.json
[E-pullpush]: reddit-2026-10-17/pullpush-live.json
[E-pullpush-topic]: reddit-2026-10-17/pullpush-topic-live.json
[E-position]: reddit-2026-10-17/reddit-archive-position-discovery.json
