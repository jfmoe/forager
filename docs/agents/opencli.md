# OpenCLI adapter 开发与实测

浏览器 route（`ssrn_browser`、`xiaohongshu_browser`）和委托研究供应方（`gemini_browser`、`chatgpt_browser`）都是通过 OpenCLI 操控用户自己的 Chrome。各 adapter 的职责见 `docs/spec/forager/04-architecture.md`，接入规则见第 7 章。本文只记录本机开发和实测时要注意的事。

## 本机安装

- forager 只支持 OpenCLI 2.x：传输层只解码 2.x 的输出外壳，遇到 1.x 的输出会报 Runtime 并提示升级到 OpenCLI 2.x 和 Browser Bridge 1.0.25。
- `opencli` 通过 npm 全局安装。安装和更新都用 `npm install -g @jackwener/opencli@2`，随后运行 `opencli daemon restart` 和 `opencli doctor`。本机使用 nvm，当前 Node 的 `bin` 目录须排在 `/usr/local/bin` 前；用 `command -v opencli` 确认命中 npm 安装的命令，切换 Node 版本后须重新确认。OpenCLIApp、登录启动项、保活辅助服务及 `/usr/local/bin/opencli` shim 已卸载；App 账号数据仍保留在 `~/Library/Application Support/BrowserBridge`，供恢复安装时使用。
- Browser Bridge 扩展须为 1.0.25 或更高。扩展不随 npm 包更新，只能手动更新：从 OpenCLI 的 GitHub release 取得扩展，在 Chrome 中替换安装，再用 `opencli doctor` 确认扩展版本和连接状态。
- adapter 的源码在 `skills/forager/opencli/<name>/`，安装位置是 `~/.opencli/clis/<name>/`。改完 JS 后，把整个目录替换过去，再用 `diff -r skills/forager/opencli/<name> ~/.opencli/clis/<name>` 确认一致。只改 JS 不用重新编译 Rust。
- OpenCLI 会加载 `~/.opencli/clis/` 下的每个目录。备份或副本（如 `forager-gemini.bak`）会注册同名 adapter，旧代码可能盖过新代码，所以那里只放正式安装的 adapter。探测脚本装成单独的名字（例如 `ssrnprobe`），用完就删。

## 实测前

- 先运行 `opencli doctor`，确认 Browser Bridge 扩展已连接、版本不低于 1.0.25。Chrome 重启后扩展可能断开，之后每条命令都要等满约 45 秒才超时，根本到不了站点。看到连续超时就停下来检查连接，不要继续循环。
- 用 `forager doctor --provider <route>` 检查单个 route。真实检索会消耗站点的访问额度，可能触发风控或人工验证，按第 7 章和 ADR 0020、0023、0024 的限制运行。

## 已知问题

- **`Navigation rejected.`**：标签页没开网络抓包时，OpenCLI 扩展会在导航前断开调试器，Chrome 随即拒绝这次导航。所以 adapter 每次顶层导航前都要先开抓包，Gemini 和 ChatGPT 的 `shared.js`、SSRN 的 `paper.js` 已经这样做。新 adapter 或新命令遇到这个报错，按同样方式处理。重启 Chrome 也能暂时消除。还观察到运行过带 `--trace` 的命令后，不带 trace 的命令会连续被拒，但没有确认原因。
- **诱饵按钮**：小红书会在按钮下面叠放同类名、同文本的不可见副本（`aria-hidden`），按类名和文本找元素会找到两个，或先找到诱饵。adapter 的点击都经 `shared.js` 原生输入块的 `clickTarget`：只认不在 `aria-hidden` 子树中、指针在中心能命中的元素，并要求恰好一个，详见小红书 route 设计的「诱饵按钮」。
- **抓包条目完成了却没有响应体**：扩展只在请求结束时向 Chrome 取一次响应体，取失败时不报错，条目照样标记完成（`captureComplete`），只是没有 `responsePreview`。2026-10-20 在 Browser Bridge 1.0.26、Chrome 155 上实测，Gemini 的 batchexecute 响应随机丢失响应体，`hNvQHb` 约一半的打开会丢；这与 adapter 在页面上的读取无关，页面加载期间不发任何命令也一样丢。Chrome 那边取失败的原因没有查明。页面重新加载后会重新取，所以 Gemini 的 `report` 遇到这种情况会重新打开会话再读。
- **Google 的异常流量页**：短时间内反复打开 Gemini 页面（实测约 25 分钟内约 50 次），会被重定向到 `www.google.com/sorry/...`，之后的读取都停在那一页，停止访问 17 分钟后仍未解除；adapter 不会也不应替用户通过那里的验证。实测时控制打开次数，不要循环探测。
- **只有实测能证明 JS 的行为**：Rust 侧的测试用的是假 `opencli`，只证明外壳解码、退出码映射和核对规则。adapter 的页面读取、抓包完成条件和点击路径，只能靠实测证明：小红书有 live smoke 用例（C27–C29），Gemini 和 ChatGPT 没有 live 用例，只能在用户授权下做真实验收（第 5 章）；SSRN 的浏览器 route 经常被 Cloudflare 拦截，没有 live 用例，也不做实测，所以 SSRN adapter 没有经过 live 验证。

## 原生输入的已知限制

规则见第 7 章「adapter 原生输入」。实现是 `forager-xhs/shared.js` 末尾以 `---- Native input` 和 `---- End of native input` 标出的块：`clickTarget`、`hoverTarget`、`wheelAt`、`typeText`、`pressNativeKey`、`humanPause` 与 `confirmWithin`，失败时抛出带原因的 `InputFailure`。其他做页面操作的 adapter 在各自的 `shared.js` 中逐字复制这个块；改动时同步修改所有副本，再用 `diff` 确认一致。

以下依据 OpenCLI 2.0.0 与 Browser Bridge 1.0.25 的源码。标「未实测」的条目没有在本机验证；OpenCLI 升级后要复核这些接口是否仍在、行为是否变化。

- **`nativeClick(x, y)` 不做命中检测。** 它在同一点连续发出 `mouseMoved`、`mousePressed`、`mouseReleased`，坐标是视口内的 CSS 像素，中间没有停顿、滚动或命中检测。坐标是先在页面里测量、再经 CDP 送回的，期间布局变化会点歪。调用前要自己确认坐标命中的就是目标元素，点击后核对页面结果。`clickTarget` 先悬停 80–250 毫秒，再在同一点重新做一次命中检测，不命中就以 `target_moved` 失败。
- **高层操作会悄悄退回脚本。** `page.click` 在元素被遮挡或原生点击失败时退回 `element.click()`；`hover`、`typeText`、`pressKey` 在原生路径失败时分别退回 `dispatchEvent`、DOM setter 和脚本键盘事件；`tryNative*` 系列吞掉错误只返回 `false`。要求「失败即停、不退回脚本」时，直接用 `nativeClick`、`nativeType`、`insertText` 或 `page.cdp('Input.*', ...)`。
- **`nativeKeyPress` 只带 `key`。** 它发出的 `Input.dispatchKeyEvent` 只有 `key` 和修饰键位，没有 `code`、`windowsVirtualKeyCode`、`text` 或 `commands`。Backspace、Delete、全选这类编辑键可能只触发 DOM 键盘事件，不实际编辑（未实测）。所以 `pressNativeKey` 改用 `page.cdp('Input.dispatchKeyEvent', ...)`：发 `rawKeyDown` 与 `keyUp`，带 `code` 和 `windowsVirtualKeyCode`，全选另按下 Meta（macOS）或 Control；在 macOS 上还带 `commands`（`selectAll`、`deleteBackward`、`deleteForward`），因为 Chrome 在 macOS 上只经编辑命令执行编辑键（未实测）。
- **`nativeType` 就是 `Input.insertText`。** 它只产生 `beforeinput` 和 `input`，没有 `keydown` 和 `keyup`。逐字输入就是按字符逐次调用。
- **滚动没有原生 helper。** `page.scroll` 和 `autoScroll` 是页面脚本。原生滚动用 `page.cdp('Input.dispatchMouseEvent', {type: 'mouseWheel', x, y, deltaX: 0, deltaY})`，滚动的是指针下最内层的滚动区域。`wheelAt` 先把指针移到滚动区域中央，再分几步滚动，每步 300–500 像素，一次调用最多滚 12 步；把目标滚进视口也用它，不用 `scrollIntoView` 或 `DOM.scrollIntoViewIfNeeded`。
- **扩展模式下 `page.cdp` 有白名单。** 放行的方法包括 `Input.dispatchMouseEvent`、`Input.dispatchKeyEvent`、`Input.insertText` 和 `Page.captureScreenshot`，其他方法报 `CDP method not permitted`。`mouseMoved` 和 `mouseWheel` 能否在后台窗口里生效，未实测。
- **不可见窗口可能不推进渲染。** 后台窗口被遮挡或不在当前桌面时，页面可能停在渲染帧上，原生输入随之等待。小红书翻页和评论加载靠 `page.screenshot` 强制渲染；为把目标滚进视口而滚动后，`clickTarget` 和 `hoverTarget` 也先截一次图，再测量位置。点击和按键前是否也要先截图，未实测。
- **原生输入不是反检测。** CDP 合成的鼠标事件仍有公开讨论过的自动化特征，比如 `screenX/Y` 可能等于 `clientX/Y`。原生输入只去掉 `isTrusted` 为 false、没有坐标这类最明显的差异。
