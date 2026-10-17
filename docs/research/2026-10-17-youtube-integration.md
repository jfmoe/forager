# YouTube 接入调研

核验日期：**2026-10-17**；代码基线：`main`，`b3e0d456bfa8f7310e3e88cf22b1a49069f6e0e8`，release `v0.13.0`。本文是调研记录，未实现平台、未修改 spec 或 ADR。现有六个平台为 arxiv、ssrn、scholar、xiaohongshu、x、hn。仓库契约与关键源码的固定基线快照见 [仓库证据][E-repo]。[平台规范][F-platform]、[架构][F-architecture]。

证据保存在 [youtube-2026-10-17/][E-dir]；[source-manifest.json][E-manifest] 记录来源、取回时间与 SHA-256，[environment.json][E-env] 记录版本和出口。实测使用 `forager 0.13.0`、OpenCLI `1.8.6` / Browser Bridge `1.0.24`、yt-dlp `2026.08.19`、gh `2.102.0`。curl 走现有本机代理，Cloudflare 观测到美国出口；这不证明它是住宅 IP，也不证明浏览器或 forager 的远端抓取供应方使用同一出口。结论不外推为所有地区、账号和机房均可用。[环境][E-env]、[出口观测][E-egress]。

调研开始时本机没有 YouTube/Google/Gemini key；forager 已有 SerpApi key，但**没有调用其 YouTube 付费/计额度端点**。之后用户在自己的 Google 账号下新建 Cloud 项目和 YouTube Data API key，用于官方 API 实测；没有绑定结算账号或购买额度。浏览器使用已有会话。字幕与第三方路线的 YouTube 内容实测限于两个公开视频：`jNQXAC9IVRw` 与 `Ks-_Mh1QhMc`；yt-dlp 只取元数据和字幕，未下载音视频。官方 Data API 已带 key 实测，见「带 key 实测」；官方 Gemini API、ScrapeCreators、SerpApi、Supadata 的带凭据可用性均为**未实测**；文档中的响应示例不算实测。Gemini 另经用户自建的 CLIProxyAPI 网关做了实测，见「经 CLIProxyAPI 网关的 Gemini 实测」。[凭据可用性][E-keys]、[字幕验收][E-sub-validation]。

## 结论摘要

接入方案见 [#219][D-video]；本文只记录事实与实测。

- **官方 Data API 能找视频，不能读任意视频的字幕。** API key 可完成搜索、元数据、评论与频道/播放列表列举；`search.list` 自 2026-10-15 起为独立配额桶（默认 100 次/日，每次 1）。`captions.download` 需要 OAuth 且调用者有视频编辑权限。[Y-quota]、[Y-search]、[Y-cap-download]。
- **字幕的可用路线都不是官方授权路线。** 本机 yt-dlp 对两个公开视频提取英文字幕成功；手写或页面签名的 timedtext 请求返回 200 空正文；OpenCLI 内置 `video` 命令导航被拒。第三方供应商（SerpApi、ScrapeCreators、Supadata）文档支持字幕，但本次均未带凭据实测，也没有找到其持有 YouTube 再许可的证明。[E-sub-validation]、[E-signed]、[E-oc-video]、[SP-transcript]。
- **条款明确限制抓取。** Developer Policies III.E.6 禁止 API Client 直接或间接抓取、或取得抓取来的数据；III.E.1、III.I.7 限制下载及分离音视频；III.E.4 对 API 数据有 30 日刷新/删除要求。[Y-policies]。
- **Gemini API 可直接理解公开 YouTube 视频的音画**，适合问答，不提供原生搜索，也不保证逐字转写。[G-video]。
- **零实现选项**：现有 `forager search` 用 `site:youtube.com` 能发现视频 URL，但不保证域约束、覆盖与字幕。[E-discovery]。

## YouTube 路线对比

“文档支持”表示供应方声明了接口；“实测”只指本次保存的实际结果。费用、配额和封锁都具有时效性。[证据清单][E-manifest]。

| 路线 | 能力、鉴权与 `keys` 匹配 | 本次实测 | 限额、额外成本与条款风险 | 结论 |
|---|---|---|---|---|
| YouTube Data API v3，API key | 视频/频道/播放列表搜索、详情、公开视频评论；`key` 是单一凭据，适合 keyed HTTP | 用户自有 key 实测：search、videos、commentThreads/comments、channels、playlistItems 均返回 200；错误形状见「带 key 实测」 | search 独立每日 100 次；其他默认每日 10,000 units；须遵守刷新、展示与用途规则 | 最优基础 route；不承诺通用字幕 [Y-quota] [Y-policies] |
| 官方 captions API，OAuth | 轨列表与下载；下载者必须有视频编辑权限；需要用户 token、scope、刷新生命周期 | 未实测 | list 50、download 200 units；不是“有 Google key 就能读公开视频字幕” | 仅自有/被授权视频的后续能力 [Y-cap-list] [Y-cap-download] |
| watch HTML / timedtext / InnerTube，HTTP | 页面元数据、字幕轨与字幕；无开发者 key，不等于无源站授权要求 | HTML 和 ANDROID player 返回 2 条人工轨；裸 timedtext、页面 signed timedtext 均 200 空正文 | 非公开契约、PO Token、客户端版本、出口封锁；无公开稳定额度 | 可研究，不作为默认长线 route [E-watch] [E-signed] [E-yta-probe] |
| yt-dlp 本地工具 | 多站点 metadata/subtitles；无开发者 key；可另用会话/PO Token，但本次没有使用 | 两视频英文 VTT 成功；均未取媒体 | 额外进程与运行时；封锁和更新；现有 transport 不接受该进程；许可风险不变 | 唯一实测取得字幕且免 key 的路线；需要新的本机工具传输 [E-sub-validation] [D-pot] [F-adr20] |
| youtube-transcript-api | Python 库，watch → InnerTube ANDROID → timedtext；区分人工/自动、翻译轨 | 只重建其 player 协议，未执行库或完成其下载链 | Python 依赖；源码主动拒绝一类需 PO Token 的轨；云 IP 受限；无 SLA | 参考算法和错误语义，不引入 Python sidecar [E-yta-source] [E-yta-probe] [E-yta-issues] |
| OpenCLI 内置 `youtube` / 自有 browser | 浏览器会话；无 `keys`；search/video/transcript/comments/channel/playlist 等 | search 成功 3 条；video 退 1，导航被拒；未执行 transcript | Chrome/Bridge、DOM 漂移、会话和自动播放；内置命令缺 forager 外壳并会忽略组合筛选 | 仅参考，自有 adapter 才可注册 opt-in route [E-oc-source] [E-oc-search] [E-oc-video] |
| SerpApi | `youtube`、`youtube_video`、`youtube_video_transcript`；复用现有 `api_key` 和凭据池 | 有现成 key，但未发起 YouTube 请求 | 免费计划文档为 250 searches/月、50/小时；多引擎共用额度；每页/详情/字幕分别可能计数 | 需字幕时工程增量最小的第三方候选；先确认用途和验收 [SP-search] [SP-video] [SP-transcript] [SP-pricing] |
| ScrapeCreators | search/video/transcript/comments/playlist/channel；`x-api-key` 适合 HTTP `keys` | 无 key，未实测 | search、字幕端点标 1 credit/请求；预付 credits；源站反制与第三方条款由客户承担 | 能力广、cursor 明确；不是源站许可 [SC-search] [SC-transcript] [SC-index] [SC-terms] |
| Supadata | search、统一 metadata、transcript；`x-api-key` 适合 HTTP `keys`；原生或 AI 生成 | 无 key，未实测 | native 1 credit；生成 2 credits/分钟；默认 `auto` 会生成；免费计划 100 credits/月、1 req/s | 文字路线候选，必须强制 native，不能隐藏 ASR [SD-search] [SD-meta] [SD-transcript] [SD-pricing] |
| Gemini API，YouTube URL | 公共视频的音画理解与问答；API key 适合 HTTP；没有 YouTube 原生搜索 | 官方 key 未实测；经用户的 CLIProxyAPI 网关实测可用（见下文） | 输入/输出/可能的 thought tokens；免费档最多 8 小时 YouTube/日；数据处理条款另算 | 适合独立 ask，不能单独满足 Platform search/fetch [G-video] [G-pricing] [G-terms] |
| Web Search `site:youtube.com` | URL discovery；复用现有搜索凭据与预算 | 一次查询找到样本原视频，也混有站外 sources | 索引滞后、域约束与模型引用不可靠；无原生页游标、字幕保证 | 零实现与补充发现；不充当平台 route [E-discovery] [F-adr19] |

## 官方 API：端点、配额与凭据

### 当前配额不能沿用“search 一次 100 units”旧经验

**2026-10-17 取回的 Quota Calculator 与 search.list 文档，均更新于 2026-10-15，明确给 search 独立配额桶：每日默认 100 次，每次 1 unit。** API Overview、Quota and Compliance Audits 也列出相同分桶。已用 curl 直接取官方域名原始 HTML 交叉核验，不仅依赖搜索摘要或抓取供应方缓存。[原始 quota 响应][E-quota-origin]、[原始 search 响应][E-search-origin]、[quota 文档][Y-quota]、[概览][Y-overview]、[审计][Y-audit]。

| 方法 | 用途与建议调用方式 | 当前文档成本 | 鉴权与重要限制 |
|---|---|---|---|
| `search.list` | `part=snippet&type=video&q=...&maxResults=...`；`pageToken` 续页 | 独立 Search Queries 桶 **1/次，默认 100 次/日** | 公共检索用 API key；一页最多 50；有 `q`、order、时间、channelId、语言相关性、地区、caption 等筛选 [Y-search] |
| `videos.list` | `id` 定位，按需取 `snippet,contentDetails,statistics,status,liveStreamingDetails` | 1 unit/次 | 公共字段用 key；某些 owner-only part 需授权；`id` 批量读取与 search 分页是两种请求，不能照搬 `maxResults` [Y-videos] |
| `commentThreads.list` | `videoId`，`part=snippet,replies`，`textFormat=plainText` | 1 unit/页 | 一页 1–100；order `time/relevance`；评论关闭是错误，不是“无人评论” [Y-commentthreads] |
| `comments.list` | `parentId` 取某一级评论的回复，或按 comment id | 1 unit/页 | thread 中自带 replies 不一定完整，要完整读取需另分页 [Y-comments] [Y-comment-resource] |
| `channels.list` | ID/handle 查频道；`contentDetails.relatedPlaylists.uploads` 找 uploads playlist | 1 unit/次 | 公开数据用 key；`mine` 等用户上下文需 OAuth；handle 到 channel ID 是联网查询 [Y-channels] [Y-channel-resource] |
| `playlistItems.list` | uploads 或指定 playlist 的条目，保留位置与 `videoId` | 1 unit/页 | 一页最多 50；私有列表需权限；不是全站搜索 [Y-playlistitems] |
| `captions.list` | 返回 caption track 资源，**不返回字幕正文** | 50 units/次 | OAuth scope 为 `youtube.force-ssl` 或 `youtubepartner`，权限不足可能 403 [Y-cap-list] |
| `captions.download` | 按 caption id 下载，可选格式/目标语言 | 200 units/次 | 同样需要 OAuth，且用户有视频编辑权限；不能下载任意公开视频字幕 [Y-cap-download] |

其他端点合计默认 **10,000 units/日**，`videos.insert` 另有每日 100 次桶，虽然本方案不使用写操作。每日在 **Pacific Time 午夜**重置；无效请求也至少消耗 1 unit，新增一页再次计费；提额要做合规审计。这些是文档默认值；新建测试项目的控制台默认值与之一致，另有文档未列的 Video Batch Get Stats 配额组，见「带 key 实测」。不要把一次 metadata fetch 的 1 unit 解释成一次付费账单金额。[Y-quota]、[Y-overview]、[Y-audit]。

预算示例（计划，不是实测）：一页 search + 一次 `videos.list` 批量增强，消耗 **Search Queries 1 + 其他桶 1**；一次 metadata fetch 为其他桶 1；一页 comments 为其他桶 1；一次官方字幕 list+download 为其他桶 250 且须有权限。默认 search 不额外取每条详情，只有用户需要 duration/counts 时才考虑一次批量增强，失败时不得补造字段。查频道最新上传优先 uploads playlist；官方 search 文档警告 `order=date` 可能索引延迟、结果不完整。[Y-search]、[Y-channels]、[Y-playlistitems]、[Y-cap-list]、[Y-cap-download]。

### `keys` 可复用，但禁止配额规避

`youtube_api` 可投影 `KeyedHttpRouteRuntimeConfig`，配置为 `providers.youtube_api.url/keys/timeout`。接入时建议 schema 将 `keys` 限为**至多一个、用户所属项目的 key**；空数组仍表示未配置。不内置共享 key，不让用户把多个项目塞入池来分摊同一客户端配额。现有池会在 QuotaExhausted/RateLimited 时轮换，所以“直接复用池”需要这个边界，不能原封不动宣传成 YouTube 提额办法。API Terms §15 与 Developer Policies III.D.1.c 要求遵守限制、同一 Client 只建一个项目。[F-platform]、[E-repo]、[Y-api-terms]、[Y-policies]。

官方返回的 `quotaExceeded` 与权限类错误可能同为 HTTP 403；需在现有网络错误归因拥有层核验供应方错误 reason，并保持唯一 status 映射纪律，不能一概 Auth 或让 adapter 复制整套 HTTP mapper。额度耗尽不自动切到 browser、其他项目或付费供应方；是否运行替代路线必须是用户已配置且用途允许的行为。单 key 先避免额度轮换；若未来允许多凭据，必须先设计不会绕过项目配额的执行规则。[API 错误][Y-errors]、[Y-api-terms]、[错误模型与 net 边界][F-architecture]。

官方 captions 不是“再加一个 bearer 字符串”即可长期支持：access token 有效期、refresh token、client identity、scope、撤销与删除需共同管理，现有 `keys: [String]` 没有这些生命周期字段。短期 token 塞进 keys 虽可发 HTTP，但不是可交付的 OAuth 方案；仅为公开 metadata 不应引入 OAuth。不要把 service account 当作通用个人 YouTube 账号的替代。[OAuth 指南][Y-auth]、[caption 授权][Y-cap-download]、[现有凭据定义][F-glossary]。

### 带 key 实测

2026-10-17 用用户自有 Cloud 项目的标准 API key（未绑定 service account）调用官方 Data API。key 的 API 限制只含 YouTube Data API v3，应用限制为 None：本机 CLI 不发 Referer，出口经代理且 IP 不固定。key 存于 macOS 钥匙串，只经 `X-Goog-Api-Key` 请求头发送，证据中没有 key 与项目编号。创建项目、启用 API、创建 key 时，Console 没有要求绑定结算账号或信用卡；官方 Getting Started 也只要求 Google 账号、项目、启用 API 和凭据。共 21 次只读请求：search 8 次，其他端点按文档计 13 units（含 key 设置时的 1 次 `videos.list` 验证），另有 1 次假 key 请求不计入本项目。评论只记录 ID、计数与长度，不记录正文和作者。[实测证据][E-live]、[脚本][E-live-script]。

**search.list**

- `type=video&maxResults=5` 第一页和用 `nextPageToken` 取得的第二页没有重叠；第二页另带 `prevPageToken`。`pageInfo.totalResults` 是近似值：通用查询报 1,000,000；3Blue1Brown 频道内 search 报 334，而 uploads playlist 报 246。
- `channelId + order=date` 按 `publishedAt` 倒序，前 5 条与 uploads playlist 第一页一致。
- `videoDuration=short + videoCaption=closedCaption` 返回的 5 条，经 `videos.list` 复核均短于 4 分钟，且 `contentDetails.caption` 均为 `"true"`。
- 日期边界两端都包含：以视频发布时间 T=`2026-09-25T13:35:02Z` 为界，`publishedAfter=T` 与 `publishedBefore=T` 两个窗口都返回该视频，与文档“at or after / before or at”一致。只加时间窗口、不指定 order 时，结果不按时间排序。
- 只传日期 `publishedAfter=2026-10-01` 返回 HTTP 400：`errors[0].reason=invalid`，`status=INVALID_ARGUMENT`，提示时间戳必须以 `Z` 结尾或带时区偏移。该 reason 不在 search 文档的错误表中。
- 合法空集返回 HTTP 200：`items: []`，`pageInfo.totalResults=0`，没有 `nextPageToken`。
- search 结果 `snippet.title` 经过 HTML 转义（`&#39;`、`&amp;`）；同一视频在 `playlistItems` 中的标题未转义。

**videos.list**

- 三个样本可一次批量取回。`duration` 为 ISO 8601 时长（`PT19S`、`PT21M3S`、`PT26M10S`），`contentDetails.caption` 是字符串 `"true"` 而不是布尔值，`publishedAt` 精确到秒并以 `Z` 结尾。`defaultLanguage` 均为 `en`；`defaultAudioLanguage` 同时出现 `en` 和 `en-US`。
- 请求了 `liveStreamingDetails`，但非直播视频的响应直接省略该字段，不返回空对象。
- 不存在的 ID `zzQq9ZxZ9Zq` 返回 HTTP 200、空 `items`、`totalResults=0`。与存在的 ID 混批时只返回存在的那条，不报错，也不标记缺失项。

**commentThreads.list 与 comments.list**

- `jNQXAC9IVRw`、`part=snippet,replies`、`maxResults=20`：`order=relevance` 与 `order=time` 各返回 20 条，并带 `nextPageToken`。thread 内嵌的回复最多 5 条；relevance 页有 16 个 thread 的内嵌回复少于 `totalReplyCount`。对其中一个 `totalReplyCount=1000` 的 thread 调用 `comments.list?parentId=...&maxResults=100`，返回 100 条和 `nextPageToken`；`comments.list` 的 `pageInfo` 没有 `totalResults`。
- `order=time` 的第一条是一条 2020 年的高赞评论，与 relevance 第一条相同，其余按时间倒序（最新为当天）。可能是置顶评论，因此不能假定 time 排序的整页严格按时间单调。
- 关闭评论的视频 `XqZsoesa55w`（`status.madeForKids=true`）：`commentThreads.list` 返回 HTTP 403，`reason=commentsDisabled`，`domain=youtube.commentThread`，`location=videoId`，与文档一致。同一视频的 `videos.list statistics` 中没有 `commentCount` 字段（单样本观察）。

**channels.list 与 playlistItems.list**

- `forHandle=@3blue1brown` 返回 1 条：channel ID `UCYO_jab_esuFRV4b17AJtAw`，`relatedPlaylists.uploads=UUYO_jab_esuFRV4b17AJtAw`。
- uploads 第一页按发布时间倒序；公开播放列表 `PLZHQObOWTQDPD3MizzM2xVFitgF8hE_ab` 按 `position` 排列。`snippet.publishedAt` 是条目加入列表的时间，可比 `contentDetails.videoPublishedAt` 晚几十秒；视频发布时间应取后者。

**错误形状**

- 格式正确的假 key 返回 HTTP **400**，不是 403：`errors[0].reason=badRequest`，`domain=global`；具体原因只出现在 `details[]` 的 `google.rpc.ErrorInfo.reason=API_KEY_INVALID`，并带 `metadata.service=youtube.googleapis.com`。官方 Data API 错误文档没有列出这一形状。
- 本次观察到三种错误形状：400 `invalid`（参数格式）、400 `badRequest` 加 `API_KEY_INVALID`（凭据）、403 `commentsDisabled`。为免影响正常配额，没有触发 `quotaExceeded`。

**控制台核对**

- 测试项目的「指标」页按 1 小时窗口统计到 21 次请求：search 8（错误率 12.5%，即日期格式那 1 次 400）、videos 6、commentThreads 3（错误率 33.33%，即 `commentsDisabled` 那 1 次 403）、playlistItems 2、channels 1、comments 1。这与脚本 20 次真 key 请求加 1 次验证完全一致；假 key 请求没有记到本项目名下。
- 「配额和系统限制」页列出 9 项默认配额：Queries per day 10,000、Queries per minute 1,800,000、Search Queries per day 100、Search Queries per minute 100、Video Uploads per day/minute 各 100、Queries per minute per user 180,000，以及 **Video Batch Get Stats Queries per day 10,000 / per minute 600**。最后这组在本次存档的官方配额文档中没有出现，控制台没有说明它对应哪个方法。
- 实测结束约 3 分钟后，所有配额的“当前使用量”仍为 0，计数有延迟；稍后再读时为：Search Queries per day **8**，Queries per day **13**，Video Batch Get Stats 与 Video Uploads 均为 0。与文档计价完全一致：search 桶包含那次 400 的日期格式请求，其他桶 13 包含那次 403 `commentsDisabled` 请求，即失败请求同样计费；带 `statistics` 的 `videos.list` 不消耗 Video Batch Get Stats 配额。

## 字幕/转写：源码、近期问题与本机结果

### 实测结果及覆盖边界

| 检查 | 结果 | 能证明什么、不能证明什么 |
|---|---|---|
| 匿名 `oembed`，`jNQXAC9IVRw` | HTTP 200，有标题/作者/嵌入信息 | 已知 URL 的轻量身份辅助；不是 native search，不提供 transcript，不满足完整平台契约 [E-oembed] |
| 匿名 watch HTML，同一视频 | 200，读到 matching videoId，英文/德文两条非 `asr` caption track | 页面 bootstrap 在本出口可读；不能据此断言字幕可下载 [E-watch] |
| 裸 `/api/timedtext?v=...&lang=en&fmt=json3` | 200、0 字节 | 这种手写 URL 未取得字幕；不据它认定视频没有字幕 [E-timedtext] |
| 页面 caption track 的 signed baseUrl 加 `fmt=json3` | 200、0 字节 | 签名链接也未成功；可能涉及播放器证明/访问条件，**本次未证实具体根因**，未重试绕过 [E-signed] |
| 按 youtube-transcript-api 源码重建 ANDROID `player` 请求 | 200，`playabilityStatus=OK`、matching ID、en/de 两轨 | 仅证实协议前半段；未运行下载的库，未验证库的 end-to-end fetch [E-yta-probe] |
| yt-dlp 两个视频的 metadata 模式 | 退出 0，均列出英文轨；初次 `--dump-single-json` 为模拟读取 | 仅 metadata/轨列表；不能计为字幕下载 [E-dlp-zoo] [E-dlp-ted] |
| yt-dlp 显式 `--no-simulate --skip-download --write-subs --write-auto-subs --sub-langs en --sub-format vtt` | 两次退出 0，VTT 文件存在；19 秒样本 6 cues、440 bytes；约 21 分钟样本 428 cues、33,409 bytes | 本出口、版本、两个样本的字幕提取成功；不证明云端稳定、自动轨/翻译/无字幕/直播都成功 [E-dlp-sub-zoo] [E-dlp-sub-ted] [E-sub-validation] |
| OpenCLI `search 'Me at the zoo jawed' --limit 3 --type video` | 退出 0，3 个结果，首条原视频 | 只验证该查询和一个 type；没有组合筛选/分页验收 [E-oc-search] |
| OpenCLI `video`，同一视频 | 退出 1：`Navigation rejected` | Bridge 健康不等于具体命令能工作；仓库已有同类导航限制说明 [E-oc-video] [F-opencli] |

yt-dlp 均使用 `--ignore-config`、`--no-playlist`、`--no-cache-dir`、重试次数 0、请求间隔 3 秒，没有 Cookie、浏览器 Cookie 导出、账号或 PO Token 插件。两个样本原本就有人工英文轨，因此 `--write-auto-subs` 同时出现不证明实际测到了自动轨；本轮没有为了补齐覆盖继续批量抓取。VTT 的时间边界验收是结构检查，不是语音准确性或视频完整性的测量。[命令记录][E-dlp-sub-zoo]、[第二样本命令][E-dlp-sub-ted]、[字幕结构][E-sub-validation]。

### youtube-transcript-api：轻量不代表正式字幕 API

浅克隆固定到 `8f150ba8836da30a36bcd40e8fca226ed179ba72`，`pyproject.toml` 版本 `1.2.4`。源码先 GET watch，抽出页面公开客户端 InnerTube key，再 POST ANDROID `20.10.38` 的 `/youtubei/v1/player`，从 `captionTracks` 分人工和自动轨，最后 GET timedtext 并解析 XML。源码还明确在轨 URL 含 `&exp=xpe` 时抛 `PoTokenRequired`，而不是已有通用 PO Token 生成器；这与用户自建 Google Data API key 无关。[源码快照][E-yta-source]。

近期维护证据应分层理解：README 明确提醒云服务 IP 可能被阻断；issue #593 的维护者回复建议住宅代理，但其 2026-10-16 新回复含第三方服务推广，不能把其中成功率数字当事实。#592 仍开放，包含 PO Token/空正文报告；#612 报告 POST 重试未覆盖的问题，仍开放。它们证明维护面临具体问题，不能证明今天所有客户端均失败，也不能据“closed/completed”断言问题已修复。本次 ANDROID probe 成功列轨，只是局部验证。[README][D-yta]、[#593 与维护者回复][E-yta593]、[#592][E-yta592]、[#612][E-yta612]、[实际 probe][E-yta-probe]。

不建议把该 Python 库作为 Rust CLI 的 sidecar，或逐行移植成 Rust 以免 Python：两者都继续承担非公开接口、封锁、客户端模拟和条款问题。若将来在许可明确的范围内自建 HTTP adapter，可参考其人工/自动轨选择和分类，但传输、限速、Deadline 与 errors 必须仍由 forager 对应拥有层实现。[E-yta-source]、[F-architecture]、[Y-policies]。

### yt-dlp：本机实测与维护状况

已用 `gh repo clone ... -- --depth 1` 只读检查 yt-dlp，固定提交 `897341241f2bfb2439fe0367f4c7c9ccd29a3828`；它与本机实际运行的 `2026.08.19` **不是同一个版本**。源码有 YouTube 客户端、字幕和 PO Token 分支；官方项目 PO Token Guide 警告 YouTube 正逐步要求证明，某些客户端的字幕也受影响。近期 #13770 在 2026-10-15 仍有字幕 429 报告，但维护者旧回复与新用户反馈的日期不同，不能把旧 issue 的关闭视为当前可用性保证。[源码][E-dlp-source]、[项目指南][D-pot]、[issue 与回复][E-dlp13770]、[环境][E-env]。

它的优势是既有站点维护和多格式解析，本次两个字幕样本也跑通；代价是二进制/脚本版本、可能的 JS runtime/PO Token 依赖、子进程管理，以及后续上游变动。不要为无关字幕需求默认开启 cookie、代理轮换、媒体下载或挑战求解；本次没有尝试这些路径。[D-readme]、[D-pot]、[F-adr20]。

### OpenCLI：内置命令只能作为原型材料

本机内置集合还含 like/subscribe/login/history 等操作，本提案仅涉及公开内容的只读命令，不接入账号写操作。源码有四个具体不匹配：[安装源码快照][E-oc-source]、[CLI 清单与 Bridge 状态][E-env]。

1. `search` 对 type、upload、sort 按优先级三选一；同时传入不会完整执行。它把非数组错误直接变成 `[]`，无法证明合法空集；channel/playlist 类型也不由目前仅识别 video/reel 的解码完整覆盖。[E-oc-source]
2. `video` 从浏览器同源 fetch watch HTML，返回 field/value 字符串数组，需要 Rust 再核对 videoId、日期精度、playability 和结构，不能只包装 stdout。本次甚至未成功进入读取步骤。[E-oc-source]、[E-oc-video]
3. `transcript` 选择轨、启动播放器、读取/再请求带 PO Token 的 timedtext，并临时改写 fetch/XHR；fallback 可能选择另一语言。它不是只读字幕面板的被动快照。本次未执行该命令，避免额外播放与会话副作用；源码存在不等于转写已实测成功。[E-oc-source]
4. 没有 `forager-youtube/<version>` 外壳、真实筛选事实、partial/empty 分类、signed URL 脱敏和本仓库的支持检查。内置更新可改变行为，不能直接登记为稳定 forager adapter。[F-platform]、[F-glossary]、[E-oc-source]

若以后选择 browser，编写自有 `forager-youtube` adapter：优先正常页面/字幕面板触发的请求，回报页面事实，由 Rust 核对；导航前按仓库模式开启抓包，原始 signed 字幕 URL 和会话令牌留在浏览器。建议每 10 秒至多一命令、并发 1、默认不重试；这是保守产品节奏提案，**不是 YouTube 授权或安全额度**。不实现挑战绕过；明确人工验证时失败为 Auth。[F-opencli]、[F-adr20]、[F-adr22]、[Y-terms]。

## 匿名读字幕的风控报告

本节回答：不带 cookie、不登录的 InnerTube 字幕读取（watch 页 → `/youtubei/v1/player` ANDROID → timedtext，与 youtube-transcript-api 相同）实际触发过哪些风控。证据共 60 条，来自 GitHub issue、yt-dlp wiki、X、Hacker News、Reddit 与 YouTube/Google 官方页面；每条记录 URL、日期、来源类型、是否一手、IP 类型、是否登录与风控类型。X 的关键帖已用 oEmbed 对照原文；Reddit 原帖本机直连和存档均不可读，只作线索。多数报告针对 yt-dlp 下载视频而非只读字幕，少有精确请求速率；下面的数字都是报告者自述，不是 YouTube 公布的阈值。[风控报告][E-risk]。

### IP 级与账号级要分开

| 层级 | 可信案例 | 是否登录 | 表现 |
|---|---|---|---|
| IP / 会话级 | 多数报告（C1–C12、C14、C17–C20、X1–X4、X9、H3） | 均为匿名 | 429、"Sign in to confirm you're not a bot"、timedtext 返回空 200、`RequestBlocked`/`IpBlocked`、限速 |
| 账号级 | C2、C13、C15、C16（另有 M4、M5、M11 维护者表态） | **全部带登录**：cookies 或 OAuth | 账号在网页端所有视频 "Video unavailable"；或账号被封、浏览器中也无法观看 |
| 匿名访问导致账号被封 | **未找到可信案例** | — | 唯一相关说法 X5 称“按 IP 封禁该 IP 下所有 Google 账号”，没有任何案例或来源；X6、X7 为相反的个人经验 |

账号级案例的共同点是把账号凭据交给自动化工具：C2 用一次性 Gmail 走 OAuth 并压测后被封，且在浏览器中也无法观看；C13 与 C16 是把 cookies 传给 yt-dlp 后账号被禁止经网页端播放。yt-dlp 维护者的表述一致：不传 cookies 就没有账号被封的风险；传 cookies 则“迟早”会被封，通常是数月的临时封禁。[E-risk] M4、M5、M11、C2、C13、C16。

### 同一出口 IP 上的浏览器与账号

- **IP 被标记后，同 IP 的新匿名浏览器会话会受影响**：Hetzner VPS 被封后，新开的或隐私模式浏览器也弹出登录验证，但一个一两个月前就存在、从未登录的 Firefox 会话仍可播放（C14）。cobalt 在极高流量下被整体标记为 bot，“这种状态下无法观看视频”（X1）。Reddit 线索中也有被封 IP 下浏览器显示视频离线、约 24 小时恢复的转述（R1，未核原文）。
- **反向报告**：家用 IP 上对自动翻译字幕持续重试、绝大多数请求 429 时，同 IP 未登录的 Chrome 照常可用（C12）；youtube-transcript-api 维护者称 IP 封锁针对的是浏览器不使用的端点（M13）。
- **同 IP 已登录账号被牵连**：没有找到案例。Google 官方只就 Google 搜索说明，同一网络、VPN 或 ISP 上他人的自动化流量会让所有用户看到 “unusual traffic”（O3）；没有找到 YouTube 关于 IP 与账号关联处罚的官方说明。

### 触发条件、持续时间与恢复

- **IP 类型是最强因素**。云和机房 IP（AWS、GCP、Azure、DigitalOcean、Hetzner、Railway、Render、Colab）常在首个或极少请求时就被拦（C1、C11、C14、M8、M10）。住宅 IP 也会被拦，ISP 共享 IP 甚至可能首次运行就被拦（C6、M13）。有一条线索称共享 IPv4 更宽松，未核原文（X11）。
- **请求量的自述差异很大**：2–3 个字幕后 timedtext 429（C5，2025-07）；约 10 个（X2）、35 个（C4）、151 个（C8）、154 个（C3）、约 600 个下载（C17）后被拦；也有人用随机间隔取得 1 万以上字幕且未被拦（X3），有人约每 3 秒一次即遇 429（X4）。yt-dlp wiki 给出的访客会话参考值约为每小时 300 个视频（约 1000 次网页或 player 请求），建议每次间隔 5–10 秒（M1）。字幕端点另有更严的限速：yt-dlp 维护者 2026-01 的结论是只影响**自动翻译**字幕，人工字幕和原语言自动字幕不受影响，等待约 60 秒可绕过（M6）。
- **并发与持续时间**：多实例并发约 1 小时后被拦（R4，未核原文）；维护者把“下载很多且不加 sleep”与可疑 IP 并列为主要诱因（M7）。
- **持续时间与恢复**：自述从十几分钟（换 VPN 国家，C4）、数小时到 1–3 天（C5、C20、R1、R3、R4）、2–3 周（C19、R2、R5）不等。恢复手段是等待或更换出口 IP（重启路由器、换 VPN 节点，C18、C4）；yt-dlp FAQ 称 429 多为软封，可在浏览器完成 CAPTCHA 后带 cookies 继续（M2），但那样就进入了带登录、承担账号风险的路径。youtube-transcript-api 维护者承认没有限速或封锁时长的公开信息，报告也不一致（M11）。

### 2025–2026 趋势

- 2025-06 起 timedtext 对网页端派生的 URL 普遍返回空 200（PO Token 执行）；youtube-transcript-api 2025-06-11 改用 InnerTube ANDROID 客户端后恢复（C9）。2026-04 仍有个别视频因 `exp=xpe` 返回 `PoTokenRequired`，维护者称当前版本不应出现并在收集报告（C10）。
- 2025-07 起 timedtext 限速明显收紧，429 被映射为 `IpBlocked`（M12、C5）。yt-dlp 方面，字幕 429 集中在自动翻译轨（M6）。
- PO Token 不是 IP 封锁的解药：带有效 PO Token 仍被拦，原因被归于请求过快或机房 IP（M3）。ANDROID、iOS 客户端不支持账号 cookies（M9），匿名 ANDROID 路线本身就不涉及账号。
- 条款层面，YouTube ToS 禁止除搜索引擎外未经许可以自动化方式访问，并保留对违约 Google 账号暂停或终止的权利（O1）；这与是否发生过技术封禁是两回事。

### 证据不足之处

- 没有 YouTube 官方的阈值、封锁时长或账号关联规则；TeamYouTube 的回复都是年龄验证与 bot 验证的模板（O2）。
- 绝大多数报告没有精确的请求速率、并发和时间窗；只读字幕的报告少于下载视频的报告。
- “同 IP 已登录账号被牵连”既无案例也无反例实测；同 IP 浏览器受影响的证据只针对新的或匿名会话。
- Reddit 线索均未读原文；X 中未经 oEmbed 核对的条目（X11、X12）只按转述使用。
- 维护者推荐住宅代理时带有 Webshare 联盟链接（M10），C2 的报告者在同一讨论中推广付费服务；这些立场不影响其自述事实，但相关建议不作中立结论。

## 第三方 HTTP 与模型路线的细节

### SerpApi：现有供应方复用收益最大

除搜索和视频详情外，当前还有独立 `youtube_video_transcript` 引擎，文档给出 `start_ms/end_ms/snippet`、chapters、available_transcripts，支持 `language_code`、轨标题和 `type=asr`。**请求语言不可用时会退到第一条可用语言**，adapter 必须核对实际轨，不能直接宣称满足 `--lang`。缓存精确匹配请求参数、有效一小时，命中不计月 searches；没有付费请求实测，不能承诺某个样本一定命中缓存或不扣额度。[SP-transcript]。

若获准选择此路线，沿 `serpapi.rs` 入口增加私有 YouTube 引擎模块，共用 URL、keys、account probe；**现有代码仍将 attempt target 固定为 Scholar，并只认 `organic_results` 成功形状**，需要按真实调用者提取传输/成功协议边界，不能直接换 engine 字符串。禁止把 Scholar 的 `Fully empty` 判定复制给所有 YouTube 响应。video、字幕和搜索分别解码与核对目标身份，评论也要处理独立 continuation。[E-repo]、[SP-video]、[SP-search]、[SP-transcript]。

免费 250/月、50/小时是当前计划文档，不是每个引擎各一份。公开视频 metadata + transcript 往往是两次供应方操作；search 再 fetch 再字幕可能三次。供应方 release notes 记录 2026-01/03/04 的 transcript 空响应修复，表明抓取维护仍必要。其法律条款禁止非法用途/侵权、限制保证；没有找到此集成所需的 YouTube 再许可证明，“Legal Shield”营销不替代源站许可。[SP-pricing]、[SP-releases]、[SP-legal]、[Y-policies]。

### ScrapeCreators：覆盖广，但语言和源站许可须核对

search 文档有 `continuationToken`、videos/shorts/channels/playlists、uploadDate、sortBy、duration；其“under_3_min”等 duration 与 Data API 的 short/medium/long **不是同一规则**。transcript 为单独 GET，1 credit，输出毫秒分段和 language；当前 `language` 不匹配时返回 null 且不扣 credits，`original_audio=true` 优先于 language。默认选轨也不等于“人工优先”，不能静默映射为统一策略。[SC-search]、[SC-transcript]。

首页宣传无账号级 rate/concurrency cap，建议小于 500 并发；这是供应商自述，不是 forager 应采用的访问节奏，也不证明源站允许。API 文档有缓存参数、命中 0 credits，但缓存新鲜度需投影出来。条款 §§2、7–9 明确非平台关联、第三方条款由客户负责、可能失效并保存请求/响应数据；首页“credits never expire”与条款保留变更/失效权的表述存在边界，预算以实际合同/账户为准。本次无 key 未验证 credits、续页和响应。[SC-home]、[SC-transcript]、[SC-terms]。

### Supadata：必须避开默认隐式生成

Supadata 同时有 search 与 metadata，因此并非只能做“字幕补丁”。search 不传 limit 时一页约 20 条、1 credit、有 nextPageToken；传 limit 时服务器自动翻页，成本 `ceil(limit/20)` 且不再返回 cursor。forager 若想保持“单页、可控预算”，首期应限为实际页容量，或显式定义有界自动分页；不能截掉余项后直接拿下一页 token、造成漏结果。[SD-search]、[SD-meta]。

统一 transcript 默认 **`mode=auto`**，原生字幕没有时会 AI 生成；提案要求 route 固定 **`mode=native`、`text=false`**，保留时间段。`lang` 只是偏好，不可用会返回首个语言，必须核验实际语言。`206 transcript-unavailable` 仍扣 1 credit；不是 HTTP 成功就能构造 `full_text`。生成是 2 credits/分钟，超过 20 分钟自动转异步 job，某些平台 URL 途中也可能转 202；超时仍可能计费，不能沿普通网络 retry 无意重复开任务。ongoing live 不支持，文档上限为 12 小时/750 MB。[SD-transcript]。

当前免费计划 100 credits/月、1 req/s，付费计划提高频率并可开启 auto-recharge；本方案不更改账户或自动充值。条款 §§4–5、8–11 保留原作者权利，把平台条款遵守责任交给用户，涉及第三方数据提供方；没有源站授权保证。未来若做 generation，必须单独设计预算、job 续接、取消/超时后结果未知和证据来源，不能作为 Native 缺失时无提示的 fallback。[SD-pricing]、[SD-terms]、[SD-intro]。

### Gemini API：视频理解成立，逐字稿与原生搜索不成立

官方文档允许把 YouTube URL 直接放入请求，处理音频和画面；只支持 public，明确排除 private/unlisted。免费档每日最多 8 小时 YouTube；2.5 及之后模型一次最多 10 个视频。付费档“没有按视频长度设的日限制”不代表无 context、rate limit 或费用限制。默认静态采帧是 1 FPS；部分当前模型支持 agentic video processing，实际行为随 model 改变。[G-video]。

这适合 `platform youtube ask VIDEO_REF QUESTION`：必需输入是视频与问题，与 X 的检索型 ask 不同。建议初版单视频、显式时间段和成本预估，答复带 ref、模型、分析区间和定位时间戳；时间戳是模型输出，仍需回看，不能标为逐字引文。默认不开 Web grounding 或扩展到模型自行寻找的其他视频；多视频比较是后续范围。官方能力文档不保证句级字幕、逐帧覆盖或搜索结果，因此 Gemini 不可单独冒充 `youtube_api` 的 search/fetch，也不能“生成 transcript 后宣布 Native full_text”。[G-video]、[F-adr26]。

费用按当前模型的输入/输出与处理策略计算；视频 token 随时长、采样与分辨率增长，agentic 的探索还涉及 thought tokens。不要固定一条“每视频几分钱”估价。当前文档给静态低/高分辨率约 100/300 tokens/s 的估算，实际以模型 usage 为准。Gemini API 免费/付费服务的数据处理条款不同，免费服务可能用于产品改进；开了 billing 的项目分类也影响条款，不能仅按本次请求金额判断。未调用 API，未检验时间戳或生成质量。[G-video]、[G-pricing]、[G-terms]。

### 经 CLIProxyAPI 网关的 Gemini 实测

用户自建的 CLIProxyAPI 网关以 Antigravity 账号提供 Gemini Flash。源码（commit `911f49c`）中，Gemini 原生端点 `/v1beta/models/<model>:generateContent` 的请求转给 Antigravity 上游（`cloudcode-pa.googleapis.com` 的 `v1internal`）时只规范化 role、改写函数名、清理思考签名，`fileData` 原样透传；OpenAI 兼容端点把 `video_url` 转成 `inlineData`，YouTube URL 不会作为 `fileUri` 发出，因此只能用原生端点。[网关实测][E-gateway]。

| 检查 | 结果 |
|---|---|
| 19 秒视频 `jNQXAC9IVRw` | 200，约 10 秒；prompt tokens 从纯文本的 8 增至 1747；答出标题与字幕中没有的画面细节（两头大象、金属栏杆、外套颜色），口述内容与原视频一致 |
| 21 分钟视频 `Ks-_Mh1QhMc` | 200，16–21 秒；prompt tokens 115,036（约 91 tokens/秒）。三个短语的时间戳都落在人工字幕对应条目的时间区间内或相差 1 秒以内；00:15–00:40 的转写与人工字幕一致（另一次带有人工字幕删去的 “um”）；画面中未朗读的文字（“JUNE 2012 / EDINBURGH SCOTLAND”、幻灯片单位 `pg/ml`）被识别 |
| `mediaResolution=MEDIA_RESOLUTION_LOW` | prompt tokens 不变；重复请求报告 113,259 个缓存 token |
| 不存在的视频 ID | 上游 403 `PERMISSION_DENIED`，没有编造内容 |
| 网关冷却 | CLIProxyAPI 对上游 401/402/403 默认冷却整个凭据 30 分钟（`sdk/cliproxy/auth/conductor_cooldown.go`），一次视频 403 使网关上全部 Gemini 模型返回 503 `auth_unavailable`；设置 `cooldown.disable-cooling: true` 后，403 不再影响后续请求 |
| 模型 | `gemini-flash-latest` 与 `gemini-3.8-flash-high` 都报告 `modelVersion` 为 `gemini-3.8-flash-n` |

这证明经网关的视频理解在技术上可用，时间戳可用于定位；它仍是模型输出（Relayed Content），不是逐字稿。经第三方反代使用 Antigravity 订阅额度是否符合 Google 的服务条款，本次没有核实。

## 条款与数据生命周期

以下为原文约束与工程影响的对应分析；**技术可用不等于用途已获许可**。网站阅读、API Client、第三方供应商和模型服务各有适用文件；开源代码许可证也不授予源站内容抓取或再分发权。具体商业/研究用途的适用性仍需确认，本文不把公开访问或已有账号视为授权。[Y-terms]、[Y-api-terms]、[Y-policies]、[SD-terms]、[SC-terms]。

| 一手条款位置 | 已核实要点 | 对本提案的影响 |
|---|---|---|
| YouTube ToS，“Your Use of the Service → Permissions and Restrictions” (1)、(2)、(3) | 限制未获授权的复制/下载/使用、规避限制、自动访问；(3) 的公开搜索引擎例外须遵守 robots，另有书面许可路径 | 普通研究 CLI 不自动成为公开搜索引擎；低频、个人本机、登录和 robots 可读均不是抓取许可 [Y-terms] |
| 同一 ToS，“License to Other Users” | 其他用户的使用权限于服务功能所支持的使用，不授权独立于服务使用内容 | “公开视频”与“可作为长期字幕语料分发”分开判断 [Y-terms] |
| API Terms §§2.1–2.2、3.1、5、7–8、10.3、15 | 协议及文档/政策共同约束访问；须合法访问、保护数据、正确归因，不得超额或绕过配额 | 官方 key 不是无限内容许可；申请提升而非多个项目轮换 [Y-api-terms] |
| Developer Policies III.D.1.b–d、III.D.3、III.D.6 | 使用分配给自身项目的 credentials；一个 Client 一个 project；key 不嵌入开源；提额/禁用有规则 | 单 key，凭据不出日志/URL/cursor，不把 key 池当配额聚合器 [Y-policies] |
| III.D.7 | 未经明确许可，不得使用未文档化的 API；按授权文档访问服务，不得逆向未文档化服务 | InnerTube/player/timedtext 的技术可用性不能替代官方 API 授权 [Y-policies] |
| III.E.1；III.I.7、III.I.14 | 未经书面批准限制音视频下载/存储；禁止分离音视频组件及通过其他技术取得 API Data | “下载音频再 ASR”不是获取字幕的无成本替代；官方+抓取混合存在额外政策冲突 [Y-policies] |
| III.E.6 | 明确禁止直接/间接抓取，或取得抓取来的 YouTube 数据/内容 | 用供应商 API 或 browser opt-in 并不当然合规；需单独确认用途/许可 [Y-policies] |
| III.E.4.b–f | 普通 Non-Authorized Data 只可有限临时保存，不超过 30 日，之后删除或刷新；一般 authorized data 也有 30 日规则；特定授权统计例外仍要每 30 日核验授权/删除状态；展示保持最新、历史数据准确标时 | API key 的公开数据也受约束；不能只对 OAuth 数据做 TTL。stdout 被保存、Markdown 和导出同样需要生命周期设计 [Y-policies] |
| III.D.2.c.i–ii 与 III.E.4.g | 用户在 Client 内撤销授权时须立即撤销 token，数据尽快且 7 日内删除；在 Google 权限页撤销时，相关数据尽快且 30 日内删除；用户请求删数据也须 7 日内完成 | 将来 OAuth 才需完整撤销链；不能混成统一“30 日删一次即可” [Y-policies] |
| III.A.1–2、III.F.2；API Terms §10.3 | Client 需条款/隐私政策、YouTube 来源归因和 Branding Guidelines；不同来源混合展示须可区分 | CLI 文本/JSON 的品牌展示如何满足要求须确认，没有“终端天然豁免”依据；至少保持平台标识、标题、频道、canonical URL、取回日期 [Y-policies] [Y-branding] |
| III.E.4.h 与 III.L | 限制替换/派生指标；2026 年新增的部分统计存储/派生指标许可仅适用于明确申请并通过审计的 analytics 场景 | 不随意派生“质量分”“热度分”；不能把新例外外推到一般 Agent/RAG 研究 [Y-policies] |
| API Terms §24.3 | 终止时停止使用并删除控制中的 API Data 等 | 服务停止后的材料退役不能只禁用网络访问 [Y-api-terms] |

原文“Scraping”短句是 “directly or indirectly, scrape”；这是本文拒绝把第三方字幕当官方方案透明增强的依据，而非把技术风险等同于确定违法。Gemini 的官方 YouTube 输入功能是单独的产品支持证据，也不等于对原作者内容任意保存、训练或传播的许可。[Y-policies]、[G-video]、[G-terms]。

**落盘建议（待设计）：** 不加服务端缓存；API 输出记录 `retrieved_at` 与来源，文档写明最多 30 日的常规刷新/删除责任。若 forager 管理持久导出目录，应支持可执行的保留策略和删除清单；如果用户把 stdout 保存到任意位置，不能承诺 forager 能回收用户副本。当前临时目录/`--content-dir` 规则只是交付机制，没有跨调用清理/刷新保障，因此官方字幕或长期证据归档需要单独解决。此项应在发布前确定，不能用“默认临时文件”代替条款满足声明。[F-platform]、[Y-policies]。

## 推广性检查：Bilibili、TikTok/抖音与 Vimeo

这部分只验证契约差异，没有做各站完整接入调研或账号授权。[E-video-source]。

| 平台 | 一手证据与本次观察 | 对共用契约的校验 |
|---|---|---|
| Bilibili | 官方开放平台面向账号授权、稿件/用户数据、投稿/直播等能力，需身份审核与应用；不能由首页推出已开放任意视频全站检索/字幕。公开 `x/web-interface/view` 两个样本 code=0；`BV12N4y1M7rh` 的 `x/player/wbi/v2` 匿名返回 `need_login_subtitle=true`、字幕列表空，另一老样本 false/空列表。yt-dlp 源码也按该标记提示登录。[B-doc] [E-bili-view] [E-bili-player] [E-bili-caption-view] [E-bili-caption-player] [E-video-source] | “有公开视频 metadata”“有字幕”“有权读字幕”三者独立；空列表不能无条件转成无字幕。BVID 是稿件身份，cid/分 P 是内容时间轴，后续需明确 `video` 与 `part` 身份/URL 往返，不能把 YouTube 单 video ID 直接套用。登录状态属于访问条件，不属于 ref |
| TikTok | Research API 需特定研究项目审批，开发者账号本身不够；当前 FAQ 为组织共享 1,000 requests/日、100,000 records/日，token 约 2 小时；搜索数据和计数可能滞后。视频 query 可请求 `voice_to_text`，但本次无 token、未验证覆盖和时间段格式。Display API 面向授权用户的视频展示，不是任意全站检索。[T-research] [T-query] [T-faq] [T-display] | Bearer 短 token/client key+secret 生命周期不适合当永久 API key 数组；voice_to_text 缺时间戳时允许无逐段时间的文字证据，但不得伪造时间段。Research route 和实时 browser 不是可互换排序/新鲜度 |
| 抖音 | 与 TikTok 是不同官方 API/权限空间。`video.list` 要申请 scope 与用户授权，读授权账号的视频；关键词搜索的官方方案仅返回近一天、与自身业务相关且预先申请的词，不能申请泛品类/竞品词。[DY-list] [DY-search] [DY-permissions] | 应分别建 Platform 与 route，不能写一个 `tiktok` route 暗中切到抖音。scope/查询目的限制是 route 支持边界，不能把受限关键词接口当通用 research search |
| Vimeo | 官方 reference 区分视频、text tracks、transcript segments，还单列企业级 AI 能力和 `ai` scope；未做带 token 调用，本次抓到的 reference 未充分展开所有 endpoint 权限细节。[V-reference] | 字幕轨/生成理解仍应分开；不能因“同为视频 URL”就承诺公开字幕或同一配额。保留视频身份与独立 caption track metadata 即可，权限留 route |

`youtu.be/ID` 可离线识别这一结论不外推到 `b23.tv`、`vm/vt.tiktok.com` 或抖音 opaque 分享短链：它们通常不携带稳定实体 ID，遇到需重定向才能识别的链接依既有规则退 2，或未来单独提供 IO resolve 操作；types 不跟随网络重定向。TikTok 的完整页面路径还可能含 handle；canonical 若依赖未知 handle，必须选择可纯往返的规范形态并用一手返回链接验收，不能先编造 URL。[E-video-source]、[F-platform]。

上述差异支持共用**证据语义和交付规则**，而非共用平台身份解析器或万能 transport；人工/平台 ASR/翻译/额外生成、语言缺失、权限不足、partial 与时间轴仍是可复用的判断维度。[E-video-source]、[SD-transcript]、[F-platform]。

## 证据缺口

已知证据缺口：官方 OAuth（captions）、`quotaExceeded` 实际响应、所有第三方调用、官方 Gemini API 调用、实际 browser transcript、自有 adapter、自动/翻译轨和无字幕/直播的完整实测、可复现第二页、长期风控、各供应商再许可证明均未完成。Bilibili 只测了两个匿名样本，TikTok/抖音/Vimeo 只核验文档与相关 extractor 源码。官方配额分桶已经由文档和测试项目控制台核验，测试项目的实际扣减（含失败请求）也与文档一致。上述缺口限制发布与稳定性承诺，不妨碍比较路线、识别契约改动和确定下一步验收顺序。[E-manifest]、[E-keys]、[E-quota-origin]。

## 来源与证据索引

文中引用优先指向本次证据快照；配对 JSON 含来源 URL、抓取时间、命令与终态，`.txt` 为取回原文的可读投影。源码按 commit 固定，项目 issue 的用户报告只证明该报告存在；未把推广回复、二手文章或模型搜索答案当作平台事实。网页抓取成功码不等于页面有效：`sc-pricing`、`supadata-limits`、`serpapi-terms` 的错误入口保留为失败证据，结论使用已取得的正确首页/文档/法律页。[E-manifest]。

[D-video]: https://github.com/jfmoe/forager/issues/219
[F-platform]: ../spec/forager/07-platforms.md
[F-architecture]: ../spec/forager/04-architecture.md
[F-glossary]: ../../GLOSSARY.md
[F-opencli]: ../agents/opencli.md
[F-adr19]: ../adr/0019-platforms-as-a-dimension-separate-from-capability-seams.md
[F-adr20]: ../adr/0020-opt-in-browser-routes-paced-per-operation.md
[F-adr22]: ../adr/0022-platform-native-full-text.md
[F-adr26]: ../adr/0026-x-platform-through-xai-x-search-with-relayed-content.md
[F-adr27]: ../adr/0027-hn-platform-combines-algolia-search-with-the-official-firebase-api.md
[E-dir]: youtube-2026-10-17/
[E-manifest]: youtube-2026-10-17/source-manifest.json
[E-env]: youtube-2026-10-17/environment.json
[E-egress]: youtube-2026-10-17/egress-trace.json
[E-keys]: youtube-2026-10-17/credential-availability.json
[E-repo]: youtube-2026-10-17/repository-contracts.json
[E-quota-origin]: youtube-2026-10-17/quota-origin.json
[E-search-origin]: youtube-2026-10-17/search-origin.json
[E-live]: youtube-2026-10-17/official-api-live.json
[E-live-script]: youtube-2026-10-17/live-api.py
[E-risk]: youtube-2026-10-17/web-route-risk-reports.json
[Y-overview]: youtube-2026-10-17/official-auth.txt
[Y-search]: youtube-2026-10-17/official-search.txt
[Y-videos]: youtube-2026-10-17/official-videos.txt
[Y-commentthreads]: youtube-2026-10-17/official-commentthreads.txt
[Y-comments]: youtube-2026-10-17/official-comments.txt
[Y-comment-resource]: youtube-2026-10-17/official-commentthread-resource.txt
[Y-channels]: youtube-2026-10-17/official-channels.txt
[Y-channel-resource]: youtube-2026-10-17/official-channel-resource.txt
[Y-errors]: youtube-2026-10-17/official-errors.txt
[Y-playlistitems]: youtube-2026-10-17/official-playlistitems.txt
[Y-cap-list]: youtube-2026-10-17/official-captions-list.txt
[Y-cap-download]: youtube-2026-10-17/official-captions-download.txt
[Y-quota]: youtube-2026-10-17/official-quota.txt
[Y-audit]: youtube-2026-10-17/official-audit.txt
[Y-auth]: youtube-2026-10-17/youtube-auth-guide.txt
[Y-terms]: youtube-2026-10-17/official-youtube-terms.txt
[Y-api-terms]: youtube-2026-10-17/official-api-terms.txt
[Y-policies]: youtube-2026-10-17/official-policies.txt
[Y-branding]: youtube-2026-10-17/youtube-branding.txt
[E-oembed]: youtube-2026-10-17/anonymous-oembed.json
[E-watch]: youtube-2026-10-17/anonymous-watch.json
[E-timedtext]: youtube-2026-10-17/anonymous-timedtext.json
[E-signed]: youtube-2026-10-17/anonymous-signed-timedtext.json
[E-yta-probe]: youtube-2026-10-17/yta-protocol-probe.json
[E-yta-source]: youtube-2026-10-17/yta-source.json
[E-yta-issues]: youtube-2026-10-17/yta-issues.json
[E-yta593]: youtube-2026-10-17/yta-593-comments.json
[E-yta592]: youtube-2026-10-17/yta-592.json
[E-yta612]: youtube-2026-10-17/yta-612.json
[D-yta]: youtube-2026-10-17/yta-readme.txt
[D-readme]: youtube-2026-10-17/yt-dlp-readme.txt
[D-pot]: youtube-2026-10-17/yt-pot-guide.txt
[E-dlp-source]: youtube-2026-10-17/ytdlp-source.json
[E-dlp13770]: youtube-2026-10-17/ytdlp-13770-comments.json
[E-dlp-zoo]: youtube-2026-10-17/ytdlp-jNQXAC9IVRw.json
[E-dlp-ted]: youtube-2026-10-17/ytdlp-Ks-_Mh1QhMc.json
[E-dlp-sub-zoo]: youtube-2026-10-17/ytdlp-subtitles-jNQXAC9IVRw.json
[E-dlp-sub-ted]: youtube-2026-10-17/ytdlp-subtitles-Ks-_Mh1QhMc.json
[E-sub-validation]: youtube-2026-10-17/subtitle-validation.json
[E-oc-source]: youtube-2026-10-17/opencli-source.json
[E-oc-search]: youtube-2026-10-17/opencli-search.json
[E-oc-video]: youtube-2026-10-17/opencli-video.json
[E-discovery]: youtube-2026-10-17/web-discovery.json
[SP-search]: youtube-2026-10-17/serpapi-youtube.txt
[SP-video]: youtube-2026-10-17/serpapi-video.txt
[SP-transcript]: youtube-2026-10-17/serpapi-transcript.txt
[SP-releases]: youtube-2026-10-17/serpapi-transcript-releases.txt
[SP-pricing]: youtube-2026-10-17/serpapi-pricing.txt
[SP-legal]: youtube-2026-10-17/serpapi-legal.txt
[SC-index]: youtube-2026-10-17/sc-index.txt
[SC-search]: youtube-2026-10-17/sc-search.txt
[SC-transcript]: youtube-2026-10-17/sc-transcript.txt
[SC-home]: youtube-2026-10-17/sc-home.txt
[SC-terms]: youtube-2026-10-17/sc-terms.txt
[SD-search]: youtube-2026-10-17/supadata-search.txt
[SD-meta]: youtube-2026-10-17/supadata-metadata.txt
[SD-transcript]: youtube-2026-10-17/supadata-youtube.txt
[SD-pricing]: youtube-2026-10-17/supadata-pricing.txt
[SD-intro]: youtube-2026-10-17/supadata-intro.txt
[SD-terms]: youtube-2026-10-17/supadata-terms.txt
[G-video]: youtube-2026-10-17/gemini-video.txt
[E-gateway]: youtube-2026-10-17/gemini-gateway-checks.json
[G-pricing]: youtube-2026-10-17/gemini-pricing.txt
[G-terms]: youtube-2026-10-17/gemini-terms.txt
[E-video-source]: youtube-2026-10-17/video-platform-source.json
[B-doc]: youtube-2026-10-17/bilibili-doc.txt
[E-bili-view]: youtube-2026-10-17/bilibili-public-view.json
[E-bili-player]: youtube-2026-10-17/bilibili-public-player.json
[E-bili-caption-view]: youtube-2026-10-17/bilibili-caption-sample-view.json
[E-bili-caption-player]: youtube-2026-10-17/bilibili-caption-sample-player.json
[T-research]: youtube-2026-10-17/official-tiktok-research.txt
[T-query]: youtube-2026-10-17/official-tiktok-videoquery.txt
[T-faq]: youtube-2026-10-17/official-tiktok-faq.txt
[T-display]: youtube-2026-10-17/official-tiktok-display.txt
[DY-list]: youtube-2026-10-17/douyin-video-list.txt
[DY-search]: youtube-2026-10-17/douyin-search.txt
[DY-permissions]: youtube-2026-10-17/douyin-permissions.txt
[V-reference]: youtube-2026-10-17/vimeo-texttracks.txt
