# 2. CLI 接口

权威来源：[#56 Resolution](https://github.com/jfmoe/smartsearch/issues/56) 及其[补充决议](https://github.com/jfmoe/smartsearch/issues/56#issuecomment-5076846003)。

## 定名

项目 / binary 名 **forager**（脱离上游 fork 网络的独立身份；crates.io / GitHub 撞名验证通过）。

## 命令面（12 顶层）

```
forager search QUERY [--capabilities CSV|none] [--model ID] [--extra-sources N]
                     [--fallback auto|off]
                     [--timeout 180] [--format json|markdown|content] [--output FILE [--receipt]] [--verbose]
forager research QUERY [--plan FILE|-] [--budget quick|standard|deep（默认 standard）]
                       [--evidence-dir DIR] [--fallback auto|off] [--timeout 600] [...]
forager fetch URL [--timeout N] [...]
forager map URL [--instructions S] [--max-depth 1..=5] [--max-breadth 1..=500]
                [--limit N>0] [--timeout 10..=150（默认 150）] [...]
forager exa search QUERY [--num-results 5] [--search-type neural|keyword|auto]
                         [--include-text [--text-max-characters 3000]] [--include-highlights]
                         [--start-published-date D]
                         [--include-domains ...] [--exclude-domains ...] [--category NAME] [...]
forager exa similar URL [--num-results 5] [...]
forager context7 library NAME [QUERY] [...]
forager context7 docs LIBRARY_ID QUERY [...]
forager anysearch search QUERY [--domain D --sub-domain S] [--sub-domain-params JSON] [--max-results 5] [...]
forager anysearch domains [DOMAIN] [...]
forager platform arxiv search [QUERY] [--category CAT]... [--author NAME] [--title TEXT]
                              [--submitted-from YYYY-MM-DD] [--submitted-to YYYY-MM-DD]
                              [--sort relevance|submitted|updated] [--limit 1..=100（默认 10）]
                              [--cursor CURSOR] [--timeout 120] [--format json|markdown] [...]
forager doctor [--provider PROVIDER] [--timeout 30] [--format json|markdown]
forager smoke [--live] [...]
forager config path|list|set|unset
forager setup [--non-interactive] [--lang zh|en]
```

- **分界规则**：点名 provider 的命令按 provider 分组嵌套（exa/context7/anysearch）；操作语义 + fallback 链的按操作命名保持顶层（fetch、map）；平台直连命令按 `platform <id> <op>` 嵌套。裸动词＝智能管线，provider 前缀＝旁路直连。
- **别名六槽**（全部 visible_alias）：`s`=search、`f`=fetch、`rs`=research、`c7`=context7、`as`=anysearch、`ls`=config list。关闭 clap `infer_subcommands`。

## 输出与退出码

- `--format json(默认)/markdown/content` 三态；**content 收窄**到 search、fetch、context7 docs 与 research，per-command ValueEnum 在解析层强制。research 的 Markdown/content 都渲染 Research Evidence Index 与 unresolved gaps，不渲染证据正文或机械答案。doctor 默认 json。
- `--output FILE` 为 **tee 语义**（写文件 + stdout 照常）。写失败＝非零终态退 3，stdout JSON 照常输出并标注写失败（#59 H15）；与 journal 旁路（非致命）区分。
- `--receipt`（须与 `--output` 同用，所有带 `--output` 的命令均支持）为**回执语义**：文件内容与 tee 完全相同；仅当命令成功（退 0）且文件写入成功时，stdout 改为单行 JSON 回执 `{"output_path", "bytes", "lines"}`，其中 `bytes`/`lines` 描述写入文件的内容。命令失败时 stdout 仍是该失败的完整瘦载荷；写失败时退回 tee 的写失败行为（完整 stdout + 标注，退 3）。它让调用方把长结果留在文件中、按需读取片段，而不是让全文进入调用方上下文。
- 退出码：`0` 成功（含直连命令的合法空结果）；`2` 参数错（clap 天然 + 坏 plan + `config set` 非法路径）；`3` config_error（含未知文件键、未知 `FORAGER_*` env、web_fetch 空链、`--output` 写失败）；`4` transport 族终态；`5` content 族终态（quality/evidence；**evidence_error 由 4 改 5**）。`1` 空缺；panic 101 不拦，为非契约异常出口。
- **JSON 飞行前终态**：clap 已成功解析并选择 `--format json` 后，search/fetch/research 的配置装载以及 research plan 读取、解析、校验失败都在 stdout 返回单个可解析错误对象，退出码沿用 2/3/4/5。clap argv 错误与 panic 豁免；Markdown/content 保持简洁 stderr 错误。尚未取得有效 `JournalRuntimeConfig` 时不得猜测 journal 配置或回退默认目录，返回 `journal_ref: null` 与 `journal_status: "unavailable"`。
- **默认 stdout 瘦载荷**：成功＝结果本身；普通命令失败＝`error_kind` + 一行 message + attempts 计数摘要（total/by_kind/providers，非全文）+ 精简 capability_gaps + `journal_ref`（nullable）与 `journal_status`。research 失败使用稳定小形状：`error_kind`、有界 `message`、完整 `evidence_dir`、可空且仅在文件可读时存在的 `summary_path`、精简 gap、`synthesis_policy` 与 journal 状态；不按编码长度切换 schema，locator 永不截断。普通路径以 4 KiB 为目标，极端长的合法路径可超出。全量 `provider_attempts` 只落 journal；`--verbose` 为 inline 全量逃生阀。
- **search 失败附带候选**（ADR 0016）：辅助能力与主搜索并发执行，主搜索失败时终态与退出码仍只由主搜索决定；已取得的 Search Candidate 不丢弃。默认失败 JSON 增加非空时才出现的 `capability_gaps`，以及 `extra_sources`（去掉各条 `summary`，按顺序保留到载荷触及 4 KiB 目标前为止）与 `extra_sources_truncated`；attempts 计数摘要同时计入辅助 attempts；`--verbose` 给出完整 `extra_sources`；Markdown 失败视图列出 Extra Sources；journal 失败结果面保存完整候选与 gaps。无候选时不输出后两个字段。
- **fetch 成功载荷**：`content` 只包含 provider 无关的 Markdown 正文；provider attempts 与 diagnostic 保持在各自字段/输出通道，不混入正文。URL 与 PDF 共享 `web_fetch` 链和失败语义，`--output` 的 tee 与 `--receipt` 语义同上。

## search 参数清理

- **砍** `--providers`（链序权威归配置）、`--platform`（伪过滤器）、`--stream/--no-stream`（持久开关走配置键，临时覆盖走 env）。
- 留 `--model`、`--extra-sources`、`--fallback`。`--extra-sources` 接受 `0..=20`：0 是分支内默认哨兵，Supplemental Web Search 映射为 3，Documentation/Vertical Search 映射为 1；1–20 原样传递，21+ 在读取配置和联网前由 clap 退出 2。数量不选择 capability，`--capabilities none` 仍不执行补强。
- 删除 `--validation`，不保留 alias、静默忽略或替代门；旧参数按未知参数退出 2。
- **`--timeout` 横切**所有网络命令：search 180 / research 600 / doctor 探测 30 / fetch、map 补齐。

### `--timeout` 语义（补充决议 A2）

整条 CLI 命令的 **hard deadline**：实现可为单次 attempt 设更短上限，但所有重试、fallback 与探针共享总预算；超时结果保留已完成 attempts（journal 全量 + stdout 摘要，#59 B2）。预算保留规则（保证 fallback 可达）见第 4 章。map 只接受 `10..=150` 秒并原样作为命令 Deadline 与 Tavily request body；每个网络 attempt 仍取命令剩余预算与 `providers.tavily.timeout` 的较小值，不设 cap、clamp 或 provider 内二次默认。

## search 输出角色

- `sources` 只表示 Primary Search Source；空 `sources` 即主回答没有可归属的引用（例如不带联网搜索的 fallback backend 作答），forager 不另设字段标记，由调用方把此类回答视为未核实。所有非主候选统一由 `extra_sources` 表示，领域类型为 Search Candidate，不再公开独立 `vertical_results`。
- 每条 Search Candidate 固定包含必填 `provider`、`capability`、`provider_data`，以及可空 `title`、`url`、`summary`。`url` 只能是真实 HTTP(S) URL；`summary` 只复制 provider-native 描述、摘要或片段；`provider_data` 只投影 provider 定义的 snake_case 强类型白名单，不透传原始 HTTP/MCP 包装或正文。
- Context7 Documentation Search 只 resolve library，候选 `url: null`、`summary` 复制 description；`provider_data` 白名单为 `library_id`、`total_snippets`、`trust_score`、`benchmark_score`、`stars`、`versions`。Research 才调用 query-docs 取证。Exa Documentation Search 使用真实 URL，白名单为 `id`、`highlights`、`published_date`、`author`、`image`、`favicon`，且不读取完整 text。直连 `context7 docs` 继续只公开可消费 `content`，structuredContent-only 响应也必须填充该字段。
- Main Search 在完整响应组装后共享执行 normalizer：先删除完整闭合、大小写不敏感且可跨行的 `<think>...</think>`，再投影末尾显式来源标题块与 `[[N]](HTTP(S) URL)`；其他策略关键词、`sources(...)`、任意 `<details>` 或尾链猜测不解析。规范化后 answer 为空为 Runtime。provider 引用注释中仅由数字构成的标题是引用序号而非页面标题，按无标题处理。来源按脱敏后的公开 URL 稳定去重。

## 契约①：`--capabilities`

CSV + 独占哨兵 `none`，未传＝自动路由。Rust 类型 `Option<CapabilitySet>` 三态：`None`＝未声明（分类器；未配置则降级默认 Web 链）、`Some(∅)`＝`none`（纯主搜）、`Some({…})`＝caller 权威。词表 4 值（docs_search/web_search/web_fetch/vertical_search）编译期 enum。

## 契约②：research 计划注入（Schema v1）

通道：`--plan FILE`，`-`＝stdin。caller 注入与分类器产出共用同一类型：

```json
{
  "plan_version": 1,
  "intent_signals": {
    "recency_requirement": "none | recent | current",
    "docs_api_intent": false,
    "source_authority_need": "normal | high",
    "claim_risk": "medium | high",
    "cross_validation_need": "normal | high"
  },
  "decomposition": [
    { "id": "sq1", "question": "…", "reason": "…", "required_capabilities": ["web_search"] }
  ]
}
```

- 相对旧版砍四块：`steps`/`capability_plan`（执行编排归引擎）、`known_url`/`locale_domain_scope`（URL 引擎自算）、`breadth_depth_budget`（`--budget` 为准）。
- 语义：有 `--plan`＝caller 权威跳过分类器；无＝分类器生成同 schema；分类器未配置＝退 3；坏 plan＝退 2；`plan_version` 不识别＝退 2。
- **严格解析**：未知/缺字段拒绝、空 `decomposition` 无效、`id` 非空且唯一、capability 重复保序归一化、`reason` 必填且非空。v2 走显式新分支，不做宽容升级。

### 权威规则（补充决议 A1，经 #57 R2 收窄）

- `decomposition[].required_capabilities` 决定允许跨越哪些 seam；plan 语境词表**限三值**（docs_search/web_search/vertical_search），用独立三值枚举 `PlanCapability`。
- `intent_signals` 只在已声明 seam 内影响**证据强度与交叉验证策略**，不得增删 capability；其对 provider 顺序的影响通道＝具名 request class 机制，**v1 未启用**（引擎永不静默偏离配置序）。
- **`web_fetch` 为 research 引擎不变量**（fetch-before-claim）：由引擎按证据需要自动执行，plan 中声明它＝退 2，错误信息说明其由引擎自动执行。
- `required_capabilities`＝seam 门（路由权威）；seam 内 provider 凭据缺口不阻断成功，经 capability_gaps 自报（#59 H7：required＝路由权威、可用性 advisory）。
- 与 ADR-0004 关系：本规则是计划注入进入 research 后对该 ADR 豁免区的首次权威定义，与其 search 侧语义并立不冲突。

### Research Evidence Index

research 是文件化证据管线，不是答案引擎；未指定 `--budget` 时使用 `standard`。默认 JSON 顶层只包含 `evidence_items`、`evidence_dir`、`plan_path`、`unconsumed_candidates: {count, path}`、`gap_check`、`capability_gaps`、`synthesis_policy: "fetch_before_claim"`、`journal_ref` 与 `journal_status`。每条 evidence item 包含 `id`、可空 `url`、可选 `library_id`/`title`、`provider`、`source_type`、`subquestion_ids`、`content_len`、`verified` 和可直接读取的 `path`；同一 locator 只抓取一次，`subquestion_ids` 记录它覆盖的全部子问题。URL evidence 使用 `[eN](URL)`；Context7 无 URL evidence 使用 `[eN]`，由同 ID 的 Index 项完成归属。

`evidence_dir` 固定写出：

- `00-plan.json`：完整规范化计划；
- `NN-evidence.md`：逐条 evidence 正文，stdout、summary 与 journal 不重复嵌入；
- `candidates.json`：`is_evidence: false` 的未消费候选完整元数据；
- `summary.json`：Research Recovery Manifest，而非 Research Evidence Index；记录 query、budget、plan source、capabilities、fallback、evidence identity/metadata/path、coverage、gap、capability gaps、终态、attempts 与 `synthesis_policy`，不包含 evidence 正文。

成功 stdout 交付 Research Evidence Index；失败通过可读的 Recovery Manifest 保证制品可恢复，不要求两种终态内联同一形状。manifest 写入失败时保留原终态与完整 `evidence_dir`，`summary_path: null` 并输出既有 diagnostic。`--verbose` 仍只负责把 `provider_attempts` 显式内联；`--output` 的 tee 与 `--receipt` 语义同上。

## 契约③：`doctor --provider`

两档：`doctor` 浅检全体（掩码配置 + 凭据存在 + 可达性（对 endpoint 发 GET 并只等待响应头，任何 HTTP 响应都算可达；部分 endpoint 对 HEAD 不响应）+ 过宽权限报告 + config list 同构生效值块）；顶层 `ok` 等于所有 `configured=true` provider 均 `reachable=true`，零配置为 true，任一不可达则 JSON/Markdown 均为 false 并退出 4，permission warning 不改变 `ok`。`config_warnings` 报告主搜索链中与首个已配置 backend 使用相同 endpoint（忽略末尾 `/`）和相同主模型的后续 backend——这类 fallback 与主 backend 处于同一故障域；该警告同样不改变 `ok`。`--provider NAME` 深探单体，值域＝9 provider 编译期 enum。需要凭据的 8 个 provider 执行凭据有效性 + 最小活体调用；`arxiv_api` 不需要凭据，恒为已配置，深探执行一次最小平台检索；openai-compatible 额外保留 stream/no-stream 双形状判定。声明访问策略的 provider（`arxiv_api`）的浅检可达性 GET 与深探请求都先经过跨进程限速，等不到窗口时浅检记为不可达、深探以 timeout 失败，且都不发送请求。

## 平台命令

`forager platform <id> <op>` 直连检索内置平台（ADR 0019；接入契约见第 7 章）。每个平台一棵静态 clap 子树；`forager platform arxiv --help` 及各操作的 help 是参数语法的唯一权威。本期交付 `forager platform arxiv search`；`fetch` 尚未实现。

### `platform arxiv search`

| 参数 | 类型与取值 | 默认值 | 语义 |
|---|---|---|---|
| 查询词（位置参数） | 字符串，可省略 | 无 | 普通关键词，按空白与双引号切分，所有词都必须出现（AND）；arXiv 查询语法字符按字面词处理 |
| `--category` | 可重复，arXiv 分类代码 | 无 | 多个分类之间为 OR，整体与其他条件 AND |
| `--author` | 字符串 | 无 | 作者名短语匹配 |
| `--title` | 字符串 | 无 | 标题短语匹配 |
| `--submitted-from` / `--submitted-to` | `YYYY-MM-DD`，UTC | 无 | 提交日期区间，两端都包含；可只给一端 |
| `--sort` | `relevance` / `submitted` / `updated` | `relevance` | 固定降序 |
| `--limit` | 1..=100 | 10 | 本页最多返回的条数 |
| `--cursor` | 上一页的 `next_cursor` | 无 | 翻页；完整恢复原请求 |

- 查询词与 `--category`、`--author`、`--title` 至少给出一个。通用 flag 为 `--timeout`（默认 120 秒，计入等待限速窗口的时间）、`--format json|markdown`、`--output`/`--receipt`、`--verbose`。
- **cursor 独占**：`--cursor` 与显式传入的查询词、任何平台选项或 `--limit` 同时出现时由 clap 退 2；解析后的默认值不算冲突；通用 flag 可以同时使用。
- **输出**：成功 JSON 为 `{platform, provider, items, next_cursor}`，`--verbose` 时另有 `provider_attempts`（含被跳过的 route）。每个 item 含 `ref`（带版本号，如 `arxiv:2401.01234v2`）、canonical `url`、`depth: "abstract"`、空白折叠后的 `title`、`authors`、`published`，以及 arXiv 数据 `abstract`（完整摘要）、`updated`、`primary_category`、`categories`、`doi`、`journal_ref`、`comment`、`pdf_url`；缺失字段为 `null`。有下一页时 `next_cursor` 为字符串；本页条数小于 limit 或已达总数时为 `null`；合法零结果为 `items: []` 且退 0。平台直连命令不写 Search Result Journal。
- **退出码**：ref、cursor 或选项无效、日期区间颠倒、缺少查询词与过滤条件、所有已配置 route 都不支持所请求的选项＝飞行前退 2；`platforms.arxiv.order` 为空或操作可用 route 为空、order 含其他平台的 route＝飞行前退 3；arXiv Atom error entry（HTTP 400 或 200）为 attempt 级 Parameter，退 4 并带 arXiv 消息；等待限速窗口时预算不足为 Timeout 退 4；限速状态不可用为 Runtime 退 4 且不发送请求。

## 收尾

- `smoke [--live]`：默认离线档；内容定义见第 5 章。
- `--sub-domain-params` 保留内联 JSON 单对象 + serde 严格校验。
- `setup` 只留 `--non-interactive`/`--lang`；键面见第 3 章。
