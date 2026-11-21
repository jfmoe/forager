# 浏览器 route 的开发与实测

浏览器 route（`xiaohongshu_browser` 与 `reddit_browser`）经 forager 自带的浏览器传输，在平台专用的 Browser Profile 中启动 Managed Browser 并运行 route 自己的页面逻辑。传输与页面逻辑的规则见规格第 7 章「浏览器传输」「小红书」与「Reddit」，模块归属见第 4 章「浏览器传输职责」，决策见 ADR 0036，Reddit 改用浏览器 route 的决策见 ADR 0037。本文只记录本机开发和实测时要遵守的事。

## 哪一层证明什么

| 层 | 运行方式 | 证明 |
| --- | --- | --- |
| 判定的单元测试（`*_tests.rs`） | 完整测试套件 | 观察结果到分类、核对、解码与输出的规则 |
| 替身浏览器的二进制测试 | 完整测试套件 | 启动参数、飞行前检查、启用说明、截止时间与进程清理；替身不讲 CDP |
| 真实浏览器测试（`browser_real_tests.rs`、`ops/browser_setup.rs` 中的登录等待与 `reddit_browser_search_tests.rs`、`reddit_browser_post_tests.rs` 中的投影） | 默认忽略，见下文 | 原生输入、唯一可命中目标、滚轮加载、抓包、profile 落盘、forager 被杀后浏览器退出、Reddit 投影脚本在页面上的取值；只访问本机页面 |
| live smoke C27–C29（小红书）、C50–C51（Reddit） | `forager smoke --live` | 真实站点上的页面驱动与成功路径；唯一能发现页面改版的一层 |

改了页面驱动（导航、输入、等待、抓包过滤），只有 live smoke 能证明它；改了判定，单元测试就够。

## 真实浏览器测试

这些测试标记为 `#[ignore]`，完整测试套件把它们列为 skipped。改动浏览器传输、`browser login` 或 Managed Browser 之后，以及每次发版之前，在装有 Managed Browser 的主机上运行：

```sh
cargo nextest run --lib --locked --run-ignored only
```

- Reddit 的投影测试（搜索页与帖子页各一个）在本机页面（`tests/fixtures/reddit_browser/*.html`）上运行页面内的投影脚本，断言输出等于判定测试读的 JSON fixture；改了投影脚本或本机页面后，设置 `FORAGER_WRITE_PROJECTIONS=1` 运行它重写 JSON，再审阅差异。
- 需要已安装的 Managed Browser（`forager browser install`），或用 `FORAGER_REAL_BROWSER_PATH` 指定另一个浏览器可执行文件。找不到浏览器时测试以原因失败，不会悄悄通过。
- 测试只打开本机起的静态页面，不访问任何真实站点，不需要登录，每个测试用自己的临时 Browser Profile。

## live 实测的前提

以下条件全部成立后，才运行会打开站点页面的命令（平台命令、`doctor --provider <route>`、`smoke --live`）。下文的 `<platform>` 是 `xiaohongshu` 或 `reddit`：

- Managed Browser 已安装：`forager browser install` 退 0。已装固定版本时它不下载，输出 `downloaded: false`。
- 用户已经亲自登录：由用户运行 `forager browser login <platform>` 并在弹出的窗口中登录（小红书扫码，Reddit 用账号登录），命令退 0 后 Browser Profile 才有登录标记。Reddit 的风险落在登录的账号上，用哪个账号由用户决定。
- `platforms.<platform>.order` 含该 route。route 由用户决定启用；用户没有要求时，不替用户修改配置文件。验证时可以只对本次命令用环境变量临时启用，如 `FORAGER_PLATFORMS__REDDIT__ORDER='["reddit_browser"]'`。`smoke --live --opt-in`（发版收尾的 live 步骤）不要求这一条：浏览器已安装、Browser Profile 有登录标记时，它也运行该平台的用例，用户的登录就是同意。
- `forager doctor` 中该 route 的 `reachable` 为 true。shallow doctor 只做飞行前检查，不启动浏览器、不访问站点；它不通过时，`message` 给出修复命令。

验证未发布的改动时运行 `./target/debug/forager`，本机安装的 `forager` 是上一次发版的版本。

## 节奏与预算

- route 自己限速（每 10 秒 1 条浏览器命令、并发 1、跨进程生效）且不重试。不要为了绕过限速并行启动多个命令。
- **页面打开预算**：每条 live 的平台命令、deep doctor 与 smoke 用例都会打开真实页面。同一站点一次排查最多约 10 次，超出前先问用户。测故障出现率这类需要多次采样的问题，也受这个预算约束。
- **每次打开都存档**：把 live 命令的完整 stdout 与 stderr 写入文件，之后从文件分析，不为看另一个字段再跑一次。
- **Reddit 的帖子 ID 只从搜索结果中取**，不凭记忆写：一个记错的 ID 曾打开一个 NSFW 版块的帖子，账号的浏览记录里会留下它。不打开 NSFW 内容。

## 停止规则

小红书出现以下任一情况，立即停止该站点的全部 live 命令与循环：

- `auth`：未登录（含接口返回 `code` −101 或 −100），HTTP 461 或 471 要求验证，或页面提示"请求太频繁""访问频次异常"；
- `parameter`：300031、300017（页面跳转或接口返回）、安全限制或访问链接异常；
- 页面显示验证码或验证页（route 认不出这类页面，报 Runtime，消息带页面标题与 URL）；
- 接口返回 route 无法定性的非 0 `code`（Runtime，消息以 `Xiaohongshu answered` 开头、带码值）；
- 连续两次 `timeout`。

Reddit 出现以下任一情况同样立即停止：`auth`（验证页 `Prove your humanity`、文档 403 或 429、页面未登录），以及连续两次 `timeout`。

停下后先把离线能做的部分做完，再向用户报告看到的错误与次数，由用户运行 `forager browser login <platform>`，在窗口中处理登录或验证。拦截不会因为等待很快解除，等待冷却后再跑只会再次触发拦截。

## 边界

所有 live 操作都以用户的账号进行，以下是硬边界：

- 登录与验证只由用户亲手完成。agent 不代为扫码、输入账号或验证码，也不替用户点击验证页。
- 登录态只留在 Browser Profile 中。不读取、复制或导出其中的 cookie、token 或任何文件（profile 在 `$XDG_STATE_HOME/forager/browser/<platform>`，未设置时为 `~/.local/state/forager/browser/<platform>`），也不把 profile 拷到别处做实验。
- 只读页面自己发出的请求。页面逻辑不发起自己的接口请求，不改写 `fetch`、XHR 或 WebSocket，改变页面状态只用原生输入（第 7 章「原生输入」）。
- 风险信号之后不重试，不换 profile、换浏览器或换指纹再试。
- 提交与实测记录中不出现 Access Token（`xsec_token`）、cookie、账号标识，以及真实用户的笔记或用户 ID；fixture 中的这些值用假值。

## 已知情况

- **登录窗口开着时检索会等待**：`browser login` 与检索命令共用 route 的限速 permit，并持有到浏览器退出。登录窗口还开着时，检索命令等待 permit，到截止时间仍等不到即为 Timeout。拿到 permit 后 Browser Profile 仍被另一个进程占用时，命令立即以 Runtime 失败（`the Browser Profile … is in use by another forager process`）。
- **固定版本变了**：升级 forager 后若固定的浏览器版本不同，命令提示已安装的旧版本并要求运行 `forager browser install`。运行时只认固定版本。
- **登录标记不等于会话有效**：会话过期时，命令以 Auth 失败并提示 `forager browser login <platform>`，飞行前检查照样通过。
- **握手失败**：浏览器在 CDP 握手前退出或 20 秒内没有答复时为 Runtime，消息带浏览器 stderr 的最后一行，退出时还带退出状态，先看这两项。
- **搜索结果页布局**：2026-11-18 只见到 `/search_result_ai` 新布局；旧布局 `/search_result` 的选择器保留但没有实测。
- **不能用 `success` 判断接口失败**：`search/onebox` 会返回 `code: 0` 而 `success: false`；账户接口 `user/me` 对访客返回 `code: 0` 而 `guest: true`。见 [实测记录](../research/2026-11-18-xiaohongshu-headless-fingerprint-browser.md)。
- **诱饵副本**：站点在按钮下叠放同类名、同文本的不可见副本，筛选面板的每个选项也渲染两份。点击都经原生输入的唯一可命中目标规则，不按类名或文本直接取第一个元素。
