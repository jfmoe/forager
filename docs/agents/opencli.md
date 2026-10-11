# OpenCLI adapter 开发与实测

浏览器 route（`ssrn_browser`、`xiaohongshu_browser`）和委托研究供应方（`gemini_browser`、`chatgpt_browser`）都是通过 OpenCLI 操控用户自己的 Chrome。各 adapter 的职责见 `docs/spec/forager/04-architecture.md`，接入规则见第 7 章。本文只记录本机开发和实测时要注意的事。

## 本机安装

- `opencli` 是 OpenCLIApp 管理的 shim（`/usr/local/bin/opencli` 转发给 `/Applications/OpenCLIApp.app`），不是 npm 全局包。OpenCLI 随 App 升级。要试别的 OpenCLI 版本，就用 npm 装进临时目录，不要替换 shim。
- adapter 的源码在 `skills/forager/opencli/<name>/`，安装位置是 `~/.opencli/clis/<name>/`。改完 JS 后，把整个目录替换过去，再用 `diff -r skills/forager/opencli/<name> ~/.opencli/clis/<name>` 确认一致。只改 JS 不用重新编译 Rust。
- OpenCLI 会加载 `~/.opencli/clis/` 下的每个目录。备份或副本（如 `forager-gemini.bak`）会注册同名 adapter，旧代码可能盖过新代码，所以那里只放正式安装的 adapter。探测脚本装成单独的名字（例如 `ssrnprobe`），用完就删。

## 实测前

- 先运行 `opencli doctor`，确认 Browser Bridge 扩展已连接。Chrome 重启后扩展可能断开，之后每条命令都要等满约 45 秒才超时，根本到不了站点。看到连续超时就停下来检查连接，不要继续循环。
- 用 `forager doctor --provider <route>` 检查单个 route。真实检索会消耗站点的访问额度，可能触发风控或人工验证，按第 7 章和 ADR 0020、0023、0024 的限制运行。

## 已知问题

- **`Navigation rejected.`**：标签页没开网络抓包时，OpenCLI 扩展会在导航前断开调试器，Chrome 随即拒绝这次导航。所以 adapter 每次顶层导航前都要先开抓包，Gemini 和 ChatGPT 的 `shared.js`、SSRN 的 `paper.js` 已经这样做。新 adapter 或新命令遇到这个报错，按同样方式处理。重启 Chrome 也能暂时消除。还观察到运行过带 `--trace` 的命令后，不带 trace 的命令会连续被拒，但没有确认原因。
- **只有实测能证明 JS 的行为**：Rust 侧的测试用的是假 `opencli`，只证明外壳解码、退出码映射和核对规则。adapter 的页面读取、抓包完成条件和点击路径，只能靠实测证明：SSRN 和小红书有 live smoke 用例（C22、C23、C27–C29），Gemini 和 ChatGPT 没有 live 用例，只能在用户授权下做真实验收（第 5 章）。
