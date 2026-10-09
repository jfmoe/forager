# 委托研究报告不是证据；只由点名命令运行的 process 供应方

`gemini_browser`（设计见 [Gemini Deep Research 浏览器 route 设计](../design/2026-10-09-gemini-deep-research-route.md)）经本机 OpenCLI 驱动用户已登录的 Chrome，按会话读取 Gemini Deep Research 的状态与报告。它与 `ssrn_browser`、`xiaohongshu_browser` 共用 OpenCLI 传输，但不是 Platform Route：它不属于任何 Platform 或 Capability Catalog，产物也不是平台条目。本记录确定四件事：报告在 forager 中的地位、会改变账号状态的写操作如何执行、OpenCLI 窗口模式的归属，以及 doctor 如何对待这类供应方。

## 委托研究报告不是证据

报告是另一个研究 agent 的综合结论。它的来源由 Gemini 自己声明，forager 没有读过；正文中的论断与来源之间的对应也只是 Gemini 的标注。

- 报告是 Delegated Research Report（`GLOSSARY.md`），不是 Research Evidence。它不进入 `forager research` 的流水线、证据索引或覆盖记账（ADR 0012），也不写入 search journal。
- 报告与来源只落盘交付：`gemini-<id>.md`（正文原样，末尾附 `## Sources` 编号列表）与 `gemini-<id>.sources.json`；stdout 只给元数据与路径（ADR 0015）。
- 使用方可以把报告作为归属明确的第三方观点转述，或用其来源定向；要作为事实陈述的论断，须先由 forager 取证。

## 写操作只执行一次 attempt

`gemini research start` 是 forager 第一个会改变用户账号状态的操作：它新建会话并消耗 Deep Research 额度。共享重试策略假定重复一次 attempt 没有副作用，这对写操作不成立：超时或断网时，forager 无法知道问题是否已经送达 Gemini，重试可能重复消耗额度，并在历史里留下重复会话。

- `start` 只执行一次 attempt，不受 `retry` 配置影响；Timeout 与 Network 也不重试。adapter 内部同样只点击一次发送、一次确认，失败的步骤不再尝试。
- 计划已生成但确认没有点到时，返回 Runtime，附会话 URL，请用户在网页上手动点击开始研究；forager 不再次点击，也不重发问题。
- forager 一旦拿到并校验过会话 id，失败载荷的 `conversation_url` 与消息都指向该会话。adapter 进程本身失败、没有交回页面事实时，消息说明提交结果未知，提示先在 Gemini 历史中检查，绝不建议重新发起。
- 只读操作（`result`、`status`）不受此约束，`result` 沿用共享重试策略。以后的写操作沿用同一规则。

## 窗口模式由命令声明

OpenCLI 的 `--window` 原先对所有命令固定为 `background`。Gemini 的工具菜单在 `document.hidden` 为 true 的后台窗口中打不开（2026-10-09 实测），发起研究（#194）必须用前台窗口；而读取在后台窗口中正常。

- `OpenCliCommand` 携带命令自己的窗口模式，传输层只负责把它写进调用参数。Gemini 的 `start` 声明前台；SSRN、小红书与 Gemini 的 `report`、`status` 都声明后台，调用参数不变。
- 前台窗口会在用户屏幕上弹出 Chrome，只允许用户主动发起、且没有后台替代的命令使用。

## 只由点名命令运行的 process 供应方

ADR 0020 让 process route 只能由用户写进平台 order 来启用，doctor 也只检查 order 列出的 process route。`gemini_browser` 不在任何 order 中，只有用户或 agent 显式运行 `forager gemini …` 才会执行，因此不新增 `enabled` 开关。

- 默认 doctor 跳过不属于任何平台的 process 供应方：不运行 OpenCLI，报告 `configured: false` 并提示用 `--provider` 检查，不影响整体健康。打开 Chrome 窗口不应成为每次体检的副作用。
- `doctor --provider gemini_browser` 运行 adapter 的只读 `status` 命令（`DoctorProbe::AdapterStatus`），报告 OpenCLI 是否可用、adapter 契约版本与登录状态；adapter 缺失或过期时消息附安装步骤。它不读取任何会话，也不发起研究。
- 判断依据是传输类型与是否属于平台 catalog，而不是 provider id。平台 process route 的门控（order 未列出时 doctor 跳过、`--provider` 拒绝）不变。
- live smoke 不为它登记用例，只做注册完整性检查：live 用例要么读取用户的私人会话，要么消耗 Deep Research 额度。catalog 一致性测试显式列出这一例外。

## 影响

- 访问策略沿用 process route 规则：每 10 秒一条 OpenCLI 命令，跨进程并发 1。
- 以后接入其他第三方研究 agent 时，报告同样按委托研究报告对待；若同为只由点名命令运行的 process 供应方，复用本记录的 doctor 门控与 smoke 例外。
