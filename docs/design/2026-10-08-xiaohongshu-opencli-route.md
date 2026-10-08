# 小红书平台与 OpenCLI 浏览器 route 设计

状态：第一期的 search 已实现（#187），fetch 与 comments 未实现；实现与设计的差异见「第一期实现记录」。日期：2026-10-08。前置调研见 [小红书 Platform 接入可行性](../research/2026-10-08-xiaohongshu-integration.md)。那份调研推荐第三方 API TikHub；本设计根据下文实测改用浏览器 route，理由见「路线选择」。

## 目标与边界

为 forager 新增平台 `xiaohongshu`，唯一的 route 为 process route `xiaohongshu_browser`：经本机 OpenCLI 驱动用户已登录的 Chrome，打开小红书网页，只读取页面自身发出的接口响应与服务端渲染状态。

- **要解决的问题**：按关键词检索小红书笔记（含排序与筛选），读取单篇笔记的正文、标签、媒体与互动数，读取评论与楼中楼。
- **不做**：逆向签名或自行构造站点请求；调用页面内部的 store 方法；改写页面的 fetch/XHR；自动登录、解验证码、换号或换代理；发帖、点赞、收藏、评论、关注等任何写操作；下载图片与视频；用户主页笔记列表（以后按需加入）。
- **使用前提**：用户在 OpenCLI 连接的 Chrome profile 中自行登录小红书，并手动把 route 加入 `platforms.xiaohongshu.order`。小红书用户协议 §4.1 禁止"非法抓取、模拟下载"，robots.txt 的通用组为 `Disallow: /`（见前置调研「条款与合规边界」）。本 route 与 `ssrn_browser` 同属个人使用、用户自愿开启、接近人工节奏的访问（ADR 0020），不进入任何默认 order。

## 路线选择

| 路线 | 结论 |
|---|---|
| 逆向签名直连接口（MediaCrawler、Spider_XHS、`xhshow`） | 不采用。`xhshow`（MIT，约 3500 行 Python）2026 年至少 4 次失效：1 月接口全量 406、2 月算法升级、3 月数据接口拒绝 `XYS_` 签名并要求浏览器才能生成的 `b1` 指纹（Cloxl/xhshow#104），9 月 13 日起翻页请求被静默返回空数据（#110，至今未关闭）。静默空结果会被 forager 当成合法空集。此外这条路线要求保管用户 cookie，并让 forager 自身成为逆向工具 |
| 第三方数据 API（TikHub） | 不作为首选。按次付费（App V2 每请求约 $0.01），条款不保证原站授权，响应没有强类型 schema |
| 浏览器 route，读页面自身响应 | **采用**。签名、`a1`、`b1` 都由小红书自己的前端在真实浏览器里生成，算法更新由站点完成；读到的是与直连接口相同的结构化 JSON。实测见下文 |

## 实测

### 环境与方法

2026-10-08 12:24–13:18 UTC，本机 OpenCLI 1.8.6（Browser Bridge 扩展 1.0.24），用户自己的 Chrome，已登录的小红书账号。用一个临时 OpenCLI 读取模块（site `xhsprobe`，测完删除）复现 forager 的调用方式：`--window background --site-session ephemeral --keep-tab false`，在命令内用 `page.startNetworkCapture` / `page.readNetworkCapture`（扩展经 CDP 抓包，单个响应体上限 8 MiB）读取响应。共约 55 次只读页面命令，相邻命令间隔 5–10 秒。全程没有出现验证码、461 或安全限制页（未登录阶段除外）。证据只记录结构、计数与时间，不保存 token、cookie 与用户资料。

### 观察与设计结论

| 观察 | 设计结论 |
|---|---|
| 未登录时，搜索页显示"登录后查看搜索结果"并弹出扫码框，页面不发搜索请求；`GET edith…/api/sns/web/v2/user/me` 返回 `guest: true`；笔记页跳转"安全限制 / 300031"，评论接口返回 HTTP 461。同一时刻 `opencli auth status --site xiaohongshu` 报告 `logged_in: true` | 登录态以页面事实为准（`user/me` 的 `guest` 或 `__INITIAL_STATE__.user.loggedIn`），不用 `opencli auth status`；未登录为 Auth |
| 登录后，`--site-session ephemeral` 的临时会话同样 `guest: false` | 临时会话沿用用户 Chrome profile 的登录态，route 可以保持 SSRN 的会话参数 |
| 搜索接口已迁到 `POST so.xiaohongshu.com/api/sns/web/v2/search/notes`（调研引用的 MediaCrawler 仍用 `edith…/v1/search/notes`）。请求体含 `keyword`、`page`、`page_size: 20`、`search_id`、`sort`、`note_type`、`session_id`，筛选后增加 `filters`。抓包条目同时带请求体与响应体 | 读取模块以请求体核对实际生效的条件与页码，以响应体解码条目 |
| 响应 `data` 为 `{has_more, items, request_dqa_instant}`；`items` 中除 `model_type: note` 外还有 `hot_query`。笔记条目含 `id`（24 位十六进制）、`xsec_token`、`note_card`：`display_title`、`type`（`normal` 或 `video`）、`user{user_id, nickname, xsec_token}`、`interact_info` 的点赞、收藏、评论、分享数（字符串）、`cover`、`image_list`、`corner_tag_info`。卡片没有正文片段 | 只解码 `model_type: note`；search 条目深度为 `metadata` |
| `corner_tag_info` 含 `publish_time`，显示文本为 `07-09`（今年）或 `2025-06-13`（往年）两种形式 | search 的 `published` 由该文本归一化（规则见「search」）；精确时间只在 fetch 中给出 |
| `GET edith…/api/sns/web/v1/search/filter` 列出 5 组筛选：`sort_type`（general / time_descending / popularity_descending / comment_descending / collect_descending）、`filter_note_type`（不限 / 视频笔记 / 普通笔记）、`filter_note_time`（不限 / 一天内 / 一周内 / 半年内）、`filter_note_range`（不限 / 已看过 / 未看过 / 已关注）、`filter_pos_distance`（不限 / 同城 / 附近） | L1 选项只开放前三组；后两组依赖账号浏览历史与地理位置，不开放 |
| 悬停 `div.filter` 打开面板，点击对应文本后，每次都发出新的第 1 页请求：`filters` 依次带上 `time_descending`、`普通笔记`、`一周内`，`search_id` 变为 `<根 id>@<子 id>`；DOM 选中项一致。请求体顶层的 `sort` 与 `note_type` 始终是 `general` 与 `0`，真实条件只在 `filters` 中 | 筛选通过 UI 设置；校验看 `filters`，不看顶层 `sort` |
| 点击后立刻读抓包，拿到的第 1 页条目没有响应体，且条目已被取走；点击后等待 3 秒再读则完整。楼中楼请求每次还多出一条无响应体的条目 | 读取模块等响应体就绪；无响应体的条目丢弃，不计为一页 |
| OpenCLI 的后台窗口是普通窗口，只是不抢焦点；被其他窗口遮挡时 `document.visibilityState` 为 `hidden`（多次运行中有时 `hidden`、有时 `visible`）。`hidden` 时滚动（`scrollTo`、`scrollIntoView`、CDP 滚动、`autoScroll`）都不触发加载更多；前台窗口中同样的滚动立即加载第 2 页 | 不能依赖窗口可见 |
| `hidden` 时，滚到底后调用两次 `page.screenshot()` 强制渲染一帧，搜索第 2、3 页与一级评论第 2、3 页都会正常加载（各运行 2 次以上） | 翻页统一用"滚到底 + 截图强制渲染"，只模拟用户操作；页面的 `search.loadMore` store action 同样可用，但不采用 |
| 同一命令内连续 3 页没有重复，`search_id` 不变，`page` 依次为 1、2、3 | 一次命令内的分页可靠 |
| 同一查询相隔约 1 分钟运行两次：第 1 页 20 条中只有 15 条相同且顺序全不同；前两页 40 条中 34 条相同 | 结果有个性化与随机成分，不能用"重跑并翻到第 N 页"跨命令续页；search 不签发 cursor |
| 已登录时只用笔记 ID 打开 `/explore/<id>`：跳转 `/404?...error_code=300031`，"当前笔记暂时无法浏览" | fetch 必须带 `xsec_token` |
| 带搜索结果中的 `xsec_token` 打开：详情完整。把 token 存到本地，在另一条命令中使用：31 秒、704 秒、1041 秒与 1322 秒后都有效。`xsec_source` 为 `pc_search`、`app_share`、`pc_feed` 都有效 | token 可跨命令复用至少 22 分钟；`xsec_source` 不影响访问，固定发 `pc_search` |
| 用 A 笔记的 token 打开 B 笔记：300031。OpenCLI 内置小红书模块的源码注释另记录：连续读取详情会触发按频率的风控，表现同样是跳转 `website-login/error?error_code=300017` 或 `300031`、页面显示"安全限制"或"访问链接异常"，冷却后常可恢复（`clis/xiaohongshu/risk-control.js`，引 OpenCLI #1825、#962） | token 与笔记绑定；300031 可能是 token 无效或过期、笔记受限或已删除、触发频率风控，读取模块无法区分 |
| 不带 cookie、带 token 的匿名 HTTP 请求：200，但页面里没有该笔记 | token 只在登录会话中起作用，单独泄露几乎没有风险 |
| 直接打开笔记页时，页面不调用 `feed` 接口，详情在 `__INITIAL_STATE__.note.noteDetailMap[<id>].note`：`noteId`、`title`、`desc`、`type`、`time` 与 `lastUpdateTime`（毫秒）、`tagList`、`imageList`、`interactInfo`、`user`、`atUserList`、`shareInfo`、`xsecToken`，视频笔记另有 `video`；`ipLocation` 有时缺失。观察到的 `desc` 长度为 103–844 字符 | fetch 读 SSR 状态，并核对 `noteId` 与请求的 ID 一致 |
| `desc` 是纯文本，话题写成 `#话题名[话题]#`；`tagList` 条目为 `{id, name, type: "topic"}` | 正文原样保留话题写法；标签取 `name` |
| 视频笔记的 `video.capa.duration` 为秒数；`video.media.stream` 分 `EF4`–`EF7` 档，每档条目带 `width`、`height`、`duration`（毫秒）、`size` 与 `masterUrl`，`masterUrl` 带 `sign` 与 `t` 参数。图片 `urlDefault` 位于 `sns-webpic-qc.xhscdn.com`，不带查询参数。测试时两者都能匿名访问（HTTP 206） | 图片 URL 按原样输出；视频只输出时长与尺寸，有时效的签名地址不输出（见「fetch」） |
| 两个无意义的查询（含空格的乱码、24 位随机串）分别返回 20 与 18 条笔记，`has_more` 为 true；页面没有出现无结果提示 | 合法空集只按接口判断：首个响应没有笔记且 `has_more` 为 false；不依赖页面文案 |
| `noteDetailMap[<id>].comments` 为 `{list, cursor, hasMore, firstRequestFinish, loading}`；页面同时发出 `GET edith…/api/sns/web/v2/comment/page`（参数 `note_id`、`cursor`、`top_comment_id`、`image_formats`、`xsec_token`），每页 10 条一级评论，带 `has_more` 与 `cursor` | 评论从抓包读取，按页核对 `cursor` 链 |
| 一级评论含 `id`、`content`、`create_time`（毫秒）、`ip_location`、`like_count`、`sub_comment_count`（字符串）、`sub_comments`（内嵌 1 条）、`sub_comment_cursor`、`sub_comment_has_more`、`user_info`、`status`、`at_users`、`show_tags` | 评论条目字段见「comments」 |
| `hidden` 时点击"展开 33 条回复"发出 `GET edith…/api/sns/web/v2/comment/sub/page`（`root_comment_id`、`num=10`、`cursor`），实际返回 5 条，带 `has_more` 与 `cursor`；再点"展开更多回复"继续翻页。回复带 `target_comment{id, user_info}` | 楼中楼由点击展开，不需要可见；回复保留实际回复对象 |
| 评论组件是生产构建的 `<script setup>`，没有可访问的加载函数；Pinia 里也没有评论 store | 评论只能经 UI 触发，证实 UI 路线是唯一不改写页面的做法 |
| 耗时：打开搜索页到第 1 页约 16–19 秒，之后每页约 3.5–4 秒；带 token 打开笔记约 17.5 秒；一次命令读 3 页评论约 30 秒；单条命令 35–48 秒 | 默认 attempt 超时 120 秒；search 的 `--limit` 上限为 100（5 页） |

## 平台 `xiaohongshu`

- **id**：`xiaohongshu`。kind 只有 `note`。
- **ref**：`xiaohongshu:<note_id>`，`note_id` 是 24 位十六进制，统一为小写，没有版本。ref 只表示身份，不含访问 token。
- **canonical URL**：`https://www.xiaohongshu.com/explore/<note_id>`，不含查询参数。往返性质：解析 canonical URL 得到同一个 ref。
- **可识别的 URL**：主机为 `www.xiaohongshu.com` 或 `xiaohongshu.com`（http 或 https），路径为 `/explore/<id>`、`/discovery/item/<id>`、`/search_result/<id>` 或 `/user/profile/<user_id>/<id>`，忽略 fragment 与末尾 `/`。查询参数中的 `xsec_token` 被单独取出（见下条），其他参数忽略。`xhslink.com` 短链需要联网展开，飞行前退 2，消息提示改用浏览器打开后的完整链接。`rednote.com` 不识别，因为它适用另一份用户协议，账号域是否互通未核实。
- **Access Token**：笔记 URL 中的 `xsec_token`，是打开该笔记所需的访问参数。它不是身份：同一笔记在不同时间、从不同入口得到的 token 不同，ref 与 canonical URL 都不含它。解析纯函数把 URL 拆成 `(ref, Option<token>)`；token 只接受 URL 安全的 base64 字符（`A-Za-z0-9_=-`，URL 解码后），长度 1–128，否则飞行前退 2。身份解析与操作要求分开：不带 token 的 ref 与 canonical URL 仍是合法身份，只有 fetch 与 comments 的请求校验要求 token。解析错误消息不回显原始输入，只说明哪一部分不合法，避免把 token 带进 stderr。
- **访问链接（`access_url`）**：由 ref 与 token 构造的规范访问链接 `https://www.xiaohongshu.com/explore/<id>?xsec_token=<token>&xsec_source=pc_search`。search 条目与 fetch 输出都用这一构造，不回显用户输入的原始链接（原始链接的路径、参数次序与 fragment 无法从 `(ref, token)` 还原，也不需要）。
- **Content Depth**：search 条目恒为 `metadata`（卡片只有标题，没有正文片段）；fetch 支持 `metadata` 与 `full_text`，含义见「fetch」。

`GLOSSARY.md` 增加术语 Access Token（平台身份之外、打开某个实体所需的访问参数；不进入 ref、canonical URL、cursor 与 journal）。

## Route `xiaohongshu_browser`

| 注册项 | 取值 |
|---|---|
| `credentials_required` | `false`（forager 不管理 key；不表示站点无需登录） |
| `transport` | `OpenCli { site: "forager-xhs", contract: "forager-xhs/1" }` |
| `access_policy` | 每 10 秒 1 次 OpenCLI 命令，并发 1，跨进程生效；permit 持有到子进程被回收（ADR 0020） |
| `probe` | `DoctorProbe::PlatformSearch { platform: Xiaohongshu, name: "search", transport: "opencli" }`（deep doctor 执行一次真实检索，与 `ssrn_browser` 相同） |
| 配置 | `providers.xiaohongshu_browser.command`（默认 `opencli`）与 `.timeout`（默认 120 秒） |
| 默认 order | `platforms.xiaohongshu.order = []` |

- **site 名**：OpenCLI 已内置 `xiaohongshu` site，forager 自有读取模块使用独立的 `forager-xhs`，避免和内置命令冲突或被覆盖。安装方式与 SSRN 相同：把 skill 的 `opencli/forager-xhs` 目录复制为 `~/.opencli/clis/forager-xhs`。
- **默认 order 为空**：平台在 catalog 中登记，但默认 order 为空。用户未启用时，平台命令按现有规则在飞行前退 3，消息点名 `platforms.xiaohongshu.order`，并说明需先在 Chrome 中登录小红书、安装读取模块。现有 catalog 校验允许空的默认 order。
- **访问间隔**：10 秒是本次实测的上限节奏（5–10 秒间隔、约 55 次命令未触发风控），不是站点给出的安全阈值。一次命令内部的页面加载、筛选点击与翻页不再单独限速，但命令内有页数上限（search 5 页、评论 5 页、楼中楼展开 10 次）。
- **不重试**：route 失败直接成为终态（平台只有这一条 route）。
- **token 经 argv 传递**：`OpenCliCommand` 只支持具名参数，token 以 `--xsec-token` 传入。现行 process 传输契约本来就用具名 argv，SSRN 禁止外泄的是有时效的签名下载地址，不是访问参数；实测 token 离开登录会话打不开笔记，且出现在小红书自己的分享链接里。新 ADR 把"token 在本机进程参数中可见"与"成功输出的 `access_url` 含 token"写为接受的边界。
- **诊断脱敏**：route 在 `execute_anonymous` 记录 attempt 之前，按本次请求已知的 token 值清理所有来自 OpenCLI stderr、外壳解码错误与页面事实的消息（与 Scholar route 按凭据值脱敏的做法相同，不进入 Provider Credential Pool）。成功输出的 `access_url` 是唯一允许出现 token 的位置；`--verbose` 的 attempts 与失败消息不含 token。

## 读取模块

读取模块在 skill 目录 `skills/forager/opencli/forager-xhs/`。它保持薄：只执行页面操作、等待并回报页面事实（接口的请求体与响应体、最终 URL、错误码、登录状态）；条件核对、页面状态分类与归一化全部在 Rust 侧完成，因此都能由进程级测试覆盖，读取模块不单独写测试。

### 原则

1. **只做用户能做的操作**：导航、悬停、点击、滚动；翻页时在滚到底后调用 `page.screenshot()` 强制渲染（隐藏窗口里不渲染就不会触发加载，见实测）。截图只用于驱动渲染，不保存。
2. **只读页面已有的数据**：页面自己发出的接口响应（`page.startNetworkCapture('xiaohongshu.com')` 与 `page.readNetworkCapture()`）和 `window.__INITIAL_STATE__`。不调用 `page.fetchJson`、store action 或组件方法，不使用 `installInterceptor`（它改写页面的 fetch/XHR）。
3. **读到的条件必须可核对**：读取模块原样回报请求体与响应体，Rust 以请求体的 `filters`、`page` 与 `search_id` 核对条件与页序，以 SSR 的 `noteId` 核对身份，以请求参数中的 `cursor` 核对评论页序。不符时为 Runtime，绝不返回可能错误的结果或假空集。
4. **停在站点门口**：遇到登录墙、验证码、安全限制立即结束，不点击、不等待它自行消失之外的任何处理。

### 外壳与命令

stdout 外壳沿用 `{contract, status, data}`，`contract` 为 `forager-xhs/1`。所有命令接受 forager 的会话参数与 `--timeout`；读取截止点为 `--timeout` 之前 3 秒（与 SSRN 相同）。

| 命令 | 参数 | data |
|---|---|---|
| `contract` | 无（不需要浏览器） | `{commands}` |
| `search` | `query`、`sort`、`note-type`、`publish-time`、`pages`（1–5） | `{page, filter_clicks, responses: [{request, body}]}`：设置筛选后按顺序抓到的搜索响应；`request` 只含 `keyword`、`page`、`search_id`、`filters` |
| `note` | `id`、`xsec-token` | `{page, note, comments_state}`：SSR 中的笔记对象（媒体只保留下文用到的字段） |
| `comments` | `id`、`xsec-token`、`pages`（1–5）、`expand`（0–10） | `{page, responses: [{kind, params, body}]}`：一级评论与楼中楼的响应，`params` 只含 `note_id`、`cursor`、`root_comment_id`、`num`；`expand` 只对读取模块收到的前 `expand` 条可展开一级评论点击 |

`page` 为页面事实：`{url, title, guest, error_code, notice, blocked_status}`。`url` 去掉 `xsec_token` 的值；`guest` 来自 `user/me` 或 `__INITIAL_STATE__.user.loggedIn`；`error_code` 取自跳转 URL（`/404` 或 `website-login/error`）的 `error_code` 参数；`notice` 是页面上"安全限制""访问链接异常""登录后查看"等提示原文；`blocked_status` 是任一小红书接口返回的 461。读取模块看到这些终态事实就停止等待并返回。外壳 `status` 恒为 `ok`；截止点前既没有等到预期响应、也没有出现终态事实时，同样返回页面事实，并带 `timed_out: true`。

**抓包的完成条件**：`readNetworkCapture()` 是消费式读取，读到的条目不会再次投递；实测点击后立刻读取，会取走一条还没有响应体的搜索响应，导致该页永久丢失。因此读取模块先等页面的 Performance Resource Timing 中出现对应请求的完成记录（`responseEnd > 0`，SSRN 读取模块已用同一信号），再读取抓包；读到的匹配条目仍没有响应体时，在截止点前继续等待同一请求的完整记录，截止点到达仍缺响应体则在事实中记为 `body_missing`，Rust 侧为 Runtime，绝不静默丢弃。`OPTIONS` 预检等非 GET/POST 条目按方法过滤。这一读取循环是否能在慢响应下拿到完整响应体，需要在实现阶段用真实页面验证一次。

Rust 侧按以下顺序分类，排在前面的优先：

| 页面事实 | 结果 |
|---|---|
| `guest` 为 true，或 `notice` 为登录提示 | Auth，消息提示在 Chrome 中登录小红书 |
| `blocked_status` 为 461 | Auth，消息提示在 Chrome 中打开小红书，确认没有验证要求后再试 |
| `error_code` 为 300031 或 300017，或 `notice` 为"安全限制""访问链接异常" | attempt 级 Parameter（消息见「fetch」） |
| 预期数据存在 | 进入条件核对与解码 |
| `timed_out` 且最终 URL 仍是预期页面（搜索结果页或该笔记页） | Timeout |
| 其他（最终 URL 不是预期页面，包括本次未见过的验证码页；`body_missing`） | Runtime，消息带页面标题与去掉 token 的 URL |

分类在 route 的 `execute_anonymous` 闭包内、成功 attempt 记录之前完成，与 `ssrn_browser` 核对页面事实的位置相同。规格第 7 章 OpenCLI 退出码表中"0 且外壳有效 → 成功"改写为"传输成功"，业务结果由 route 判定；77→Auth、75→Timeout 等映射不变。

## search

### 参数

| 层 | 参数 | 页面操作 | `filters` 校验 |
|---|---|---|---|
| L0 | 查询词 | 打开 `/search_result?keyword=<编码后的查询词>&source=web_explore_feed` | 请求体 `keyword` 等于查询词 |
| L0 | `--limit` | 1–100，默认 20；读取 ⌈limit / 20⌉ 页 | 每页 `page` 依次递增，`search_id` 不变 |
| L1 | `--sort` | `comprehensive`（默认）/ `latest` / `most-liked` / `most-commented` / `most-collected`，点击"排序依据"下的综合 / 最新 / 最多点赞 / 最多评论 / 最多收藏 | `sort_type` 为 `general` / `time_descending` / `popularity_descending` / `comment_descending` / `collect_descending` |
| L1 | `--note-type` | `all`（默认）/ `image` / `video`，点击"笔记类型"下的不限 / 图文 / 视频 | `filter_note_type` 为 `不限` / `普通笔记` / `视频笔记` |
| L1 | `--publish-time` | `any`（默认）/ `day` / `week` / `half-year`，点击"发布时间"下的不限 / 一天内 / 一周内 / 半年内 | `filter_note_time` 为对应文本 |

- 查询词原样输入，小红书不提供布尔运算符或短语语法，规格写明"不承诺精确短语或运算符"。去空白后为空时飞行前退 2。
- 只有非默认的条件才点击；全部默认时不打开筛选面板，此时请求体可以没有 `filters`。条件按 sort、note-type、publish-time 的顺序设置，每次点击后等到新的第 1 页响应（含响应体）再继续。最终用于结果的是最后一次设置后的第 1 页及其后续页；之前的响应丢弃。
- `filter_note_range` 与 `filter_pos_distance` 不开放：前者依赖账号浏览历史，后者依赖地理位置，结果不可复现。

### 分页与 cursor

- 一次命令内读取所需页数：第 1 页之后，每页执行"滚到底 + 两次截图强制渲染"，等待下一个 `page` 的响应；`has_more` 为 false 时提前结束。
- 页内与页间按笔记 ID 去重，去重后截到 `limit` 条。
- **不签发 cursor**：`next_cursor` 恒为 `null`，CLI 不提供 `--cursor`，route 的支持检查拒绝任何非首页的页位置。跨命令重跑会得到不同排序（实测前 40 条只有 34 条相同），因此续页既不能复现，也无法去重。规格第 7 章与 `PlatformSearchPage.next_cursor` 的文档改为：`null` 表示没有可用的跨命令续页标识，本身不证明结果已穷尽；小红书为显式例外。
- **未列全诊断**：只要上游最后一页 `has_more` 仍为 true，或本次已解码的有效条目在截到 `limit` 时有未交付的余项，stderr 就输出一条诊断：还有更多结果，小红书不支持续页，需要更多时加大 `--limit`（上限 100）。不以"凑满 limit"为前提：过滤 `hot_query`、无效条目与重复项后可能不足 limit。
- **合法空集由 route 判定**：共享链执行器把空 `items` 一律当作合法空集，所以 route 在 attempt 内先判定：计入的第一个响应没有笔记且 `has_more` 为 false 才返回空成功；没有笔记而 `has_more` 为 true、或缺少预期响应字段时为 Runtime。

### 解码

| 输出字段 | 来源 |
|---|---|
| `ref`、`url` | `id` → `xiaohongshu:<id>` 与 canonical URL |
| `depth` | `metadata` |
| `title` | `note_card.display_title`，去空白后为空则为 `""`（小红书允许无标题笔记） |
| `authors` | `[note_card.user.nickname]`，缺失时为 `[]` |
| `published` | `corner_tag_info` 中 `type` 为 `publish_time` 的 `text`：`YYYY-MM-DD` 原样输出；`MM-DD` 补上请求时的北京时间年份，若得到的日期晚于请求当天则用上一年；其他形式（例如相对时间）为 `null` |
| 平台字段 `note_type` | `note_card.type`：`normal` 输出为 `image`，`video` 原样 |
| 平台字段 `author_id` | `note_card.user.user_id` |
| 平台字段 `likes`、`collects`、`comments`、`shares` | `interact_info` 的计数原文（字符串，可能是 `1.2万` 这类缩写），缺失为 `null` |
| 平台字段 `published_text` | `publish_time` 的原文，缺失为 `null` |
| 平台字段 `access_url` | `https://www.xiaohongshu.com/explore/<id>?xsec_token=<条目的 xsec_token>&xsec_source=pc_search`；fetch 与 comments 的推荐输入 |

`model_type` 不是 `note` 的条目跳过。`id` 不是 24 位十六进制或缺少 `xsec_token` 的笔记条目跳过，并在 stderr 汇总一条诊断；上游有笔记而全部被跳过时为 Runtime。

## fetch

### 输入

- 输入为带 `xsec_token` 的笔记 URL（通常是 search 条目的 `access_url`，或用户从浏览器复制的笔记链接）。只给 ref 或不带 token 的 URL 时，飞行前退 2，消息说明：小红书需要访问 token，请使用 search 结果的 `access_url` 或浏览器中的完整笔记链接。这是纯函数检查，不发请求。
- `PlatformFetchRequest` 增加 `access: Option<AccessToken>`。`AccessToken` 的 `Debug` 打码，不 `Serialize`；它只从命令行输入流向 route，不进入输出的 ref、`url`、cursor 与 journal。`url` 字段仍是 canonical URL；fetch 输出的 `access_url` 由 ref 与 token 构造（见「平台」），供后续调用 comments。

### Depth

| depth | 含义 | 实现 |
|---|---|---|
| `metadata` | 标题、作者、发布时间、类型、标签、互动数、图片 URL、视频元数据；不含正文 | `note` 命令，内联输出 |
| `full_text`（默认） | 在 `metadata` 之外，正文 Markdown 写入文件 | 同一次 `note` 命令；正文由 route 生成，见下文 |
| 其他 | 不支持 | 支持检查拒绝，飞行前退 2 |

### 元数据

| 输出字段 | 来源 |
|---|---|
| `title` | `note.title` |
| `authors` | `[note.user.nickname]` |
| `published` | `note.time`（毫秒）转为北京时间 ISO 8601 时间戳 |
| 平台字段 `updated` | `note.lastUpdateTime` 同样转换；与 `time` 相同时仍输出 |
| 平台字段 `note_type`、`author_id`、计数 | 同 search（计数来自 `interactInfo` 的 `likedCount`、`collectedCount`、`commentCount`、`shareCount`） |
| 平台字段 `tags` | `tagList[].name` |
| 平台字段 `images` | `imageList[]` 的 `{url: urlDefault, width, height}`，只保留 HTTP(S) URL |
| 平台字段 `video` | 视频笔记为 `{duration_seconds, width, height}`：时长取 `video.capa.duration`，尺寸取 `video.media.stream` 中第一个非空档位的首个条目；图文笔记为 `null`。不输出 `masterUrl`，因为它是带 `sign` 与 `t` 的有时效地址 |
| 平台字段 `ip_location` | `note.ipLocation`，缺失为 `null` |

`noteId` 与请求的 ID 不一致为 Runtime。页面事实为 300031 / 300017 时映射为 attempt 级 Parameter：`Xiaohongshu note unavailable: xiaohongshu:<id> (300031: <页面提示>); the access token may be stale, the note restricted or removed, or the account rate-limited`。读取模块不在同一命令内重试或冷却重载（与 OpenCLI 内置模块不同），以免在风控状态下继续访问。skill 的平台 reference 规定恢复步骤：重新 search 取得新的 `access_url` 再试一次；仍然 300031 时停止访问小红书，过一段时间再试。

### 正文来源

笔记正文已经在浏览器里读到，不存在可以交给 Web Fetch 的 URL：canonical URL 打不开（需要 token 与登录），访问链接交给第三方 Web Fetch provider 也会因为没有登录态而失败。现行规格的正文来源只有 URL 与本地 PDF，且必经 Web Fetch 链与薄正文门（至少 200 字符），短笔记会被误判为过薄。因此新增第三种正文来源：

- `FullTextSource::Native(String)`：route 在同一 attempt 内取得并核对过身份的 Markdown 正文，只携带正文字符串；归因所需的 route 与条目已在 core 的元数据阶段持有，不重复存放。core 的 `platform_fetch` 遇到它时不运行 Web Fetch 链，也不运行薄正文门，用 `PlatformContent::new(canonical URL, route id, 正文)` 构造结果（`source_file` 为空），交给 CLI 现有的交付逻辑（文件、`--format content`、`--content-dir`、写失败为 Runtime）。不补记 Web Fetch attempt；正文不完整的 Quality 在 route 的同一个 attempt 内产生。
- **Web Fetch 预检**：现有 CLI 在执行任何 route 之前，只要是 `full_text` 且没有已配置的 Web Fetch provider 就退 3，而正文来源要等 route 返回后才知道。因此由注册信息声明每条 fetch route 的全文是否依赖 Web Fetch；`plan_fetch` 根据计划中的 route 暴露"本计划是否需要 Web Fetch"，CLI 只在需要时预检。`xiaohongshu_browser` 声明不依赖；arXiv 与 SSRN 的 route 依赖，保持飞行前零请求退 3。不按"process route"一概豁免，也不把预检挪到 route 执行之后。
- **语义调整**：`content_provider` 定义为实际产出 Markdown 的 provider：Native 时为 route id，其他来源仍为 Web Fetch provider。`content_url` 对 Native 是规范来源身份，不承诺可匿名抓取。Native 不产生原始文件，小红书不提供 `--keep-pdf`；SSRN 的本地文件分支不变。Native 正文仍受 OpenCLI stdout 4 MiB 协议上限约束，不享有 Web Fetch 的截断例外。
- 正文 Markdown 由 route 生成：`# <title>`（无标题时省略），空行后为 `desc` 原文（纯文本，话题写法 `#话题名[话题]#` 原样保留），空行后为 `标签：` 加逗号分隔的标签名，最后是 `![](<图片 URL>)` 列表；视频笔记写一行 `（视频笔记，时长 <秒> 秒，视频文件未下载）`。
- 判定完整：`noteId` 一致，且 `desc`、`title`、`imageList` 至少一项非空。三项都为空时为 Quality。

这条改动需要一份新 ADR："route 自己已读到且核对过身份的平台原生正文"作为第三种全文来源，取代 ADR 0020 后果一节中"平台 fetch 编排对两种来源运行同一 Web Fetch 链"的穷举表述。ADR 0009（Web Fetch 的 provider 优先、质量门与 4 MiB 行为）与 ADR 0015（文件引用交付）保持原作用域。同步规格第 4、7 章、第 3 章的 full_text 预检条款、`PlatformContent` 注释与 GLOSSARY 中的 Platform Fetch 定义。

## comments（L2 平台操作）

`forager platform xiaohongshu comments URL [--limit N] [--replies M]` 列出一篇笔记的一级评论，并可展开前几条一级评论的楼中楼。它的必需输入与结果种类都不同于 fetch，按 L2 规则成为新操作，只由 `xiaohongshu_browser` 实现，写成 route 的 inherent 方法。

- **输入**：与 fetch 相同，必须带 `xsec_token`。
- **`--limit`**：一级评论条数，1–50，默认 20；读取 ⌈limit / 10⌉ 页，翻页方式与 search 相同（滚到底 + 截图）。
- **`--replies`**：展开楼中楼的一级评论数，0–10，默认 0。只对最终交付的一级评论中、`sub_comment_has_more` 为 true 的前 M 条各点击一次"展开 N 条回复"，只读第一页回复（实测每页 5 条）。
- **核对**：每个一级评论响应的请求 `note_id` 必须等于请求的笔记，请求 `cursor` 必须等于上一个响应返回的 `cursor`（第一页为空）；每个楼中楼响应的 `note_id` 与 `root_comment_id` 必须对应一条本次选中展开的一级评论。任何不符为 Runtime。楼中楼第一页的请求 `cursor` 是否恒等于该评论的 `sub_comment_cursor` 未经实测，不作为核对条件，实现阶段确认后再加。
- **输出**：`{platform, provider, note: ref, comments, has_more}`。评论条目为 `{id, author, author_id, text, likes, published, ip_location, reply_count, replies, replies_has_more}`；回复条目为 `{id, author, author_id, text, likes, published, ip_location, reply_to}`，`reply_to` 为 `target_comment.id`（缺失时为所属一级评论的 id）。`published` 由 `create_time` 转为北京时间 ISO 8601 时间戳。`replies` 先放内嵌的一条，再追加展开得到的回复，按 `id` 去重。
- **不签发 cursor**：页面只能在打开笔记后从第一页顺序加载，命令之间无法注入 cursor。`has_more` 表示"还有未交付的一级评论"：上游最后一页 `has_more` 为 true，或已读取的评论在截到 `limit` 时有余项，都为 true。`replies_has_more` 按展开后的结果计算：该评论仍有未交付的回复即为 true。
- **空评论**：笔记没有评论时返回空列表，退 0，不按 fetch 的"不存在"语义报错。
- **落法**：沿用 Scholar cited-by 的 L2 写法（factory 声明唯一 route 集合与支持检查，按 order ∩ 该集合规划，经共享链执行器与 `AttemptTarget::Platform` 记录 attempt），但不复用 search 的可序列化 `PageRequest` 与 `PlatformSearchOutcome`：comments 有自己的非序列化请求（携带同一种 Access Token）、结果类型与执行入口，CLI 有自己的输出渲染与 `--verbose` attempts。不新增 trait，也不登记为所有平台必须提供的操作。checklist 的 `single_route_operations_have_transport_fixtures` 目前只从 cited-by 的 route 集合构造期望，需要把 comments 的集合加入。
- 一级评论与回复的 `content` 原样输出，不做表情或 @ 渲染。

## 错误归因

| 情况 | 阶段 | 结果 |
|---|---|---|
| 只给 ref 或缺 token 的 URL、短链、`rednote.com`、token 格式不合法、选项超出值域 | 飞行前 | 退 2 |
| `platforms.xiaohongshu.order` 为空（默认） | 飞行前 | 退 3，消息说明启用步骤 |
| 非 Unix 系统 | 支持检查 | route 记为 Skipped，全链不可用时退 2（沿用 process route 规则） |
| 页面事实为未登录或 HTTP 461 | 飞行后 | Auth，退 4，消息提示在 Chrome 中手动处理 |
| 300031 / 300017 | 飞行后 | attempt 级 Parameter，退 4 |
| 截止点前没有等到预期响应，且仍停在预期页面 | 飞行后 | Timeout，退 4 |
| 请求体条件、页序、cursor 链、评论所属笔记或根评论、`noteId` 与请求不符；响应体缺失；最终页面不是预期页面；外壳或数据形状不对 | 飞行后 | Runtime，退 4 |
| 首个搜索响应没有笔记且 `has_more` 为 false | 成功 | `items: []`，退 0 |
| 首个搜索响应没有笔记但 `has_more` 为 true | 飞行后 | Runtime，退 4 |
| 笔记没有评论 | 成功 | `comments: []`，退 0 |
| 正文三项皆空 | 飞行后 | Quality，退 5 |

OpenCLI 退出码到 attempt 错误类型的映射完全沿用 `providers/opencli`，不新增。

## doctor 与 smoke

- **doctor shallow**：只在 order 启用该 route 时运行 `contract`，核对契约版本；未启用时报告 `configured: false`，不影响 `ok`。
- **doctor deep**（`doctor --provider xiaohongshu_browser`）：沿用 `PlatformSearch` 探针，执行一次真实检索（`forager doctor`，limit 1），与 `ssrn_browser` 相同；检索本身要求登录，所以同时验证了登录态。不新增 session 探针或 session 命令。未启用时以 config 失败退 3。
- **smoke 用例**：只在 order 含 `xiaohongshu_browser` 时运行，需要真实的 OpenCLI、Chrome 登录态与已安装的读取模块。没有固定 canary（token 会过期），后续用例在本轮 smoke 的内存中接收 C27 的访问链接，不持久化 token。
  - C27 search：`--limit 25 --sort latest --publish-time week`，覆盖非默认筛选与第 2 页。
  - C28 fetch：C27 第一条的 `access_url`，`--depth full_text`。
  - C29 comments：C27 中评论数最多的一条的 `access_url`，`--limit 15 --replies 1`，覆盖评论第 2 页与楼中楼展开。该笔记没有可展开回复时，C29 记为未验证（skipped 并写明原因），不记为通过。
  - 判定通过时不要求 `title` 非空（小红书允许无标题笔记）。
- **smoke 的停止规则**：现有 live smoke 对每个用例最多重试 2 次，并在失败后继续后续用例。小红书用例改为只执行一次；任一用例以 Auth 或 attempt 级 Parameter（300031/300017）失败时，结束本轮所有小红书访问，依赖它的后续用例不启动、不记为通过。第 5 章同步写明这个重试例外。

## 实现改动清单

按规格第 7 章接入清单逐项完成：

1. **types**：`Platform::Xiaohongshu`；`platform_xiaohongshu` 叶子模块包含 `XiaohongshuRef`（解析、canonical URL、URL 拆出 token、构造 `access_url`）、`AccessToken`、`XiaohongshuSearchOptions`、`XiaohongshuItemData` 与 comments 的请求和结果形状；`PlatformFetchRequest.access`；`FullTextSource::Native`；`PlatformRef`、`PlatformSearchOptions`、`PlatformItemData` 各加一个变体。新增的内部类型沿用 `pub(crate)`，只有 CLI 需要渲染的结果形状经 types 门面公开；types 保持零 IO，不依赖 redact 或 config。
2. **config**：`providers.xiaohongshu_browser.command` 与 `.timeout`，`platforms.xiaohongshu.order`（默认空）；runtime 投影与 `platform_route_config` 覆盖新 route。
3. **route**：`providers/xiaohongshu_browser` 模块，按 SSRN 的拆法分为命令调用与解码（search、note、comments 各自的 DTO），只依赖 `providers/opencli`；页面事实分类、条件核对与 token 值脱敏都在这里。注册信息增加"全文是否依赖 Web Fetch"的声明。
4. **core**：`plan_fetch` 暴露计划是否需要 Web Fetch；`platform_fetch` 处理 `Native` 来源；comments 的规划与执行入口。
4a. **ops**：smoke 的小红书用例单次执行、阻断即停、C27 的访问链接在本轮内传给 C28/C29。
5. **catalog**：`ProviderId::XiaohongshuBrowser`、注册信息、`PLATFORMS` 条目（search 与 fetch 的 route 集合为 `[xiaohongshu_browser]`，默认 order 为空），smoke 用例 C27–C29。
6. **CLI**：`XiaohongshuSearchArgs`、`XiaohongshuFetchArgs`、`XiaohongshuCommentsArgs`；缺 token 与 order 为空的消息。
7. **读取模块**：`skills/forager/opencli/forager-xhs/`（`contract.js`、`search.js`、`note.js`、`comments.js` 与共享的 `shared.js`，含抓包完成条件、页面事实收集、翻页与筛选操作）。
8. **测试与清单**：`tests/acceptance-manifest.json` 的 `(xiaohongshu_browser, platform:xiaohongshu:search|fetch|comments)`（comments 在第二期加入）；checklist 的样例 ref 与单 route 操作集合；`SPECIFICATION_CASE_IDS`、第 5 章矩阵与 `tests/smoke.rs`。
9. **文档**：`GLOSSARY.md`（Access Token）；规格第 2、3、4、5、7 章；新增 ADR（小红书只经用户启用的浏览器 route、只读页面自身数据，以及 Native 正文来源）；skill 的 `platform-vocabulary.json`、`references/platforms.md`（启用步骤、登录、`access_url` 的用法、`next_cursor` 的含义）与 `references/cli.md`。

## 测试接缝

| 接缝 | 测什么 |
|---|---|
| 进程级测试，用假的 OpenCLI 可执行文件输出录制的外壳（主接缝，沿用 SSRN 的 fake opencli 测试） | 页面事实分类（未登录、461、300031、超时停在预期页为 Timeout、非预期页面与 `body_missing` 为 Runtime）；search：条件核对（`filters` 不符、页序跳号、`search_id` 变化为 Runtime）、跨页去重与截断、`hot_query` 跳过、`published` 归一化、合法空集与"无笔记但 has_more"为 Runtime、未列全诊断（末页小 limit 截断、过滤后不足 limit 两个反例）；fetch：缺 token 零调用退 2、`noteId` 不符、300031 映射、Native 正文在没有 Web Fetch 配置时写文件、arXiv/SSRN 在没有 Web Fetch 配置时仍零调用退 3、`--format content` 不落盘、写失败退 4、三项皆空为 Quality；comments：多页与 cursor 链、错误 `note_id`/错误根评论/断链为 Runtime、末页小 limit 时 `has_more` 为 true、展开回复、`reply_to`、去重、空评论；token 卫生：假 token 不出现在 ref、`url`、attempt 消息、`--verbose` attempts 与失败 stderr（覆盖 OpenCLI stderr 回显 `--xsec-token` 值、外壳解码错误、非法输入链接三类），成功输出的 `access_url` 含它 |
| types 公开函数 | ref 与各类 URL 的解析、token 拆分与格式校验、canonical URL 往返、短链与 `rednote.com` 拒绝 |
| 规格 checklist 测试 | R1–R8，包括空默认 order 的平台与 comments 的单 route 操作 fixture |
| smoke（ops 层，假子进程） | 小红书用例只执行一次；阻断后后续用例零调用、不记为通过 |

录制外壳来自本设计的实测结构，期望值写字面量。fake OpenCLI 测试不证明读取模块的页面行为（第 5 章已有此约定）；读取模块的筛选、翻页、楼中楼展开与抓包完成条件由 smoke C27–C29 与每期一次的真实验收证明，样本不满足条件时记为未验证。

## 分期

1. **第一期**：search（含三个 L1 选项）与 fetch（`metadata`、`full_text`，Native 正文来源与 Web Fetch 预检调整）、Access Token、doctor、smoke C27–C28 与停止规则、读取模块、新 ADR 与规格同步、skill 的启用与消费指引（词表、平台 reference、CLI reference），以及本期真实验收（非默认筛选、两页搜索、一次全文读取）。
2. **第二期**：comments（含 `--replies`）、smoke C29、对应的规格与 skill 指引更新，以及本期真实验收（两页评论、一次楼中楼展开）。
3. **按需**：用户主页笔记列表；`thread` 深度（笔记加有界评论）是否值得引入，等 comments 用过之后再评估。

## 第一期实现记录

第一期（#187）交付平台骨架与 search。以下是实现时对本设计所做的决定与修正，规格第 7 章「小红书」已按实现写明。

- **fetch 暂缺时的接入清单**：清单 R1、R6、R7 要求每个平台的 search 与 fetch 都有 route、smoke 用例与 fixture，而第一期只交付 search。checklist 增加待交付操作表 `PENDING_OPERATIONS = [(xiaohongshu, fetch)]`（引 #188），只豁免这一组合的 fetch 检查；catalog 的 fetch route 集合暂为空，factory 的 fetch 分支不可达。fetch 交付时删除该豁免。
- **请求体字段**：OpenCLI Browser Bridge 扩展 1.0.24 的抓包条目以 `requestBodyPreview` 携带请求体（扩展源码 `extension/src/cdp.ts`），与响应体的 `responsePreview` 对应。
- **抓包完成条件**：扩展在 `readNetworkCapture()` 时取走条目并清空请求索引，此后同一请求的 `loadingFinished` 不再写回响应体，所以读走后"继续等待同一请求"做不到。读取模块改为：Resource Timing 出现完成记录后再等 1 秒（扩展在 `loadingFinished` 后异步取响应体），再读抓包；读到的搜索条目仍无响应体就记为 `body_missing`，不再等待；命令结束或到达截止点时读到的在途请求不是命令等待的响应，不计入。完成记录由页面加载后注册的 `PerformanceObserver`（`buffered: true`）计数。
- **第一页的完成信号**：第二次真实运行中，第一页响应始终没有被计数，命令一直等到截止点（当时的分类把它报成"筛选点击数不符"）。原因没有确认，最可能是响应在计数开始之前已完成且记录已不在缓冲区。修正：第一页另以页面出现笔记卡片为完成信号（抓包在导航之前开启，响应已在其中）；筛选点击与翻页之后的响应仍以计数为准，因为那时计数已在运行。分类同时调整为先判断截止与页面，再判断点击数：截止时仍在结果页为 Timeout，消息说明完成了几次点击。
- **筛选点击**：沿用 OpenCLI 内置小红书模块已验证的 DOM 结构（`.search-layout__top > .filter` 触发、`.filter-panel` 面板、`.filters` 分组的标签文本、`.tags` 选项），先对触发元素派发悬停事件，面板未出现时再点击它，然后在页面内点击选项。点击失败时读取模块在 `filter_failure` 中写明原因（例如 `no_filter_panel for 排序依据 最新`），route 把它附在 Runtime 消息后。
- **token 脱敏**：本设计要求 route 按请求携带的 token 值清理诊断。search 的请求不带 token，响应中的 token 只经解码进入 `access_url`，所以第一期没有需要按值清理的消息；按值脱敏随 fetch 一起实现。
- **smoke 停止规则**：第一期只有 C27，它只执行一次、不重试；"阻断后后续小红书用例不启动"随 C28 一起实现。

### 第一期验收

2026-10-08 14:35–14:52 UTC，OpenCLI 1.8.6（Browser Bridge 扩展 1.0.24），用户自己已登录的 Chrome，读取模块按 skill 的方式安装为 `~/.opencli/clis/forager-xhs`。共 6 次页面命令（相邻两次之间至少间隔 20 秒）与 1 次不打开浏览器的 `contract`，没有出现登录墙、461、300031/300017 或安全限制。

| 运行 | 命令 | 结果 |
|---|---|---|
| 1 | 读取模块直接运行：`search --query 咖啡 --sort latest --publish-time week --pages 2` | 52 秒。两次筛选点击后共 4 个响应，全部有响应体：第 0 次点击的第 1 页、第 1 次点击（`sort_type=time_descending`）的第 1 页、第 2 次点击（另加 `filter_note_time=一周内`）的第 1、2 页；`search_id` 在点击后变为 `<根 id>@<子 id>`，第 2 次点击后的两页相同；`filter_note_range` 与 `filter_pos_distance` 为 `不限`。最后两页 43 条中 39 条笔记、4 条 `hot_query`，笔记 ID 无重复，token 全部为 46 个合法字符 |
| 2 | `forager platform xiaohongshu search 咖啡 --limit 25 --sort latest --publish-time week` | 112 秒后失败：第一页响应没有被计数，读取模块等到截止点。据此做了上文「第一页的完成信号」的修正 |
| 3 | 同运行 2（修正后） | 32 秒，退 0：25 条，`next_cursor: null`，stderr 给出"未列全"诊断；22 条图文、3 条视频，ref 无重复 |
| 4 | `forager doctor --provider xiaohongshu_browser` | 18 秒，深探通过（默认条件、limit 1） |
| 5 | 同运行 3，读取模块改为只在完成信号之后的读取中判定 `body_missing`（结束时读到的在途请求不再计入） | 35 秒，退 0：25 条，`next_cursor: null`，"未列全"诊断 |
| 6 | `forager platform xiaohongshu search 咖啡 --limit 5 --note-type video --sort most-liked` | 24 秒，退 0：5 条全部为视频，点赞数依次递减，`published` 均为 `YYYY-MM-DD` |

条目字段与本设计的实测一致：笔记条目为 `{id, model_type, note_card, xsec_token}`，`interact_info` 含 `liked_count`、`collected_count`、`comment_count`、`shared_count`。新观察：按"最新"排序、限一周内时，`publish_time` 全部是相对时间（`1分钟前`、`9小时前`、`3天前`），因此这类检索的 `published` 全部为 `null`，只有 `published_text` 有值。是否把相对时间换算成日期，等用过之后再定。

## 仍未实测的部分

- **token 的有效期上限**：实测到 22 分钟仍有效，更长时间未测。过期表现预计与 300031 相同，已按 Parameter 处理并提示重新搜索。
- **验证码**：本次没有触发，页面特征未知。读取模块把 461 归为 Auth，把无法识别的页面归为 Runtime，第一次真实遇到时补充特征。
- **合法空集**：两个无意义查询都返回了笔记，没能观察到真正的空结果；判定规则按接口字段设计，未经实测。
- **相对时间**：第一期验收观察到 `N分钟前`、`N小时前`、`N天前`，按规则输出 `null` 与原文；"昨天"等其他形式仍未出现。
- **抓包完成条件**：第一期验收中五次成功运行的全部搜索响应都带响应体；慢响应下的表现仍只有这两次样本。第一页响应未被计数的原因没有确认（见「第一期实现记录」）。
- **楼中楼 cursor**：楼中楼第一页的请求 `cursor` 是否等于 `sub_comment_cursor` 未比较，暂不作为核对条件。
- **截图驱动渲染的稳定性**：依赖 Chrome 在后台窗口中为截图渲染一帧的行为。若某个 Chrome 版本不再这样，翻页会以 Timeout 失败而不是返回不完整结果，届时再评估。
- **评论翻页的跨次一致性**：评论顺序在两次运行中第一页相同，但只看了前两条 ID，没有系统比较；comments 不签发 cursor，不依赖这一点。
- **账号风险**：约 55 次命令、5–10 秒间隔未触发风控，不代表长期安全；社区有只读访问被判违规的报告。skill 的平台 reference 建议使用非主力账号。
