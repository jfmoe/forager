# Context7 必要性评估：与 Exa、Tavily + Fetch、主搜索的实测对比

> 实测日期：2026-09-25。被测对象为本机安装的 `forager` 0.5.1（配置了全部 provider），代码引用对应仓库提交 `a06ba8a`。本文回答三个问题：Context7 是否仍有必要；它在哪些场景优于或劣于其他来源；forager 引擎与 skill 应采用哪些路由和门控规则。
>
> 证据口径：“实测”指本次命令输出、本地 journal 统计或对照官方页面的关键事实匹配；标注“（判断）”的结论依赖作者阅读，不是测量值。

## 1. 结论

**保留 Context7，但收窄路由：保留直接命令 `forager context7`；在自动路由中，Context7 只在通过库名与相关性门控后使用；普通搜索的 Documentation Search 改为 Exa 优先。**不建议整体移除：在主流库上，Context7 能快速返回紧凑、可按版本固定的代码片段，只是本次测量中没有发现它独有、而其他来源拿不到的事实。

决定性数字：

| 指标 | 数值 |
|---|---|
| 14 条查询的关键事实覆盖（共 40 项） | Context7 19（48%）；Exa 37（93%）；Tavily + Fetch 26.5（66%）；主搜索 40（100%） |
| 非库类查询（开放平台、数据接口、App 手册，共 14 项） | Context7 3.5（25%）；Exa 14（100%）；主搜索 14（100%） |
| 端到端中位延迟 | Context7 8.2 s，平均 2.4 次调用；Exa 2.4 s，1 次调用；主搜索 37.8 s，1 次调用 |
| 能通过 thin 门、实际却答非所问的 Context7 正文 | 本次 14 次 docs 调用中有 3 次；历史 research 接受的 50 条 Context7 证据中，按标题判断有 9 条离题 |
| 历史 research 中 Context7 thin 拒绝率 | 171 次尝试中 71 次（42%）；估算在实际读取（query-docs）调用中约占 59% |
| 普通搜索 Documentation Search 落到 Exa fallback 的次数 | 233 次 docs_search 尝试中 0 次 |
| 本次 3 个 quick research 中，因 Context7 挤占候选而导致失败或空转的 | 3 个中有 2 个：飞书为 0 条证据、终态失败；Tushare 仅采用营销文案，却以 `evidence_converged` 收敛 |

## 2. 方法与查询集

### 2.1 四个来源的调用方式

| 代号 | 路径 | 调用 |
|---|---|---|
| A | Context7 | `forager context7 library NAME QUERY` 选出最佳 `library_id`（首选不当时允许按包名再解析一次），再用原问题调用 `forager context7 docs LIBRARY_ID QUERY` |
| B | Exa（当前 Documentation Search 的 fallback） | `forager exa search QUERY --include-highlights --include-domains 官方域名` |
| C | Tavily 候选 + 抓取 | `forager search QUERY --capabilities web_search` 取 Tavily 候选，再对其中最佳页面执行 `forager fetch URL`（有官方页优先官方页，没有则取最相关的第三方页） |
| D | 主搜索 | `forager search QUERY --capabilities none`，由 grok 实时联网作答 |

另外做了两组补充测量：

- **Research 抽样**：3 次 `forager research --budget quick`。每个计划含两个子问题，sq1 声明 `docs_search`，sq2 声明 `docs_search` 与 `web_search`。
- **Exa 去掉域名过滤**：对 5 条查询重跑 Exa、不带 `--include-domains`，以贴近自动 Documentation Search 中 Exa 的实际调用方式（该路径不带域名过滤）。

### 2.2 Ground truth 与评分

- 每条查询都抓取一页权威官方文档作为 ground truth。从中选出 2–3 个回答问题所必需的**关键事实**；选哪些事实属于判断。然后用 `grep` 并辅以阅读，检查各来源输出是否包含这些事实。部分覆盖记 0.5。
- 相关性、正确性和新鲜度在表内以备注标出：离题、事实错误、旧版本或过期镜像。
- 输出规模按字符数计，作为 token 的近似：A 取 `content`，B 取所有 highlights，C 取 Tavily 摘要加抓取正文，D 取 `answer`。
- 延迟按命令墙钟时间计。C 只计 Tavily 尝试耗时与 fetch 耗时；实际 `forager search --capabilities web_search` 还包含主搜索，墙钟中位数约 50 s。
- 本次共约 109 次 forager 调用。

### 2.3 查询集

| # | 类型 | 查询 | Ground truth |
|---|---|---|---|
| q01 | 主流库 | React 19 `useActionState` 的参数、返回值与 pending 状态 | react.dev/reference/react/useActionState |
| q02 | 主流库 | Tokio `CancellationToken` 的 `child_token`/`cancel` 语义与优雅停机 | docs.rs tokio-util `CancellationToken` |
| q03 | 主流库 | clap derive `ValueEnum`：派生、重命名和隐藏变体 | docs.rs clap `_derive` |
| q04 | 主流库 | Effect-TS `Layer.merge` / `Layer.provide` / `Layer.provideMerge` | effect.website layers |
| q05 | 版本变更 | Next.js 15 中 `cookies()`/`headers()` 改为异步 | nextjs.org cookies |
| q06 | 版本变更 | Pydantic v2 `field_validator`/`model_validator` 模式，以及 v1 `@validator` 的替代 | docs.pydantic.dev validators |
| q07 | 版本变更 | Tailwind CSS v4 用 `@theme` 定义颜色 | tailwindcss.com/docs/theme |
| q08 | 小众/新库 | jiff 将 `Zoned` 舍入到 15 分钟 | docs.rs jiff `Zoned` |
| q09 | 小众/新库 | uv workspace 成员依赖 `tool.uv.sources` | docs.astral.sh uv workspaces |
| q10 | 产品 API（时效） | Anthropic prompt caching 的 1h TTL 与最小可缓存长度 | Claude 文档 prompt-caching |
| q11 | 非库，中文 | 飞书开放平台发送消息 API：地址、`receive_id_type`、`content` 格式 | open.feishu.cn im-v1 message/create |
| q12 | 非库，中文 | Surge `RULE-SET` 与 `DOMAIN-SET` 的区别 | manual.nssurge.com/rules/ruleset.html |
| q13 | 非库，中文 | Tushare `fund_adj` 参数、字段与积分 | tushare.pro doc_id=199 |
| q14 | 非库，中文 | 同花顺 iFinD HTTP API 的 `access_token` 获取与有效期 | quantapi.10jqka.com.cn FAQ |

## 3. 逐查询对比

单元格格式为“关键事实覆盖 / 总数”，后附问题标注。

| # | A Context7 | B Exa | C Tavily + Fetch | D 主搜索 | 备注 |
|---|---|---|---|---|---|
| q01 | 2/3，缺 `permalink?` 参数 | 3/3 | 2/3，Tavily 只给第三方博客 | 3/3 | 各来源都相关 |
| q02 | 首次 0/3（`/websites/rs_tokio` 不含 tokio-util，正文 0 处 CancellationToken，但通过了 thin 门）；按 `tokio-util` 再解析后 3/3 | 3/3 | 2/3，候选为 2022 年课程镜像，**版本陈旧** | 3/3 | A 需要调用方知道 API 属于哪个 crate |
| q03 | 1.5/3，缺 `#[value(skip)]` 和 `name =` | 3/3 | 1/3，第三方镜像的 trait 页 | 3/3 | Context7 片段检索没有命中具体属性 |
| q04 | 2.5/3，给的是 main 分支 `Layer.ts` 源码（v4 类型签名） | 3/3，同时给出 v3 和 v4 页面 | 2/3，教程站 | 3/3 | A 有版本漂移风险（判断：源码取自 v4 beta 分支，官方文档默认 v3） |
| q05 | 2/3，缺 codemod | 3/3 | 3/3，nextjs.org 错误说明页 | 3/3 | 使用版本化 ID `/vercel/next.js/v15.1.8` |
| q06 | 2/3，缺 `plain` 模式 | 0/3（传入的 `docs.pydantic.dev` 域名已迁移，highlights 为 FastUI 等无关页）；去掉域名过滤后约 3/3 | 2/3，官方迁移指南，49K 字符 | 3/3 | B 的失败来自调用方给错域名 |
| q07 | 2.5/3（首次用 `tailwindcss` 只解析出插件库，需要用 `tailwind css` 再解析一次） | 3/3 | 2.5/3，第三方博客 | 3/3 | |
| q08 | 0/2，只有 `round(Unit::Second)`，没有 increment | 2/2 | 2/2，但官方页有 172K 字符 | 2/2 | Context7 返回相关库，但片段没有回答问题 |
| q09 | 3/3 | 3/3 | 3/3 | 3/3 | 各来源都好 |
| q10 | 1.5/2，只给 Opus 5 的 512 token 下限 | 2/2 | 1/2，博客把 Opus 4.7 的下限写成 4,096（官方为 2,048），**事实错误** | 2/2（多列出一个官方页未出现的 “Opus 5.5”，未核实） | A 首次解析到 SDK 仓库，需要按 “claude api docs” 再解析 |
| q11 | 1/3，正文是“获取用户 ID”和“获取消息”页，`receive_id_type` 只列出 3/5 个值，含占位符 `field1`；首次用中文名解析到无关的 `/websites/open_qfei_cn`（4 个 snippet） | 3/3 | 0/3，Tavily 只给历史版本页和镜像页，镜像页抓取 thin 失败（18 s） | 3/3 | Context7 有飞书索引，但检索质量差 |
| q12 | 1/3，只有 RULE-SET 示例，没有 DOMAIN-SET；引用的是已失效的旧路径 `/rule/ruleset.html` | 3/3 | 3/3，官方中文手册有专节对比 | 3/3 | 旧路径抓取仅得 36 字符，新路径为 `/rules/`，说明 Context7 索引滞后 |
| q13 | 0/3，`/websites/tushare_pro` 只有 3 个 snippet，正文为首页营销文案（1,173 字符，通过 thin 门） | 3/3 | 0/3，Tavily 给的是股票 `adj_factor` 页（**错接口**） | 3/3 | |
| q14 | 0/3，解析结果为 ESLint、Knip、Podman 等，**无索引** | 3/3 | 3/3，官方 PDF 手册 | 3/3 | |

### 3.1 规模、延迟与调用次数

| 来源 | 每查询调用次数 | 延迟中位数（最大值） | 输出字符中位数（最大值） | 零覆盖（0 分）查询数 |
|---|---|---|---|---|
| A Context7 | 2.36（14 条共 33 次，其中 6 次为再解析） | 8.2 s（14.6 s）；单次 MCP 调用约 3.5–4.4 s | docs 正文 4.7K（7.1K）；加上 library 列表 6.3K | 4（q02 首次、q08、q13、q14） |
| B Exa | 1 | 2.4 s（4.2 s） | 9.7K（20.5K），5 条结果的 highlights | 1（q06，域名给错） |
| C Tavily + Fetch | 1.9 | 8.3 s（26.4 s），不含主搜索；命令墙钟约 50 s | 15.0K（186K） | 2（q11、q13） |
| D 主搜索 | 1 | 37.8 s（67.0 s） | 3.8K（7.6K） | 0 |

按查询类别分组的关键事实覆盖：

| 类别 | A | B | C | D |
|---|---|---|---|---|
| 库类 q01–q09（26 项） | 15.5（60%）；按包名再解析后 18.5（71%） | 23（88%）；q06 去掉域名过滤后约 26 | 19.5（75%） | 26（100%） |
| 非库/产品 q10–q14（14 项） | 3.5（25%） | 14（100%） | 7（50%） | 14（100%） |

Context7 首次解析的 top-1 质量：用自然库名解析 14 条查询，8 条 top-1 可用；4 条需要再解析（q02 应为 `tokio-util`、q07、q10、q11）；2 条无可用索引（q13 只有 3 个 snippet，q14 无匹配）。

去掉域名过滤的 Exa 抽样（q03、q06、q11、q13、q14）：5 条的 top-1 均为官方页，关键事实都出现在 highlights 中。可见 B 的表现并不依赖调用方提供正确域名。

### 3.2 Research 抽样

| 运行 | 子问题 | 结果（实测） |
|---|---|---|
| R1 Tokio | sq1 为 child_token 语义；sq2 为 TaskTracker 优雅停机 | 13.1 s，status ok。sq1 唯一证据来自 `/websites/rs_tokio_tokio`，2,972 字符，**CancellationToken 出现 0 次**；sq2 证据来自 tokio-util，相关。全程没有抓取任何网页 |
| R2 飞书 | sq1 为发送地址与 `receive_id_type`；sq2 为 `content` 格式 | 15.7 s，**终态 `quality` 失败，0 条证据**。6 次 Context7 读取全部 thin，Tavily 已返回候选但一次也没被抓取。gap 写着“attempting 3 candidate URLs”，实际尝试的是 3 个 Context7 library ID |
| R3 Tushare | sq1 为参数与字段；sq2 为积分与限量 | 7.9 s，status ok，`gap_check` 为 `evidence_converged`。两个子问题的唯一证据都是同一段 1,173 字符的 tushare.pro 营销文案，不含 `fund_adj`。**这是静默失败** |

读 `src/evidence/research.rs` 的结论（判断，基于代码阅读并与上述实测一致）：

1. `bound_discovery_candidates` 按到达顺序，为每个子问题最多保留 `discovery_limit` 个候选，其值为 `evidence_cap × 3`：quick 为 3，standard 为 6，deep 为 9。
2. 发现阶段按计划里的能力顺序进行，`docs_search` 排在 `web_search` 之前，所以 Context7 候选先占满名额，Tavily 候选在 quick 预算下被整体丢弃。
3. `evidence_cap` 在 quick 预算下为 1。只要 Context7 返回一段通过 thin 门的正文，子问题就算满足，因此离题正文会让子问题“收敛”。
4. `documentation_read` 只做 `is_thin` 检查：少于 200 字符，或唯一行数 ≤3 且少于 500 字符才拒绝，不检查相关性。

## 4. 本地 journal 统计

来源为 `~/.local/state/forager/journal/search_result_*.json`，时间范围 2026-08-26 至 2026-09-24，不含本次评估写入的记录。一次“尝试”是一次 Context7 MCP 调用，`resolve-library-id` 与 `query-docs` 都计入。

| 命令类型 | 含 Context7 的运行数 | 尝试数 | 成功 | thin（quality） | 网络错误 | 延迟 p50 / p90 / max |
|---|---|---|---|---|---|---|
| search（`--capabilities docs_search`） | 44 | 45 | 44（98%） | 0（不调用 query-docs） | 1（2%） | 3.9 / 4.7 / 6.2 s |
| research | 17 | 171 | 96（56%） | 71（42%） | 4（2%） | 3.2 / 4.4 / 6.0 s |

research 中的进一步拆分：

- **读取成功率**：research 共接受 50 条 Context7 证据。按“50 条接受 + 71 条 thin”估算约 121 次 query-docs 读取，thin 约占 59%；其余约 46 次成功尝试是 resolve。这一拆分是估算，journal 没有区分这两种操作。
- **thin 按主题分布**：iFinD 18，飞书 15，FastAPI 架构 8，Surge/Tailscale 7，推送通道选型 6，AmazingData 6，Kimi K3 评测 4，Effect 3，其他 4。非库或非 API 主题至少占 52/71（73%）：iFinD、飞书、Surge/Tailscale、推送通道、AmazingData 合计。
- **已接受证据的相关性**（判断，依据证据标题；正文已被清理）：50 条中有 9 条离题。例如飞书调研拿到 Pushover，AmazingData 调研拿到 Hikvision 和 Anaplan，iFinD 调研拿到 STM32 和 geoip.sh，FastAPI 架构调研拿到 XIRR。另有 3 条是版本错配：Kimi K3 问题拿到 Kimi K2/K2.5 文档。
- **普通搜索的候选质量**（判断，依据 top-1 `library_id`）：44 次中 14 次正确，10 次部分相关，20 次错误。例如 “Codex config.toml” 解析到 `/toml-lang/toml`，“Tailscale MagicDNS” 解析到 `/kubernetes/dns`，“同花顺 iFinD” 解析到 `/websites/npmjs_package_mexc-api-sdk`。
- **Exa fallback 从未触发**：233 次 docs_search 尝试全部落在 Context7（含本次 research 新增的尝试），Exa 为 0 次。原因是 `resolve-library-id` 几乎总会返回候选，而 Documentation Search 链只在候选为空时才继续到 Exa。ADR 0011 设计的“无可消费来源则继续下一个 provider”在实际中不可达。

## 5. Context7 的优势与劣势

**占优或持平的场景**

- **主流库、明确包名、问题落在该库的常用 API 上**（q01、q05、q07、q09）：覆盖 2–3/3，正文 2–5K 字符，单次调用约 4 s，以代码片段为主，并附来源 URL。
- **可按版本固定**：`/vercel/next.js/v15.1.8` 返回的正文全部来自 v15.1.8 标签（实测：Source 路径全部含 `v15.1.8`）。
- **紧凑**：docs 正文中位 4.7K 字符，小于 Exa 5 条 highlights（9.7K）和抓取官方页（中位 15K，最大 186K）。在 token 预算紧的 research 中，这一点有实际价值（判断）。

**劣势的场景**

- **非库主题**（开放平台、数据服务、App 手册、定价配额、产品评测）：覆盖 25%，同时是历史 thin 失败的主要来源。有索引也不等于可用：飞书和 Surge 都有 `/websites/...` 索引，但片段检索没有命中问题。
- **精确事实查询**（某个属性、某张下限表、某个参数的完整取值）：q03、q08、q10、q12 中，Context7 返回了同一主题的相邻片段，却漏掉了回答问题的那一条。
- **库归属不明显**：`CancellationToken` 属于 tokio-util，`tailwindcss` 被解析为插件库，中文名“飞书开放平台”解析错误。调用方必须懂得换名再解析。
- **相关性不设防**：三类离题正文都能通过 thin 门，即错库（q02 首次、R1 sq1）、营销首页（q13、R3）和错页（q11）。在 research 中，它们会占满证据名额，甚至让子问题“收敛”。
- **新鲜度**：Surge 引用了已失效的旧路径（实测）；Effect 取自 main 分支的 v4 源码（判断）。

**其他来源的对应表现**

- **Exa**：本次最稳定且最快。去掉域名过滤后，top-1 仍然是官方页。它的输出是 URL + highlights，可以直接进入 fetch_before_claim 流程。
- **Tavily 候选**：偏向博客和镜像。出现过陈旧镜像（q02）、事实错误（q10）和错接口（q13），作为官方文档来源最弱。
- **主搜索**：覆盖 100%，并且 13/14 的 `sources` 含官方页。代价是中位 38 s，而且它的回答是未验证的综合，仍须抓取来源页才能作为证据。

## 6. 建议

### 6.1 Skill 指引：何时声明 `docs_search`

修改位置：`skills/forager/references/research.md`、`ordinary-search.md` 与 `capability-vocabulary.json` 中的 `select_when`。

1. **只在子问题指向一个具名开源库、框架或 SDK，并询问其 API 用法或代码时声明 `docs_search`。**子问题文本应以包名开头，例如 “tokio-util CancellationToken …” 而不是 “Tokio 的 CancellationToken …”，因为 research 用整句问题作为 `libraryName` 去解析库。
2. **以下情况不声明 `docs_search`**，改用 `web_search`，并把已知的官方文档 URL 作为 known URL 写进子问题：
   - 厂商开放平台和 REST API，例如飞书、iFinD、Tushare、AmazingData；
   - App 手册，例如 Surge；
   - 定价、配额、资格、政策；
   - 模型或产品评测；
   - “当前状态”类问题；
   - 架构或最佳实践比较。
3. **在当前引擎下，不要在同一子问题里同时声明 `docs_search` 和 `web_search`**，应拆成两个子问题。否则在 quick 预算下，web 候选会被挤掉（见 3.2）。
4. **精确事实**（某个属性、限额表、枚举取值）优先用 `web_search` 或 known URL 抓取官方页。直接检索时优先 `forager exa search`。
5. **直接使用 `forager context7` 时**，先核对 top-1 的 `title` 与包名是否一致，以及 `total_snippets` 是否足够。本次可用索引至少有数百个 snippet；3–4 个 snippet 的索引都不可用，具体阈值属于判断。不一致时，用包名再解析一次；仍不合适就改用 Exa。

**预期效果**：按 journal 分布，规则 2 可避开至少 73% 的历史 thin 失败（52/71）；如果把架构比较和模型评测也排除，可避开约 90%（64/71）。按标题判断，9 条离题证据中有 8 条出自这类主题，也会一并消失。规则 3 可直接避免 R2 型的“0 证据失败”。

### 6.2 引擎侧门控

以下改动涉及 Documentation Search 的顺序和 research 的候选分配。落地前需要先更新 ADR 0011 与 `docs/spec/forager/04-architecture.md` 中的相应约定。

1. **普通搜索的 Documentation Search 改为 Exa 优先**，也就是把 `DOCS_SEARCH` 的顺序从 `[context7, exa]` 改为 `[exa, context7]`。如果只希望普通搜索改序，也可以按入口分别定序。
   - 依据：普通搜索中 Context7 只返回无正文、无 URL 的库定位符，按判断 top-1 错误率 20/44；Exa 覆盖 93%，延迟 2.4 s，返回可直接抓取的 URL。
   - 预期：`extra_sources` 从“需要再调一次 context7 docs 的库 ID”变为可直接 fetch 的官方 URL，Exa fallback 的 0% 可达性问题随之消失。
2. **在 research 中按能力公平分配候选名额。**`bound_discovery_candidates` 目前按到达顺序截断，应改为在同一子问题声明的各能力之间轮转分配，并让 docs 候选最多占 ⌈limit/2⌉ 个名额。
   - 预期：R2 型失败中，至少有 1–2 个 Tavily 或 Exa URL 会被抓取；本次抽样中，这类 URL 对飞书问题可达 3/3 覆盖（B、D 两路实测）。
3. **在 resolve 阶段加相关性门控**，丢弃满足以下任一条件的 Context7 候选：
   - `title` 或 `library_id` 与问题中的库名 token 没有交集，例如 iFinD 解析出的 ESLint、Knip；
   - `total_snippets` 低于阈值，例如 20（阈值属于判断，对应 q13 的 3、q11 首选的 4）。

   候选全部被丢弃后，走已有的“无可消费来源”分支，继续到下一个 provider。预期：非索引主题在 resolve 阶段就转向 Exa，省下每个候选约 3 s 的无效 query-docs 读取。
4. **在读取阶段加锚词门控**：除 thin 检查外，如果问题中含 ASCII 标识符形态的锚词（带 `_`、`::`、`.` 或驼峰，例如 `fund_adj`、`CancellationToken`、`receive_id_type`），Context7 正文至少要命中一个，否则按 `quality` 拒绝。
   - 预期：能拒绝 R1 sq1、R3/q13 和 q02 首次这三类离题正文，让子问题继续尝试其他候选，而不是以错误证据收敛。
   - 局限：拦不住 q11 这种锚词命中但页面不对的情况；对纯中文问题无效，应只在存在锚词时启用。
5. **收敛条件与诊断**：如果子问题同时声明了 `web_search`，仅凭未经锚词门控的 Context7 docs 证据不应判为 `evidence_converged`。gap 文案中的 “candidate URLs” 应改为 “candidates”，并单独报告 docs 读取的 thin 次数。预期：R3 型静默失败会显式暴露为 gap。

建议的落地优先级为 1 → 2 → 3，这三项风险低、收益清晰；4 和 5 需要配套测试以控制误拒。

## 7. 局限与不确定性

- **样本小**：14 条查询、3 次 research，每个来源各跑一次，没有重复采样。延迟受网络和 provider 负载影响，只能看量级。
- **评分主观**：关键事实由作者依据官方页选取，部分覆盖的 0.5 分属于判断。统计按“是否包含”计，不衡量表述质量。
- **A 路径偏乐观**：A 由作者挑选最佳 `library_id` 并允许再解析一次，是 Context7 的上界。自动路径用整句问题作为 `libraryName`，没有这种人工纠正。
- **B 使用了域名过滤**：带 `--include-domains` 的主测中，q06 反而因为域名已迁移而失败。去掉域名的抽样只有 5 条，结论方向一致，但样本更小。
- **D 的成本未计量**：主搜索的 token 与计费成本没有计入，回答中存在一处无法在官方页核实的型号（q10 的 “Opus 5.5”）。
- **journal 数据有缺口**：research 证据正文已被清理，相关性只能按标题判断；resolve 与 query-docs 的拆分是估算。
- **research 抽样只覆盖 quick 预算**：standard（limit 6）和 deep（limit 9）下挤占程度较轻，但本次未测。
- **结论有时效**：Context7 索引、Exa 排序与官方文档都会变化，本结论对应 2026-09-25 的状态。
- **未测量计费成本**：没有比较各 provider 的 credit 或 token 花费。
