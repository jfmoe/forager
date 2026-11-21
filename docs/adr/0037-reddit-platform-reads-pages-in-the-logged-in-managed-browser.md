# Reddit 平台改为在登录的 Managed Browser 中读取页面

`reddit` 平台只剩一条 route：浏览器 route `reddit_browser`。它在 Reddit 专用的 Browser Profile 中，以用户亲自登录的 Reddit 账号无头打开站点的搜索页与帖子页，读取页面渲染出的内容。原来的匿名 HTTP route `reddit_web` 完整退役，不保留兼容层（沿用 ADR 0030、0031、0035）。本记录取代 ADR 0033，只保留其中启用 HTTP/2 的一条（见「取代范围」）。实测依据见 [Reddit 浏览器 route 的实测矩阵](../research/2026-11-22-reddit-browser-route-live-matrix.md)。

## 背景

2026-11-21，在维护者唯一可用的出口上（AS906 DMIT Cloud Services 的机房 IP，经本机代理）：

- `reddit_web` 的 search 与 fetch 全部得到 HTTP 403，页面是 Reddit 的 “blocked due to a network policy”。同一出口上 curl 请求首页、`/svc/shreddit` 片段、embed 页与匿名 `.json` 端点，也都是 403。整站拒绝这个出口，与路径和解析无关。
- 匿名的 Managed Browser 打开搜索页，文档状态为 200，但页面是 reCAPTCHA 验证页 “Prove your humanity”（2 次样本）。
- 用户登录过的 Managed Browser 正常打开了搜索页与帖子页：9 次页面打开，没有验证页，reddit.com 的请求全部是 2xx。

ADR 0033 已经预见机房 IP 更容易被拦截。`reddit_web` 首发时（2026-10-28）在同一个出口 IP 上可用，此后 24 天没有任何一层测试实际运行过它：它不在默认 order 中，而发版收尾的 `smoke --live` 使用维护者的真实配置，它的 live 用例因此每次都记为 `unconfigured`。发版流程已改为带 `--opt-in` 运行 live smoke（规格第 5 章）。

官方 public Data API 不再受理新的访问申请，并将按迁移公告的时间表关闭（ADR 0033 的背景）。Reddit User Agreement 与 robots.txt 都不允许自动访问。

## 决定

- **一条浏览器 route。** `reddit_browser` 的传输为 `Browser`，浏览器环境为 Reddit 的 Browser Profile、语言 `en-US`、需要登录。用户用 `forager browser login reddit` 在有界面的窗口中亲自登录一次，之后的检索命令都在无头模式下复用这个 profile。访问策略为每 10 秒 1 条浏览器命令、并发 1、跨进程生效；route 不重试。route 不在默认 order 中，配置只有 `providers.reddit_browser.timeout`（默认 120 秒）。非 Unix 主机由支持检查拒绝。
- **页面内只读脚本取值，Rust 判定。** 每个操作只导航到真实页面 URL，然后在页面中求值一段只读脚本，把需要的内容取成紧凑的 JSON（投影）交给 Rust。投影分两部分：页面事实（最终 URL、title、登录属性的原值、验证页标记、加载错误，以及各操作自己的标记），和数据记录（元素的 attribute 值与正文容器的 HTML）。脚本只做选择和取值，不做判断、不发请求、不改页面，不读组件的 JS 属性、store 或 shadow DOM。识别验证页与未登录、核对帖子身份、校验必需字段、映射输出、把正文 HTML 转成 Markdown，这些判定全部在 Rust 中。这是 ADR 0036 允许的“只读的页面状态脚本”，先例是小红书笔记的读取。理由：
  - 文档响应体太大，不适合抓包。2026-11-21 一个 1369 条评论的帖子，文档为 4 066 580 字节，已到被动抓包单个响应体 4 MiB 上限的 97%。从 DOM 求值受单条 CDP 消息 16 MiB 的上限约束，余量大得多。
  - 页面滚动后加载的内容本来就进入 DOM。从 DOM 读一次就能覆盖首屏与续页，不需要按请求序号把片段归属到滚动。
  - 判定写成以投影为输入的纯函数，不启动浏览器就能测试。脚本只取值，所以可以在本机页面上运行脚本，断言它产出的投影等于判定测试读的 JSON fixture，把两者绑定在一起。
- **数据最小化：查看者的信息不离开页面。** 脚本不带出登录账号的标识与页面令牌。帖子页的 screen-view 记录在页面内解析，只取出帖子 ID 与成人标志两个值。帖子元素上的查看者账号 ID 等属性不读。被动抓包只对 `www.reddit.com` 的请求记录状态，不保留任何响应体。
- **不发起自己的接口请求。** 包括旧的 `.json` 接口与 `/svc/shreddit/*` 片段。这沿用 ADR 0036 的边界，在 2026-11-21 的样本上也没有必需字段需要它们。
- **search** 用 URL 表达选项（全站 `/search/`，版块内 `/r/<sub>/search/` 加 `restrict_sr=1`）。首屏约 7 条，不够 `--limit` 时用原生滚轮让页面自己加载下一页，最多滚 6 次。route 不签发续页 cursor，`next_cursor` 恒为 `null`，Reddit 的 CLI 删除 `--cursor`。没有列全时成功返回已有条目，并在 stderr 说明。
- **fetch** 导航到 `/comments/<id>/?sort=top`，在页面首次出现帖子元素时投影一次，交付帖子与页面随文档带来的第一批评论，不滚动加载更多，也不点击展开回复。`order` 取页面评论树报告的排序，映射为封闭取值 `top`、`best`、`new`、`controversial`、`old`、`qa`。
- **遇到验证就停止。** 验证页（title 含 “Prove your humanity”、表单中有 reCAPTCHA，或有 `js_challenge` 字段）、文档状态 403 或 429、页面未登录，都判为 Auth：不重试，提示运行 `forager browser login reddit` 并在窗口中处理，或等待后再试。forager 从不尝试通过验证。已登录、页面加载完成却没有帖子元素时，判为 attempt 级 Parameter（帖子不存在、已删除、私有、受限，或被账号的成人内容设置挡住）。
- **启用风险说明统一登记。** 所有需要用户自行启用的 route，不分 HTTP 与浏览器传输，都在平台 catalog 的同一处登记启用风险说明。配置错误中的启用说明读这一处：HTTP route 附上 `config set` 命令；浏览器 route 先给通用的三步启用说明（安装浏览器、登录、加入 order），再附上风险说明。生成启用说明时不按 route id 特判。Reddit 的风险说明是：自动访问违反 Reddit User Agreement，风险落在登录的账号上，不在出口 IP。小红书的风险说明（站点禁止爬虫，风险落在登录的小红书账号上，站点可能限流或封禁）是建这份登记时新写的。ADR 0020 只记录了“由用户自行启用”的原则，没有对应的表述。
- **完整退役 `reddit_web`，不留兼容层。** order 中出现 `reddit_web`，或配置中出现 `[providers.reddit_web]` 段时，由现有的未知 route、未知键检查拒绝（退 3）。`doctor --provider reddit_web` 与 `platform reddit search --cursor` 退 2。与传输无关的部分保留：ref 与 URL 解析、搜索选项、CLI 参数树、输出字段、正文 HTML 转 Markdown、讨论串 Markdown 的渲染。

## 风险

- **账号。** 自动访问违反 Reddit User Agreement，风险从出口 IP 转到用户登录的 Reddit 账号上。用哪个账号由用户决定。
- **可行性证据很少。** 只有一个账号、一个出口、一个晚上：原型 9 次已登录页面打开，forager 自己的 route 实测 13 次，退役后的 live smoke 3 次（实测矩阵）。长期使用、更高频率、其他账号与其他出口都没有验证。
- **站点在后台运行无感的人机评分。** 已登录的每个页面都有一个 reCAPTCHA iframe，页面自己会请求生成 captcha 令牌。分数不可见，分数多低会升级成可见的验证，也不知道。
- **无头特征没有全部伪装**（ADR 0036 的已知风险）。2026-11-21 的读数：`outerHeight` 比 `innerHeight` 大 121；`document.hasFocus()` 在 9 次中 5 次为 true、4 次为 false，不稳定。
- **页面没有公开契约。** 读取依赖 Reddit 前端自定义元素的属性名（例如评论总数的属性名在 2026-10 的匿名片段中是 `totalcomments`，2026-11 的浏览器页面上是 `total-comments`）。改版只有 live smoke 能发现。
- **主机受限。** 浏览器传输只支持 Unix，Managed Browser 只有 macOS arm64 安装包，其他主机要经 `browser.path` 自行指定浏览器。匿名 HTTP route 原本没有这些限制。

## 不选的方案

- **保留 `reddit_web`，与浏览器 route 并存。** 匿名 route 有自己的长处：不涉及账号，任何主机都能运行，成本低。但它在维护者唯一可用的出口上完全不可用，没有用户启用过它，也就没有人能持续证明它可用。两条 route 并存时，两套页面解析、两种 cursor 语义都要维护；fallback 的执行顺序还会让同一条命令先消耗出口 IP 的信用，再动用账号。
- **匿名浏览器加一次性人工验证。** 匿名的验证页是表单 POST 回原 URL（带 `captcha=1`），通过后很可能下发一个放行 cookie。但它的名称、有效期，以及是否绑定出口、指纹或有无界面，都没有证据；放行一失效就需要人工介入。这样做还要改 `browser login` 的契约：入口只服务需要登录的 route，成功时写入的是登录标记。
- **页面内主动查询端点，包括旧 `.json` 接口。** ADR 0036 禁止页面逻辑自行请求站点接口，在样本上也没有必需字段依赖它们。主动重取翻页片段并不可靠：带页面原样的请求头重放同一地址，得到的是再下一页，翻页 cursor 在服务端有状态。`.json` 在登录态下返回完整的结构化字段，但页面自己从不请求它，请求会带上账号 cookie，风险性质与“只读页面自己的请求”不同。采用它需要先修订 ADR 0036。
- **Arctic Shift 归档。** 它是第三方存档，不是实时数据（分数与评论数可能尚未更新），关键词查询在 2026-10-17 的样本中得到 422 “slow down”，删除同步与授权情况不明，也没有可用性保证。
- **ScrapeCreators。** 需要付费 key，没有实测；数据要经过供应商，供应商把源站条款的责任留给客户。它可以作为另一条 route 另行评估，与本决定不冲突。

## 影响

- 相对 `reddit_web` 的契约差异：需要浏览器与登录；search 删除 `--cursor`，`next_cursor` 恒为 `null`，没有列全时有诊断；fetch 的 `order` 由恒为 `top` 改为上述封闭取值；第一批评论的数量由页面决定（样本中约 100 条）；帖子的正文与元数据来自帖子页本身，不再请求 embed 页。
- live smoke 的 Reddit 用例为 C50（search）与 C51（fetch）。C51 读取 C50 结果中第一条不是 NSFW 的帖子，不用固定的 canary。C47、C48 随 `reddit_web` 退役成为空号，不复用。
- 从 `reddit_web` 迁移：手工从 `platforms.reddit.order` 中删除 `reddit_web`，删除 `[providers.reddit_web]` 段；运行 `forager browser install` 与 `forager browser login reddit`；再把 `reddit_browser` 加入 `platforms.reddit.order`。

## 取代范围

- 本记录取代 ADR 0033 的全部决定，只保留“全进程的 HTTP client 启用 reqwest 的 `http2` feature”一条。ALPN 声明 `h2, http/1.1` 与 curl、浏览器相同，是 HTTP 客户端的常规配置；2026-10-28 曾实测 Reddit 拒绝只声明 `http/1.1` 的客户端。这一条与 Reddit 用哪条 route 无关，继续有效。
- ADR 0036 不修改：本记录在它保留的边界之内（只读页面自己发出的请求、改变页面状态只用原生输入、不解验证码）。
- 带日期的设计与调研文档作为历史记录保留。

## 重新评估的条件

- 经这条 route 使用的账号收到警告、限制或封禁，或者已登录的页面开始出现验证页。
- Reddit 提供适用于个人 CLI 的官方访问方式。
- 需要在 macOS arm64 之外的主机上可靠地读取 Reddit。
