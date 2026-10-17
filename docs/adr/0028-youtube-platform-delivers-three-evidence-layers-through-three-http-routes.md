# YouTube 平台用三条 Http route 分别交付三层证据

forager 的第七个平台 `youtube` 只交付文字证据，并按来源把证据分成三层，层与层之间不互相冒充：

| 证据层 | 内容 | 在契约中的位置 | route |
|---|---|---|---|
| 发布者文字 | 标题、描述、章节 | `metadata`、`abstract` | `youtube_api`（YouTube Data API v3，用户自备 key）；`youtube_web` 也能交付 |
| 平台字幕 | 平台上已有的一条字幕：发布者上传的或平台自动识别（ASR）的 | `full_text`，作为 Transcript 写入文件 | `youtube_web`（匿名读取网页与未公开的播放器、字幕接口） |
| 模型感知 | 模型看过画面、听过声音后写出的回答或字幕 | Platform Operation Ask 与 Transcribe，整体是 Relayed Content | `gemini_api`（Gemini 原生 `generateContent`，用户自备 key，端点可配置） |

三条 route 都走现有的 Http 传输。

## 背景

2026-10-17 的实测与条款调研（`docs/research/2026-10-17-youtube-integration.md`）：

- 官方 Data API 能检索、读元数据与评论、列播放列表和频道，但读不到别人视频的字幕：`captions.download` 需要 OAuth 与视频的编辑权限。默认配额是每天 100 次 search 与 10,000 单位其他读取，失败的请求同样计费。
- 网页的未公开接口（watch 页 → `/youtubei/v1/player` → 字幕轨 `baseUrl`）能匿名取回人工字幕与自动字幕，字幕轨标明是否为 ASR。风险落在出口 IP 上，是临时的，没有找到匿名访问导致账号被封的案例；但它违反 YouTube Developer Policies III.D.7 与服务条款对自动访问的限制。
- Gemini 能直接理解 YouTube 视频 URL 的画面与声音，按 token 计费。
- Developer Policies 要求一个客户端只用一个项目、不得规避配额。

## 决定

- **三层证据各有一条 route，不互相回退。** 字幕、描述与模型输出的可信度不同，调用方需要知道一段文字来自谁。`full_text` 只交付平台上已有的字幕；视频没有字幕时 `full_text` 失败，不回退到模型生成的字幕。模型生成的 Transcript 只经调用方显式请求的 Transcribe 交付。
- **`youtube_web` 由用户手动启用。** 它是合法的 order 项，但不在默认 order 中：使用未公开接口的条款风险与出口 IP 的风控风险由用户知情后自己承担。启用说明写明，与官方 key 同时启用时，Developer Policies III.E.6（不得取得抓取来的数据）可能使用户的 Google 项目被停用。
- **`youtube_api` 只接受一个 key。** 凭据池在额度耗尽时换 key，等于用多个项目绕开配额。配置 schema 拒绝多于一个 key（退 3）；`quotaExceeded` 与 `dailyLimitExceeded` 归为 QuotaExhausted，不轮换。
- **key 放在 `X-Goog-Api-Key` 请求头中，不放在 URL 的 `key` 参数中。** Data API 两种方式都接受（实测用的是请求头）。请求头不会进入 reqwest 错误消息中的 URL、attempt 与日志，所以不需要再对 URL 脱敏。
- **错误按上游 reason 归因，status 映射仍只在 net。** 无效 key 是 HTTP 400，额度耗尽与其他权限错误都是 HTTP 403，只看 status 无法区分。route 提供 reason 识别函数（`API_KEY_INVALID` → Auth，`quotaExceeded`、`dailyLimitExceeded` → QuotaExhausted），经 net 读取边界的 reason 钩子生效；没有识别出的 reason 沿用 net 的共享 status 映射。
- **search 结果用一次批量 `videos.list` 补充时长、计数与字幕标记。** 补充只花 1 单位配额；失败时保留检索结果并给出诊断，与 ADR 0027 的增强规则一致。

## 不选的方案

- **新增本机工具传输调用 yt-dlp**：能读字幕，但为一个平台新增一种传输类型、外部依赖与安装说明不划算；`youtube_web` 用现有 Http 传输即可。它的 VTT 自动字幕还带滚动重复（同一条轨 252 KB，对比 srv1 XML 56 KB）。若 `youtube_web` 频繁失效，再重新评估。
- **SerpApi 的字幕引擎**：静默忽略 `type=asr`，返回人工字幕却回显 `type: asr`，响应不标明实际返回的是哪条轨，字幕来源无法可靠标注；读一个视频要两次计费调用。
- **模型生成的字幕作为 `full_text` 的回退**：调用方会把模型的听写当作平台字幕引用，混淆证据层。
- **`youtube_api` 使用凭据池**：见上文配额规则。
- **只用官方 API**：读不到字幕，视频内容只剩描述。
- **OpenCLI 浏览器 route**：需要登录态与浏览器，而匿名 Http 已能取回字幕。

## 影响

- 不配置任何 key 时，YouTube 平台只有手动启用的 `youtube_web` 可用，而它不提供 search。
- `youtube_api` 的日常瓶颈是每天 100 次 search；fetch、comments 与每页 videos 各花 1 单位。deep doctor 与 live smoke 的 C36 各消耗 1 次 search 配额。
- YouTube 改动播放器接口时 `youtube_web` 会失效，以 youtube-transcript-api 的修复为参照维护。机房 IP 往往一请求就被拦，`youtube_web` 不适合在 CI 中运行。
- API 数据有 30 天刷新或删除的要求；forager 不做跨调用缓存，`--content-dir` 中的文件由用户自己管理。
- 经第三方 Gemini 兼容网关使用订阅额度是否符合 Google 条款没有核实，`gemini_api` 默认指向官方 API。
