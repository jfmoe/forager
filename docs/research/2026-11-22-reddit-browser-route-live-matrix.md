# Reddit 浏览器 route 的实测矩阵

实测时间：**2026-11-21 20:49 至 2026-11-22 01:20**（北京时间，下同；全部在 UTC 2026-11-21 之内）。宿主为 macOS arm64，浏览器为 Managed Browser fingerprint-chromium `148.0.7778.215`。出口经本机代理，观测到的是 AS906 DMIT Cloud Services（洛杉矶）的机房 IP。这是维护者唯一可用的出口。

本文记录 `reddit` 平台从匿名 HTTP route `reddit_web` 改为登录浏览器 route `reddit_browser` 所依据的实测：同一出口上匿名与登录的对照、可行性原型对搜索页与帖子页的观测、四次页面内主动查询的结果，以及 forager 自己的 route 在真实站点上的运行结果。决定见 ADR 0037，规格见第 7 章「Reddit」。

文中「事实」指存档的命令输出、落盘页面或抓包可以复核的内容；「推断」是基于事实的判断；「未验证」是本次没有覆盖的问题。原始存档不入库：落盘页面与抓包含账号信息、页面令牌与其他用户的内容。本文不出现账号信息、令牌、cookie、真实用户名或帖子正文，出口只写到 ASN。

## 结论

- **这个出口拒绝匿名访问，登录后可读（事实）。** 匿名 HTTP 全部 403，匿名浏览器得到 reCAPTCHA 验证页；已登录的浏览器打开搜索页与帖子页都正常。
- **页面渲染加原生滚动覆盖了必需字段（事实）。** 在原型的样本上，search 与 fetch 现有输出的每个字段都能从页面 DOM 中读到，没有一个字段必须依赖主动查询端点。
- **主动查询不可靠，也不必要（事实）。** 翻页片段的 cursor 在服务端有状态，同一地址重取得到的是再下一页。
- **全部已登录的页面打开都没有出现风控信号（事实）。** 没有验证页、4xx 或限流。站点在后台运行无感的人机评分，分数不可见。
- **证据量很小（推断）。** 一个账号、一个出口、一个晚上，约 25 次已登录的页面打开，不能说明持续使用下的情况。

## 出口

| 时间 | 观测 | 来源 |
| --- | --- | --- |
| 2026-10-17 | 同一个出口 IP（经 Cloudflare trace）；当天从这台机器匿名请求 `/svc/shreddit` 片段返回 200 | `docs/research/youtube-2026-10-17/egress-trace.json`、`docs/research/2026-10-17-reddit-integration.md` |
| 2026-10-28 | `reddit_web` 首发时的 live 验证通过；提交正文没有记录出口 | 首发提交 `e424689` |
| 2026-11-21 20:49 | 出口为 AS906 DMIT Cloud Services，与 10-17 是同一个 IP | `ipinfo.io` |

推断：首发时可用、之后 Reddit 对这个 IP 或 ASN 的策略变了，可能性最大。但本机代理可能按域名分流，无法确认 reddit.com 在这几个时点走的是同一个节点；10-28 到 11-21 之间也没有观测，变化发生的时间未知。第三方项目 last30days-skill 的变更记录（2026-11-15）同样提到 Reddit 站内搜索开始返回 403。

## 匿名与登录的对照（2026-11-21 20:49–22:40）

| 访问方式 | 次数 | 结果 |
| --- | --- | --- |
| `reddit_web` 的 search 与 fetch | 4 次命令 | 全部 HTTP 403，Auth，退 4。页面是 Reddit 的 “blocked due to a network policy” |
| curl：首页、`/svc/shreddit/search/` 片段、embed 页 | 各 1 次 | 全部 403。片段请求走的是 HTTP/2，排除了 ALPN 的原因 |
| curl：匿名 `search.json` 与 `/comments/<id>.json` | 4 次（两种 UA） | 全部 403，“You've been blocked by network security.” |
| 匿名 Managed Browser 打开搜索页 | 2 次（裸跑浏览器 1 次，经 forager 传输 1 次） | 文档状态 200，title 为 `Reddit - Prove your humanity`，页面是 Google reCAPTCHA 表单，没有搜索结果，也没有 `js_challenge` 字段 |
| 已登录的 Managed Browser（原型） | 9 次页面打开 | 全部是结果页或帖子页；reddit.com 的请求全部 2xx；没有验证页 |

匿名验证页的表单把 reCAPTCHA 的结果 POST 回原 URL，并附加 `captcha=1`，属于站点边缘层的验证。通过后会下发什么放行凭证、有效多久、是否绑定出口或指纹，都未验证。

## 可行性原型（2026-11-21 22:32–22:40）

原型在 forager 的浏览器传输上运行一个临时探针（分支 `spike/reddit-browser`，不合入），用户事先用 `forager browser login reddit` 亲自登录。9 次页面打开之间至少间隔 10 秒，页面内的 4 次同源 GET 之间也至少间隔 10 秒。

### 搜索页

- **字段（事实）**：现有输出的全部字段都在页面 DOM 中：帖子 ID、标题、作者、发布时间、版块、permalink、分数、评论数、NSFW 与 spoiler 标志、搜索摘录。`post_type` 与 `link_url` 在搜索结果中没有，与 `reddit_web` 相同。
- **URL 直接表达选项（事实）**：全站 `sort=top&t=year` 返回高分的跨版块结果；`/r/<sub>/search/?q=…&restrict_sr=1&sort=new` 只返回该版块的帖子，按时间倒序。
- **翻页（事实）**：首屏 7 条。原生滚轮滚动 2 次，页面自己请求了 2 个下一页片段，DOM 中的结果从 7 条增到 21 条。两次独立打开得到的第 2 页完全相同。

### 帖子页

- **第一批评论随文档返回（事实）**：一个 94 条评论的帖子，DOM 中有 1 个帖子元素和 94 个评论元素，页面没有请求任何评论端点。帖子的元数据在帖子元素的属性中（标题、作者、分数、评论数、创建时间、帖子类型、permalink、外链），正文 HTML 在 `#t3_<id>-post-rtjson-content`，现有的正文转 Markdown 函数可以直接处理。
- **大讨论串（事实）**：一个 1369 条评论的帖子，首屏 DOM 有 100 条评论。顶层的下一批是滚到底部才由页面自己 POST 加载的片段；楼中楼的“更多回复”需要点击。滚动 18 次到底部后，页面自己发出一次 more-comments POST，评论增加到 110 条。这个帖子的文档有 4 066 580 字节，达到被动抓包单个响应体 4 MiB 上限的 97%。
- **属性名与默认排序（事实）**：评论树总数的属性名是 `total-comments`（2026-10 的匿名片段中是 `totalcomments`）。页面默认的评论排序是 `CONFIDENCE`（Best）。
- **不可见的帖子（事实）**：一个凭记忆写下的帖子 ID 打开后跳到一个 NSFW 版块的帖子，页面上没有帖子元素和评论元素，账号不显示成人内容。帖子页的 screen-view 记录中，这个帖子的成人标志为 true，其他样本为 false。
- **副作用（事实）**：这次打开留在了账号的浏览记录里。之后的实测只从搜索结果中取帖子 ID，不打开 NSFW 内容。

### 四次页面内主动查询

都是页面内的同源 GET，各 1 次，带登录 cookie。

| # | 请求 | 结果 | 判断 |
| --- | --- | --- | --- |
| G1 | 页面请求过的翻页片段 URL，用普通 `fetch`，不带页面的请求头 | 200，但返回整页 HTML，7 条结果与页面拿到的第 2 页不同 | 不带片段的 `accept` 头时，服务端按整页渲染 |
| G2 | 同一翻页片段，带页面原样的请求头重放 | 200，形状与页面拿到的片段相同，但内容是再下一页 | 翻页 cursor 在服务端有状态，重取不幂等 |
| G3 | `reddit_web` 用的评论树片段 `/svc/shreddit/comments/r/<sub>/t3_<id>?sort=top`（页面自己没有请求它） | 200，整页 HTML，带完整评论树，没有帖子正文 | 能用，但页面不会请求它，形状也与 10 月的匿名片段不同 |
| G4 | 旧 API `/comments/<id>.json?limit=5&depth=1&raw_json=1` | 200 JSON，帖子的完整结构化字段加 5 条顶层评论 | 登录态下可用；匿名时是 403 |

4 次 GET 之后，同一会话里后续的页面打开都正常。“没有触发验证”只基于这 4 次样本。

### 风控信号与无头特征

- **没有可见的信号（事实）**：没有验证页、4xx 或限流。
- **后台评分（事实与推断）**：已登录的每个页面都有一个 reCAPTCHA iframe，页面自己请求生成 captcha 令牌。这是站点在后台运行的无感评分（推断），分数不可见；分数多低会升级为可见的验证，未知。
- **无头特征（事实）**：`navigator.webdriver` 为 false；UA 为 `Chrome/148.0.0.0`；`outerHeight` 比 `innerHeight` 恒大 121，与 ADR 0036 记录的读数相同；`document.hasFocus()` 在 9 次中 5 次为 true、4 次为 false。

## forager 自己的 route（2026-11-22 00:01–00:47）

命令为 `./target/debug/forager`，只对本次命令用环境变量 `FORAGER_PLATFORMS__REDDIT__ORDER` 启用 route，用户配置文件没有改动。

### search（6 次页面打开）

| 用例 | 结果 |
| --- | --- |
| 全站，`relevance`，`--limit 5` | 5 条，首屏 7 条，没有滚动，页面有下一页 |
| 版块内，`--sort top --time month --limit 5` | 5 条，全部属于该版块，都发布于近一个月内，按分数降序；页面没有下一页，没有未列全诊断 |
| 全站，`--sort comments --limit 25` | 滚动 3 次：7→14（2.5 秒）→21（3.1 秒）→28，截到 25 条；按评论数大致降序 |
| 版块内，`--sort hot --limit 3` | 3 条，全部属于该版块 |
| 版块内带 `--time hour`，为了得到空集 | 第一次的查询仍有结果；换一个查询后得到 `items: []`，页面带无结果标记，退 0 |

每次滚动后新结果在 2.5–3.1 秒内到达，route 的 4 秒等待够用。

### fetch 与 smoke（7 次页面打开）

帖子 ID 都取自第 1 条命令的搜索结果，都不是 NSFW。

| 用例 | 结果 |
| --- | --- |
| 版块内 `--sort new --limit 25` | 25 条，均非 NSFW |
| 文本帖，`--depth metadata` | `post_type: text`，正文 Markdown 正常 |
| 同一帖子，`thread` | `order: top`，`comments_delivered` 18，`comments` 21，`has_more_comments: false` |
| 外链帖，`--depth full_text` | `post_type: link`，`link_url` 为外链文章；Web Fetch 读到外链正文 |
| 零评论帖，`thread` | `order: top`，`comments_delivered: 0`，`post_type: crosspost` |
| `smoke --live` 的 C50 与 C51 | 都通过，各 1 次 attempt |

观察（事实）：

- 导航时带 `sort=top` 生效，两次 thread 的页面评论树都报告 `TOP`，零评论的帖子也有排序元素。
- 真实页面上评论正文的容器 id 是 `<thingid>-comment-rtjson-content`，13 位的新式评论 ID 读取正常。
- 帖子类型出现了 `crosspost`，不在 forager 已知的类型中。

## 退役后的完整 live smoke（2026-11-22 01:10–01:20）

`reddit_web` 退役之后，用 `./target/debug/forager smoke --live --opt-in --timeout 1200` 以维护者的真实配置跑了一次完整的 live smoke，用时约 4 分钟：40 个通过，3 个失败，1 个 `unconfigured`。

| 用例 | 状态 | 原因 |
| --- | --- | --- |
| C50（Reddit search） | 通过 | 1 次 attempt |
| C51（Reddit fetch） | 通过（补跑） | 完整运行中，smoke 没有为它准备临时目录，命令在启动浏览器之前失败；修正 smoke 之后，用只启用 Reddit 的临时配置补跑，C50 与 C51 都通过，各 1 次 attempt |
| C41（YouTube ask）、C42（YouTube transcribe） | 失败 | 各 1 次 attempt 后停止，按 smoke 的重试规则是 Auth 或 attempt 级 Parameter；smoke 的消息不带具体原因。同一时段 `doctor --provider gemini_api` 通过（只列模型，不调用）。配置中的 `gemini_api` 经自建的网关访问。为了不额外调用付费模型，没有单独复现 |
| C49（openai 主搜索） | `unconfigured` | 维护者的配置中没有 `providers.openai.keys` |

本次对 reddit.com 共打开 3 次页面：完整运行中的 C50 一次，补跑中的 C50 与 C51 各一次。没有出现验证页、Auth 或超时。

## 未验证

- 长期使用、更高频率、其他账号与其他出口下的风控情况；站点后台评分的分数与升级阈值。
- `comments_delivered` 少于 `comments` 而页面没有更多标记的原因。推断是页面不渲染已删除或被移除的评论。
- 匿名浏览器加一次性人工验证能否复用：放行凭证、有效期与绑定条件。
- `reddit_web` 在住宅出口上是否仍然可用。
- 登录本身在其他出口上是否总能完成，不被验证页挡住。
- 帖子类型 `crosspost` 的 `full_text`：目前报 Quality，是否应读取被转帖的帖子，没有设计。
- 滚动加载更多顶层评论与点击展开回复，在 forager 的 route 中没有实现，也没有实测。原型只观察到页面自己发出的 POST 能被被动抓到。
