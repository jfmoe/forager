# 在条件约束下接受 OpenAI 使用条款风险

`chatgpt_browser` 经本机 OpenCLI 驱动用户已登录的 Chrome，打开 ChatGPT 网页版的 Deep Research 会话，读取页面自己收到的会话响应，把报告与来源交付为委托研究报告（ADR 0023）。调研与实测见 [ChatGPT Deep Research 接入可行性](../research/2026-10-10-chatgpt-deep-research.md)。这条 route 的核心动作本身就落在 OpenAI 使用条款的禁止范围内。本记录写明这一风险，以及维护者接受它的条件。

## 风险

- OpenAI 面向个人用户的[使用条款](https://openai.com/policies/terms-of-use/)（2026-01-01 生效）明文禁止 “Automatically or programmatically extract data or Output”，也禁止绕过限流、防护措施或安全缓解。
- [`chatgpt.com/robots.txt`](https://chatgpt.com/robots.txt)（2026-10-10 读取）对 `User-agent: *` 列出少数允许路径后以 `Disallow: /` 收尾；会话页 `/c/` 与 `/backend-api/` 都在禁止范围内。
- 这比 Gemini 高一级：Google 的条款只禁止违反 robots.txt 的自动访问，而 OpenAI 的禁止不以 robots.txt 为条件，直接覆盖程序化提取输出。涉及的又是用户的付费账号。ADR 0020 对 SSRN 的风险接受不能直接沿用到这里。
- 以上是对条款文本的解读，不是法律意见。

## 接受风险的条件

维护者接受这一风险，前提是以下条件同时成立：

- **仅限个人使用，只由点名命令触发**：只有用户或 agent 显式运行 `forager chatgpt …` 才会执行。
- **只读取页面自己收到的响应**：adapter 像用户一样打开页面，只回报页面自己发出的请求得到的响应体；不改写页面的 fetch、XHR 或 WebSocket。
- **不导出也不使用 cookie 或 access token**，不调用 `/api/auth/session`，forager 不保管 ChatGPT 会话。
- **不构造请求**：不构造 `chat-requirements`、工作量证明或 Turnstile 字段，不自行请求 `backend-api`。
- **节奏接近人工**：每 10 秒一条 OpenCLI 命令，跨进程并发 1（ADR 0020 的 process 限速规则）。
- **写操作只执行一次**：会改变账号状态的命令只执行一次 attempt，不重试（ADR 0023）。
- **不进入任何默认链或自动流程**：`chatgpt_browser` 不属于任何 Capability Catalog 或 platform catalog；默认 doctor 跳过它；smoke 只做注册完整性检查，不读取会话，也不发起研究。

不做：自动登录、解验证码、绕过额度或限流、换号。

## 修订规则

违反任一条件的改动，须先修订本记录，说明新的风险与理由，再改代码。

## 影响

- 代码与文档引用本记录作为上述条件的出处：provider 入口、访问策略与读取模块各自写明它们守住的条件。
- 条件之外的需求（例如官方 API route）另行设计，不放宽本记录。
