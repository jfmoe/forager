# 浏览器 route 的后台运行方案

核验日期：**2026-10-18**。仓库基线：`a76d324ef470398e95b624de2484cc61b5589959`；本机 `forager 0.15.0`、Node `v24.22.0`、Chrome `155.0.8059.80`，macOS / Apple Silicon。本机 OpenCLI 为 npm 全局安装的 `1.8.8`。上游分别核查 `v1.8.8`（`8271afc67e8504bda94c147f446ee29775d08274`）、`v2.0.0`（`74fe1dc11c26d44b2a01102fd341aa55ae407774`）源码，以及已有临时目录内的 npm `2.0.0` 包；不把上游 main 的后续行为算进 2.0。

本文是选型调研，记录各候选方案的评估与最终决定；没有实现替代运行时。证据来自仓库、上游源码、官方文档及明确标注的上游 issue；**没有访问 SSRN、小红书、Gemini、ChatGPT 的真实页面或账号，没有读取、解密、复制真实 cookie，没有启动浏览器实测**。调研期间没有修改 OpenCLI 安装、`~/.opencli` 或 Chrome profile。外部资料的读取日期均为 2026-10-18，具体链接见文末。文中「事实」指源码或文档可证；「推断」指基于这些事实的工程判断；「待测」不代表已可用。

## 最终决定

维持 OpenCLI 默认的窗口与标签页行为，不另建运行环境：

- **窗口保留。** adapter 命令继续在 OpenCLI 自有的 automation 窗口中运行，Background 或 Foreground 按命令声明；调用结束后窗口留在桌面上。这个窗口已经满足下文的目标 1：命令不切走用户正在浏览的标签页，automation 窗口也不创建标签组。
- **不设置清理开关。** forager 不设置 `OPENCLI_CLOSE_OWNED_TAB` 等改变回收方式的环境变量，也不在调用结束时追加收尾命令。同一次调用里的连续命令（attempt 重试，或 research 在一次调用中读取多个小红书条目）复用同一个空白占位标签页，窗口不会反复开关。
- **不建专用 profile。** 继续使用用户日常 Chrome 中的 Browser Bridge 和登录状态。

取舍依据：

- A 只把账号环境与日常浏览分开，不去掉可见窗口，也不保证两条 start 不抢焦点（见目标 3）。代价是在新 profile 中重装扩展、为各站点账号重新登录、用显式 `OPENCLI_PROFILE` 固定路由，并在新环境中重新做真实站点验收。在可见窗口仍然存在的前提下，这些成本没有对应的收益。
- B、C、D 的理由见「调研结论摘要」及各节；F 的运维成本最高，只有「始终不占宿主桌面」成为硬性要求时才值得投入。
- agent-browser 属于 D 类替换运行时，LCU 依赖模型逐步操作并需要人工审批，两者都不采用，见「agent-browser」和「LCU」两节。
- OpenCLI 2.x 的升级按它的功能和修复单独评估，不以「更后台」为目标（见 E）。

以后如果需求变为隔离账号环境或不占宿主桌面，可从「推荐实施顺序与验收清单」中 A 或 F 的验收项开始。

## 调研结论摘要

**可以把浏览器运行环境与日常 Chrome 分开，但不应通过复制主 Chrome 的 cookie 实现。当前技术栈下，最优投入顺序是 A：专用、有界面的 Chrome 系浏览器、独立持久化 profile、用户手动登录、保留 OpenCLI 扩展与现有 adapter。** 浏览器自己在该 profile 内保存、续期和使用会话；forager 不导出凭据。这解决账号环境与日常浏览的耦合，且保留 SSRN 下载。它是否能在隐藏状态完成所有操作，仍须验收。[O1][O2][O3][D1]

必须区分三个目标：

1. **不切走用户正在浏览的标签页**：OpenCLI 的 owned automation window 已处理这类干扰；`background` 不代表无窗口。[O1][O7]
2. **日常运行少打扰用户**：A 是当前最合适方案。最小可行验证可继续使用 **1.8.8**，在独立 user-data-dir 手动安装 Bridge、登录，并以 `OPENCLI_PROFILE=<contextId>` 固定路由。多 profile 能力在 1.8.8 已存在，**不需要先升级 2.0**。[O2][O3]
3. **包括 Gemini/ChatGPT start 在内，始终不占用户桌面或焦点**：本机隐藏窗口、另一个 Space、最小化均没有这项保证。若这是硬性验收条件，推荐 **F：独立图形桌面中的 headed 浏览器 + OpenCLI 扩展**，优先本机 VM，必要时独立设备。浏览器可以在自己的桌面保持前台，用户的宿主桌面不被接管；站点兼容性和资源成本仍待测。[O1][F1][F2]

**不推荐 B 作为默认方案，不采用 C，不为本需求直接重写 D。** Headless 能消除可见窗口，却增加挑战兼容性与登录不确定性；cookie 迁移违反 ChatGPT 的明确约束，也不能可靠迁移完整会话；Playwright 等解决的是自动化 API 和生命周期，不会自动解决站点风控与隐藏页面渲染。[C2][G1][D1][R3] agent-browser 同属替换运行时；LCU 要求模型在回路中逐步操作，不适合确定性 route。两者都不采用。

**E 只能改善现状，2.0 没有新的无界面窗口模式。** 升级还有独立协议成本：2.0 的 JSON 成功输出多一层 `{ok:true,data:...}`，错误改为 stdout JSON；基线版本的 forager 解码顶层 `contract`、从 stderr 取错误。不能只升级全局包就宣称完成迁移。[O1][O4][R1]

## 当前能力与必须保留的边界

| Route | 现有动作与窗口 | 替代运行时必须保留的能力 | 证据 |
|---|---|---|---|
| `ssrn_browser` | search 在空白页执行站点同款匿名 GET；详情与 PDF 用浏览器；均 Background | 页面内请求、页面读取、`waitForDownload` 的完成状态与本地文件路径；签名下载 URL 不输出；遇挑战停止，由用户处理 | [ADR 0020][R2]、[ADR 0025][R5]、[paper.js][R6] |
| `xiaohongshu_browser` | 搜索、笔记、评论，均 Background；只读页面响应与状态 | 登录、页面原生签名、被动网络捕获、可见元素筛选、滚动与截图推进渲染；不得把安全限制当合法空结果 | [route 设计][R7]、[adapter][R8] |
| `gemini_browser` | `start` Foreground；`status` / `result` Background | 工具菜单、输入、发送及计划确认；捕获页面自己收到的 batchexecute 响应；写操作仅一次，结果未知时不重发 | [ADR 0023][R4]、[start.js][R9] |
| `chatgpt_browser` | `start` Foreground；`status` / `result` Background | 工具菜单、输入、发送、计划卡倒计时、重开会话与读取响应；不读 cookie/token、不构造 backend-api 请求、不改写 fetch/XHR/WebSocket | [ADR 0024][R3]、[start.js][R10]、[shared.js][R11] |

两个需要纠正的混淆：

- **持久化 profile 不等于 persistent site session。** 前者保存浏览器自身的登录与站点存储；后者是 OpenCLI 的标签页租约策略。现有 `--site-session ephemeral --keep-tab false` 可以继续使用，浏览器 profile 的登录不会因此随每条命令消失。[O1][O3][O5]
- **Gemini 与 ChatGPT 的隐藏窗口证据不同。** ADR 0023 记录 Gemini 在 `document.hidden=true` 时菜单打不开；小红书的隐藏页滚动也曾不加载，截图可推进渲染。ChatGPT 既有调研记录后台菜单可以渲染，但完整后台 start 未验证，基线代码的 ChatGPT start 选择 Foreground；不能把 Gemini 的实测结论直接套到 ChatGPT。[R4][R7][R12]

所有方案均须保留现有 opt-in / 点名触发、每 5–10 秒一条命令、并发 1、挑战不自动解决、start 不重试。换浏览器不扩大这些权限。[R2][R3][R4][R5]

## 方案对比

「可覆盖」指底层能力存在，未表示在新浏览器上通过真实站点验收。风险为相对工程判断，不是封号概率；A、B、D、F 均不能消除既有条款风险。

| 方案 | 不占前台 / 不打扰 | SSRN | 小红书 | Gemini | ChatGPT | 检测与账号风险；实现维护成本 |
|---|---|---|---|---|---|---|
| **A1 专用 headed Chrome profile + Bridge** | 不影响主 profile；后台普通窗口仍可见；start 可抢焦点 | search/详情/PDF 能力完整 | 能力完整，隐藏翻页待测 | 读取可覆盖；隐藏 start 未通过 | 读取可覆盖；隐藏 start 未通过 | 最接近现有环境；新会话仍可能验证；**低**，主要是安装、严格路由与验收 [O1][O2] |
| **A2 独立 CfT/Chromium app + Bridge** | 额外隔离 app；可以单独隐藏，但不能保证隐藏时可操作 | 同 A1 | 同 A1，换浏览器指纹待测 | 手动 Google 登录、菜单与流式响应待测 | 登录、菜单与倒计时待测 | 版本更新与登录兼容成本高于 A1；**低至中** [C1][C3][G1] |
| **A3 专用 headed 浏览器 + OpenCLI 直连 CDP** | 连接本身不抢焦点；窗口位置需另管 | search/详情候选；**现成接口缺下载等待** | 捕获/点击/截图存在，实际分页待测 | 菜单和响应完成语义待测 | 同左，且保持 ADR 0024 | CDP 不是无检测承诺；**中**，须补下载、target 与清理 [O5][O6] |
| **B new Headless + 专用 profile** | 无可见窗口，满足桌面隔离 | 技术上可下载，但 OpenCLI CDP 仍缺接口；Cloudflare 不保证通过 | 高不确定性，不能按“有 cookie 即可”判断 | Google 登录与完整 start 未验证 | 登录、计划卡与完整 start 未验证 | Cloudflare 明确不支持自动化框架解决生产挑战；**中至高** [C2][C4][G1] |
| **C 主 Chrome cookie 导出 / 注入浏览器或 HTTP** | HTTP/headless 可隐藏；单靠 cookie 迁移不决定 UI 行为 | 详情/下载与 clearance 不可保证；HTTP search 与 ADR 0025 不符 | cookie 不能代替页面签名/指纹 | 会话续期与绑定不可靠 | **违反 ADR 0024，排除** | 增加敏感凭据保管、失效和会话冲突问题；**高且不合边界** [C5][C6][G2][R3] |
| **D Playwright / Patchright / Camoufox / rebrowser** | Headless 可隐藏；headed 仍有 A 的显示问题 | Playwright 有完整下载能力 | 原生页面与被动捕获可实现，需移植 | 可实现 UI 流程，后台行为仍待测 | 可实现，但必须逐项守住 ADR | 反检测项目声明不等于站点承诺；**高**，新增自己的运行时 [D1][D2][D3][D4] |
| **E 保持 OpenCLI，仅调窗口/2.0** | 减少标签切换及窗口重复创建；**不是无窗口** | 保留现有覆盖 | 保留现有覆盖 | start 仍 Foreground | start 仍 Foreground | 风险接近现状；窗口调参低，2.0 协议升级中 [O1][O4][O7] |
| **F VM / 独立图形桌面中的 headed + Bridge** | **隔离宿主桌面**；初次登录与人工挑战仍需接管 | 保留下载能力；需把文件带回宿主 | 保留能力，VM 图形/指纹待测 | 可在来宾桌面前台运行，站点仍需验收 | 同左，不能扩大凭据和请求权限 | 不等于免检测；**中至高运维成本**，比重写 adapter 更集中 [F1][F2] |

| 方案 | ADR 0020 | ADR 0023 | ADR 0024 | 其他约束 |
|---|---|---|---|---|
| A | 机制原则不冲突；「用户自己的 Chrome」的部署说明需补清专用 profile | 可以保留单次写操作；隐藏 start 成功后再更新窗口决定与实测依据 | 浏览器自己使用其登录会话不等于 forager 导出凭据；不新增取 cookie/token 代码即可保留条件 | SSRN 继续按 ADR 0025 匿名浏览器内请求 |
| B | 禁止自动过挑战；挑战出现须停 | 单次写操作不变；需要新的 start 证据 | headless 本身未被禁止；解验证码、伪造防护字段或自行请求仍被禁止 | 不以 stealth 或反复重试作为验收通过方式 |
| C | cookie 迁移不是既有风险接受的自然延伸；HTTP 替代需另作决定 | 未解决单次写操作与未知结果 | 明确冲突，必须先修订，本文不建议放宽 | 小红书设计拒绝 cookie + 逆向签名；SSRN search 要求不带 cookie [R5][R7] |
| D | 变更 OpenCLI process 传输须同步设计、文档和实际限速单位 | 保留副作用与结果未知语义，不能采用自动重试写操作的默认策略 | 库可用不等于配置合规；禁止 cookie/storageState 导出、请求重放和网络函数改写 | 若模块/归属变化，同步架构 spec；不能只替换进程名称 |
| E | 基本不冲突；2.0 的清理行为须重验 | 不把窗口问题变成 start 重试 | 不新增使用 OpenCLI cookie API 的代码 | 不因 slot pool 提高并发 |
| F | 仍为个人环境；部署依赖与本地文件交付语义须更新 | guest 前台不等于宿主前台；需记录新环境实测 | 手动在 guest 登录、只读页面响应可维持条件；不把会话交给云服务商 | 远端清理、超时与文件传输不能沿用本地保证而不验证 |

## A：专用浏览器，手动登录并原地保留会话

### 浏览器与 profile 的选择

**建议先用已有正式版 Chrome + 独立 `--user-data-dir` 验证 A1；需要单独隐藏 app 或固定版本时，再验证 CfT app 的 A2。** 不使用主 profile，也不把主 profile 的 `Cookies`、`Local State` 或整个目录复制过去。正式版 Chrome 的优势是部署增量小、沿用用户熟悉的登录环境；不足是同一 app 的隐藏与激活可能同时影响日常 Chrome，独立 data dir 本身不保证独立 Dock/Space 行为。后一句是 macOS 交互层面的待测项，不能当隔离承诺。

CfT 是 Chrome 官方为测试与自动化提供的版本化发行物，适合与日常浏览器分开；代价是需要自己管理版本更新，不能长期固定旧版本。[C1] 普通 Chromium 也能用扩展，但无需为了本需求先增加另一套发行渠道。**是否能登录 Google 账号不能从“Chrome 内核相同”推导**：Google 登录帮助明确列出「由软件自动控制」等不允许登录的环境，没有给 CfT 或调试实例普遍放行的保证。[G1]

可尝试由用户在未接入自动化前，在专用浏览器中正常手动登录，随后使用同一个 profile。这是在建立新的合法会话，不是搬运旧 cookie；也不能承诺此顺序必然解决登录拒绝。遇到拒绝时停止，不自动重试登录、不用 cookie 导入补洞。CfT 登录失败且 A1 成功时，优先 A1；两者均失败则保留现状并报告限制。

Chrome 136 起，默认 data dir 不接受 `--remote-debugging-port` / `--remote-debugging-pipe`；独立非默认 data dir 才是官方推荐的调试隔离方式。**扩展的 `chrome.debugger` 路径不需要启动远程调试端口**，不应把这个限制误写成「现有 Bridge 不能控制主 Chrome」。A1/A2 用扩展时可以完全不开放 CDP 端口。[C3][O1]

### 扩展与 daemon 如何共存

这是现有能力，而非需要自建的新路由层：

- Bridge 把随机生成的 `contextId` 存入扩展自己的 `chrome.storage.local`，连接 daemon 时报告它；daemon 以该 id 保存多个扩展连接。不同 id 共存；同一 id 的新连接会替换旧连接。因此应在新 profile **重新安装扩展**，不复制带扩展存储的 profile。[O1][O2]
- CLI 的 `--profile` / `OPENCLI_PROFILE` 是**硬要求**，目标离线则失败；`profile use` 保存的默认值是偏好，离线时可能落到唯一仍在线的 profile。这里应固定显式 id，避免专用浏览器关闭后误操作日常 Chrome。[O2][O3]
- 初次连接后可用 `opencli profile list` 找到 id；使用命令级环境 `OPENCLI_PROFILE=<contextId> forager ...`。Rust 传输用普通 `Command::new`，没有清空环境，故现有子进程能继承该选择；不必改全局默认 profile。此行为是源码判断，尚未在第二个浏览器上实测。[O2][R1]
- 1.8.8 已具备上述路由；2.0 随发布包含扩展 1.0.25，而 1.8.8 源码中的扩展是 1.0.24。实际部署时核对 CLI/扩展兼容性，不能只换 Node 包。[O2][O4]

扩展安装有两个渠道，需分清：正式版 Chrome 137 起移除的是 `--load-extension` **命令行加载**；并非取消所有扩展或开发者手动「Load unpacked」。Chromium/CfT 继续支持该 flag；Chrome 139 又移除了正式版的 `--disable-extensions-except` 等 flag。A1 优先手动安装 Bridge，不把自动侧载参数当安装方案。[C7][C8][O8]

### 隐藏、遮挡、最小化和其他 Space

这些只能作为不同实验条件，不能统称「后台」。至少分别记录 `document.visibilityState`、`document.hasFocus()`、rAF 是否前进、菜单是否实际渲染、滚动是否产生页面请求。

| 参数 / 手段 | 可确认的作用 | 没有证明的部分 |
|---|---|---|
| `--disable-background-timer-throttling` | 禁用后台页面 timer task 节流 | 不承诺把 `document.hidden` 变为 false，也不承诺组件展开 [C9] |
| `--disable-renderer-backgrounding` | 避免实际降低 renderer 的进程优先级 | 源码明确仍会计算优先级；不是页面可见性模拟 [C9] |
| `--disable-backgrounding-occluded-windows` | Chromium 源码把它列为避免遮挡窗口后台化的测试参数 | 本机 Chrome/macOS 对遮挡、隐藏、最小化、非活动 Space 的分别效果未测 [C9] |
| 放到其他 Space / 移到屏幕外 / 隐藏 app | 可能降低窗口对当前桌面的干扰（推断） | 是否 hidden、是否继续绘制、Foreground 是否切回该 Space、重新开窗是否留在原 Space，均待测 |
| 最小化 | 用户暂时看不到页面内容 | 可能停止可视渲染；start 请求 focus 后是否恢复、是否抢焦点，均待测 |

Chrome 官方后台策略文档把 rAF 停止与 timer 节流分别说明；虽然该文档较旧，但足以说明两者不是同一机制，不能用一个 timer flag 证明菜单在隐藏窗口中可用。[C10] 仓库的 Gemini 和小红书实测已经给出实际反例。[R4][R7]

**现成 OpenCLI 会明确请求焦点**：新窗口 `focused: mode === 'foreground'`；复用窗口时 Foreground 调用 `chrome.windows.update(...,{focused:true})`。将独立浏览器放在另一 Space，仍未改变这段行为。[O1] 在真实验收前保留两条 start 的 Foreground；不能用全局后台参数悄悄改掉它们。若 hidden start 可行，再修改命令声明与 ADR 0023 的依据；失败时保留用户主动发起的前台例外，或升级到 F。

## B：Headless 的能力与限制

Chrome 的新 Headless 已与 headful 统一实现；旧模式从 Chrome 132 起独立为 `chrome-headless-shell`。因此「Headless 不支持扩展」这个笼统说法已经不正确：Playwright 官方展示 persistent context + `channel:'chromium'` 在 headless 下加载 MV3 扩展。[C4][D5]

但这不代表 **OpenCLI Bridge** 在同样条件下可用。OpenCLI #1700 记录 Linux 新 Headless 下 Bridge service worker/连接未正常起来，headed + Xvfb 则成功；维护者把原生 headless-extension 模式留作独立功能。这是特定版本、特定 Linux 环境的报告，不是 macOS 的结论；它与 Playwright 文档并不矛盾：框架能加载某个扩展，不保证该扩展的完整生命周期和窗口 API 使用都兼容。[O8] 不建议为保住 Bridge 的 headless 路径先投入调试。

直连 CDP 可省扩展，但仍有 A3 的下载和生命周期缺口。使用 headless-shell 也不会自动获得完整 Chrome/Bridge 能力，不是本项目的低成本替换。

检测方面，当前 Chromium UA 构建代码会在 headless 时给 product 加 `Headless` 前缀，因此默认 UA 的 `HeadlessChrome` 仍是可观察差异；修改 UA 只改变一个信号，不能据此判定等同真实用户。[C11] Cloudflare 的当前支持文档明确不支持用自动化浏览器、Playwright/Puppeteer 等框架解决生产挑战；这同样提醒 A/D/F 没有挑战通过保证，而非仅针对 B。[C2]

对本站点组合的判断：

- SSRN 的 search 在现有浏览器中可匿名请求，不证明 headless 的相同请求能通过 Cloudflare；详情、PDF 更不能据此保证。[R5]
- 小红书已有登录、签名、指纹与 461/安全限制风险，headless 不改变这些约束；迁入新 profile 是新环境，风险增加幅度未知。[R7]
- Gemini/ChatGPT 可先用同一专用 profile 在 headed 下由用户登录，再关闭后以 headless 打开，技术上不必导出 cookie。但 Google 是否允许登录、会话能否续期、菜单和倒计时是否正常均未测；不得同时打开两个进程写同一 profile。[G1][D1]

因此 B 仅适合后续独立试验。当前低频、单并发没有需要以 headless 换吞吐的收益，失败恢复却更复杂。不能通过自动过 Turnstile、修改防护字段或循环换环境来补齐通过率。[R2][R3]

## C：为什么不迁移主浏览器 cookie

要把两个诉求分开：**复用登录状态**可以通过同一专用 profile 长期运行实现；**复用主 Chrome 的现成 cookie 到另一个运行时**是凭据迁移，增加了不同的问题。

1. **macOS 磁盘存储不是明文会话。** Chromium 当前 Keychain 实现的品牌版默认服务名是 `Chrome Safe Storage`；key provider 通过钥匙串取得材料并派生磁盘加密 key，访问可能被拒绝。本文只读公开实现，未触碰本机钥匙串或 Cookie 数据库。[C5][C6]
2. **不同 data dir 的可移植性不能从同机推导。** Chrome 136 官方说明非默认 data dir 使用不同加密 key；但当前 macOS provider 又存在按品牌取钥匙串材料的路径，不能把该博客概括为「所有 macOS profile 必定各用一个完全不同的 Safe Storage 条目」，也不能反向宣称原始 SQLite 复制必然可解密。当前本机版本、构建与存储路径的具体行为未测；不需要靠解密真实数据来决定本方案应排除。[C3][C5][C6]
3. **DBSC 不再是仅 Windows 的未来事项。** Chrome Enterprise 官方发行说明列出 Chrome 147 macOS 的逐步 rollout；Chromium 2026-08-24 提交将 macOS 核心 DBSC 默认开启，当前源码也将 Windows/macOS 设为 enabled。Google 说明 macOS 可利用 Secure Enclave，短期 cookie 续期需要不可导出私钥的持有证明。复制 cookie 不等于复制这项能力。[G2][G3][G4][G5]
4. **不夸大 DBSC 覆盖。** 上述材料不能证明本机特定 Google 账号已经注册，也不能证明小红书、SSRN、ChatGPT 都使用 DBSC。浏览器支持、rollout、站点启用、某个会话实际受保护是四个层次；同一台 Mac 的另一个 profile 能否复用某个会话，也没有「只导 cookie 就行」的证据。[G2][G3]
5. **轮换不等于必然双端登出。** DBSC 的确依赖短期 cookie、浏览器续期和密钥；拷出的快照可能很快失效。但没有找到一手依据证明「把 Google cookie 拷到另一浏览器，两个会话必定互踢」。主会话与复制快照是否发生替换、撤销或异常检查取决于具体服务，本文将其列为未验证风险；分别手动登录得到独立会话更合理。[G2][G3]
6. **Cloudflare clearance 不能视为通用通行证。** 官方文档说 `cf_clearance` 绑定特定访问者与设备，且未过期仍可能被重新挑战。该文档没有公开足以复现的完整绑定算法，不把它简化为「只要 UA 和 IP 相同就能复用」。[C12]
7. **cookie 不等于全部站点状态。** persistent profile 还有站点本地存储等状态；小红书更依赖页面产生的签名/指纹。即使注入后显示登录，也不能证明搜索、评论、下载及长期续期都正常。[D1][R7]

对 ChatGPT，选型已经有确定答案：ADR 0024 禁止 forager 导出或使用 cookie/access token，也禁止自己构造 backend-api 请求。无论导入 headless 浏览器还是 HTTP 客户端，都不能用「只在本机」为理由绕过。浏览器正常保有自己手动登录的会话，与 forager 调用 `getCookies()` / Playwright `storageState()` 导出会话，是不同边界。[R3]

## D：替换 OpenCLI 的运行时

### Playwright 是需要重写时的首选候选

Playwright persistent context 可以在独立 userDataDir 中保持浏览器状态；官方反对自动化主 Chrome 默认 profile。原生 download 事件、完成后 `saveAs`、响应事件及浏览器 CDP session，足以构建现有四条 route 所需基础能力。`connectOverCDP` 官方说明 fidelity 低于 Playwright 自身协议；若未来自建运行时，优先明确管理自己的 persistent browser，而不是把「随便接一个 CDP target」当成完整替代。[D1][D2][D6]

这并不要求重写全部约 2.2k 行业务逻辑。可保留页面内脚本、响应解码规则和 adapter `{contract,status,data}`，通过小型兼容层适配现有 `page` 调用；但也不是简单换 import。需要接管的职责包括：

- adapter 注册/命令参数、进程 stdout/错误协议、启动与版本锁定；
- browser/profile 锁、页面/会话创建与回收、超时中断后的清理；
- 网络响应的字段映射、完成标记、8 MiB 边界、导航前捕获、流式响应结束；不能用另发 HTTP 请求代替页面原响应；
- SSRN 下载完成、临时文件持久化与 `%PDF-`/论文 id 核对；Playwright context 关闭会删除其临时下载，必须先交付文件；
- 小红书主世界页面状态、诱饵元素过滤、滚动/截图与 cursor 链；
- Gemini 单次发送/确认、ChatGPT 计划 iframe 倒计时和未知提交结果，不允许通用 retry 包住整个 start。[R1][R6][R7][R9][R10][O6][D2]

成本判断：Rust 进程组、限速与契约解码可以保留，但要把当前 OpenCLI 专属传输抽成真正拥有新运行时的职责；doctor、安装、错误映射、测试和分发也随之变化。Node 本机已有，**新增 Node 运行时本身不是主要成本**；主要是从「依赖外部 OpenCLI」变成「维护自己的 Node package、锁文件、浏览器版本/下载和生命周期」。Rust 主程序仍可单二进制，完整 browser 功能依旧需要外部环境；若把依赖藏进 skill 或每次动态下载 latest，不会消除这些维护责任。[R1][D1]

### 反检测候选逐一判断

| 候选 | 当前可确认事实 | 对本项目的取舍 |
|---|---|---|
| **原生 Playwright** | 官方 persistent context、下载、网络事件；headed/headless 可选 | 新运行时的首选基线；成熟能力先于反检测改造，但 Cloudflare 不承诺支持 [D1][D2][D6][C2] |
| **Patchright** | Chromium-only 的 Playwright driver 补丁；改变 execution context/默认参数，关闭 Console API；维护者承认不通过全部 Playwright 测试，部分 init script 通过修改 HTML 响应注入 | 可作为有具体兼容证据后的限定试验；不能把作者的不可检测声明当验证结果。小红书读取主世界状态须重验；ChatGPT 的响应/请求与注入边界须审计 [D3] |
| **Camoufox** | 修改后的 Firefox，使用 Playwright Firefox/Juggler；有持久化 profile，当前同时有 Python 与 TypeScript/Node launcher；`virtual` Xvfb 仅 Linux | 不必强制新增 Python，但必须新增修改版 Firefox及其版本维护；无法直接接现有 Chrome Bridge/CDP adapter。主世界状态、网络捕获及页面行为的迁移面最大，不适合作为本需求首选 [D4] |
| **rebrowser** | 本文核查开源 `rebrowser-patches` 及其 drop-in 包，不包括托管云浏览器；补丁作用于 Playwright/Puppeteer，项目自己说明随上游变化而脆弱；isolated 模式无法直接读主世界变量 | 仍需自建生命周期；不能自动修复 OpenCLI，因为 OpenCLI 使用自己的 CDP/扩展实现而非这些被补丁的库。优先级低于原生 Playwright，除非证据证明具体收益 [D7][O5] |

这些项目截至读取日有近期提交，不宜简单称为「无人维护」；活跃也不等于对四个目标站点有通过或免封保证。没有在真实账号上比较它们，不能给成功率排名。[D3][D4][D7]

ADR 判断以**实际行为**为准：换 Playwright 不必然违反 ADR 0024；调用它导出登录态、自行请求接口、改写 fetch/XHR/WebSocket或绕过防护则违反。反检测库并不是可以跳过这项审查的特殊许可。Google/OpenAI 的账号和条款风险也不会因使用「stealth」库消失。[R3][G1][G6][G7]

## agent-browser：替换运行时的一种

agent-browser 是面向 agent 的浏览器自动化 CLI。仓库 2026-09-26 的 SSRN 调研记录了本机 `0.38.1`：Rust CLI/daemon 直接控制 CDP，不依赖 Playwright，提供 snapshot/ref、语义定位、JSON 输出、独立 session 与 profile。[R13] 本文没有重新实测它。

- **分类。** 它自己启动和管理浏览器会话，或经 CDP 连接已有浏览器，与 OpenCLI 的 Browser Bridge 扩展无关，属于 D 类替换运行时。采用它意味着放弃 OpenCLI 的 adapter 注册、扩展通道与窗口租约，由 forager 承担 D 节列出的职责：命令协议、生命周期与清理、网络捕获、下载完成与文件交付。
- **复用登录态。** 同一次 SSRN 实测中，它的独立 Chrome 会话被站点以 `Content Blocked` 拒绝，`--auto-connect` 也找不到可连接的现有 Chrome。[R13] 连接用户主 Chrome 需要远程调试端口，而 Chrome 136 起默认 data dir 不接受该端口，[C3] 所以复用登录态只能落到专用 profile，即 A 的全部成本再加上更换运行时。
- **能力重叠。** snapshot/ref 和语义定位服务于 agent 的交互探索；forager 的 route 已经有固定的 adapter 页面逻辑和外壳，不需要这一层。

结论：不采用。它适合一次性探索页面结构，不作为 route 的运行时。

## LCU：由模型驱动的界面自动化

LCU 通过 MCP 向 agent 提供界面自动化：模型在一个脚本环境中调用它的 API 观察并操作界面，操作需要人工审批。运行时闭源。本文依据它对 agent 公开的接口与说明评估，没有取得源码，也没有用它访问目标站点。

- **模型须在回路中。** forager 的浏览器 route 是确定性命令：Rust 进程按固定的 adapter 逻辑运行，有截止时间、限速、契约解码和退出码，不调用模型。LCU 要由模型逐步决定操作，同一请求的操作序列不可复现，每一步还增加模型时延与费用。Gemini 和 ChatGPT start 的「只执行一次、结果未知时不重发」（ADR 0023）依赖确定的执行过程，逐步决策的模型无法给出同等保证。[R4]
- **需要人工审批。** platform 命令、research 和 `smoke --live` 都按无人值守运行，逐项审批与之冲突。
- **闭源运行时。** 本文关于 OpenCLI 窗口、抓包与错误行为的结论都来自源码核查；LCU 无法做同样的核查，出了问题也只能从外部观察。

结论：不采用。它可以在用户陪同下完成一次性界面操作，不进入 route。

## E：OpenCLI 内能改善到哪里

2.0 源码可确认的窗口与会话行为：

- interactive browser 与 adapter automation 分属 owned container；新 automation window 仍是 1280×900 的 `type:'normal'`，不是 hidden/offscreen/headless window。[O1]
- interactive 用 tab group；automation 分支明确不再创建可见的「OpenCLI Adapter」分组。tab group 是组织标签，不是取消窗口。[O1]
- ephemeral 命令结束会释放租约；最后一个 owned tab 通常导航回空白页作为 reusable placeholder，保留 container。因此 `--keep-tab false` 不应被解释成「退出后完全没有 Chrome 窗口」。[O1][O5]
- 真正的环境变量是 **`OPENCLI_WINDOW`**，值只有 foreground/background；没有发现 `OPENCLI_WINDOW_MODE` 或第三种不可见模式。显式 `--window` 优先于该环境变量；forager 每条命令都会显式传入，所以设置 `OPENCLI_WINDOW=background` 不会覆盖两条 start。[O5][R1]
- `OPENCLI_BROWSER_IDLE_TIMEOUT` 控制租约 idle timeout，不是 container window 寿命；persistent adapter session 和 borrowed bound tab 不按该 timeout 释放。daemon 的 idle 寿命也不是窗口隐藏策略。[O1][O6]
- `OPENCLI_SITE_SLOTS` 解决每站点独占标签、排队和 pacing。forager 强制 `--site-session ephemeral`，当前不会因设此变量自动进入 pooled persistent session；低频、并发 1 也无扩池需要。改 persistent/slot 会改变清理保证，须另行验证。[O5]
- 2.0 的 native Chrome API 可操作部分窗口资源，但 managed resource 有保护；并非所有 managed automation window 都能无条件经 native API 改为 minimized。源码没有自动「运行后最小化」的窗口模式；手工最小化也未解决隐藏渲染。[O1][O9]

2.0 的主要升级理由应是它的功能或修复，而不是「更后台」。先适配成功外壳和 stdout 错误，再检查退出码、Auth/Timeout/Network 分类、doctor 契约与 write outcome；最后重新验证 Bridge 断连、超时和 SIGKILL 后清理。基线版本的传输把 stderr 当错误来源，只去掉成功外壳并不足够。[O4][R1]

### A3：直连 CDP 的具体缺口

对 1.8.8 / 2.0.0 均核查后，不能将官方「支持 CDP」理解为与扩展能力等价：

1. `CDPBridge.connect` 接受 `windowMode`、`siteSession` 等选项，但实际连接逻辑只使用 endpoint、timeout、初始化脚本等；没有创建/聚焦/最小化窗口的调用，**`--window foreground/background` 在这里不执行扩展的窗口策略**。[O6]
2. HTTP endpoint 通过 `/json` 挑一个已有 page；`OPENCLI_CDP_TARGET` 帮助筛选。不是为每个命令自动创建独立 page；`tabs()` 返回空数组、`selectTab()` 无操作。[O6]
3. `CDPPage` 与继承链没有 `waitForDownload` 实现，也没有扩展路径的 `closeWindow`；OpenCLI 的 optional close 调用会跳过它。连接断开不等于 page/browser 被清理，因此现有 ephemeral + timeout 回收保证不能直接搬过去。[O5][O6]
4. 有原生点击/输入、截图及网络捕获，不仅是 `evaluate`。但捕获体在 `Network.loadingFinished` 后取回；Gemini/ChatGPT 的流式请求、draining read 与截断/完成字段仍须按当前 adapter 验收。[O6][R9][R11]
5. CDP 内建会注入 stealth 初始化脚本，不是完全没有页面改动的裸连接；这也不是四站免检测的证据。不建议再叠加未审计 init script，尤其 ChatGPT 不得改写网络函数。[O6][R3]

若将来要补 A3，应在 OpenCLI 所拥有的一般 CDP 机制里补 browser-level 下载事件、target 与生命周期，而不是在 SSRN 写一个拿 cookie 的 HTTP 下载特例。下载路径、完成状态、命令关联和超时回收都要一起覆盖；只加一个同名函数不足以等价替换。

## F：严格不占宿主前台时的补充方案

**将 headed 浏览器及其 Bridge/daemon 放到独立图形桌面**，让 foreground 指向该桌面，而不是用户当前 macOS 会话。这比依赖隐藏页面还继续绘制更直接。Apple 官方提供 Apple Silicon 上运行 macOS VM 的 Virtualization 示例；OpenCLI Linux + Xvfb 的上游报告也证实过在独立 display 中运行正常 Chromium/扩展的路径。[F1][F2]

优先比较本机 macOS VM 与已有独立设备：前者不需要把账号移到第三方云主机，且能继续用 Chrome app；后者少一层虚拟图形开销，但增加设备维护。Linux + Xvfb 属于另一种部署，不是可直接加到 macOS Chrome 命令上的 flag。VM 的图形、耗电、磁盘与账号信任环境代价，都高于 A；没有实测数据支持具体内存或成功率承诺。

F 保留 adapter 业务逻辑，但有实际集成工作：

- daemon 与扩展应在 guest 本地通信；不要直接暴露宿主/guest 的 CDP 或 Bridge 到公网。若采用 tunnel，限制为本机/SSH；此为部署建议，不沿用 OpenCLI 文档中任意公网 CDP 示例。
- Rust 当前 kill 的是本地进程组；杀死 SSH wrapper 不证明 guest 中命令与租约结束。需 guest 侧 deadline、终止和结果未知处理，不可重发 start。[R1][R4]
- SSRN 返回 guest 文件路径，宿主的 Web Fetch 无法直接读取。须在命令完成前把已完成 PDF 传回宿主临时目录，验证路径/内容/论文 id；不传签名 URL，不复制 cookie。[R2]
- guest profile 持久化、备份与人工登录/挑战窗口都需管理；平时可以关闭 VM 显示窗口，但不能因此暂停 VM 或停止它的图形渲染。具体虚拟化工具的行为待测。

这是一条满足**宿主桌面隔离**的架构路线，不是已经通过本站点验收的成品。出现新设备验证、VM 指纹限制或挑战时，仍停下来让用户处理。

## 推荐实施顺序与验收清单

本节是调研给出的实施顺序与验收项。最终决定没有采用 A 或 F（见「最终决定」）；以后重新评估时可以从这里开始。

### 最小可行方案：A1，继续用 1.8.8

先只验证独立 user-data-dir + 正式版 Chrome + 手装 Bridge + 显式 `OPENCLI_PROFILE`。用户在专用 profile 中分别手动登录；不改全局默认，不复制 profile。保留现有窗口声明、ephemeral 与 keep-tab 策略。该方案依赖现有 Node/Chrome，暂不引入 Playwright、Python、CfT 下载管理或 2.0 协议迁移。

此 MVP 的完成标准是**隔离主 Chrome、验证四条 route 的能力**，不是已经承诺所有命令都无窗口。随后按实际需要验证 A2、隐藏与防节流参数；只有实测证明两条 start 可后台运行才调整它们。若接受研究 start 短时前台例外，A 可以作为最终部署；若不能接受，直接选择 F，不继续堆叠隐藏窗口技巧。

### 上线前必须通过的本地、无目标站点测试

以下测试均未执行。测试使用临时 data dir，只访问 loopback 测试页；安装浏览器或修改本机正式安装前须征得用户同意。

| 测试 | 方法与通过标准 |
|---|---|
| profile 选择与防串线 | 两个干净测试 profile 分别打开带不同标记的 loopback 页；显式 id 只操作目标。断开目标后必须失败，不能降级到另一 profile；重启、重装扩展导致 id 变化也要可诊断 |
| 窗口状态矩阵 | 无参数基线、逐个参数、组合参数；分别测遮挡、最小化、隐藏 app、非当前 Space、屏幕外。记录宿主焦点/Space、visibility、hasFocus、rAF/timer、菜单点击、截图和流式页面更新；不能只检查导航成功 |
| Foreground 的宿主影响 | 对新建与复用 container 分别运行 foreground 命令，确认是否激活 Chrome、恢复窗口或切 Space；这是「不打扰」能否成立的直接判据 |
| 下载与文件交付 | loopback 提供 attachment PDF、延迟完成与中断下载；确认完成状态、命令归属、本地路径、文件保留/清理及中断错误。若试 CDP，先证明补全下载接口，禁止跳过 SSRN full_text |
| 捕获契约 | loopback 页面自行发 JSON、batched、流式、失败、超限响应；验证与现有 adapter 使用的字段、完成标记、drain 和导航捕获顺序一致；不记录认证头或真实会话 |
| 生命周期 | 正常完成、超时、SIGKILL、Bridge 断连/重连、浏览器重启；命令回收后无仍在执行的写动作，profile 登录状态可持久化。区分保留空白 container 与泄漏活动页面 |
| 2.0 升级（若做） | 临时包与隔离环境检查成功/错误外壳、stdout/stderr、退出码、doctor、未知结果分类；再做现有四 adapter 回归，不先改全局包 |
| F（若选） | guest 前台持续运行而宿主不切焦点；隐藏 VM 显示不暂停渲染；宿主杀 wrapper 时 guest 及时停止；下载文件回到宿主后可被现有 Web Fetch 读取 |

### 必须另获用户授权的真实站点测试

**下面的访问都须用户事先授权，不自动执行。** 先做只读验证，再单独申请会消耗额度的 start；不能把授权一次 start 解释成可批量试错。

| 站点 / 操作 | 授权后的最小测试与验收 |
|---|---|
| Google / ChatGPT / 小红书手动登录 | 用户在专用浏览器亲自登录并处理 MFA/设备验证；自动化不填密码、不接管认证、不导出状态。核对重启后登录仍在；CfT/被调试实例拒绝登录时保留准确错误并停止 |
| SSRN | 一次 search、一篇详情及一次 PDF。验证 Cloudflare 未被绕过、下载完整且 `%PDF-`/论文 id 正确、签名 URL 未落日志；有挑战时以 Auth 停止 |
| 小红书 | 一页搜索、一篇笔记、至少一次搜索或评论翻页，比较响应与页面计数、cursor 链。验证诱饵过滤及隐藏下截图推进；遇 461/安全限制立即停止，不通过重试探通过率 |
| Gemini 只读 | 无新建研究的登录状态检查，以及用户指定已有会话的 status/result；核对页面原响应、报告与来源完整性，避免重复取私人内容 |
| ChatGPT 只读 | 同上，只读用户指定会话；确认没有 auth/session、cookie/storageState 导出、自行 backend-api 请求或 fetch/XHR/WebSocket 改写 |
| Gemini start：单独授权额度 | 在本地窗口矩阵筛出的**一个**候选配置执行一次。菜单/发送/确认均完成，得到可核对会话；超时或失联先人工查历史，绝不再发送。失败后继续前台例外或选 F |
| ChatGPT start：单独授权额度 | 一个候选配置执行一次；菜单、提及、发送、iframe 计划倒计时、重开会话均完成，确认确已开始。不能因为菜单出现就判定整个后台流程通过 |
| 会话持续性与日常共存 | 用户正常使用主 Chrome 时，观察专用 profile 的一个自然续期/重启周期；只记录是否仍登录、有无额外验证和窗口干扰。不得为测试而复制 cookie、触发风控或强制撤销主会话 |

当前未解决的决定性缺口是：**A2 的 Google 登录、本机各隐藏状态的真实绘制行为、两条 start 的完整后台执行、以及 F 的 guest 图形/文件/终止链路。** 在这些测试前，能推荐的是架构和实施顺序，不能交付「四条 route 全程静默」的可用性承诺。C 的双端登出是否必然、每个站点是否启用 DBSC、各反检测库的实际封号概率仍未知；它们不是推迟 A/F 验证、或去读取真实 cookie 的理由。

## 参考链接

下列外部资料均于 **2026-10-18** 读取。GitHub 源码通过 `gh` 和临时浅克隆核查；非 GitHub 网页优先经 forager 取正文。搜索结果只用于定位，不把模型搜索摘要当事实；没有使用二手文章推定 DBSC rollout 或反检测成功率。可变文档以读取日为准，源码优先固定版本/提交。

### 仓库依据（读取：2026-10-18）

- [R1：OpenCLI 进程传输][R1]；[R2：ADR 0020][R2]；[R3：ADR 0024][R3]；[R4：ADR 0023][R4]；[R5：ADR 0025][R5]。
- [R6：SSRN 下载 adapter][R6]；[R7：小红书 route 设计与既有实测][R7]；[R8：小红书 adapter][R8]；[R9：Gemini start][R9]；[R10：ChatGPT start][R10]；[R11：ChatGPT 网络读取][R11]；[R12：ChatGPT 既有调研][R12]；[R13：SSRN 接入调研中的 agent-browser 实测][R13]。

### OpenCLI（读取：2026-10-18）

- [O1：2.0 扩展窗口、租约与 profile 实现][O1]；[O2：1.8.8 profile 选择][O2]；[O3：1.8.8 多 profile daemon][O3]。
- [O4：2.0 发布说明与协议变化][O4]，另见 [JSON 输出实现](https://github.com/jackwener/OpenCLI/blob/74fe1dc11c26d44b2a01102fd341aa55ae407774/src/command-output.ts)。
- [O5：2.0 adapter 执行与 session/slot/window 选项][O5]；[O6：2.0 CDP 实现][O6]、[继承的页面方法](https://github.com/jackwener/OpenCLI/blob/74fe1dc11c26d44b2a01102fd341aa55ae407774/src/browser/base-page.ts)、[CDP 官方指南](https://github.com/jackwener/OpenCLI/blob/74fe1dc11c26d44b2a01102fd341aa55ae407774/docs/advanced/cdp.md)、[idle 配置转发](https://github.com/jackwener/OpenCLI/blob/74fe1dc11c26d44b2a01102fd341aa55ae407774/src/browser/daemon-client.ts)。
- [O7：#721，后台 adapter window 的设计解释][O7]；[O8：#1700，headless Bridge 与 Xvfb 的用户/维护者记录][O8]；[O9：Chrome native API 文档][O9]。issue 是具体环境证据，不作为普遍平台保证。

### Chrome / Chromium / Cloudflare（读取：2026-10-18）

- [C1：Chrome for Testing 的目的与版本管理][C1]；[C2：Cloudflare 支持与不支持的浏览器环境][C2]；[C3：Chrome 136 远程调试限制][C3]；[C4：统一 Headless 与 headless-shell][C4]。
- [C5：macOS Keychain key provider][C5]；[C6：Safe Storage 名称与 Keychain 材料][C6]；[C7：Chrome 137 移除 load-extension 的官方公告][C7]；[C8：Chrome 139 后续扩展 flag 调整][C8]。
- [C9：后台参数的 Chromium 定义][C9]；[C10：Chrome 后台页面调度说明][C10]（旧文档，只用于机制区分）；[C11：Headless UA 构建代码][C11]；[C12：Cloudflare clearance 的绑定与再挑战][C12]。

### 登录、会话与条款（读取：2026-10-18）

- [G1：Google 登录支持的浏览器与自动化限制][G1]；[G2：DBSC 官方机制文档][G2]；[G3：Google DBSC 公告，2026-04-09][G3]。
- [G4：Chrome Enterprise release notes，Chrome 147 macOS DBSC rollout][G4]；[G5：2026-08-24 macOS 默认启用 DBSC 提交][G5]，交叉检查 [当前 net feature 定义](https://github.com/chromium/chromium/blob/d15c7054891604504f6d8a96fbed1682a671ffae/net/base/features.cc)。这些不证明特定用户会话启用。
- [G6：OpenAI 使用条款，页面所列 2026-01-01 生效][G6]；[G7：Google 使用条款][G7]。本文只据条款说明现有接受风险没有因换工具消失，不给封号概率或法律保证；调研期间未访问目标服务的 robots.txt。

### 替代运行时与隔离桌面（读取：2026-10-18）

- [D1：Playwright persistent context、主 profile 限制及 CDP fidelity][D1]；[D2：下载与临时文件生命周期][D2]；[D5：headless MV3 扩展示例][D5]；[D6：网络事件与响应读取][D6]。
- [D3：Patchright driver 的补丁、限制与维护说明][D3]；[D4：Camoufox 当前架构与语言支持][D4]，另见 [Node launcher / persistent 与 virtual 模式](https://github.com/daijro/camoufox/blob/18604faea2a7d4cf70442ab9ce9ab8c7e2810664/typescript/README.md)；[D7：rebrowser 补丁机制与兼容限制][D7]。项目自己的不可检测声明不视为验证结果。
- [F1：Apple Silicon macOS VM 官方示例][F1]；[F2：OpenCLI headed + Xvfb 已报告的部署路径][F2]。

[R1]: ../../src/capabilities/providers/opencli.rs
[R2]: ../adr/0020-opt-in-browser-routes-paced-per-operation.md
[R3]: ../adr/0024-accept-openai-terms-risk-under-conditions.md
[R4]: ../adr/0023-delegated-research-reports-and-explicit-command-process-providers.md
[R5]: ../adr/0025-ssrn-search-requests-the-site-search-api-in-the-browser.md
[R6]: ../../skills/forager/opencli/ssrn/paper.js
[R7]: ../design/2026-10-08-xiaohongshu-opencli-route.md
[R8]: ../../skills/forager/opencli/forager-xhs/
[R9]: ../../skills/forager/opencli/forager-gemini/start.js
[R10]: ../../skills/forager/opencli/forager-chatgpt/start.js
[R11]: ../../skills/forager/opencli/forager-chatgpt/shared.js
[R12]: 2026-10-10-chatgpt-deep-research.md
[R13]: 2026-09-26-ssrn-integration.md
[O1]: https://github.com/jackwener/OpenCLI/blob/74fe1dc11c26d44b2a01102fd341aa55ae407774/extension/src/background.ts
[O2]: https://github.com/jackwener/OpenCLI/blob/8271afc67e8504bda94c147f446ee29775d08274/src/browser/profile.ts
[O3]: https://github.com/jackwener/OpenCLI/blob/8271afc67e8504bda94c147f446ee29775d08274/src/daemon.ts
[O4]: https://github.com/jackwener/OpenCLI/releases/tag/v2.0.0
[O5]: https://github.com/jackwener/OpenCLI/blob/74fe1dc11c26d44b2a01102fd341aa55ae407774/src/execution.ts
[O6]: https://github.com/jackwener/OpenCLI/blob/74fe1dc11c26d44b2a01102fd341aa55ae407774/src/browser/cdp.ts
[O7]: https://github.com/jackwener/OpenCLI/issues/721#issuecomment-4423652560
[O8]: https://github.com/jackwener/OpenCLI/issues/1700
[O9]: https://github.com/jackwener/OpenCLI/blob/74fe1dc11c26d44b2a01102fd341aa55ae407774/docs/guide/browser-bridge.md
[C1]: https://developer.chrome.com/blog/chrome-for-testing
[C2]: https://developers.cloudflare.com/cloudflare-challenges/reference/supported-browsers/
[C3]: https://developer.chrome.com/blog/remote-debugging-port
[C4]: https://developer.chrome.com/docs/automation-and-testing/headless
[C5]: https://github.com/chromium/chromium/blob/d15c7054891604504f6d8a96fbed1682a671ffae/components/os_crypt/async/browser/keychain_key_provider.mm
[C6]: https://github.com/chromium/chromium/blob/d15c7054891604504f6d8a96fbed1682a671ffae/components/os_crypt/common/keychain_password_mac.mm
[C7]: https://groups.google.com/a/chromium.org/g/chromium-extensions/c/1-g8EFx2BBY/m/S0ET5wPjCAAJ
[C8]: https://groups.google.com/a/chromium.org/g/chromium-extensions/c/FxMU1TvxWWg/m/daZVTYNlBQAJ
[C9]: https://github.com/chromium/chromium/blob/d15c7054891604504f6d8a96fbed1682a671ffae/content/public/common/content_switches.cc
[C10]: https://developer.chrome.com/blog/background_tabs
[C11]: https://github.com/chromium/chromium/blob/d15c7054891604504f6d8a96fbed1682a671ffae/components/embedder_support/user_agent_utils.cc
[C12]: https://developers.cloudflare.com/cloudflare-challenges/concepts/clearance/
[G1]: https://support.google.com/accounts/answer/7675428?hl=en
[G2]: https://developer.chrome.com/docs/web-platform/device-bound-session-credentials
[G3]: https://blog.google/security/protecting-cookies-with-device-bound-session-credentials/
[G4]: https://support.google.com/chrome/a/answer/10314655?hl=en
[G5]: https://chromium.googlesource.com/chromium/src/net/+/fed835d4f3165f6e1a813cfd40649475f9667712
[G6]: https://openai.com/policies/terms-of-use/
[G7]: https://policies.google.com/terms
[D1]: https://playwright.dev/docs/api/class-browsertype#browser-type-launch-persistent-context
[D2]: https://playwright.dev/docs/downloads
[D3]: https://github.com/Kaliiiiiiiiii-Vinyzu/patchright/blob/2d2b887b8e3f8bfa2aca79e75798025acae39415/README.md
[D4]: https://github.com/daijro/camoufox/blob/18604faea2a7d4cf70442ab9ce9ab8c7e2810664/README.md
[D5]: https://playwright.dev/docs/chrome-extensions
[D6]: https://playwright.dev/docs/network
[D7]: https://github.com/rebrowser/rebrowser-patches/blob/77f7ab716385902adb9eb86b6643ab5d6bb2b566/README.md
[F1]: https://developer.apple.com/documentation/virtualization/running-macos-in-a-virtual-machine-on-apple-silicon
[F2]: https://github.com/jackwener/OpenCLI/issues/1700#issuecomment-4516657329
