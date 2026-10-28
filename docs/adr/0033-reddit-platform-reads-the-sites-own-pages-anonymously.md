# Reddit 平台匿名读取站点自己渲染的页面

forager 的第九个平台 `reddit` 只有一条匿名 HTTP route `reddit_web`。它读取 Reddit 网站自己渲染的三种页面：搜索结果片段、帖子的评论树片段，以及帖子的 embed 页。这条 route 不在默认 order 中，由用户手动启用。

## 背景

2026-10-17 的调研（`docs/research/2026-10-17-reddit-integration.md`）：

- 官方 public Data API 要先经审批。按 Reddit 的迁移公告，2026-10-31 起不再受理新的访问申请，2027-01-12 起关闭未登记应用的访问，2027 年 3 月关闭全部剩余的 public 访问。
- 匿名访问时：`.json` 端点返回 403，old.reddit 302 跳转到登录页，RSS 将于 2026-11-13 停止支持。Shreddit 的 `/svc` 页面片段匿名返回 200。
- ScrapeCreators 需要付费 key，没有实测；浏览器 route 依赖 Chrome 与已有会话。
- Reddit User Agreement 与 robots.txt 都不允许自动访问。

last30days-skill 用同一组 `/svc/shreddit` 片段匿名读取 Reddit。2026-10-28 的实测结果：

- **搜索片段**（`/svc/shreddit/search/`；限定版块时为 `/svc/shreddit/r/<sub>/search/`）每页约 7 条。每条结果的 tracking JSON 带帖子 ID、标题、成人与剧透标志、版块、作者，以及搜索显示的摘录。下一页是一个带 `cursor` 的懒加载片段。
- **评论树片段**（`/svc/shreddit/comments/r/<sub>/t3_<id>`）：路径中写任何版块名都返回该帖第一页 top 序的评论。它带帖子的 permalink 与评论总数，但不带帖子本身。
- **embed 页**（`embed.reddit.com<permalink>`）带帖子的标题、作者、分数、类型、外链与正文。帖子页本身匿名访问只返回一个 `js_challenge` 验证页，`.json` 返回 403，所以 embed 页是匿名能读到帖子正文的唯一页面。
- **TLS 握手**：客户端的 ALPN 只声明 `http/1.1` 时，Reddit 返回 403。同一个 rustls 配置不带 ALPN，或声明 `h2, http/1.1` 时，返回 200；curl 与 native-tls 也返回 200。forager 原先的 reqwest 没有启用 `http2` feature，ALPN 只有 `http/1.1`。

## 决定

- **一条 route、两个 origin、一份访问策略。** `providers.reddit_web.url`（www）与 `.embed_url`（embed）共用每 2 秒 1 个请求、并发 1 的访问策略，跨进程生效。route 不重试：对正被限制的 IP 重复请求只会延长限制。
- **默认关闭。** 用户知情后自己承担条款风险，启用说明写明这一点。风险落在出口 IP 上，是临时封锁，不涉及账号：route 不读 cookie、不登录。
- **遇到验证页就停止。** 页面带 `js_challenge` 时为 Auth，消息说明出口 IP 被拒，route 不尝试通过验证。403 与 429 的消息同样指向出口 IP。
- **ref 只命名帖子。** kind 只有 `post`，ref 为 `reddit:<id>`。评论 permalink 与 `/s/` 分享短链在飞行前退 2：前者的消息给出所属帖子的 ref；后者要联网才能解析。
- **fetch 的三种深度：**
  - `thread`（默认）：帖子加第一页 top 序评论写成一个文件，不跟随 `more-comments` 片段。
  - `metadata`：帖子正文内联在结果中。
  - `full_text`：link 帖的外链走 Web Fetch；有正文的帖子以 Native 来源交付；两者都没有时为 Quality。
- **search 只搜帖子**（`type=posts`），一次命令最多读 5 个片段页。cursor 记录上游 cursor 与该页已交付的条数，续页不会漏掉上一页没交付的帖子。
- **全进程的 HTTP client 启用 reqwest 的 `http2` feature。** ALPN 因此声明 `h2, http/1.1`，与 curl 和浏览器相同。这是 HTTP 客户端的常规配置，不伪造指纹。client 仍是唯一一个，仍用 rustls。

## 不选的方案

- **官方 Data API**：受新申请截止日与关闭时间表限制，还要建设 OAuth 凭据的生命周期管理。
- **ScrapeCreators**：付费，没有实测，数据要经过供应商。以后可以作为第二条 route 接入。
- **OpenCLI 浏览器 route**：依赖 Chrome 与会话，而匿名 HTTP 已经能读到所需页面。
- **模拟浏览器指纹的客户端**（`wreq` 等基于 BoringSSL 的客户端）：属于规避检测，还要替换整个 HTTP 栈。
- **换成 native-tls**：各平台行为不一致，Linux 构建还要带 OpenSSL；而 403 的诱因是 ALPN，与 TLS 库无关。
- **读帖子页**：匿名访问只得到验证页。
- **跟随 `more-comments`**：每个片段都是一次 POST。按 2 秒的访问间隔，一个大讨论串要几十个请求。首期只交付第一页，并用 `has_more_comments` 标明还有更多。

## 影响

- 这些页面没有公开契约，结构一变 route 就报 Runtime（消息为 “the page format may have changed”）。维护时以 last30days 与 fixture 中的页面形状为参照。
- 机房 IP 更容易被拦截，route 不适合在 CI 中运行。
- 一次 fetch 发 2 个请求，至少等一个 2 秒的窗口；一次 search 最多发 5 个请求。
- HTTP/2 对所有 provider 生效：支持 HTTP/2 的服务端改走 h2。本地 fixture 走明文 HTTP，不经过 ALPN，所以仍是 HTTP/1.1。
