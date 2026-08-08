# 5. 验收契约与切换步骤

权威来源：[#59 Resolution](https://github.com/jfmoe/smartsearch/issues/59)、[补充决议①](https://github.com/jfmoe/smartsearch/issues/59#issuecomment-5081831697)、[补充决议②（Codex 对抗审查 21 条裁定）](https://github.com/jfmoe/smartsearch/issues/59#issuecomment-5081958567)，以及 [smartsearch → forager 迁移 Q1–Q14 最终裁决](https://github.com/jfmoe/forager/issues/108)。迁移裁决 supersede 与其冲突的旧验收文字。

**判定形态**：自动化为主 + 人工清单；基准为本规格的**纯规格断言**，不与 Python 旧版对拍（旧版仅人工参考）。整套测试面是**长期回归网**而非一次性验收：unit + fixture + offline-e2e 每次改动必跑（PR CI），**首次全绿即切换门槛**。

## Tier 0：七条核心不变量（任一挂掉即判「新版不可用」）

- **I0-1 终态与退出码**：一次调用恰达一个终态，终态↔退出码一一对应（0/2/3/4/5）；直连空结果＝0，证据管线证据不足＝5；Timeout 属 Transport 族；`--timeout`＝整命令 hard deadline，超时终态保留已完成 attempts（journal 全量 + stdout 摘要）；panic 101 为非契约异常出口。
- **I0-2 能力路由权威**：`--capabilities` 存在即权威、三态 CSV/none/缺省；裸调用由 classifier 兜底；`web_fetch` 恒为引擎不变量（plan 声明即退 2）。
- **I0-3 尝试链与预算**：每 seam 按配置 order 走链；凭据池轮询，429/quota 同请求内换凭据（轮换先于重试、429 不重试）；fallback 在共享 deadline 内可达（预算保留 `min(层上限, 剩余预算/剩余槽位)`）。
- **I0-4 归因确定性**：只按每 provider 最终 attempt 归约，与重试次数/失败顺序无关；进入证据阶段的终局失败 Content 优先退 5；全 provider 无可验证响应才退 4；同质失败透传原 kind；全序表见第 4 章。
- **I0-5 结果装配与缺口自报**：`sources` 只表示 Primary Search Source；所有非主结果统一为 `extra_sources` Search Candidate，并以 URL 或 provider-owned typed locator 定位。候选保留 provider-native summary，在被对应读取/取证操作消费前不是 evidence；capability_gaps 自报（结果字段 + stderr），无 minimum profile 门禁。
- **I0-6 输出通道与载荷边界**：clap 已成功解析且选择 JSON 后，search/fetch/research 的配置与 plan 飞行前失败在 stdout 返回单个干净 JSON；Markdown/content 飞行前错误留在 stderr。research 成功只渲染 Research Evidence Index，正文只存在逐条 evidence Markdown；失败以稳定小载荷和可空 `summary_path` 指向 Research Recovery Manifest，locator 不截断。普通失败以 4 KiB 为目标而非硬拒绝长合法路径。协议成功载荷硬上限 4 MiB；只有 Web Fetch 可将 UTF-8 安全前缀截断为成功，SSE/JSON/MCP 超限整体 Runtime。`--output` tee 时 stdout 不变，`--verbose` 为 provider attempts 的内联逃生阀。豁免：clap argv 错（退 2）与 panic（101）。
- **I0-7 旁路隔离与降级**：主终态/退出码/主结果不被旁路失败改变——journal 写失败（非致命告警、journal_ref 置 null + journal_status）、游标文件损坏（安全复位）、classifier 已配置但失败（降级 + 警告 + 落痕；research 用固定最小降级 plan）；`--output` 写失败例外＝显式请求，退 3。

## Tier 1：分域覆盖

条件→期望用例（输入/前置 → 退出码 + 关键输出/副作用），每条标 offline/live 档位。内容＝下述 CLI 断言清单 + 各票输入注记的 harvest；用例逐条编 ID，见 M19 追踪门。

## 测试四层

1. **unit 真值表**：归因总函数、预算切片、plan 严格解析、URL 脱敏；另在 `net` 最低接缝以单个带 `Location` 的 redirect canary 证明 shared client 返回原 3xx 且目标零请求，并以一张紧凑断言证明 3xx → Runtime、不可重试、不可轮换。不建立 provider×status×同/跨源矩阵。
2. **HTTP fixture**（本地桩 + 真实 reqwest 栈）：8 provider 响应解码、SSE、MCP 各配 fixture 组。主搜索覆盖 xAI role-array、failed/incomplete 分类、completed 无文本失败、OpenAI-compatible 非空 clean EOF，以及两者遇到畸形非空 JSON 帧都整体 Runtime；normalizer 只覆盖 think block、显式来源 grammar、空结果 Runtime 与公开 URL 去重。MCP 覆盖 optional-session、AnySearch 固定 client header、非数组 content Runtime 与 structuredContent；成功协议超 4 MiB 整体失败。
3. **offline-e2e（验收主面）**：spawn 真实 `forager` 二进制 + transport-mock，覆盖路由/归因/fallback/退出码/config/plan/journal 副作用/I0-6/I0-7。CLI 覆盖 JSON 飞行前终态与 Markdown/content 豁免；删除 validation 后未知参数/配置；`extra_sources` 的 `0 → web 3/docs 1/vertical 1`、20 成功与 21 clap 失败零网络；shallow doctor 聚合；debug/trace attempts 投影；journal 只清理自有独占文件；Recovery Manifest 成功/写失败 locator。Search Candidate 覆盖统一信封、Context7 typed locator 与无 URL evidence、Exa 字段投影、AnySearch Markdown/required 标记；Supplemental 覆盖 Tavily advanced profile、合法空集 fallback、严格 container 与窄 URL 容错。Web Fetch 覆盖三家 request profile、共享 provider 顺序、薄正文 Quality fallback、全链 Quality和 4 MiB 截断；Firecrawl fixture 精确断言 Markdown、`onlyMainContent: true`、`timeout: 60000` 与 `waitFor` 缺失，并复用薄正文用例证明每个逻辑 attempt 只 scrape 一次。map 覆盖 timeout 10/150、depth 1/5、breadth 1/500 的 wire，以及越界 clap 退出 2 与零网络，单次 attempt cap 使用合法命令 timeout + 较小 provider timeout。direct fetch、search-side Web Fetch 与 research 共享 engine 入口，不复制调用面矩阵；不新增 live-e2e。
4. **live-e2e**：见下。

## live-e2e（`smoke --live` 用例面）

- 离线档（无 `--live`）：不碰网络——配置可解析且过值域、registry 完整、凭据在位状态（不验真伪）、journal 目录可写；进 PR CI。
- `--live` 用例面分两类，全部原子化编 ID（H12 修订：原 L0–L8 粗分档废止，改为下述规范化清单；「全绿」以用例 ID 计）：

**流程门 L0**：`doctor --provider <p>` × 8（L0.1–L0.8，每 provider 一发深探）——凭据迁移验收门，每里程碑前置。

**管线用例**（端到端编排，不入 provider 矩阵）：
- **P1** `search` 端到端（真实 classifier 路由 + 主链 + 补强）
- **P2** `research` 裸调用端到端（classifier 计划 + 多 seam 编排 + 证据）

**Provider 契约用例矩阵**（每行恰为一个 `(provider, operation, transport)` 三元组）：

| ID | provider | operation | transport |
|---|---|---|---|
| C01 | xai | main_search（流装配） | SSE |
| C02 | openai_compatible | main_search（stream=false） | HTTP |
| C03 | openai_compatible | main_search（stream=true，env 覆盖） | SSE |
| C04 | classifier 端点 | 能力分类 + 计划生成 | HTTP |
| C05 | tavily | web_search | HTTP |
| C06 | firecrawl | web_search | HTTP |
| C07 | jina | web_fetch | HTTP |
| C08 | tavily | web_fetch | HTTP |
| C09 | firecrawl | web_fetch | HTTP |
| C10 | context7 | docs_search：library resolve | MCP |
| C11 | context7 | docs_search：docs 获取 | MCP |
| C12 | exa | docs_search | HTTP |
| C13 | exa | similar | HTTP |
| C14 | anysearch | vertical_search：academic.search（带域） | MCP |
| C15 | anysearch | vertical discovery（无域形态） | MCP |
| C16 | anysearch | domains | MCP |
| C17 | tavily | site_map（`map` 命令直连） | HTTP |

AnySearch **extraction 裁决**（消解 #53 砍 `anysearch-extract` 与 #59 L8 原文的矛盾）：extraction 本质为纯 fetch，**彻底删除**——不保留内部操作、不入矩阵、不入 registry。

- 断言统一：退 0 + 可解析 + 该 seam 形状非空 + 无 Parse/Runtime 类 ErrorKind；不断言内容质量。失败模式排列全留 offline-e2e。
- 抗瞬时故障（H13）：金丝雀查询固定；每用例最多重试 2 次（共 3 次尝试）。失败可按 **outage 豁免规则**延期：证据须为 provider 官方状态页异常，或规格外最小独立探测（如 `curl` 同端点）同样失败；留档记录 case ID、时间戳、证据链接。**豁免仅延期、不计 PASS**——必须择日重跑转绿后才能过切换门；不设定时哨兵。
- 凭据只走统一体系（`keys` + `FORAGER_` env）；废除 `ANYSEARCH_API_KEY(S)` 特读与 `ANYSEARCH_LIVE_ACCEPTANCE` 分档。退出码：全 PASS（SKIP 不计）→0、任一已配置凭据探测失败→4、配置坏→3。
- 档次落流程不落旗标：每里程碑跑 L0 全绿 + 手动 P1/P2/C14；切换前跑 `smoke --live` 全量（P1–P2 + C01–C17）；live 档不进 PR CI。

## Medium 落地件（补充决议② M17–M21 定稿）

- **M17 辅助 seam 预算槽位定义**：槽位＝该层中「尚未尝试、凭据在位、未被断路器熔断」的候选数；classifier model 链及 web_fetch、supplemental、tavily_map 等辅助 provider 链继续按槽位均分，并受各自阶段或单次 timeout 上限约束。断路器熔断项不计入；`剩余预算/剩余槽位 < 5s` 时跳过该槽（journal 记 skipped），最后一槽可用全部剩余预算。main search 不适用槽位均分：主 backend → 主 model → SSE 首次尝试可使用全部剩余预算，fallback 只消费残余；`--fallback off` 仍只执行链头并禁用 provider 内 model fallback，classifier 链不受该旗标影响。单 backend + 单 model 没有 in-seam 超时重试，保障来自 fallback 链；共享 endpoint、model 或凭据池的 fallback 不提供故障隔离，隔离由配置负责（ADR 0007）。
- **M18 瘦载荷边界**：普通路径的默认失败载荷以 **4 KiB** 为目标；列表字段（by_kind、providers、capability_gaps.providers_skipped）各截断至 8 项并置 `truncated: true`，message 截断至 500 字符。Research failure 使用稳定 schema，`evidence_dir`/`summary_path` locator 永不截断；极端长合法路径允许超过目标，不在创建 artifact 前预演拒绝。
- **M19 覆盖完整性门**：Tier 0/Tier 1 → 测试 ID → provider/seam 追踪清单入库。**验收用例 ID 的权威源＝本章用例清单本身**（受版本控制，随规格入新仓），不由 provider registry 派生（registry 保持第 4 章 F10 最小职责）。集合断言拆两条**同型比较**进 CI：① registry 的 `(provider, seam)` 投影 ＝ fixture 集的 `(provider, seam)` 投影——新 provider/seam 无 fixture 即红；② 本清单的 live 用例 ID 集（P1–P2 + C01–C17）＝ `smoke --live` 实际注册的 ID 集——清单与实现不许漂移。L0 为流程门，不入集合。
- **M20 随迁 manifest**：见第 6 章。
- **M21 冻结题集 + rubric**（人工质量抽查，行为丢失报警器，不做旧版对拍）：三题字面冻结——① 「Rust 的 async drop 现状与最新提案是什么？」（docs+web 混合、时效）；② 「对比 figment 与 config-rs 的分层覆盖模型，给出出处」（docs、交叉验证）；③ 「近一个月各大社区关于 Coding Agent 的讨论」（web+x 混合、时效、观点聚合）。rubric 三行，每行 pass/fail：相关性（回答对准问题）；来源去重（无重复/近重复来源）；引用支持结论（每主张可溯源到给出的 citation）。

## 人工验收清单五项

① `smoke --live` 全量（凭据配齐，SKIP=0、P1–P2 + C01–C17 全绿）；② research 质量抽查（M21 冻结题集 + rubric）；③ journal 走查（双面合理、URL 脱敏生效）；④ setup 四步走查 + 二跑增量；⑤ **skill 实战验证**——新 forager skill 在**隔离环境**（独立 profile/容器，H11）经 `npx skills` 装入任一受支持的真实 Agent 会话，Agent 按新流程（生成 plan → `research --plan`）完成一次任务；通过后才在本机执行「先删旧后装新」。此处“受支持”指 `npx skills` 能识别该 Agent 目标并把 Skill 安装到其项目级发现路径，且该 Agent 能读取 Skill、写入计划文件并执行 CLI；不限定 Claude Code，满足同一隔离、安装和实际执行约束的 Codex CLI 等 Agent 同样有效。顺序：①→④ 任意，⑤ 最后。skill 重写按新流程重构（旧 `deep` 的 `steps[].command` 工作流已死），非文本替换迁移。

## 切换步骤（三阶段）

本仓**零代码删除**（整体 archive 冻结）；「Cargo 工作区落地顺序」作废。

1. **开发期**：新仓建仓即随迁 CONTEXT/ADR（修订内容见第 6 章）。
2. **验收期**：前置——手动迁移本机凭据至 forager config.toml（第 3 章映射表）→ **L0 门**（doctor 8-provider 全绿）→ 自动化四层全绿 → **制品门**（H10）：从正式 GitHub Release 资产干净安装（权限/架构/PATH/`--version`）→ doctor → P1/P2/C14 → 人工清单五项。
3. **退役链**：以 [#61 原链为唯一权威](https://github.com/jfmoe/smartsearch/issues/61)（本章不复述，见第 6 章）。回退轻量化（H16）：archive 前验证本机旧 `smart-search` 命令已不可达；回退路径一行——archive 可逆、npm 旧版可重装；不做完整 rollback 方案。
