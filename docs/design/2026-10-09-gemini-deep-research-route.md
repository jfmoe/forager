# Gemini Deep Research 浏览器 route 设计

状态：第一期（只读的 `result`）已实现，规格见第 2 章「Gemini 命令」与 ADR 0023；`start` 仍为提案。可行性已于 2026-10-09 实测确认。日期：2026-10-09。前置调研与实测见 [Gemini Deep Research 接入可行性](../research/2026-10-09-gemini-deep-research.md)。

## 目标与边界

新增 provider `gemini_browser`：经本机 OpenCLI 驱动用户已登录的 Chrome，在 Gemini 网页版发起 Deep Research，并在研究完成后把报告与来源落盘交付。

- **要解决的问题**：用户明确要求“用 Gemini Deep Research 研究某个问题”时，forager 能发起研究、按会话取回报告，并把报告交给 agent 或用户，同时给出可供取证的来源列表。
- **不做**：
  - 导出或保管 Google cookie；自行构造 `StreamGenerate` 或 batchexecute 请求；填写或伪造反滥用字段；改写页面的 fetch/XHR。
  - 自动登录、绕过额度或限流、换号。
  - 把报告当作 Research Evidence，或接入 `forager research` 的流水线（ADR 0012）。
  - 修改计划、上传文件、选择 Gem 或模型、删除或重命名会话。
  - 在 smoke、doctor 或任何自动流程中发起研究。
- **使用前提**：用户在 OpenCLI 连接的 Chrome profile 中登录 Gemini，账号可以使用 Deep Research；安装 forager 自有 adapter。Gemini 网页版的 robots.txt 禁止自动访问 `/app/`，Google 服务条款禁止违反 robots 的自动访问。本 provider 与 `ssrn_browser`、`xiaohongshu_browser` 同属个人使用、用户主动调用、接近人工节奏的访问（ADR 0020）。

## 路线选择

| 路线 | 结论 |
|---|---|
| 原样调用 gemini-webapi（Python 子进程） | 不采用。需要导出 Google 会话 cookie，从 Chrome 导出的 cookie 只能维持数小时；Deep Research 请求的 `[3]` 槽位用随机串填充，服务端一旦校验就会失效；2026 年内有数月不可用（#329）和每日周期性失效（#261）；另需 Python 运行时，许可为 AGPL-3.0 |
| 将其协议移植到 Rust | 不采用。风险同上，另需引入 TLS 指纹伪装依赖，forager 自己成为伪造请求的客户端 |
| OpenCLI 内置 `gemini` 命令 | 不直接使用。结果命令只给 Google Docs 链接；上游 adapter 没有 forager 可固定的契约版本 |
| 官方 Interactions API | 不在本设计范围。可作为以后的独立 route `gemini_api`，命令面为它预留位置 |
| **forager 自有 OpenCLI adapter** | **采用**。页面自己生成所有请求字段（包括反滥用字段），cookie 不离开浏览器；adapter 只做用户会做的操作并读取页面收到的响应；gemini-webapi 只作为字段位置的参考（独立实现，不复制代码） |

## 概念

在 `GLOSSARY.md` 中新增：

**Delegated Research Report（委托研究报告）**：
第三方研究 agent 针对一个问题生成并交付的综合报告，附带它自己声明的来源。它不是 Research Evidence：报告正文只能用于定向，或作为归属明确的第三方观点；其中的论断须经 forager 取证后才能作为事实引用。
_Avoid_: research evidence、research result、deep research evidence

## 命令面

按“点名 provider 的命令按 provider 分组嵌套”的规则（第 2 章）：

```
forager gemini research start QUERY [--timeout 240] [--format json|markdown]
forager gemini research result CONVERSATION [--report-dir DIR]
                                [--timeout 120] [--format json|markdown|content]
                                [--output FILE [--receipt]]
```

- `CONVERSATION` 接受 `https://gemini.google.com/app/<id>` 或 `<id>`（十六进制，长度以实测为准），其他输入为参数错误（退 2）。
- 研究通常需要 5–20 分钟，最长 60 分钟，超过 forager 单命令的 600 秒上限。因此发起与取回拆成两个命令，由 agent 轮询；forager 不提供阻塞等待。
- `start` 的成功载荷：`conversation_id`、`conversation_url`、`plan`（`title`、`steps`、`eta_text`）、`route`。`plan` 是 Gemini 提出的研究计划，与 Research Plan Schema v1 无关，输出中不使用 `research_plan` 这一名称。
- `result` 的成功载荷：`status` 为 `awaiting_confirmation`、`running` 或 `completed`。
  - `awaiting_confirmation`：计划已生成但研究尚未开始；附计划，提示用户在网页上确认。
  - `running`：会话信息与进度，即已访问来源数和最新一条思考的标题。
  - `completed`：另有 `title`、`report_path`、`sources_path`、`content_len`、`source_count`。
  - `--format content` 直接输出报告 Markdown；未完成时与 json 相同，输出状态。

## Provider 与配置

- 注册信息：`gemini_browser`，传输 `OpenCli { site: "forager-gemini", contract: "forager-gemini/1" }`，不需要凭据，拥有两个操作 `gemini_research_start`、`gemini_research_result`（与 Tavily `site_map` 相同的操作归属，满足 `catalog::has_owner`）。它不属于任何 Capability Catalog 或 platform catalog。
- 配置节与其他 process route 相同：

  ```toml
  [providers.gemini_browser]
  command = "opencli"
  timeout = 240
  ```

- 访问策略：每 10 秒一条 OpenCLI 命令，跨进程并发 1，permit 持有到进程回收（ADR 0020 的 process route 规则）。
- 启用方式：provider 不进入任何默认链，只有用户或 agent 显式运行 `forager gemini …` 才会执行，因此不新增 `enabled` 开关。forager skill 规定只有在用户明确要求时才调用它，见下文“skill 集成”。

## 读取模块 `forager-gemini`

位于 `skills/forager/opencli/forager-gemini/`，安装方式与 `forager-xhs` 相同。沿用小红书模块的原则：JS 只像用户一样操作页面，并报告页面自己发出和收到的内容；协议解码、校验与归一化在 Rust 中完成，用固定样本测试。

| 命令 | 窗口 | 页面操作 | 返回的页面事实 |
|---|---|---|---|
| `status` | 后台 | 打开 `/app` | 登录状态；只读，供 doctor 使用 |
| `start --query Q` | **前台** | 打开新会话，开始网络捕获；点击“上传和工具”，进入“更多工具”子菜单，选择 Deep Research，并以“取消选择 Deep Research”按钮出现作为选中确认；填入问题并点击发送；等待第一轮 `StreamGenerate` 结束且计划卡片出现；点击计划卡片上的确认按钮；等待第二轮 `StreamGenerate` 结束 | 两轮 `StreamGenerate` 的响应体、最终 URL、页面上的额度或拒绝提示、每一步是否完成 |
| `report --conversation ID` | 后台 | 先开始网络捕获，再打开 `/app/<id>`，等待页面发出的第一个 `hNvQHb` 响应 | 该 `hNvQHb` 响应体、最终 URL、登录或“会话不存在”提示 |

- **窗口**：实测中，`document.hidden` 为 true 的后台窗口打不开工具菜单的内容，前台窗口正常；读取在隐藏的后台窗口中也正常。因此 `start` 使用前台窗口，会在用户屏幕上出现约 1 分钟的 Chrome 窗口；这是用户主动发起时可以接受的代价。`providers/opencli` 目前对所有命令固定传 `--window background`，需要改为由命令声明窗口模式。
- **标签**：工具与按钮按中英文标签匹配，并以 OpenCLI 内置 adapter 的标签列表为起点。实测中文界面下确认按钮为英文 "Start research"。
- **大小**：完成后的 `hNvQHb` 响应实测约 0.46 MB（其中大部分是进度），两轮 `StreamGenerate` 分别约 25 KB 与 6 KB，都在 OpenCLI 传输的 4 MiB 上限内。adapter 只返回匹配的响应，不返回整页捕获。

## 解码

Rust 端 `gemini_browser` 解码模块是纯函数，输入为响应体文本。下列字段位置已由 2026-10-09 的实测确认（见调研的 live-probe 记录）。rich content 块是 JSPB 数组，高编号字段收在末尾的对象中，键为字段号加 1（字段 55 的键为 `"56"`）。

- **帧**：`StreamGenerate` 由长度前缀分隔的多个 JSON 块组成，每块中的 `wrb.fr` 外壳携带二次 JSON 编码的内层数组；batchexecute 响应以 `)]}'` 开头，`wrb.fr` 外壳的第二项为 rpc id。
- **轮次**：`hNvQHb` 内层的 `[0]` 为轮次列表，最新的在前。每轮的候选结果在 `[3][0][0]`，rich content 块在候选结果的 `[12]`。
- **状态**：rich content 字段 69。2 为计划等待确认，3 为研究中，5 为完成。
- **计划**：字段 55：`[0]` 标题，`[1]` 步骤（`[序号, 标签, 说明]`），`[2]` 预计时间，`[3][0]` 确认文案，`[5][0]` 修改文案。
- **研究文档**：候选结果的 `[30][0]`：`[2]` 标题，`[3]` 任务 id（运行中和完成后都是占位符 `agency-placeholder-task-id`，只用来区分研究文档与其他附件，不用来判断完成），`[4]` Markdown 正文，`[17][0]` 与正文相同。
- **进度**：字段 57 的 `[1][4][2]` 为条目列表。`[5]` 非空的是思考（`[标题, 正文]`）；`[4][2]` 非空的是已访问来源（`[favicon, URL, 标题, …]`）。`result` 只输出计数与最新思考标题。
- **来源**：文档 `[17][1]`（备用 `[5]`）的字段 43，为引用组列表。每组 `[0][0]` 是标记文本（如 `[cite: 2, 3, 4]`），`[1]` 是按标记顺序排列的条目，条目 `[3][0]` 为 `[favicon, URL, 标题, null, null, 摘要]`。同一编号以首次出现为准，按编号排序。实测 26 个被引编号全部可解析，且都是直链。
- **判定**：在最新的轮次中按状态取值：
  - 状态 5 且正文非空：`completed`。
  - 状态 3 且正文为空：`running`。
  - 状态 2：`awaiting_confirmation`。
  - 状态与正文矛盾，或必需字段缺失、类型不符：Runtime，消息写明“Gemini 响应结构已变化”及位置。不要当成 `running`，否则轮询永远不会结束。
  - 会话中没有计划也没有研究文档：参数错误（不是 Deep Research 会话）。
- **`start` 的成功判定**：第一轮带计划（状态 2），第二轮带研究文档容器且状态为 3，最终 URL 为 `/app/<id>`。

## 交付

- 文件位置沿用平台 fetch 的规则：默认写入系统临时目录下按调用隔离的目录，`--report-dir` 可以覆盖。
- 文件：
  - `gemini-<id>.md`：报告正文原样写入，末尾附 `## Sources` 编号列表，使 `[cite: N]` 在离开 Gemini 后仍可解析。
  - `gemini-<id>.sources.json`：`[{id, title, url}]`。
- stdout 只给元数据与路径，与 research 文件化交付一致；写入失败为 Runtime，不回退为内联输出。

## 写操作与重试

`start` 是 forager 第一个会改变用户账号状态的操作：它新建会话并消耗 Deep Research 额度。因此：

- `start` 只执行一次 attempt，不论 `retry` 配置如何都不重试；Timeout 与 Network 也不重试，避免重复发起研究。
- 一旦拿到会话 id，即使后续步骤失败，失败载荷和消息中也必须带上 `conversation_url`，让用户能检查或在网页上手动确认。
- 计划已生成但确认按钮没有点到时，返回 Runtime，并附会话 URL 和“请在网页上点击开始研究”的提示。不再次点击或重发。之后的 `result` 会报告 `awaiting_confirmation`。
- `result` 是只读的，沿用共享重试策略。

## 错误归因

| 情况 | 归因 |
|---|---|
| 未登录、登录页、账号需要验证 | Auth（adapter 以 77 退出） |
| 页面提示 Deep Research 额度已用尽，或响应带用量超限错误码 | QuotaExhausted |
| 工具菜单中没有 Deep Research 入口 | Runtime，提示检查账号是否可用该功能 |
| Gemini 回复了普通文本而非计划（拒绝或模型不支持） | Runtime，消息附回复开头（有界） |
| 会话不存在或无权访问 | attempt 级 Parameter（退 4）；只有无法识别的会话参数在飞行前退 2 |
| 响应结构变化 | Runtime |
| 其余 OpenCLI 退出码 | 沿用第 7 章的 OpenCLI 退出码映射 |

## doctor 与 smoke

- `forager doctor` 默认不检查 `gemini_browser`。只有 `--provider gemini_browser` 时才运行 adapter 的 `status`，报告 OpenCLI、adapter 契约版本、登录状态与 Deep Research 入口。
- smoke 只加注册完整性检查。live smoke 不发起研究，也不读取用户会话。

## skill 集成

- 在 `skills/forager/references/` 新增 Gemini Deep Research reference，在 SKILL.md 中加一条路由：**只有用户明确要求使用 Gemini Deep Research 时**才进入这个分支。它不属于成本阶梯的任何一级，ordinary search 或 research 的不足也不会自动升级到这里。
- 流程：
  1. 运行 `start`，把计划告诉用户。
  2. 每隔约 2 分钟运行一次 `result`，直到 `completed` 或超过 60 分钟。
  3. 读取报告文件。
- 使用报告时遵守委托研究报告的定位：可以把报告作为“Gemini 的结论”转述并注明出处；要作为事实陈述的关键论断，先用 `forager fetch` 读取对应来源再引用。报告来源也可以作为 `forager research` 计划中的已知 URL。

## 实现改动清单

- `skills/forager/opencli/forager-gemini/`：`contract.js`、`shared.js`、`status.js`、`start.js`、`report.js`。
- `src/capabilities/providers/opencli.rs`：窗口模式由命令声明，不再对所有命令固定为后台；规格第 7 章的调用形式同步修改。
- `src/capabilities/catalog.rs`：注册 `gemini_browser`、操作与访问策略。
- `src/capabilities/providers/gemini_browser.rs`：命令调用与 route 判定；解码放在同目录的独立文件中，按职责拆分。
- `src/infra/types/`：新增叶子 `gemini_research`，包含会话 id、计划预览、报告状态与来源的形状。
- `src/cli/args.rs`、新增 `src/cli/gemini.rs`、`src/cli/dispatch.rs`：命令树、上下文、渲染与落盘。
- 配置：`providers.gemini_browser`；`src/ops/` 下的 doctor 探针。
- 文档：
  - 规格：第 2、3、4 章。
  - `GLOSSARY.md`。
  - 新 ADR：委托研究报告不是证据；首个写操作 provider 不重试。

## 测试接缝

- 解码：用实测样本覆盖计划、运行中、已完成、非研究会话、结构变化这几种情况。样本须脱敏：删除会话 id、轮次 id、用户提示以外的账号信息，并把进度与正文截短；实测的原始响应只保存在仓库外。
- route 判定：用 `tests/support/opencli.rs` 的假 OpenCLI 覆盖退出码映射、`start` 不重试，以及失败载荷带会话 URL。
- CLI：会话参数解析、文件写入与 `--format content`。

## 分期

0. **实测（已完成，2026-10-09）**：确认了字段位置、状态与进度字段、`hNvQHb` 响应带完整报告与来源、响应大小、来源为直链，以及发起必须用前台窗口。额度提示文案未遇到，仍待观察。
1. **最小可行：只读的 `result`**：用户在网页上手动发起研究，forager 只负责按会话取回报告和来源并落盘。全程只读，与现有浏览器 route 的风险相同，先补上最缺的“拿到正文与来源”这一环。
2. **`start`**：加入写操作、不重试规则、额度错误归因与 skill 集成。
3. **以后按需**：官方 API route `gemini_api`，或支持计划修改轮。

## 仍未验证

- 2026-10-09 起无订阅账号只能用 Flash-Lite；这类账号能否在网页版发起 Deep Research，以及能用几次（实测账号有订阅）。
- 研究失败、额度用尽、模型拒绝时的响应字段与页面文案；错误归因表中这几行目前是推断。
  - `start` 按 gemini-webapi 的约定，从 `StreamGenerate` 外壳的 `[5][2][0][1][0]` 读取错误码，并把 1037（用量超限）归为 QuotaExhausted；页面额度文案按中英文猜测的模式匹配。两者都未在真实额度用尽时验证。
- `start` 期间页面是否还会发出发送与确认之外的 `StreamGenerate`；adapter 按请求顺序把前两个当作两轮响应。
- 英文界面下的菜单与按钮标签。
- Deep Research Max 或长时间研究的响应大小。
- Gemini 消费者版对输出再使用的条款。
