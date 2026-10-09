# Gemini Deep Research 接入可行性

核验日期：**2026-10-09**。仓库基线：`168a2be`（forager 0.9.0）；本机 OpenCLI `1.8.6`。本文是调研，**尚未实现任何 Gemini 接入**；设计见 [Gemini Deep Research 浏览器 route 设计](../design/2026-10-09-gemini-deep-research-route.md)。下文区分已核验事实、推断与待验证事项。

## 结论

- forager 的 research 引擎只接受自己取到的证据（ADR 0012、0013）。Gemini Deep Research 交付的是 LLM 综合报告，不能成为 Research Evidence，只能作为**委托研究报告**交给用户或 agent：报告本身用于定向，其中的来源 URL 可作为待取证候选。
- 网页版路线中，gemini-webapi 是目前协议知识最完整的开源实现，但**不建议原样引入或移植它的传输方式**（导出 Google cookie、自行构造请求、伪造反滥用字段）。建议沿用 forager 已有的 OpenCLI 浏览器 route 模式：在用户已登录的 Chrome 里像用户一样操作页面，只读取页面自己收到的响应，再按 gemini-webapi 揭示的字段位置解码。2026-10-09 的完整实测证实了这条路线的发起与读取都可行，见“实测”。
- 官方 Interactions API 是唯一有书面授权的程序化入口，但按 token 计费，并受 Grounding with Google Search 条款约束。它与网页版 route 互不替代，可以作为以后的独立 route。

## 路线对比

| 路线 | 能力（已核验） | 主要代价与风险 | 结论 |
|---|---|---|---|
| 官方 Interactions API | agent `deep-research-preview-04-2026` 与 `deep-research-max-preview-04-2026`；必须 `background=true`，后台轮询或流式；默认工具为 Google Search、URL Context、Code Execution，可接远程 MCP 与 File Search；最长 60 分钟，多数 20 分钟内完成；不支持结构化输出。[文档][API-doc] | 官方估算每次约 $1–3（Max 约 $3–7）[API-doc]；Grounded Results 有展示、存储与“不得用 Links 定位抓取页面”等限制，见下文 | 合规的程序化入口；条款限制了 forager 对报告的再加工，留作以后的独立 route |
| OpenCLI 内置 `gemini deep-research` / `deep-research-result` | 前者在 UI 中选择 Deep Research 工具、发送并点击确认，返回 `status` 与会话 URL；后者只把报告导出为 **Google Docs 链接**。[start 源码][OC-start]、[result 源码][OC-result] | 拿不到报告正文与来源；上游 adapter 没有 forager 可固定的契约版本 | 不直接使用；其 UI 标签与确认流程可作参考 |
| gemini-webapi（Python，原样调用） | 网页版逆向客户端，完整支持计划、确认、轮询与报告读取，报告为 Markdown 并带来源列表。[README][GW-readme] | 需导出 Google 会话 cookie；Chrome 的 cookie 只能维持数小时；反滥用字段用随机串填充；Python 运行时与 AGPL-3.0；稳定性记录不佳。详见下文 | 不采用；作为协议知识参考 |
| 将 gemini-webapi 协议移植到 Rust（HTTP + cookie） | 同上 | 风险同上，另需 TLS 指纹伪装依赖（gemini-webapi 依赖 `curl-cffi` 的 `chrome145` 伪装）[常量][GW-const] | 不采用 |
| **forager 自有 OpenCLI adapter（推荐）** | 由真实页面发出全部请求，adapter 读取页面自己的 `StreamGenerate` 与 `hNvQHb` 响应；2026-10-09 实测完成一次发起与读取，状态、进度、正文与来源均可读到（见“实测”） | 依赖 Chrome、OpenCLI 与 UI；发起必须在前台窗口；Deep Research 发起是写操作且消耗账号额度 | 采用，见设计文档 |

其他开源项目：[pminervini/deep-research-mcp][DRM]（MIT）把官方 API 包装成 MCP，超时后不取消、以 `research_status` 按 id 取回，这一“提交与取回分离”的做法值得借鉴；abe238/gemini-deep-research（MIT，3 star）参考价值低。

## gemini-webapi 深入

基线为 [`HanaokaYuzu/Gemini-API@1ad0d4a`][GW-commit]（2026-10-08），最新发布 v2.1.1（2026-08-27），约 3.6k star，许可 AGPL-3.0。

### 运行形态与认证

- 必需依赖只有 `curl-cffi`、`loguru`、`orjson`、`pydantic`，运行时不启动浏览器；可选依赖 `browser-cookie3` 只用于从本地浏览器读取 cookie，README 称目前仅支持 Firefox。[pyproject][GW-pyproject]、[README][GW-readme]
- 认证使用 `__Secure-1PSID` 与 `__Secure-1PSIDTS`。初始化时请求 `https://gemini.google.com/app`，从页面中提取 `SNlM0e`（访问令牌 `at`）、`cfb2h`（构建标签 `bl`）、`FdrFJe`（会话 `f.sid`）等值。[get_access_token][GW-token]
- 进程存活期间在后台经 `accounts.google.com/RotateCookies` 刷新 `__Secure-1PSIDTS`。README 写明：新版 Chromium 系浏览器启用 Device Bound Session Credentials，从中导出的 cookie 只能维持数小时且无法续期，建议改用 Firefox，或在无痕会话中登录后立即关闭。[README][GW-readme]
- 初始化时依次调用用户状态、偏好、活动同步、最近会话、用量与配额等 RPC，账号状态码 1016 表示未登录，1037 表示用量超限，1060 表示 IP 被临时封锁或地区不支持。[client][GW-client]、[常量][GW-const]

### Deep Research 流程

Deep Research 在网页版中是一个普通多轮会话，没有独立的任务实体。[research_mixin][GW-mixin]

1. **生成计划**：向 `StreamGenerate` 发送带 Deep Research 标志的消息。请求体中与之相关的槽位是 `[3]`、`[4]`、`[49]`、`[54]`、`[55]`。其中 `[3]` 被填为 `"!"` 加 2600 字符的随机串，`[4]` 为随机 UUID。[client][GW-client] 网页前端在 `[3]` 位置放置的值形如反滥用令牌；**这是推断**，但它说明服务端当前没有校验该槽位，一旦开始校验，这条路线会立即失效。
2. **读取计划**：计划位于候选结果 rich content 块的字段 55（备选 56），包含标题、步骤、预计时间、确认文案（缺省为 "Start research"）与修改入口。[utils/research][GW-research]
3. **确认**：在同一会话中再发一轮确认文案，研究在服务端开始，与客户端会话脱离。
4. **轮询**：用 batchexecute 的 `hNvQHb`（列出会话轮次）按会话 id 读取最近 10 轮，直到某个模型轮次带有非空报告正文。任务 id 字段在完成前一直是字面量 `agency-placeholder-task-id`，两个任务列表 RPC 始终为空，gemini-webapi 因此以报告正文出现作为唯一的完成信号，并认为没有进度信息。[research_mixin][GW-mixin] 本次实测发现候选结果中另有状态字段与进度字段，见“实测”。
5. **报告结构**：报告不在回复正文里（回复只是一句“已完成”之类的提示），而在候选结果的 `[30][0]`：`[0]` 文档 id、`[2]` 标题、`[3]` 任务 id（以此区分研究文档与其他附件）、`[4]` Markdown 正文（`[17][0]` 为镜像）。正文含 `[cite: N]` 标记，来源在 `[17][1]` 或 `[5]` 的字段 43 中按标记分组，每条为 favicon、URL、标题。[utils/research][GW-research]、[utils/citation][GW-citation]

### 稳定性与已知问题

- [#329][GW-329]：v2.0.0 起 Deep Research 卡在排队，2026-04 报告，直到 v2.1.1（2026-08）才修复。
- [#359][GW-359]：未指定模型时后端默认使用 Flash-Lite，它不支持 Deep Research，直接拒绝；需显式选择 `gemini-flash`。维护者称免费账号的 `gemini-pro` 也不提供 Deep Research。
- [#261][GW-261]：用户报告每天固定时段失效数十分钟，推测为软限流。
- [#267][GW-267]：用户担心账号或 IP 被封，未给出 Google 方面的证据。
- 账号层面：Google 自 2026-10-09 起将无订阅账号的网页版模型限制为 Flash-Lite（[第三方报道][PPC-oct9]，转述官方帮助页）；官方限额页仍列出无订阅账号可用 Deep Research，但高峰期可能不可用，且额度按算力计、每 5 小时刷新并有周上限。[官方限额][GA-limits] 免费账号在此变更后能否通过网页版发起 Deep Research **未验证**。

### 许可

gemini-webapi 为 AGPL-3.0，forager 为 MIT。参考其公开的协议事实（端点、RPC id、字段位置）独立实现不受影响；不得复制其代码或注释进入 forager。以子进程调用用户自行安装的 gemini-webapi 在许可上可行，但因上文的认证与稳定性问题仍不采用。

## 条款

- **Google 服务条款**禁止“using automated means to access content from any of our services in violation of the machine-readable instructions on our web pages (for example, robots.txt files …)”。[Google ToS][G-tos] `gemini.google.com/robots.txt` 的通用组为 `Disallow: /app/` 与 `Disallow: /chat/`（2026-10-09 读取）。因此网页版自动化与 SSRN、小红书属于同一风险类别：只能个人使用、由用户显式开启、接近人工节奏（ADR 0020）。
- **Gemini API 附加条款**（Grounding with Google Search 部分，2026-03-23 生效）要求 Grounded Results 只展示给提交提示的终端用户并附带 Search Suggestions；禁止缓存、聚合、分析 Grounded Results，禁止“using Links to identify destination pages for crawling or scraping”；除聊天记录（最长 2 年）等例外，不得复制、存储；未经书面许可不得修改或插入其他内容。[API 条款][API-terms] 官方文档说明 Deep Research 默认启用 Google Search 并适用这些限制。[API-doc] 这意味着 API route 若把报告引用交给 forager fetch 取证或拆分落盘，可能违约；此为对条款文本的解读，不是法律意见。
- 网页版（消费者 Gemini Apps）的输出使用条款本次未核验。

## 实测

2026-10-09，本机 OpenCLI 1.8.6，用户自己的 Chrome（中文界面，有订阅的账号），Gemini 已登录。经用户授权发起了一次 Deep Research。探针只用 DOM 点击与填写操作页面，从 OpenCLI 网络捕获读取页面自己收到的响应，没有自行构造任何请求。记录只含结构、计数与时间，不含 cookie、令牌、账号身份与会话 id。完整记录见 [live-probe.json](gemini-deep-research-2026-10-09/live-probe.json)。

### 只读检查

| 操作 | 观察 |
|---|---|
| `opencli gemini status` | `Status: Connected`，`Login: Yes` |
| 打开 `/app`，检查 `WIZ_global_data` | 对象存在；`SNlM0e` 为非空字符串，`cfb2h` 与 `FdrFJe` 为字符串 |
| 打开一个已有会话，列出请求 | 页面自己发出 30 余个 batchexecute 请求，其中包括 **`hNvQHb`（会话轮次）**，全部为 200 |

### 一次完整运行

| 阶段 | 观察 |
|---|---|
| 选择工具 | 第一次的后台窗口中 `document.hidden` 为 true，“上传和工具”菜单展开了但没有渲染任何项；前台窗口正常。Deep Research 不在第一级菜单，而在“更多工具”子菜单中。选中后出现“取消选择“Deep Research””按钮 |
| 计划 | 发送后 15 秒内出现计划卡片，URL 变为 `/app/<id>`。`StreamGenerate` 响应 24,642 字符。字段 55 依次为标题、步骤、预计时间、确认文案、确认内容 URL、修改文案，与 gemini-webapi 一致。字段 69（状态）为 **2**。中文界面下确认按钮文案仍为英文 "Start research" |
| 确认 | 第二轮 `StreamGenerate` 6,415 字符。候选结果 `[30][0]` 已出现研究文档：有标题，任务 id 为占位符，正文为空。字段 69 为 **3** |
| 运行中 | 页面打开期间每约 10 秒调用一次 `hNvQHb`，响应从 6.9K 增长到 93K 字符。字段 57 是**进度**：思考摘要（标题与正文）与已访问来源（favicon、URL、标题）。确认后约 1 分钟时有 13 条思考、123 个来源。页面文案显示“正在研究 N 个网站…”。这推翻了 gemini-webapi “没有进度信号”的说法 |
| 完成 | 确认后约 5–6 分钟完成。字段 69 为 **5**，任务 id **仍是占位符**。`hNvQHb` 响应 463,592 字符：正文 `[4]` 为 15,170 字符的 Markdown，`[17][0]` 与之相同；有 84 处 `[cite: N]`，共 26 个编号。`[17][1]` 与 `[5]` 的字段 43 各有 35 组引用，26 个编号全部可解析，条目为 favicon、URL、标题、摘要，**全部是直链**，没有跳转链接。报告语言跟随界面（中文），而非英文提示 |
| 全新会话读取 | 用新开的后台会话打开该对话：运行中能读到同样的文档容器；完成后页面在 14 秒内发出 `hNvQHb`，响应含完整正文与全部引用，此时 `document.hidden` 为 true，不影响读取 |

### 结论

- **读取（`result`）可行**：在后台窗口打开会话，读取页面自己的 `hNvQHb` 响应即可拿到状态、进度、正文和来源，不需要打开侧栏，也不需要另一个 RPC。响应约 0.5 MB，远低于 OpenCLI 传输的 4 MiB 上限。
- **发起（`start`）可行，但必须在前台窗口执行**：后台窗口里工具菜单渲染不出来。发起过程中会在用户屏幕上出现约 1 分钟的 Chrome 窗口。
- **状态判定可以直接用字段 69**：2 为等待确认，3 为研究中，5 为完成；再用正文是否为空做交叉校验。任务 id 不能用来判断完成。

### 仍未验证

- 无订阅账号能否发起 Deep Research（本次账号有订阅）。
- 英文界面下的菜单与按钮文案。
- 研究失败、额度用尽、被拒绝时的响应与页面文案。
- Deep Research Max 或更长的研究的响应大小。

[API-doc]: https://ai.google.dev/gemini-api/docs/deep-research
[API-terms]: https://ai.google.dev/gemini-api/terms
[G-tos]: https://policies.google.com/terms
[GA-limits]: https://support.google.com/gemini/answer/16275805?hl=en
[PPC-oct9]: https://ppc.land/google-cuts-two-of-three-gemini-models-for-free-users-from-october-9/
[DRM]: https://github.com/pminervini/deep-research-mcp
[OC-start]: https://github.com/jackwener/OpenCLI/blob/24136945847afbfad266c6c46a8cd335377f9112/clis/gemini/deep-research.js
[OC-result]: https://github.com/jackwener/OpenCLI/blob/24136945847afbfad266c6c46a8cd335377f9112/clis/gemini/deep-research-result.js
[GW-commit]: https://github.com/HanaokaYuzu/Gemini-API/tree/1ad0d4a93e505e77c79769d78ef2997641f973c8
[GW-readme]: https://github.com/HanaokaYuzu/Gemini-API/blob/1ad0d4a93e505e77c79769d78ef2997641f973c8/README.md
[GW-pyproject]: https://github.com/HanaokaYuzu/Gemini-API/blob/1ad0d4a93e505e77c79769d78ef2997641f973c8/pyproject.toml
[GW-const]: https://github.com/HanaokaYuzu/Gemini-API/blob/1ad0d4a93e505e77c79769d78ef2997641f973c8/src/gemini_webapi/constants.py
[GW-token]: https://github.com/HanaokaYuzu/Gemini-API/blob/1ad0d4a93e505e77c79769d78ef2997641f973c8/src/gemini_webapi/utils/get_access_token.py
[GW-client]: https://github.com/HanaokaYuzu/Gemini-API/blob/1ad0d4a93e505e77c79769d78ef2997641f973c8/src/gemini_webapi/client.py
[GW-mixin]: https://github.com/HanaokaYuzu/Gemini-API/blob/1ad0d4a93e505e77c79769d78ef2997641f973c8/src/gemini_webapi/components/research_mixin.py
[GW-research]: https://github.com/HanaokaYuzu/Gemini-API/blob/1ad0d4a93e505e77c79769d78ef2997641f973c8/src/gemini_webapi/utils/research.py
[GW-citation]: https://github.com/HanaokaYuzu/Gemini-API/blob/1ad0d4a93e505e77c79769d78ef2997641f973c8/src/gemini_webapi/utils/citation.py
[GW-329]: https://github.com/HanaokaYuzu/Gemini-API/issues/329
[GW-359]: https://github.com/HanaokaYuzu/Gemini-API/issues/359
[GW-261]: https://github.com/HanaokaYuzu/Gemini-API/issues/261
[GW-267]: https://github.com/HanaokaYuzu/Gemini-API/issues/267
