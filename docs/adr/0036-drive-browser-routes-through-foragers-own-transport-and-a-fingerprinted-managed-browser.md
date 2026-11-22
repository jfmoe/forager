# 浏览器 route 改用 forager 自带的浏览器传输与指纹版 Managed Browser

`xiaohongshu_browser` 原先经本机 OpenCLI 驱动用户日常使用的 Chrome。本记录决定由 forager 自己启动一个无头的指纹浏览器（[adryfish/fingerprint-chromium](https://github.com/adryfish/fingerprint-chromium)），经 CDP 管道直接驱动它，登录态保存在该平台专用的 Browser Profile 中。route id、三个操作的参数、输出字段与错误分类都不变。OpenCLI 传输、forager 的 JS adapter 与 `providers.xiaohongshu_browser.command` 在同一个版本中删除，不保留兼容层（沿用 ADR-0030、ADR-0034、ADR-0035）。可行性依据是 2026-11-18 的实测，见 [小红书无头指纹浏览器可行性实测](../research/2026-11-18-xiaohongshu-headless-fingerprint-browser.md)。

## 决定

- **浏览器传输**：每条浏览器命令启动一个浏览器进程，放入独立进程组，经 `--remote-debugging-pipe` 通信，不开调试端口，不设常驻进程。传输不含站点知识；页面逻辑与判定属于各条 route，全部用 Rust 写。改变页面状态的操作只用 CDP 原生输入；数据只来自被动抓包与只读的页面状态脚本。规则见参考文档 `docs/reference/platforms.md`「浏览器传输」。
- **Managed Browser**：固定 fingerprint-chromium `148.0.7778.215`，从上游 GitHub Release 下载，按 forager 内置的 SHA256 校验，由 `forager browser install` 安装到缓存目录中带版本号的位置。只固定补丁源码已经公开的版本。固定版本只为 macOS arm64 提供安装包。`browser.path` 可以指定替代的本地浏览器，forager 不检查它是不是指纹版。
- **Browser Profile**：每个平台一个，位于状态目录。指纹种子在创建时随机生成一次，之后不变。`forager browser login <platform>` 打开一个有界面的窗口，由用户自己登录，之后的检索命令都在无头模式下复用这个 profile。
- **浏览器环境由 route 声明**：注册信息的传输类型为 `Browser`，并携带该 route 的浏览器环境（使用哪个 Browser Profile、语言、是否需要登录）。配置检查、飞行前检查、doctor 与接入清单按这份声明判断，不按 route id 分支。以后的平台只写自己的页面逻辑。

## 推翻的边界

参考文档 `docs/reference/platforms.md`原先规定原生输入"不做 stealth 改造、不伪造指纹"；2026-10-18 的 [后台运行调研](../research/2026-10-18-background-browser-automation.md) 规定"不以 stealth 或反复重试作为验收通过方式"。本记录推翻这两条：forager 接受一个由 Managed Browser 按固定种子生成浏览器指纹的运行环境。依据：

- **OpenCLI 干扰桌面，后台调用时失败。** OpenCLI 在用户的 Chrome 里开一个自动化窗口。这个窗口被其他窗口遮挡时，OpenCLI 2.x 拒绝发送原生输入，search 与 comments 以 Runtime 失败（#248）。agent 在后台调用 forager 时，这个窗口大多处于被遮挡的状态。
- **OpenCLI 的外部依赖多。** route 需要全局安装 OpenCLI 2.x、手动更新 Browser Bridge 扩展、把 JS adapter 复制到 `~/.opencli/clis/`，还需要 Node。任何一项版本漂移都会让 route 失效（ADR-0035）。
- **不带指纹的无头浏览器有站点可见的特征。** 原版 Chromium 的新 Headless 在 UA 中报告 `HeadlessChrome`。[xpzouying/xiaohongshu-mcp](https://github.com/xpzouying/xiaohongshu-mcp) 默认以无头模式运行，2026-06 至 07 月有用户报告账号被警告、禁言或封号（issue #720、#722、#726、#728）；该项目随后在 PR #737（2026-07-20 合入）中改用内置的指纹浏览器，它的 macOS 安装包与本记录固定的版本逐字节相同。
- **实测可行。** 2026-11-18 用一个真实账号，以同一版本、同一组启动参数完成了有界面登录与无头的 search、fetch、comments，没有出现风控信号。

新的边界：指纹参数只由 Managed Browser 根据 Browser Profile 的固定种子生成，指纹平台与宿主一致；一个 profile 始终表现为同一台设备，forager 不轮换指纹，也不为了绕过拦截而更换环境。forager 不向页面注入 stealth 脚本，不改写页面的任何 API。

## 保留的边界

- 不解验证码，不绕过验证页或安全限制。遇到未登录、HTTP 461、验证、安全限制或请求太频繁，立即停止，不重试；被拦截的结果永不当作合法空结果。
- 不导出登录态：forager 不读取、不导出、不注入 cookie 或 token，登录态只存在于 Browser Profile 中。登录由用户在 `browser login` 打开的窗口中亲手完成。
- 不自行请求站点接口：只读页面自己发出的请求与响应，不改写 `fetch`、XHR 或 WebSocket。
- 访问节奏不变：每 10 秒 1 条浏览器命令，并发 1，跨进程生效；`browser login` 计入同一节奏。route 只能由用户手动加入 `platforms.<id>.order`（ADR-0020）。
- 只读：不做点赞、收藏、评论、发布等写操作。

## 实现确定的两点

- **forager 被终止时浏览器自行退出，因此不处理信号。** forager 被 SIGINT、SIGTERM 或 SIGKILL 终止时，内核关闭 forager 一端的调试管道，浏览器随即退出，不留下进程。默认不运行的"真实浏览器"测试 `the_browser_exits_after_its_driver_is_killed` 对三种信号逐一验证，实测浏览器在约 0.1 秒内退出。截止时间到达或命令 future 被丢弃时，传输仍会强杀整个进程组。
- **无头下的屏幕尺寸按登录时的屏幕复现。** 无头浏览器默认报告 800×600、DPR 1 的屏幕，与窗口尺寸不符。`browser login` 在有界面的窗口里读取宿主的屏幕尺寸、可用区域、DPR 与窗口尺寸，记入 Browser Profile；之后的无头启动以 `--screen-info` 与 `--window-size` 复现它们。从未登录过的 profile 没有记录，保持默认值。

## 考虑过的方案

- **页面逻辑用 Rust，而不是 Node sidecar。** 保留 JS（改为 Playwright 或独立的 Node 进程）可以复用现有 adapter 的代码，但会把 Node 与另一套浏览器运行时留作依赖，测试也需要 Node；原 adapter 的代码又绑定在 OpenCLI 的接口上，并不能原样复用。用 Rust 写，运行与测试都不需要 Node，判定部分可以写成纯函数直接做单元测试。代价是重写三个操作的页面驱动。
- **由 forager 下载固定版本，而不是让用户自己安装。** fingerprint-chromium 的补丁源码延迟公开、各平台安装包发布不齐，由用户自行安装很难保证装上的是哪个版本、包是否完整。forager 固定版本并校验哈希，`browser install` 一条命令完成，升级 forager 后固定版本变化时命令会明确提示重新安装。安装包取自上游 GitHub Release，不取第三方镜像。用户自装的浏览器经 `browser.path` 保留为应急手段。
- **原地替换，而不是与 OpenCLI 并存。** route 的输入、输出与错误契约不变，只换传输；并存意味着保留 OpenCLI、Browser Bridge 与 Node 依赖和两套代码路径，却没有需要它的用户场景。这是破坏性变更：旧的 `command` 键与对应环境变量按未知键拒绝。
- **测试不做会讲 CDP 的假浏览器。** 它要模拟页面导航、网络事件与求值结果，成本远高于原先的假 `opencli`；页面逻辑对着模拟跑通，只说明它与模拟一致。测试分为：判定纯函数的单元测试；以只记录启动参数的 shell 替身浏览器做的二进制测试（启动参数、飞行前检查、截止时间与进程清理）；CDP 客户端对内存管道的单元测试；默认不运行、以真实 Managed Browser 访问本地页面的测试；以及 live smoke C27–C29。代价是离线测试里没有小红书命令成功路径的端到端用例，页面驱动逻辑只由 live smoke 证明。

## 已知风险

- **账号风险没有消除。** 可行性实测只证明可行：一个账号、一次会话，共 2 次搜索、2 次打开笔记、2 次读评论。2026-11-19 forager 自己的页面逻辑在同一账号上跑了 22 次浏览器命令，覆盖 search、fetch、comments 的主要参数组合，没有出现风控信号（[功能矩阵实测](../research/2026-11-19-xiaohongshu-browser-route-live-matrix.md)），用量仍然很小。长期会话保持、持续使用下的风控、其他账号是否得到相同的页面布局都没有验证。xiaohongshu-mcp 的 issue #720 在 2026-10-30 仍有用户报告"只是获取信息就封号"，没有说明所用版本。
- **上游补丁源码延迟公开。** 上游 README 说明补丁在下一个版本发布时公开（通常晚一个月）。2026-11-19 时最新的 150 版本标签中只有 README 与 LICENSE；148 版本二进制发布于 2026-06-21，带补丁的标签提交日期为 2026-09-28。只固定源码已公开的版本，意味着固定版本落后于上游最新版。
- **macOS 安装包滞后。** 150 版本没有 macOS 安装包，下一次升级固定版本要等上游补发。固定版本越旧，缺少的 Chromium 安全修复越多。
- **无头特征没有全部伪装。** 上游 README 说明它在无头模式下只把 UA 中的 `HeadlessChrome` 改为 `Chrome`，其他无头特征不变。forager 只修正了屏幕尺寸。2026-11-19 的读数：无头窗口的 `outerHeight` 比 `innerHeight` 大 121，`navigator.webdriver` 为 false；`document.hasFocus()` 为 false，与在后台的浏览器相同，与用户前台正在看的标签页不同。
- **其他主机没有安装包。** macOS arm64 之外的主机只能经 `browser.path` 自行指定浏览器；非指纹浏览器会忽略指纹参数，以自己的真实特征访问站点。

## 取代范围

- 本记录取代 ADR-0035 的「保留 xiaohongshu_browser 作为过渡」与「OpenCLI 冻结」两节。
- 本记录取代 ADR-0020 中绑定 OpenCLI 的表述：访问策略的单位从一条 OpenCLI 命令改为一条浏览器命令（permit 仍持有到浏览器进程被回收）；route 不再依赖用户自己的 Chrome、OpenCLI 与安装好的 adapter；被杀命令的清理由进程组强杀与管道关闭保证，不再依赖 OpenCLI daemon 回收标签页。opt-in 与不解站点验证的原则继续有效。
- 本记录取代 ADR-0022「接受的边界」中"token 在本机进程参数中可见"一条：Access Token 只在导航时经 CDP 交给浏览器，不进入任何进程参数；以及「影响」中 OpenCLI stdout 4 MiB 的上限：Native 正文改受单条 CDP 消息 16 MiB 与单个响应体 4 MiB 的上限约束。
- 带日期的设计与调研文档作为历史记录保留。

## 重新评估的条件

- 经这条 route 使用的账号收到警告、禁言或封号。
- 上游停止公开补丁源码或停止发布 macOS 安装包，使固定版本无法前进。
