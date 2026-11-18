# 小红书无头指纹浏览器可行性实测

实测日期：**2026-11-18**；上游仓库与参考项目的核对日期：**2026-11-19**。仓库基线：`80ff776f0f8f4ae12f6f3fd563db705058aedce7`（forager 0.19.0，当时 `xiaohongshu_browser` 仍经 OpenCLI 驱动用户的 Chrome）。宿主为 macOS、Apple M4 Pro（arm64，14 核、24 GB 内存）。

本文回答一个问题：forager 能否不经 OpenCLI、不占用用户的 Chrome，自己启动一个无头浏览器，用一个已登录的专用 profile 完成小红书的 search、fetch 与 comments。实测用的是一套独立的探针脚本，不是 forager 的代码；据此做出的决定见 ADR 0036。文中「事实」指日志、上游仓库或 API 可以复核的内容；「推断」是基于事实的判断；「未验证」是本次没有覆盖的问题。

## 结论

- **可行（事实）。** fingerprint-chromium `148.0.7778.215` 以新 Headless 加专用 profile 运行，经 CDP 管道驱动，全部页面操作为 CDP 原生输入、全部数据来自被动抓包，完成了有界面扫码登录、无头 search（含筛选与翻页）、打开笔记与读取评论（含楼中楼）。整个过程没有出现风控信号，也没有窗口或 Dock 图标。
- **只证明可行，不证明长期安全（推断）。** 用量很小：一个账号、一次会话，共 2 次搜索、2 次打开笔记、2 次读评论。持续使用下的风控与会话保持都没有覆盖，见「未验证项」。

## 方法

### 浏览器与启动参数

- 浏览器：[adryfish/fingerprint-chromium](https://github.com/adryfish/fingerprint-chromium) 的 macOS 安装包 `ungoogled-chromium_148.0.7778.215-1.1_macos.dmg`，从上游 GitHub Release 下载，SHA256 为 `b72f091e2e1a7583eed389c4b8e3534ed355e568af8c8bbf8fc30a25e23ca679`，与 Release API 给出的 asset digest 一致。
- 启动参数：`--user-data-dir=<专用 profile>`、`--fingerprint=<固定种子>`、`--fingerprint-platform=macos`、`--fingerprint-brand=Chrome`、`--lang=zh-CN`、`--accept-lang=zh-CN,zh`、`--remote-debugging-pipe`、`--no-first-run`、`--no-default-browser-check`、`--window-size=1440,900`；无头运行另加 `--headless=new`。有界面与无头使用同一个 profile、同一个种子。
- 驱动：Node 写的探针经 `--remote-debugging-pipe` 收发 CDP 消息，不开调试端口。

### 交互与读取规则

- 改变页面状态的操作只用 CDP `Input.*`：指针分 8–18 步移动到目标，在目标中心附近随机取点，先悬停再点击；文本逐字 `Input.insertText`；按键用 `Input.dispatchKeyEvent`；翻页用 `mouseWheel` 滚动指针下的滚动容器。操作之间有随机停顿。
- 数据只来自页面自己发出的请求：探针记录小红书各域名的每个响应的状态码，保留搜索、筛选、笔记、评论、楼中楼、首页信息流与账户接口 `user/me` 的响应体。探针不自行请求接口，不读取或导出 cookie。
- 风控判断：HTTP 461 或 471、跳转到 `/website-login` 或 `/404`、页面出现验证码元素或安全限制、请求太频繁等提示，以及业务接口返回失败，都记为风险信号；记下任何风险信号后，探针拒绝执行后续操作。

### 运行顺序

时间为北京时间。

1. 22:14，首次以无头启动后立即退出，只确认浏览器能启动。
2. 22:14–22:18，有界面运行：打开首页，用户在窗口中扫码登录，之后关闭浏览器。
3. 22:18–22:19，第一次无头运行：打开首页、点击搜索框、输入查询词并回车。这次运行在第一个搜索响应之后停止，原因见「结果」中的 `search/onebox`。
4. 22:19–22:24，第二次无头运行：搜索，打开筛选面板选"最新"，滚动翻到第 3 页；从结果中点开一篇笔记，滚动评论到第 2 页，展开一条评论的回复；关闭笔记后，带 Access Token 直接打开同一篇笔记的 URL。

原始日志（事件流、抓到的响应、截图）与探针脚本保存在本机，不进仓库：其中有账号与笔记的标识。

## 结果

### 登录与会话

- 有界面运行中，扫码前 `user/me` 先返回 `code: -101`（"无登录信息"），随后返回 `code: 0` 而 `guest: true`（站点为访客分配的身份）；扫码后返回 `code: 0`、`guest: false`。因此"已登录"要看 `guest`，不能只看 `code`。
- 同一 profile 关闭后以无头重启，首个 `user/me` 即为 `guest: false`。两次无头运行都保持登录。

### search

- 首页地址为 `/explore`。点击搜索框、输入并回车后，页面跳到 `/search_result_ai`（新布局），没有出现 `/search_result`。
- 回车到第 1 页响应约 1.0–1.2 秒；首屏随即自己请求第 2 页（第 1 页之后约 0.3–0.5 秒），每页 20 条笔记。
- 选"最新"之后，新的请求体带 `filters`：`sort_type` 为 `time_descending`，`filter_note_type`、`filter_note_time`、`filter_note_range`、`filter_pos_distance` 均为 `不限`。请求体顶层的 `sort` 仍为 `general`、`note_type` 仍为 `0`，不反映筛选。
- 筛选后共读到 3 页（第 3 页由滚轮触发），60 条笔记 ID 互不重复，3 页的 `search_id` 相同；筛选前后的 `search_id` 不同。
- `search/onebox` 返回 HTTP 200、`code: 0`、`success: false`，两次搜索都是如此。第一次无头运行把它记成了业务失败并因此停止。不能用 `success` 字段判断小红书接口失败。

### 笔记与评论

- 从结果页点开笔记：页面在原地打开笔记层，请求 `feed`（笔记详情）与 `comment/page`（第 1 页评论）。
- 在笔记的滚动面板中滚动，评论第 2 页在滚动开始后约 2.3 秒到达；点击"展开 N 条回复"后约 1 秒收到 `comment/sub/page`。
- 带 Access Token 直接打开笔记 URL（`/explore/<id>?xsec_token=…&xsec_source=pc_search`），导航约 2.0 秒完成，页面请求了 `comment/page`，没有请求 `feed`。笔记详情因此只能来自页面初始状态 `__INITIAL_STATE__`（推断；日志不记录页面状态的读取结果）。

### 风控信号

- 日志共记录小红书各域名的 1201 个响应，全部为 HTTP 200，其中 178 个是 CORS 预检 `OPTIONS`；两次无头运行占 868 个。没有 461 或 471，没有跳转到登录页或 `/404`，没有验证码或提示文字，登录没有掉。
- 唯一记下的"风险信号"是上面 `search/onebox` 的误报。

### 性能

- 浏览器启动到 CDP 可用：约 0.5 秒（三次运行为 474–538 毫秒）；本机第一次启动这个应用时为 3.8 秒。
- 打开首页到加载完成：约 2.0–2.6 秒。
- 无头运行期间桌面上没有窗口，Dock 中没有图标。

### 浏览器特征

探针在有界面与无头两种模式下各读取了一次页面可见的浏览器特征：

- 两种模式相同：UA 为 `Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) … Chrome/148.0.0.0 Safari/537.36`（不含 `HeadlessChrome`）；`navigator.webdriver` 为 `false`；UA-CH 品牌为 Chromium、Google Chrome，`fullVersionList` 报 `148.0.7778.217`；平台 `macOS`，`platformVersion` 为 `15.3.1`；WebGL 为 `ANGLE (Apple, ANGLE Metal Renderer: Apple M4, …)`；`hardwareConcurrency` 为 24、`deviceMemory` 为 16；语言为 `zh-CN, zh`；`visibilityState` 为 `visible`。
- 版本号、系统版本、核数与内存都与宿主不同（宿主为 148.0.7778.215 的二进制、14 核、24 GB），是指纹种子生成的值。
- 只有屏幕不同：有界面时为宿主显示器的 1920×1080、DPR 2；无头时为 800×600、DPR 1，`screenX/Y` 为 0，而窗口尺寸仍为 1440×900。800×600 小于窗口，是无头模式可见的特征。forager 的实现在 `browser login` 时记下有界面窗口的屏幕，之后的无头启动以 `--screen-info` 与 `--window-size` 复现（规格第 7 章「浏览器传输」）。

## 未验证项

- 长期使用：会话能保持多久、持续使用下是否触发风控、每 10 秒 1 条命令的节奏能否长期维持。
- 其他账号：其他账号是否同样落到 `/search_result_ai` 新布局，旧布局 `/search_result` 是否仍会出现。
- 与本机正式版 Chrome 的特征对比：本次只比较了指纹浏览器的有界面与无头两种模式，没有记录同一台机器上正式版 Chrome 的读数。
- 屏幕之外的无头特征：上游 README 说明其余无头特征没有改动（见下文），本次没有逐项检测。
- 其他主机：Linux、Windows 与 Intel Mac 都没有实测。

## 参考实现：xpzouying/xiaohongshu-mcp

[xpzouying/xiaohongshu-mcp](https://github.com/xpzouying/xiaohongshu-mcp)（Apache-2.0，Go，基于 go-rod）是一个经浏览器操作小红书的 MCP 服务，默认以无头模式运行（`-headless` 默认 `true`）。

事实：

- 2026-06-28 至 07-10，用户在 issue [#720](https://github.com/xpzouying/xiaohongshu-mcp/issues/720)、[#722](https://github.com/xpzouying/xiaohongshu-mcp/issues/722)、[#726](https://github.com/xpzouying/xiaohongshu-mcp/issues/726)、[#728](https://github.com/xpzouying/xiaohongshu-mcp/issues/728) 报告：登录一次后次日被警告、被禁言一周、低强度抓取一天后被警告、采集两次后被封号。
- PR [#737](https://github.com/xpzouying/xiaohongshu-mcp/pull/737)（2026-07-20 合入）加入"内置浏览器"：首次运行从项目自建的 CDN（`cdn.one-world.ai`）下载，按同目录的 `SHA256SUMS` 校验；可用 `XHS_FP_SEED` 为每个账号固定指纹种子；点击、输入、滚动与停顿统一经一个拟人化输入包。浏览器启动参数为指纹平台按宿主选择、品牌 `Chrome`、语言 `zh-CN`，并关闭了它自己的 stealth JS。
- 2026-07-27，维护者在上述 issue 下回复：已改用内置的指纹浏览器、每账号固定一套画像、交互全面拟人化，测试账号持续数天状态正常，建议升级到 v2.1.1；随后关闭这些 issue。
- 2026-10-30，#720 下有用户评论"只是获取信息就封号了"，没有说明所用版本。
- 它的内置浏览器版本为 `148.0.7778.215`（`browser/browser_version.txt`）。CDN 上的 `SHA256SUMS` 中 `macos-arm64.dmg` 的哈希为 `b72f091e…ca679`，与 fingerprint-chromium 上游 Release 的 macOS 安装包逐字节相同。CDN 上 Linux 与 Windows 包的哈希与上游任何 asset 都不同。仓库的浏览器更新工作流注明"公开 repo 不引用上游"，代码中没有写出浏览器的来源。

推断：

- 它的 macOS 内置浏览器就是 fingerprint-chromium 148.0.7778.215；Linux 与 Windows 包是重新打包的，来源无法从公开信息确认。
- 封号报告集中在改用指纹浏览器之前，说明原版无头配置的风险可观察；改用之后是否足够，只有维护者的测试账号与一条未注明版本的反例，证据都很弱。
- 可借鉴的是它的页面就绪判断、风控页判定与输入时序；它的 cookie 文件、代理与写操作不在 forager 的边界内。

## 上游：fingerprint-chromium

事实（2026-11-19 核对）：

- 许可为 BSD-3-Clause。最近的两个 Release：`150.0.7871.186`（2026-10-02）只有 Linux 与 Windows 安装包，没有 macOS 包；`148.0.7778.215`（2026-06-21）有 macOS、Linux 与 Windows 包。更早的版本中，144、138、134、133 同样没有 macOS 包。
- README 说明源码采用延迟公开：当前版本只发布二进制，补丁在下一个版本发布时公开（"typically one month later"）。README 的版本表中，150 的源码一栏写着"Released with Chrome 151"。
- 标签 `150.0.7871.186` 中只有 `LICENSE`、`README.md`、`README-ZH.md` 与一张图片，没有补丁；标签 `148.0.7778.215` 有完整的 `patches/` 等构建文件，指向的提交日期为 2026-09-28，晚于该版本二进制发布约三个月。
- README 关于无头的原文："Only changes `User-Agent` from `HeadlessChrome` to `Chrome`. Other headless characteristics remain unchanged, use with caution." 同一节还列出：避免 `Runtime.enable` 引起的 CDP 检测，`navigator.webdriver` 为 `false`。
- 指纹由 `--fingerprint`（32 位整数种子）启用，`--fingerprint-platform` 取 `windows`、`linux` 或 `macos`，`--fingerprint-brand` 指定 UA 与 UA-CH 中的品牌，`--fingerprint-hardware-concurrency` 不给时由种子派生。

推断：

- 只采用补丁源码已公开的版本，固定版本至少落后上游最新版一个版本；macOS 包又不是每个版本都有，固定版本可能更旧，缺少的 Chromium 安全修复随之增加。

## 来源

- 本机实测日志与探针脚本（不进仓库）。
- fingerprint-chromium：[仓库与 README](https://github.com/adryfish/fingerprint-chromium)、[Release 148.0.7778.215](https://github.com/adryfish/fingerprint-chromium/releases/tag/148.0.7778.215)、[Release 150.0.7871.186](https://github.com/adryfish/fingerprint-chromium/releases/tag/150.0.7871.186)。
- xiaohongshu-mcp：[仓库](https://github.com/xpzouying/xiaohongshu-mcp)、[PR #737](https://github.com/xpzouying/xiaohongshu-mcp/pull/737)、issue #720、#722、#726、#728（链接见上文）、CDN 校验文件 `https://cdn.one-world.ai/browsers/148.0.7778.215/SHA256SUMS`。
- 仓库内：[2026-10-18 后台运行调研](2026-10-18-background-browser-automation.md)、[2026-10-08 小红书接入调研](2026-10-08-xiaohongshu-integration.md)、#248（自动化窗口被遮挡时原生输入被拒）。
