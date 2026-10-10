# ChatGPT Deep Research 接入可行性

核验日期：**2026-10-10**。仓库基线：`100d41b`（forager 0.10.0）；本机 OpenCLI `1.8.6`（上游最新 `1.8.8`，源码引用固定在 [`jackwener/OpenCLI@77af6bc`][OC-commit]）。本文是调研，**尚未实现任何 ChatGPT 接入**。下文区分已核验事实、推断与待验证事项；对条款的解读不是法律意见。可对照 [Gemini Deep Research 接入可行性](2026-10-09-gemini-deep-research.md) 与 [Gemini Deep Research 浏览器 route 设计](../design/2026-10-09-gemini-deep-research-route.md)。

## 结论

- **定位不变**：ChatGPT Deep Research 交付的是 LLM 综合报告，在 forager 中只能是**委托研究报告**（ADR 0023），不是 Research Evidence（ADR 0012、0013）：正文用于定向或作为归属明确的第三方观点，来源 URL 作为待取证候选。
- **官方 Deep Research API 已不存在**。`o3-deep-research` 与 `o4-mini-deep-research` 已于 2026-07-23 停止服务，官方替代型号是 `gpt-5.6-sol`。[弃用公告][OAI-deprec] 现行文档把“深度研究”描述为一种用法：推理模型（`gpt-5.5`，推理强度 `high` 或 `xhigh`）加 `web_search` 工具，再加 background 模式。[Web search 指南][OAI-ws] 它没有 ChatGPT 网页版的澄清、计划和报告界面，是另一种产品。官方渠道里也没有任何入口能程序化地发起 ChatGPT 网页版的 Deep Research，见“其他官方入口”。
- **网页版路线技术上可行，已经实测确认**。2026-10-10 完成了一次发起与读取（见“实测”）：在“聊天”模式的工具菜单中选择“深度研究”并发送，计划卡片约 60 秒倒计时后自动开始，约 4 分钟后完成；在后台窗口打开会话时，页面自己请求的 `/backend-api/conversations/<id>` 响应中有状态、报告正文和逐条可解析的来源 URL，完成后约 133 KB。非浏览器客户端在 Cloudflare 一层就被拦截（见“实测”），发送消息还要经过 sentinel 的工作量证明和 Turnstile 校验。[chat2api][C2A-req] 因此，与 `gemini_browser` 相同的做法——在用户已登录的 Chrome 里像用户一样操作，只读取页面自己收到的响应——是唯一可以接受的技术形态。
- **条款风险比 Gemini 高一级**。OpenAI 面向个人用户的使用条款明文禁止 “Automatically or programmatically extract data or Output”。[ToU][ToU] Google 的条款只禁止违反 robots.txt 的自动访问。对 ChatGPT 来说，浏览器 route 读取报告这一核心动作本身就落在禁止条款之内，涉及的又是付费账号。是否接受这一风险由维护者决定，并应在实现前以 ADR 记录，不能只沿用 ADR 0020 对 SSRN 的风险接受。
- **推荐方案**（以接受上述风险为前提）：新增独立的 process 供应方 `chatgpt_browser` 与 forager 自有 OpenCLI adapter `forager-chatgpt`，复用 `gemini_browser` 的全部基础设施，分两期：
  1. **最小可行方案：只读的 `result`**。用户在网页上手动发起研究；forager 按会话打开页面，读取页面自己收到的会话响应，解码状态、报告与来源，把引用标记替换为编号链接后落盘。全程只读，不消耗额度。
  2. `start`：写操作，只执行一次 attempt。发送后等计划卡片出现即可返回，倒计时结束后研究自动开始，不需要确认点击；仍需处理可能出现的澄清问题。
- **零实现的合规兜底**：用户在网页上导出报告，agent 直接读取文件。代价是导出的 Markdown 中引用是不透明标记，没有来源 URL。导出的 PDF 据第三方报告带可点击链接，未核验。见“网页版”。
- **官方 API route 不采用**：它不是 ChatGPT Deep Research，能力上与 forager 自己的 research 引擎、现有搜索供应方重叠。它的输出条款比 Gemini Grounding 宽松得多（见“条款”）。以后如果需要一个“OpenAI 代写报告”的供应方，可作为独立 route 另行设计。

### 与 `gemini_browser` 的复用与差异

| 方面 | 复用 | 差异（ChatGPT 侧，未标注者为推断或待验证） |
|---|---|---|
| 传输 | `OpenCli { site, contract }` 注册、process route 限速（每 10 秒 1 条命令、并发 1）、permit 持有到进程回收（ADR 0020） | 实测中 OpenCLI 的 `browser click` 能打开菜单并选中“深度研究”。上游记录模型选择器是忽略合成事件的 Radix 弹层（[#2462][OC-2462]），`forager-gemini` 用的 `el.click()` 能否操作工具菜单未验证，adapter 应优先使用 OpenCLI 的点击原语 |
| 窗口模式 | 由命令声明（ADR 0023） | 读取在后台窗口完成（实测）。发起在前台窗口完成；后台窗口中工具菜单能渲染，但后台发起整条流程未验证 |
| start/result 拆分 | 研究耗时数分钟到数十分钟，超过 600 秒的命令上限，拆分仍然必要 | 计划卡片约 60 秒倒计时后**自动开始**，不需要确认点击（实测）；可能先出现**澄清问题**（帮助页，本次未出现），需要 `awaiting_clarification` 一类状态 |
| 报告落盘 | `<site>-<id>.md`（末尾附 `## Sources`）与 `.sources.json`，stdout 只给元数据与路径（ADR 0015、0023） | 正文中的引用是私有区字符包裹的标记，例如 `\uE200cite\uE202turn10search8\uE202turn4search0\uE201`，必须按 `content_references` 换成编号链接，否则离开 ChatGPT 后无法解析。实测中每个标记都能按位置找到对应条目及其全部 URL |
| 写操作 | `start` 只执行一次 attempt，失败载荷带会话 URL | 发送由页面自己完成 sentinel 校验；forager 不得自行构造 `chat-requirements` 或工作量证明。输入框内容会作为草稿保存，失败时须清理或在消息中说明 |
| doctor 与 smoke | 默认 doctor 跳过；`doctor --provider chatgpt_browser` 运行只读 `status`；live smoke 不登记用例 | 无 |
| 解码 | 纯函数解码、固定脱敏样本测试 | 会话响应是普通 JSON，不是 JSPB，解码更简单。当前接口返回扁平的 `messages` 列表，不是 OpenCLI 解码所依据的 `mapping` 树；研究状态在 `widget_state` 这个 JSON 字符串中，需要二次解析。完成后约 133 KB，远低于 4 MiB 上限（实测） |
| 会话 id | 会话参数在飞行前校验 | URL 形如 `/c/<UUID>`，项目内会话为 `/g/<project>/c/<UUID>`；另有独立的 `async_task_conversation_id`，含义未明 |

## 路线对比

| 路线 | 能力（已核验） | 主要代价与风险 | 结论 |
|---|---|---|---|
| 官方 Deep Research 模型（`o3-deep-research`、`o4-mini-deep-research`） | 2026-07-23 停止服务。[弃用公告][OAI-deprec] OpenRouter 仍保留模型页，但没有可用端点。[OpenRouter][OR-o3dr] Azure 上同名模型 2026-11-19 退役，原 Deep Research 工具已弃用。[Azure 工具][AZ-dr]、[Azure 退役表][AZ-ret] | 无法使用 | 不采用 |
| 官方现行替代：Responses API 推理模型 + `web_search` + background | `gpt-5.5` 的 `high`/`xhigh` 推理，可用 `return_token_budget: "unlimited"`；background 后台轮询或 webhook；`url_citation` 注释给出 URL、标题与位置。[Web search 指南][OAI-ws]、[Background][OAI-bg] | 按 token 加每千次搜索 $10 计费，官方没有单次估算；不是 ChatGPT 产品；与 forager research 引擎重叠 | 不采用；以后可作为独立的“OpenAI 代写报告”route |
| Sign in with ChatGPT（ChatGPT plan usage） | 开源客户端经 OAuth 用用户的 Plus/Pro 套餐调用 Responses API。[概览][SIWC-overview] | 必须 `stream: true`、`store: false`；不支持 `background`、`max_tool_calls`、文件检索、Code Interpreter、托管 MCP；web search 受模型与账号策略限制。[限制][SIWC-limits] 不提供 Deep Research | 不适用于本需求 |
| Workspace Agents API | 用访问令牌触发已发布的工作区 agent，返回会话 URL，可轮询运行状态。[文档][WA] | 仅工作区套餐；agent 的回复**无法通过 API 取回**；工作区 agent 能否使用 Deep Research 未核验 | 不适用于单人用户 |
| OpenCLI 内置 `chatgpt ask --deep-research` 与 `deep-research-result` | 前者选择工具并发送；后者读取会话响应中的报告与来源，另报告计划、等待用户等进度状态。[ask][OC-ask]、[result][OC-drr] | 读取在网络捕获失败时会**导出浏览器 cookie、伪造 Chrome UA，用 Node 直接请求后端**，必要时再取 Bearer 令牌。[源码][OC-cookie] 工具菜单选择器已与当前界面不符；它捕获的是 `/backend-api/conversation/`，而页面现在请求 `/backend-api/conversations/<id>` 且返回扁平列表（见“实测”）；上游 adapter 没有 forager 可固定的契约版本 | 不直接使用；字段位置可作参考 |
| 逆向 HTTP 客户端（revChatGPT、ChatGPT-to-API、chat2api） | 用 access token 或 cookie 直连 `backend-api`；chat2api 自行生成 `chat-requirements` 令牌和工作量证明答案，并外接 Turnstile 求解服务。[chat2api][C2A-req] | 已归档或停更；需要导出凭据、绕过防护；没有 Deep Research 支持 | 不采用 |
| **forager 自有 OpenCLI adapter `forager-chatgpt`（推荐，有条件）** | 由真实页面发出全部请求，adapter 只读取页面自己收到的 `/backend-api/conversations/<id>` 响应。2026-10-10 实测完成一次发起与读取，见“实测” | 使用条款明文禁止程序化提取输出；依赖 Chrome、OpenCLI 与前端 UI；`start` 消耗额度 | 维护者以 ADR 接受风险后采用，先做只读 `result` |
| 手动导出 | 报告可导出为 Markdown、Word、PDF。[帮助页][HC-dr] | Markdown 中的引用是不透明标记，不含 URL。[andylizf][AZL-skill]、[DeepCite][DC-readme] | 作为不需要实现的兜底 |

## 官方 API 深入

### Deep Research 模型的退役

- OpenAI 于 2026-04-22 公告弃用一批旧型号，`o3-deep-research(-2025-06-26)` 与 `o4-mini-deep-research(-2025-06-26)` 于 **2026-07-23** 停止服务，替代型号为 `gpt-5.6-sol`。[弃用公告][OAI-deprec] Deep Research 指南页首也写明这一点，并保留旧示例仅供参考。[指南][OAI-dr]
- 停用前的用法（供理解第三方实现）：Responses API，至少包含一个数据源工具（`web_search_preview`、文件检索或远程 MCP），可加 Code Interpreter；建议 background 模式加 webhook；用 `max_tool_calls` 控制成本与时长；不含澄清与改写提示的步骤；远程 MCP 必须实现 search/fetch 接口，且 `require_approval` 为 `never`。[指南][OAI-dr] `o3-deep-research` 的价格为输入 $10、输出 $40（每百万 token）。[模型页][OAI-o3dr]
- 指南说明了网页版的三步流程：中间模型澄清意图，中间模型改写提示，再交给研究模型。API 不包含前两步。[指南][OAI-dr]

### 现行替代：推理模型 + `web_search` + background

- Web search 指南把“deep research”列为第三种搜索方式：推荐 `gpt-5.5`，推理强度 `high` 或 `xhigh`，配合 background 模式。新接入应使用 `{ "type": "web_search" }`，旧的 `web_search_preview` 不支持 `filters`、`external_web_access`、`return_token_budget`。长时间研究可设 `return_token_budget: "unlimited"`。[Web search 指南][OAI-ws]
- 推荐型号在不同页面不一致：弃用表写 `gpt-5.6-sol`，web search 指南写 `gpt-5.5`，示例代码用 `gpt-6-astra`；模型目录首推 `gpt-6-astra`。[模型目录][OAI-models] 这说明官方没有一个专门的“研究型号”。
- 第三方实现印证了这一点：[pminervini/deep-research-mcp][DRM-openai]（MIT）的 OpenAI 后端已改用 `gpt-6-sol` 等通用型号，加 `web_search`（研究强度下 `return_token_budget: "unlimited"`、`tool_choice: "required"`）、Code Interpreter 和 `background: true`，超时后可选择取消，并以任务 id 取回结果。

### 输出结构

- 输出数组包含 `web_search_call` 项，`action` 为 `search`、`open_page` 或 `find_in_page`。最终 `message` 的 `output_text` 带 `annotations`，其中 `url_citation` 给出 `url`、`title`、`start_index`、`end_index`。用 `include: ["web_search_call.action.sources"]` 可以拿到模型查阅过的完整 URL 列表，数量通常多于引用数。[Web search 指南][OAI-ws]
- 文档要求：向终端用户展示网页结果或其中的信息时，内联引用 “must be made clearly visible and clickable”。[Web search 指南][OAI-ws]

### 时长、成本与速率

- 时长：官方称深度研究可运行数分钟到数十分钟，没有给出上限。[指南][OAI-dr]、[Web search 指南][OAI-ws]
- 计费：web search 每千次调用 $10，另加按模型费率计费的搜索内容 token。`gpt-5.5`（短上下文）输入 $5、输出 $30（每百万 token）；`gpt-6.1-sol` 输入 $2、输出 $10。[定价][OAI-pricing] 官方**没有**给出单次研究的估算（Gemini 给出了 $1–3）。**推断**：数十到上百次搜索、数十万到上百万 token 的搜索内容，单次成本大致在一美元到十余美元之间，取决于型号与 `return_token_budget`。需要实测。
- 速率：按模型分层限流，例如 `gpt-5.5` 的 Build 层为 5,000 RPM、1,000,000 TPM。[模型页][OAI-gpt55] 在当前文档中没有找到组织验证（verified organization）的要求，**未验证**。

### 数据保留

- background 模式把响应数据写盘约 10 分钟以支持轮询，因此与 Zero Data Retention 不兼容；ZDR 凭据出于兼容原因仍接受 `background=true`，但需要 ZDR 时应关闭。[指南][OAI-dr]、[Background][OAI-bg]
- `/v1/responses` 默认（或 `store=true` 时）保留应用状态至少 30 天；滥用监控日志最长 30 天。[数据控制][OAI-data]

### 其他官方入口

- **Sign in with ChatGPT 的套餐用量**：开源客户端可以经 OAuth 动态注册，让 Plus/Pro 用户用自己的套餐调用 `POST https://api.openai.com/v1/responses`，不需要 API key；文档明确要求不要指向 ChatGPT 的 `backend-api`。[模型与推理][SIWC-inference] 但它不支持 background、文件检索、Code Interpreter、托管 MCP，web search 受策略限制。[限制][SIWC-limits] 条款要求令牌只经官方流程获取，并且 “Do not ask users for their ChatGPT passwords or session cookies”。[SIWC 条款][SIWC-terms] 它是用 ChatGPT 套餐调用模型的正规途径，但不提供 Deep Research。
- **Agents API / Agents SDK**：Agents API 是托管的 Codex harness，可接 `web_search` 与子 agent，按 API 费率计费。[概览][OAI-agents] 它可以用来自建研究 agent，但不是 ChatGPT Deep Research。
- **Workspace Agents API**：`POST https://api.chatgpt.com/v1/workspace_agents/{id}/trigger` 返回会话 URL，带 beta 头时可以轮询运行状态，但 “The agent's response cannot currently be retrieved through the API”。[文档][WA] 只有工作区套餐可用。
- **Compliance API**：帮助页称 Deep Research 的活动记录在企业版 Compliance API 的 Conversation API 中。[帮助页][HC-dr] 只适用于企业版。
- **Azure / OpenRouter**：见“路线对比”。两者都只提供已退役或即将退役的 `o3-deep-research`，Azure 推荐的替代同样是 `gpt-5.6-sol`。

## 网页版深入

### 前端流程

以下来自官方帮助页与发布说明：

- 入口：在输入框键入 **/Deepresearch**、从输入框的工具菜单（+）选择、或从侧栏选择。可以限定或优先指定网站（Sites → Manage sites），可以使用支持 Deep Research 的已连接应用。[帮助页][HC-dr]
- 流程：ChatGPT 可能先提澄清问题，然后给出研究计划，用户可以在开始前查看和修改；运行中显示实时进度，可以中断并调整方向。完成后报告在全屏视图中打开，有目录、“sources used”与活动记录，可导出为 Markdown、Word、PDF。[帮助页][HC-dr]
- 默认使用最新模型，也可以选择旧模型。[帮助页][HC-dr]
- 2026 年的变化：2026-02-10 上线改版，新增侧栏入口、全屏报告、可编辑计划与运行中调整；2026-03-26 移除旧版 Deep Research 模式，历史结果仍可访问。[发布说明][HC-rel]

第三方观察（[andylizf/deep-research-skill][AZL-skill]，2026-09；不是一手来源）：

- 入口页 `https://chatgpt.com/deep-research`。发送后出现研究计划和约 10 秒的倒计时（“Plan starts in N seconds”，`Start` 按钮），倒计时结束**自动开始**；也可能出现澄清问题。运行中页面显示 “Researching...” 与 “Stop research”，完成后显示 “Research completed in Nm” 与 “N sources”。
- 报告不在主文档中，而在两层 iframe 里：`iframe[title="internal://deep-research"]` 内还有 `iframe#root`，导出菜单也在其中。另一份第三方说明称该 iframe 是 `oaiusercontent.com` 的跨源页面。[halanhuang][HH-readme]
- 额度用尽时页面显示用量提示与重置时间；无权限时输入框位置显示升级提示。

OpenCLI 的 PR 说明在实际会话中见到过 `waiting_for_user_response_on_plan` 状态，测试样本中还带有 `waiting_for_user_response_on_plan_until` 截止时间，与“倒计时后自动开始”相符。[PR #2061][OC-2061]、[样本][OC-fixture]

2026-10-10 的实测（中文界面、Team 工作空间账号）修正并确认了上述描述：

- `/deep-research` 加载后前端把路径改为 `/`，不是入口。输入框上方有“聊天 / 工作”模式切换（`aria-label` 为“撰写器模式”）；“深度研究”只出现在**聊天**模式的“添加文件等内容”菜单中，工作模式的菜单里没有。
- Deep Research 在网页版中是一个内部应用：选中后，输入框中插入一个指向 `app://connector_openai_deep_research` 的提及标记；其设置页 `/settings/plugins-settings/apps/connector_openai_deep_research` 的说明是，宿主模型调用这个工具后不要自己回答，由 Deep Research 向用户更新进度。
- 发送后宿主模型调用该工具，计划卡片在 15 秒内出现，按钮为“编辑”“取消”与带约 60 秒倒计时的“开始”。不点击时，倒计时结束后研究自动开始，卡片变为“正在研究…”与进度条。本次没有出现澄清问题。
- 研究进度由卡片所在的沙箱 iframe（`web-sandbox.oaiusercontent.com`）经 `POST /backend-api/ecosystem/call_mcp` 拉取，另有 websocket 推送；会话接口中的 `widget_state` 也会更新。

### 后端数据

以下各条来自开源实现，没有官方文档；2026-10-10 的实测结果附在本节末尾，与开源实现不一致处以实测为准。

- **会话响应**：OpenCLI 先开启对 `/backend-api/conversation/` 的网络捕获，再打开 `/c/<id>`，从页面收到的会话 JSON 中解码。[result][OC-drr] 研究在会话 `mapping` 某个节点的 `message.metadata.chatgpt_sdk.widget_state` 中（JSON 字符串，也兼容直接放在 `metadata.widget_state`），同一节点的 `chatgpt_sdk.response_metadata` 带 `async_task_conversation_id`、`openai/widgetSessionId`、`openai/asyncStatus`（样本中为数字 7）与 `venus_message_type`（样本为 `initial_loading_message`）。[解码][OC-extract]、[样本][OC-fixture] 字段名中的 `chatgpt_sdk` 与 iframe 标题说明，Deep Research 在网页版中以内部应用（widget）的形式运行；这同样是推断。
- **状态**：`widget_state.status` 已知取值有 `waiting_for_user_response_on_plan` 与 `completed`；`widget_state.plan` 含 `plan_id`、`title`、`steps`，另有 `step_statuses_by_plan`。OpenCLI 用正则把其他取值归为 running、needs_user_action 等，说明它没有完整的取值表。[解码][OC-extract] 其 PR 说明，外层的 `/stream_status` 可能已报告 `COMPLETE`，而 widget 仍在等待用户确认计划，因此完成必须以 `report_message` 出现为准。[PR #2061][OC-2061]
- **报告与来源**：`widget_state.report_message.content.parts[0]` 是 Markdown 正文；`report_message.metadata` 中有 `content_references`、`safe_urls`、`search_result_groups`。[解码][OC-extract] 另一个独立实现（导出扩展）按 `content_references[].matched_text` 匹配正文中的整段引用标记，再用同一条目的 `safe_urls` 解析出 URL。[qpwm06][QP-content] 这与导出文件中 `\uE200cite\uE202turnNsearchN\uE201` 形式的标记一致。[YQYANG2233][YQ-readme]
- **发送与校验**：逆向客户端的发送流程是：先 `POST /sentinel/chat-requirements`（携带客户端生成的需求令牌），按响应决定是否要 Turnstile、Arkose 与工作量证明（seed、difficulty），再把 `openai-sentinel-chat-requirements-token`、`openai-sentinel-proof-token`、`openai-sentinel-turnstile-token` 等头带到 `POST /conversation`。[chat2api][C2A-req]、[请求头][C2A-headers] 浏览器 route 由页面自己完成这些步骤，forager 不接触。
- **读取权限**：OpenCLI 的后备路径从 `/api/auth/session` 取 `accessToken` 作为 Bearer。[源码][OC-cookie] 也有第三方报告，从页面外直接请求私有会话接口得到 `conversation_not_found`，分享链接接口返回 “output of this plugin was redacted”。[halanhuang][HH-readme] 因此分享链接不能作为读取路径，打开会话时页面自己发出的请求是最可靠的数据源。

2026-10-10 实测（完整记录见 [live-probe.json](chatgpt-deep-research-2026-10-10/live-probe.json)）：

- **接口**：打开 `/c/<id>` 时，页面自己发出 `GET /backend-api/conversations/<id>?num_turns=10&include_has_versions=true`（复数 `conversations`）。响应是扁平的 `messages` 列表加 `page_info` 游标，没有 `mapping` 树；OpenCLI 捕获的 `/backend-api/conversation/` 前缀与按 `mapping` 解码的逻辑都与之不符。
- **位置**：研究轮次中，接收者为 `api_tool.call_tool` 的 `tool` 消息带 `metadata.chatgpt_sdk`。其中 `tool_response_metadata` 是工具调用当时的快照（`openai/asyncStatus` 为 7，`venus_widget_state.status` 一直是 `waiting_for_user_response_on_plan`，不会更新）；`widget_state` 是 JSON 字符串，记录**当前**状态，含 `status`、`plan`、`step_statuses_by_plan`、`research_started_at`、`research_stopped_at`，完成后另有 `report_message`。
- **状态**：本次观察到 `widget_state.status` 为 `researching` 与 `completed`；会话级 `async_status` 运行中为 7，完成后为 null。完成时最后一步仍标为 `in_progress`，步骤状态不能作为完成信号；应以 `status` 为 `completed` 且 `report_message` 正文非空为准。
- **报告**：`report_message.content.parts[0]` 为 Markdown 正文，`channel` 为 `final`，`author.metadata.real_author` 为 `tool:web`。会话中紧随工具调用的 assistant 文本只是一句“已启动深度研究”的提示，不是报告。本次提示为简体中文，报告却是繁体中文。
- **来源**：`content_references` 中每个 `grouped_webpages` 条目的 `start_idx`、`end_idx` 恰好覆盖正文中的一个引用标记；条目的 `safe_urls` 去掉 `?utm_source=chatgpt.com` 变体后，数量与标记内的 id 数相同，`items[0]` 另给出标题、站点、摘要与发布时间。5 个标记共 12 个不同 id，解析出 12 个不同的直链，没有跳转链接。`search_result_groups` 中的 `ref_id` 只能解析其中 3 个 id，不能作为主要映射；另有一个 `sources_footnote` 条目列出 3 个来源。id 与 URL 按顺序一一对应是**推断**。
- **大小**：会话响应运行中约 23 KB，完成后约 133 KB；进度接口 `call_mcp` 的响应完成后约 303 KB。都远低于 OpenCLI 传输的 4 MiB 上限。

### 反自动化

- 不带浏览器环境的 HTTP 请求在 Cloudflare 一层收到 `403` 与 `cf-mitigated: challenge`，首页与 `/backend-api/` 都是如此（见“实测”）。
- 发送消息要通过 sentinel 的工作量证明，并可能要求 Turnstile；逆向客户端为此外接求解服务。[chat2api][C2A-req]
- 一个第三方 skill 明说它驱动系统 Chrome 是为了让 “Cloudflare can't tell it's automated”。[andylizf README][AZL-readme] 这正是使用条款中“绕过防护措施”可能被援引的情形，见“条款”。
- 结论：伪造或求解任何防护字段的路线都不接受；页面自己生成全部请求、adapter 只读页面收到的响应，是唯一可接受的方式。这与 `gemini_browser` 的原则相同。

### OpenCLI 内置 `chatgpt` adapter

本机 OpenCLI 1.8.6 列出 `chatgpt` 站点命令：`ask`、`send`、`read`、`detail`、`history`、`deep-research-result`、`new`、`status`、`model`、`image`、`project-list`、`project-file-add`、`login`、`whoami`，全部为 cookie 策略（复用浏览器登录态）。另有面向 macOS 桌面版的 `chatgpt-app`。

- `ask --deep-research`：点击 `button[data-testid="composer-plus-btn"]`，在菜单中找 “深度研究 / Deep Research” 并点击（`nativeClick`），然后发送；不处理计划或澄清问题。[ask][OC-ask]、[选择工具][OC-select]
- `deep-research-result <id> [--wait --timeout --stable]`：按顺序尝试：页面网络捕获；cookie 导出后的 Node 请求、Bearer 请求、页内 `fetch`；同源 iframe DOM；CDP frame 与无障碍树。输出 `status`、`report`、`sources` 与进度字段；没有完成的报告且没有进度时报 EmptyResult。[result][OC-drr]、[编排][OC-orch] 2026-06-30 加入（PR #2023），2026-07-11 加入进度状态（[PR #2061][OC-2061]）。
- 前端变化频繁：2026-08-29 侧栏会话不再是 `<a href="/c/...">`，导致 `history` 与 `ask` 失效（[#2435][OC-2435]）；2026-09-05 模型选择器改为忽略合成事件的 Radix 弹层（[#2462][OC-2462]），两者都未关闭。

forager 如果自建 adapter，只采用第一种方法（读取页面自己收到的响应）。cookie 导出、伪造 UA、自行构造请求的后备路径都不采用。字段位置可以参考，但要独立实现。

### 账号额度

- 官方帮助页不再公布具体次数：用量 “varies by plan”，以产品内计数为准；有固定月度额度的套餐从首次使用起每 30 天重置；可用性取决于套餐和国家或地区。[帮助页][HC-dr]
- 定价页：Free 与 Go 为 “Limited deep research”，Plus 为 “Expanded”，Pro（$100、$200、$500 三档）为 “Maximum”。[定价页][CG-pricing]
- Business、Enterprise、Edu 按积分计：Deep Research 每次约 50 积分；Edu 每滚动 24 小时含 5 次。[Rate Card][HC-rate]
- 实测（Team 工作空间账号）：研究轮次的 `usage_refund_receipt` 显示 `user_plan_type` 为 `team`，额度窗口为每 43,200 分钟（30 天）25 次，本次运行后剩余 24 次，`credit_usage_amount` 为 0，即这次没有扣积分。页面顶部当时显示“工作空间中有一名成员已达到使用上限”的横幅，没有影响这次运行。
- 2025-05 的存档帮助页曾公布 Free 每月 5 次（轻量版）、Plus/Team 每月 25 次、Pro 每月 250 次，另有轻量版额度。[存档][HC-archive] 这些数字已不是现行口径。

## 开源客户端

| 项目 | 状态与许可 | 认证 | 对 Deep Research 的支持 | 结论 |
|---|---|---|---|---|
| [acheong08/ChatGPT][REV]（revChatGPT） | 2023-08 起归档；GPL-2.0 | access token | 无 | 不采用 |
| [acheong08/ChatGPT-to-API][C2A-old] 及其分支 [xqdoo00o/ChatGPT-to-API][C2A-xq] | 前者 2023-08 归档；后者最后提交 2025-02；无 LICENSE 文件 | access token | 无 | 不采用 |
| [lanqian528/chat2api][C2A] | 最后提交 2025-03-28，MIT | access token 或 refresh token | 无；有未关闭的 issue 报告 401 “session ID was changed”（#276）与登录后无法对话（#282） | 不采用；作为 sentinel 流程的参考 |
| [pminervini/deep-research-mcp][DRM] | 活跃，最后提交 2026-09-28，MIT | OpenAI API key；实验性 `openai-codex` 提供方使用设备码登录，访问未公开的 `chatgpt.com/backend-api/codex` | OpenAI 后端是 web search 加推理模型，不是 ChatGPT Deep Research；Codex 提供方不支持 background 与 Code Interpreter。[README][DRM-readme] | 不采用；其“提交与取回分离”的做法已被 Gemini 设计借鉴 |
| [andylizf/deep-research-skill][AZL] | 2026-09 更新；README 称 MIT，但没有 LICENSE 文件 | 驱动用户 Chrome profile（web-plane） | 发起、等待、从 iframe 提取正文；不能取回来源 URL | 只作为前端流程的线索 |
| [halanhuang2025-lgtm/chatgpt-deep-research][HH] | 2026-03；无 LICENSE 文件 | Playwright 浏览器 | 用无障碍树读取跨源 iframe 中的报告 | 只作线索 |
| [qpwm06/chatgpt-deep-research-md-exporter][QP]、[FallenMax/deepcite][DC]（MIT） | 2026 年内更新的 Chrome 扩展 | 用户手动操作 | 把导出或复制的 Markdown 中的引用标记还原为链接 | 印证引用标记与 `content_references` 的结构 |
| [YQYANG2233/chatgpt-deep-research-md-citation-fixer][YQ] | GPL-3.0 | 无（本地文件） | 按位置对齐导出 Markdown 的标记与导出 PDF 中的链接 | 印证“Markdown 导出无 URL、PDF 带链接” |

没有找到任何维护中的、通过 HTTP 直连支持 ChatGPT 网页版 Deep Research 的逆向客户端。

## 条款

- **个人用户使用条款**（2026-01-01 生效）列出的禁止行为包括 “Automatically or programmatically extract data or Output”、“Interfere with or disrupt our Services, including circumvent any rate limits or restrictions or bypass any protective measures or safety mitigations”、逆向工程，以及 “You may not share your account credentials or make your account available to anyone else”。同一条款规定用户拥有 Output。[ToU][ToU] 与 Google 相比，这里的禁止不以 robots.txt 为条件，直接覆盖“程序化提取输出”。浏览器 route 的读取与发起都在其范围内；导出 cookie 交给其他工具使用还可能触及“共享账号凭据”。用户拥有报告本身，所以手动导出后由 agent 阅读不受这些限制。
- **`chatgpt.com/robots.txt`**（2026-10-10 读取）：对 `User-agent: *` 先列出首页、分享页（`/share/`、`/s/`、`/g/`）、公开内容与营销页等允许路径，然后 `Disallow: /`，另有对 `/backend-anon/sentinel/*`、`/backend-anon/conversation$` 等的显式禁止。会话页 `/c/` 与 `/backend-api/conversation/` 都在禁止范围内。CCBot、anthropic-ai、PerplexityBot 等被整体禁止。[robots][robots]
- **API**：服务协议规定客户拥有全部 Output；限制条款禁止逆向工程、用 Output 开发竞争模型、“extract data from the Services other than as permitted through the Services”、绕过限流与防护措施。[服务协议][SvcAgreement] 服务条款要求按文档使用 API，并且如果客户禁用或忽略引用功能，输出侵权赔偿不适用。[服务条款][SvcTerms] 文档要求展示网页结果时引用可见且可点击。[Web search 指南][OAI-ws] **与 Gemini Grounding 对比**：Gemini API 附加条款禁止缓存、聚合、分析 Grounded Results，禁止用其中的链接定位抓取页面，并限制存储。OpenAI 的 API 条款中**没有**这类限制。按条款文本，API 路线下把报告落盘、用 forager fetch 读取被引来源都不受额外约束，只要交付物保留可点击的引用。这是对条款文本的解读。
- **Sign in with ChatGPT 条款**：令牌只能经官方流程取得，不得索取 ChatGPT 密码或会话 cookie；套餐用量只能服务于用户连接的那个应用，不得作为通用 API 转给其他工具。[SIWC 条款][SIWC-terms]

## 实测

2026-10-10，本机 OpenCLI 1.8.6，用户自己的 Chrome（中文界面），ChatGPT 已登录，账号属于 Team 工作空间。先做只读检查，随后经用户授权发起了一次 Deep Research。探针只用 OpenCLI 的点击与输入操作页面，从 OpenCLI 网络捕获读取页面自己收到的响应，没有自行构造请求，没有导出 cookie。记录只含结构、计数与时间，不含 cookie、令牌、账号身份与会话 id；原始响应只保存在仓库外。完整记录见 [live-probe.json](chatgpt-deep-research-2026-10-10/live-probe.json)。

### 只读检查

| 操作 | 观察 |
|---|---|
| `opencli chatgpt status --window background --site-session ephemeral` | `Status: Connected`，`Login: Yes` |
| 不带浏览器环境，用 curl 请求 `https://chatgpt.com/` 与 `/backend-api/conversation/<全零 UUID>` | 都返回 `403`，响应头 `server: cloudflare`、`cf-mitigated: challenge` |
| 打开 `https://chatgpt.com/deep-research` | 文档以该地址加载，随后前端把路径改为 `/`；没有出现 Cloudflare 挑战 |
| 检查输入框按钮 | 工具按钮的标签为“添加文件等内容”，**没有** OpenCLI `ask --deep-research` 依赖的 `data-testid="composer-plus-btn"`；输入框上方有“聊天 / 工作”模式切换 |
| 工作模式下点开“添加文件等内容” | 菜单项为添加照片和文件、从资料库添加、创建图像、网页搜索，**没有**“深度研究” |
| 聊天模式下点开同一菜单 | 多出“深度研究　获取详细报告”与“绘图”；选中后输入框插入指向 `app://connector_openai_deep_research` 的提及标记 |

入口只在聊天模式中出现；输入框停留在工作模式时，菜单和侧栏中都找不到它。

### 一次完整运行

提示为“用不超过 500 字总结：截至 2026 年，Rust 异步运行时 async-std 的维护状态，以及其官方推荐的替代方案。”时间为 UTC。

| 阶段 | 观察 |
|---|---|
| 发送（13:11:44，前台窗口） | 先用 OpenCLI `type` 输入问题，它会替换输入框全部内容并删掉提及标记，于是在问题之后重新选择“深度研究”，再点击“发送”。URL 变为 `/c/<id>` |
| 计划 | 宿主模型思考 4 秒后调用 Deep Research 工具，15 秒内出现计划卡片：标题、4 个步骤，按钮“编辑”“取消”和带约 60 秒倒计时的“开始”。没有澄清问题 |
| 自动开始（13:12:51） | 没有点击“开始”。倒计时结束后卡片变为“正在研究…”与进度条；卡片 iframe 开始调用 `POST /backend-api/ecosystem/call_mcp`，首个响应带 websocket 地址与 `session_id` |
| 运行中读取 | 在另一个 OpenCLI 后台窗口打开该会话：页面自己请求 `GET /backend-api/conversations/<id>?num_turns=10`（约 23 KB），`widget_state.status` 为 `researching`；同时 `call_mcp` 返回 32 条进度消息（思考标题、搜索查询、`web.run` 调用）与 60 个已检索来源，约 133 KB |
| 完成（13:16:33） | 研究耗时约 3 分 42 秒。13:18 的轮询中 `widget_state.status` 为 `completed`，会话响应约 133 KB，`report_message` 正文 830 字符（繁体中文），5 个引用标记、12 个不同 id，全部按 `content_references` 解析为 12 个直链 |
| 额度 | 研究轮次的用量回执：Team 套餐，30 天窗口 25 次，剩余 24 次，未扣积分 |
| 清理 | 研究会话留在用户的历史中，没有删除 |

页面打开会话时还会自己请求 `sentinel/chat-requirements/prepare` 与 `finalize`。发送时的请求序列因捕获缓冲在首次读取时被清空而没有记录下来。

### 结论

- **读取（`result`）可行**：在后台窗口打开会话，读取页面自己的 `conversations/<id>` 响应即可拿到状态、正文与来源；运行中与完成后都能读，响应远低于 4 MiB 上限。
- **发起（`start`）可行**：前台窗口中切到聊天模式、选择“深度研究”、输入、发送，四步都可以用 OpenCLI 的点击原语完成。计划会自动开始，`start` 在计划卡片出现后即可返回，不需要点击确认，比 Gemini 少一次写操作。
- **状态判定**：以 `widget_state.status` 加 `report_message` 正文非空为准；`tool_response_metadata` 中的状态是调用时的快照，步骤状态在完成后仍可能是 `in_progress`，两者都不能用来判断完成。
- **adapter 需要独立实现**：OpenCLI 内置命令的入口选择器、捕获前缀和 `mapping` 解码都已与当前页面不符。

## 建议的实测步骤

参照上节的做法，下列步骤仍待进行：

1. **后台窗口发起**：在后台窗口重复一次完整发起，确定 `start` 的窗口模式；同时从发送前开始保存捕获，记录发送时的请求序列。
2. **澄清问题**：用一个含义模糊的问题发起，观察澄清问题出现时 `widget_state.status` 的取值与页面文案。
3. **长报告**：发起一次范围较大的研究，记录完成后的会话响应大小、引用数量，以及 id 与 URL 按顺序对应的推断是否成立。
4. **导出对比**：在网页上导出 Markdown 与 PDF，确认引用标记与链接的形态，评估“手动导出”兜底的可用程度。

## 仍未验证

- `start` 能否在后台窗口完成；发送时页面发出的请求序列。
- `widget_state.status` 的完整取值（本次只见到 `researching`、`completed`），以及澄清问题出现的条件与对应状态。
- 引用标记中的 id 与 `safe_urls` 是否总按顺序一一对应；`turnNviewM` 类 id 对应的标题只能从 `call_mcp` 的 `source_searches` 或 `items[0]` 部分获得。
- 长报告的会话响应大小。
- 报告语言为何与提示不一致（简体提示得到繁体报告）。
- 额度用尽、研究失败、被拒绝时的页面文案与响应字段。
- 无订阅账号（Free、Go）与 Plus、Pro 的实际可用次数；官方不再公布次数，本次只见到 Team 的 30 天 25 次。
- 导出 PDF 是否保留全部引用链接。
- 官方现行替代方案单次研究的实际成本与时长；是否需要组织验证。
- 工作区 agent 能否使用 Deep Research。

[OC-commit]: https://github.com/jackwener/OpenCLI/tree/77af6bc433c1818a4abb1c8928e2a7fd0bbae376
[OC-drr]: https://github.com/jackwener/OpenCLI/blob/77af6bc433c1818a4abb1c8928e2a7fd0bbae376/clis/chatgpt/deep-research-result.js
[OC-ask]: https://github.com/jackwener/OpenCLI/blob/77af6bc433c1818a4abb1c8928e2a7fd0bbae376/clis/chatgpt/ask.js
[OC-select]: https://github.com/jackwener/OpenCLI/blob/77af6bc433c1818a4abb1c8928e2a7fd0bbae376/clis/chatgpt/utils.js#L822-L930
[OC-extract]: https://github.com/jackwener/OpenCLI/blob/77af6bc433c1818a4abb1c8928e2a7fd0bbae376/clis/chatgpt/utils.js#L1337-L1583
[OC-cookie]: https://github.com/jackwener/OpenCLI/blob/77af6bc433c1818a4abb1c8928e2a7fd0bbae376/clis/chatgpt/utils.js#L1621-L1760
[OC-orch]: https://github.com/jackwener/OpenCLI/blob/77af6bc433c1818a4abb1c8928e2a7fd0bbae376/clis/chatgpt/utils.js#L1781-L2094
[OC-fixture]: https://github.com/jackwener/OpenCLI/blob/77af6bc433c1818a4abb1c8928e2a7fd0bbae376/clis/chatgpt/utils.test.js#L229-L298
[OC-2061]: https://github.com/jackwener/OpenCLI/pull/2061
[OC-2435]: https://github.com/jackwener/OpenCLI/issues/2435
[OC-2462]: https://github.com/jackwener/OpenCLI/issues/2462
[OAI-dr]: https://developers.openai.com/api/docs/guides/deep-research
[OAI-deprec]: https://developers.openai.com/api/docs/deprecations#2026-04-22-legacy-gpt-model-snapshots-july-2026-shutdown
[OAI-ws]: https://developers.openai.com/api/docs/guides/tools-web-search
[OAI-bg]: https://developers.openai.com/api/docs/guides/background
[OAI-pricing]: https://developers.openai.com/api/docs/pricing
[OAI-data]: https://developers.openai.com/api/docs/guides/your-data
[OAI-models]: https://developers.openai.com/api/docs/models
[OAI-o3dr]: https://developers.openai.com/api/docs/models/o3-deep-research
[OAI-gpt55]: https://developers.openai.com/api/docs/models/gpt-5.5
[OAI-agents]: https://developers.openai.com/api/docs/guides/agents-api/overview
[SIWC-overview]: https://developers.openai.com/siwc/token-sharing-open-source
[SIWC-inference]: https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference
[SIWC-limits]: https://developers.openai.com/siwc/token-sharing-open-source/preview-limitations
[SIWC-terms]: https://openai.com/policies/sign-in-with-chatgpt-terms/
[WA]: https://developers.openai.com/workspace-agents/trigger-runs
[HC-dr]: https://help.openai.com/en/articles/10500283-deep-research-in-chatgpt
[HC-rel]: https://help.openai.com/en/articles/6825453-chatgpt-release-notes
[HC-rate]: https://help.openai.com/en/articles/11481834-chatgpt-rate-card-business-enterpriseedu
[HC-archive]: https://web.archive.org/web/20250517232934/https://help.openai.com/en/articles/10500283-deep-research-faq
[CG-pricing]: https://chatgpt.com/pricing
[ToU]: https://openai.com/policies/terms-of-use/
[SvcTerms]: https://openai.com/policies/service-terms/
[SvcAgreement]: https://openai.com/policies/services-agreement/
[robots]: https://chatgpt.com/robots.txt
[AZ-dr]: https://learn.microsoft.com/en-us/azure/foundry-classic/agents/how-to/tools-classic/deep-research
[AZ-ret]: https://learn.microsoft.com/en-us/azure/foundry/openai/concepts/model-retirement-schedule
[OR-o3dr]: https://openrouter.ai/api/v1/models/openai/o3-deep-research/endpoints
[DRM]: https://github.com/pminervini/deep-research-mcp
[DRM-readme]: https://github.com/pminervini/deep-research-mcp/blob/31213c1b98c29ea2e30239fb748913f6e50473ed/README.md
[DRM-openai]: https://github.com/pminervini/deep-research-mcp/blob/31213c1b98c29ea2e30239fb748913f6e50473ed/src/deep_research_mcp/backends/openai_backend.py
[REV]: https://github.com/acheong08/ChatGPT
[C2A-old]: https://github.com/acheong08/ChatGPT-to-API
[C2A-xq]: https://github.com/xqdoo00o/ChatGPT-to-API
[C2A]: https://github.com/lanqian528/chat2api
[C2A-req]: https://github.com/lanqian528/chat2api/blob/a63a90cec90dea3fea3e272d22f8c5799de14e30/chatgpt/ChatService.py#L180-L260
[C2A-headers]: https://github.com/lanqian528/chat2api/blob/a63a90cec90dea3fea3e272d22f8c5799de14e30/chatgpt/ChatService.py#L279-L303
[AZL]: https://github.com/andylizf/deep-research-skill
[AZL-readme]: https://github.com/andylizf/deep-research-skill/blob/1d84729fc7584fc826b98b3ba6ee53f902bb6fed/README.md
[AZL-skill]: https://github.com/andylizf/deep-research-skill/blob/1d84729fc7584fc826b98b3ba6ee53f902bb6fed/SKILL.md
[HH]: https://github.com/halanhuang2025-lgtm/chatgpt-deep-research
[HH-readme]: https://github.com/halanhuang2025-lgtm/chatgpt-deep-research/blob/380d9a14e2d5459deadf92070271f4c4528adca4/README.md
[QP]: https://github.com/qpwm06/chatgpt-deep-research-md-exporter
[QP-content]: https://github.com/qpwm06/chatgpt-deep-research-md-exporter/blob/d43a6b5c3d111279f38db9844fc8ff9efa1e2ecf/content.js#L177-L256
[DC]: https://github.com/FallenMax/deepcite
[DC-readme]: https://github.com/FallenMax/deepcite/blob/94099089dce79aad0394719e377a3fe69aed8248/README.md
[YQ]: https://github.com/YQYANG2233/chatgpt-deep-research-md-citation-fixer
[YQ-readme]: https://github.com/YQYANG2233/chatgpt-deep-research-md-citation-fixer/blob/d26a17ba44950f710aa25c84dd7dd0187d981830/README.md
