# smartsearch research 流程编排梳理（as-is）

> 用途：[定稿 research 流程重构方案](https://github.com/jfmoe/forager/issues/96) 的对照输入——精确判断 forager 迁移中 research 编排丢失与超越了什么。
> 生成：2026-08-04 由 codex CLI（read-only）探索 smartsearch 仓库产出，经本会话对 minimum profile、CLI 参数、skill 引导层等关键声明抽查核验。
> 行号锚点：smartsearch `4e76689`（已归档）。本文档只做 as-is 梳理，不含改进建议。


## 全流程阶段总览

```text
deep / dr（离线 planner）
  CLI 参数解析
      │
      ▼
  build_deep_research_plan()
      ├─ 本地 rules 推导 intent_signals
      ├─ known URL / 无 URL 两套 decomposition
      ├─ capability_plan
      ├─ 生成有序 steps[].command / output_path
      ├─ DEEP_ALLOWED_TOOLS 过滤
      └─ quick budget 裁剪
      │
      ▼
  输出 plan（JSON / Markdown / content）
      │
      └──── 此命令到此结束，不调用 provider、不抓取页面
                 │
                 ▼
        agent / 用户另行执行 steps
                 │
                 ▼
              gap_check


research / rs（引擎内 live executor）
  CLI 参数解析
      │
      ▼
  fallback 参数校验
      │
      ▼
  validate_minimum_profile()
      │
      ▼
  build_deep_research_plan() ──复用 plan 与 intent_signals
      │
      ▼
  IntentRouter.route(..., plan_intent_signals=...)
      │
      ▼
  _research_capability_routes()
      │
      ▼
  写 00-plan.json
      │
      ▼
  known_url_fetch
      │
      ▼
  [docs intent] docs_discovery
      │
      ▼
  [条件满足] web_discovery
      │
      ▼
  [auto + official intent] Exa 补强
      │
      ▼
  [vertical intent] vertical_discovery
      │
      ▼
  _select_candidate_urls(limit=6)
      │
      ▼
  candidate_fetch（逐 URL、逐 provider 串行）
      │
      ▼
  evidence / gaps / gap_status
      │
      ▼
  _evidence_only_synthesis()
      │
      ▼
  写 summary.json，返回 result JSON
```

`deep` 是同步、纯本地计划构造；`research` 是异步函数，但其阶段和 provider 调用按 `await` 顺序串行执行。`research` 复用 `deep` 的 plan，却不执行 plan 中的 `steps[].command`。`src/smart_search/service.py:991-1264`、`src/smart_search/service.py:1267-1529`

---

## 1. CLI 入口与参数

两个命令及别名分别为 `deep`/`dr`、`research`/`rs`。`src/smart_search/cli.py:40-70`

| 参数 | `deep` | `research` |
|---|---|---|
| 位置参数 | `query` | `query` |
| `--budget` | `quick\|standard\|deep`，默认 `standard` | `quick\|standard\|deep`，默认 `deep` |
| `--evidence-dir` | 默认 `""` | 默认 `""` |
| `--fallback` | 不存在 | `auto\|off`，默认 `auto` |
| `--format` | `json\|markdown\|content`，默认 `json` | 同左 |
| `--output` | 默认 `""` | 同左 |
| `--capabilities` | 不存在 | 不存在 |
| `--timeout` | 不存在 | 不存在 |

定义位置：`src/smart_search/cli.py:3139-3160`、公共输出参数见 `src/smart_search/cli.py:1332-1334`。测试明确断言 `research --capabilities none` 会被 argparse 拒绝。`tests/test_cli.py:2187-2195`

CLI 分派为：

```python
build_deep_research_plan(
    args.query,
    budget=args.budget,
    evidence_dir=args.evidence_dir,
)

await research(
    args.query,
    budget=args.budget,
    evidence_dir=args.evidence_dir,
    fallback=args.fallback,
)
```

`src/smart_search/cli.py:2594-2608`

`--output` 写入的是按 `--format` 渲染后的完整结果，同时仍向 stdout 输出；JSON stdout 会额外经过终端编码安全处理。它与 `--evidence-dir` 下的 research 自动 artifacts 是两套独立输出。`src/smart_search/cli.py:1235-1240`、`src/smart_search/cli.py:1313-1322`

---

## 2. `build_deep_research_plan()`：输入规范化与基础字段

函数签名默认 `budget="standard"`、`evidence_dir=""`。它：

1. 对 `query` 执行 `strip()`。
2. 通过 `_deep_budget()` 规范 budget：只有 `quick`、`standard`、`deep` 原样保留，其余值变为 `standard`。
3. 显式 `evidence_dir` 会先 `strip()`；为空时生成默认目录。
4. 提取 query 中的 HTTP(S) URL。
5. 使用本地 `build_rules_route(..., mode="rules")` 判断 docs 与中文当前意图，不调用远程 IntentRouter。`src/smart_search/service.py:980-999`

默认 evidence 路径为：

```text
{tempfile.gettempdir()}/smart-search-evidence/{YYYYMMDD-HHMM}-{slug}
```

`slug` 会去掉 `http://`/`https://`，把非 ASCII 字母数字或中文字符转成 `-`，最多 48 字符；空 slug 使用 `deep-research`。该阶段只生成路径字符串，不创建目录。`src/smart_search/service.py:928-937`

直接调用 `build_deep_research_plan(..., budget="")` 或传非法非空 budget 时会得到 `standard`。`research()` 则先使用 `budget or "deep"`，因此其空 budget 会按 `deep` 处理，而非法非空值仍会归一为 `standard`。`src/smart_search/service.py:980-982`、`src/smart_search/service.py:1312-1313`

---

## 3. `DEEP_*` 关键词集与 `intent_signals`

### 3.1 关键词集

| 常量 | 内容 | 当前作用 |
|---|---|---|
| `DEEP_TRIGGER_KEYWORDS` | `深度搜索`、`深度调研`、`深入搜索`、`deep search`、`deep research`、`核验`、`验证`、`交叉验证`、`选型`、`对比`、`评测` | 当前源码中只有定义，没有 planner 或 CLI 消费方；代码入口由显式 `deep` 命令触发 |
| `DEEP_HIGH_COMPLEXITY_KEYWORDS` | `对比`、`选型`、`核验`、`验证`、`为什么`、`架构`、`方案`、`趋势`、`优缺点`、`风险`、`区别`、`怎么选`、`compare`、`comparison`、`evaluate`、`architecture`、`tradeoff`、`trade-off`、`risk` | `_is_deep_complex()` |
| `DEEP_RECENT_KEYWORDS` | `最近`、`最新`、`当前`、`现在`、`今天`、`实时`、`刚刚`、`本周`、`本月`、`recent`、`latest`、`current`、`today` | `recency_requirement` |
| `DEEP_CURRENT_KEYWORDS` | `今天`、`实时`、`刚刚`、`当前`、`现在`、`today`、`current`、`live`、`realtime` | 强制 `recency_requirement="current"` |
| `DEEP_CHINA_KEYWORDS` | `中国`、`国内`、`中文`、`政策`、`监管`、`公告`、`A股`、`港股` | `locale_domain_scope="china"` |
| `DEEP_EXA_DISCOVERY_KEYWORDS` | `官方`、`官网`、`论文`、`paper`、`papers`、`research paper`、`产品页`、`product page`、`可信站点`、`trusted`、`known domain`、`known domains`、`site:`、`白皮书`、`standard`、`standards` | planner 中决定是否增加 Exa；runtime 中产生 `official_low_noise_intent` |

`src/smart_search/service.py:65-148`

`DEEP_TRIGGER_KEYWORDS` 不参与自动分流；planner 返回的 `trigger_source` 固定为 `explicit_cli`。`src/smart_search/service.py:80-92`、`src/smart_search/service.py:1232-1239`

### 3.2 `intent_signals` 推导

| 字段 | 推导规则 |
|---|---|
| `recency_requirement` | 命中 `DEEP_CURRENT_KEYWORDS` 或 rules 的 `zh_current_intent` → `current`；市场词 `行情/价格/走势/币圈/股票/市场` 与最近词同时出现 → `current`；否则命中最近词 → `recent`；其余 → `none` |
| `docs_api_intent` | `_is_docs_intent()`，即本地 rules 路由中的 docs/API intent |
| `locale_domain_scope` | 命中中国关键词 → `china`；否则 → `global`；存在 URL 时无条件覆盖为 `known_domains` |
| `known_url` | 是否提取到至少一个 HTTP(S) URL |
| `source_authority_need` | docs intent、高 `claim_risk`，或命中 `官方/文档/论文/标准/政策/监管/official` → `high`；否则 `normal` |
| `claim_risk` | 时效要求为 `recent/current`，或命中 `核验/验证/真假/价格/行情/财经/医疗/政策/监管/risk` → `high`；否则 `medium` |
| `cross_validation_need` | `claim_risk="high"`，或命中 `对比/选型/核验/验证/compare/versus` → `high`；否则 `normal` |
| `breadth_depth_budget` | 规范化后的 budget |

`src/smart_search/service.py:996-1025`

当前 planner 实际只产生：

- `locale_domain_scope`: `global`、`china`、`known_domains`
- `claim_risk`: `medium`、`high`
- `difficulty`: `standard`、`high`

### 3.3 `_is_deep_complex()`

复杂度满足任一条件即为真：

1. budget 为 `deep`；
2. query 命中任一 `DEEP_HIGH_COMPLEXITY_KEYWORDS`；
3. 移除 URL 后，分隔符正则 `[/、,，]| 和 | 与 | vs | VS | versus ` 的匹配数不少于 2。

移除 URL 只用于分隔符计数；关键词判断仍使用原始 query。复杂时 `difficulty="high"`，否则为 `standard`。`src/smart_search/service.py:985-988`、`src/smart_search/service.py:1013-1014`

---

## 4. planner 的 capability declaration 与 step 通用规则

所有普通 `search` step 使用同一个 `--capabilities` 声明：

- docs intent → 加 `docs_search`
- 有时效要求、`locale_domain_scope=="china"` 或高交叉验证 → 加 `web_search`
- known URL → 加 `web_fetch`
- 空集合写成字面量 `none`
- 多 capability 按 catalog 顺序规范化，即 `docs_search`、`web_search`、`web_fetch`、`vertical_search`；planner 当前不声明 `vertical_search`。`src/smart_search/service.py:1027-1035`、`src/smart_search/intent_catalog.py:82-136`

通用 command 模板：

```text
smart-search search "<query>" \
  --capabilities <CSV_OR_NONE> \
  --validation balanced \
  --extra-sources <N> \
  --format json \
  --output "<evidence_dir>/<NN>-search.json"

smart-search exa-search "<query>" \
  --num-results 5 --format json --output "<...>/<NN>-exa.json"

smart-search zhipu-search "<query>" \
  --count 5 --format json --output "<...>/<NN>-zhipu.json"

smart-search fetch "<key-url>" \
  --format markdown --output "<...>/<NN>-fetch.md"
```

每个 step 固定包含：

```text
id
subquestion_id
tool
purpose
command
output_path
```

step ID 按生成顺序为 `s1`、`s2`……；文件名前缀以当前 step 数量生成。`command` 内的 `--output` 与 `output_path` 使用同一路径。参数引用使用 PowerShell 风格转义：反引号翻倍，`$` 和 `"` 前加反引号。`src/smart_search/service.py:940-964`、`src/smart_search/service.py:1041-1058`

---

## 5. known URL 分支

只要 query 中存在 URL，就进入该分支；planner 只使用提取结果中的第一个 URL。`src/smart_search/service.py:1063-1066`

### decomposition

| ID | `question` | `reason` | `required_capabilities` |
|---|---|---|---|
| `sq1` | `这个已知来源页面本身说了什么？{url}` | `用户已经给出 URL，Deep Research 必须先抓正文再扩展。` | `["page_evidence"]` |
| `sq2` | `围绕 {host} 还需要哪些相邻来源或交叉来源？` | `已知好 URL 适合用相似页面和广泛发现扩展证据。` | `["adjacent_source_discovery", "broad_discovery"]` |

### capability plan

| `capability` | `tools` | `reason` |
|---|---|---|
| `page_evidence` | `["fetch"]` | `Fetch the user-provided URL before making claims.` |
| `adjacent_source_discovery` | `["exa-similar"]` | `Find pages adjacent to the known source.` |
| `broad_discovery` | `["search"]` | `Broaden the context if the fetched page leaves gaps.` |

### steps

1. `fetch` 第一个 URL，输出 `01-fetch.md`。
2. `exa-similar` 同一 URL，`--num-results 5`，输出 `02-similar.json`。
3. `search` 原问题，输出 `03-search.json`，`--extra-sources 1`。

`src/smart_search/service.py:1067-1092`

该分支不再执行无 URL 分支中的 docs、Zhipu、复杂问题补齐或通用末尾 fetch 逻辑。因此即使 `difficulty="high"` 或 budget 为 `deep`，仍是两个子问题、三个 steps。search step 的 `--capabilities` 仍由前述统一声明规则决定。

runtime `research()` 与此不同：它会按出现顺序抓取 query 中的全部 URL，而非只抓第一个。`src/smart_search/service.py:1341-1359`

---

## 6. 无 URL 分支

### 6.1 固定 broad discovery

总是先创建：

```text
sq1.question = "{question} 的整体问题轮廓和候选来源是什么？"
sq1.reason = "先做 broad discovery，避免一开始把问题拆错。"
sq1.required_capabilities = ["broad_discovery"]
```

并增加：

```text
capability = "broad_discovery"
tools = ["search"]
reason = "Find the initial answer shape and candidate sources."
```

第一个 step 是普通 `search`；quick 使用 `--extra-sources 1`，standard/deep 使用 `--extra-sources 3`。`src/smart_search/service.py:1093-1103`

### 6.2 docs/API 分支

`docs_api_intent=true` 时增加 `sq2`：

```text
question = "{question} 的官方文档、API 或 SDK 证据在哪里？"
reason = "docs/API intent should resolve the library docs first, with Exa only as official-domain discovery."
required_capabilities = ["docs_source_discovery", "page_evidence"]
```

对应 capability：

```text
docs_source_discovery
tools = ["context7-library", "context7-docs"]
reason = "Resolve official library/API documentation first; use Exa only for official-domain or supplemental discovery."
```

生成顺序：

1. `context7-library`
2. `context7-docs`
3. 仅当命中 `DEEP_EXA_DISCOVERY_KEYWORDS` 时，再增加 `official_domain_discovery/["exa-search"]` 与 Exa step

library hint 是 question 中前两个匹配 `[A-Za-z][A-Za-z0-9_.-]*` 的 token，以空格连接；没有 token 时使用 `<library-name>`。`src/smart_search/service.py:1105-1144`

### 6.3 时效或中国范围分支

当 `recency_requirement!="none"` 或 `locale_domain_scope=="china"` 时，追加一个新 subquestion：

```text
question = "{question} 的最新或中文/国内来源如何交叉验证？"
reason = "Current or China-scoped prompts benefit from Zhipu web-search reinforcement."
required_capabilities = ["current_or_locale_source_discovery"]
```

对应 capability 为 `current_or_locale_source_discovery/["zhipu-search"]`，并生成一个 `zhipu-search` step。`src/smart_search/service.py:1146-1159`

### 6.4 complex decomposition 补齐

complex query 的目标子问题数：

- quick/standard：至少 2
- deep：至少 4

按当前 decomposition 长度依次补：

| 当前长度 | 新子问题 | `required_capabilities` |
|---|---|---|
| 1 | `"{question} 里有哪些主要选项、说法或路线需要分别验证？"` | `["cross_validation"]` |
| 2 | `"{question} 的成本、风险、限制和适用边界是什么？"` | `["low_noise_source_discovery", "page_evidence"]` |
| 3 | `"基于已抓取证据，{question} 应该如何形成可执行结论？"` | `["gap_check"]` |

若 capability plan 尚无 `cross_validation`，增加：

```text
capability = "cross_validation"
tools = ["search"]
reason = "Compare independent sources before final claims; supplemental tools depend on intent."
```

deep budget 且命中 Exa 关键词时，额外生成一个归属 `sq3` 的 Exa step，用于 `risks limitations comparison`。`src/smart_search/service.py:1161-1182`

`low_noise_source_discovery` 和 `gap_check` 可出现在 subquestion 的 `required_capabilities` 中，但该循环不会为它们各自创建同名 capability-plan item。

### 6.5 高交叉验证补强

当 `cross_validation_need=="high"`：

1. 确保 `cross_validation` capability 存在。
2. 目标 subquestion 为当前 decomposition 的最后一个。
3. 按以下互斥优先级补 tool：
   - 时效、中国范围或中文当前意图：加入 `zhipu-search`，且尚无 Zhipu step 时生成 step；
   - 否则 docs intent：加入 `context7-library`、`context7-docs`，不另建 step；
   - 否则命中 Exa 关键词：加入 `exa-search`，且尚无 Exa step 时生成 step。

`src/smart_search/service.py:1184-1203`

最后无条件增加：

```text
page_evidence
tools = ["fetch"]
reason = "Fetch key URLs before claim-level conclusions."
```

并追加 `<key-url>` fetch step。只有 `sq1` 时归属 `sq1`；否则归属最后一个 subquestion。`src/smart_search/service.py:1205-1206`

---

## 7. budget 裁剪规则

| budget | complex 判定 | 无 URL 子问题 | step 限制 | broad search `extra_sources` |
|---|---|---|---|---|
| `quick` | 仅关键词或分隔符触发 | simple 可为 1；complex 先补到至少 2，最终最多保留 2 | 最多 4 | 1 |
| `standard` | 仅关键词或分隔符触发 | 1～3；complex 至少 2，无最终上限裁剪 | 无统一 cap | 3 |
| `deep` | 无条件 complex | 无 URL 时补到至少 4，当前生成逻辑最终为 4 | 无统一 cap | 3 |

known URL 分支对三种 budget 都固定为 2 个子问题、3 个 steps，search 的 `extra_sources=1`。`src/smart_search/service.py:1063-1092`、`src/smart_search/service.py:1161-1163`

quick 裁剪的具体语义：

1. decomposition 超过 2 时取前 2。
2. steps 超过 4 时先取前 4。
3. 如果前 4 没有 fetch，从完整 steps 中找到第一个 fetch，改写为：
   - command target：`<key-url>`
   - output：`04-fetch.md`
   - steps：原前三步加该 fetch
4. 最终重新编号为 `s1...s4`。
5. 引用了已裁掉 subquestion 的 step，改指向保留的最后一个 subquestion。

`src/smart_search/service.py:1211-1230`

---

## 8. `DEEP_ALLOWED_TOOLS` 与过滤

源码 whitelist 共 13 项：

```text
search
exa-search
exa-similar
zhipu-search
zhipu-mcp-search
zhipu-mcp-reader
zhipu-mcp-search-doc
zhipu-mcp-repo-structure
zhipu-mcp-read-file
context7-library
context7-docs
fetch
map
```

`src/smart_search/service.py:65-79`

返回前：

- 每个 `capability_plan[].tools` 只保留 whitelist 中的值；
- `steps` 删除 tool 不在 whitelist 的项；
- 不会删除过滤后 `tools=[]` 的 capability item；
- 顶层 `allowed_tools` 是该集合按字典序排序后的列表。`src/smart_search/service.py:1208-1210`、`src/smart_search/service.py:1261`

当前 planner 分支实际可生成的 step tool 为：

```text
search
exa-search
exa-similar
zhipu-search
context7-library
context7-docs
fetch
```

`map` 和五个 `zhipu-mcp-*` 当前只在 whitelist 中，不由 planner 分支生成。

---

## 9. deep plan 完整 shape 与策略字段

正常返回的顶层字段完整列表为：

```text
ok
mode
query_mode
question
trigger_source
difficulty
intent_signals
decomposition
capability_plan
evidence_policy
preflight
steps
gap_check
final_answer_policy
usage_boundary
allowed_tools
evidence_dir
elapsed_ms
```

固定值和策略：

```json
{
  "ok": true,
  "mode": "deep_research",
  "query_mode": "deep",
  "trigger_source": "explicit_cli",
  "evidence_policy": "fetch_before_claim",
  "preflight": {
    "tool": "doctor",
    "command": "smart-search doctor --format json",
    "when": "configuration or provider availability is uncertain",
    "executed_by_deep_command": false
  },
  "gap_check": {
    "required": true,
    "rule": "fetch missing evidence for key claims or downgrade unsupported claims to unverified candidates",
    "unsupported_claim_action": "downgrade_to_unverified_candidate"
  },
  "final_answer_policy": "cite fetched evidence, list unverified candidates, and include key commands",
  "usage_boundary": {
    "search": "smart-search search runs live fast/broad search immediately.",
    "deep": "smart-search deep is an offline planner; it does not execute provider calls or fetch pages.",
    "execution": "An AI agent or user executes the listed steps with existing CLI commands, then performs gap_check."
  }
}
```

`src/smart_search/service.py:1232-1264`

这里的 `gap_check` 是交给执行者消费的计划策略，不是 `deep` 自己执行的状态检查。

---

## 10. `research()` 预检与 IntentRouter

### 10.1 早退顺序

`research()` 依次执行：

1. `query.strip()`。
2. 将 fallback 规范为小写；空值按 `auto`。
3. fallback 不是 `auto/off` 时立即返回 `parameter_error`。
4. 调用 `validate_minimum_profile()`；失败立即返回。
5. 构造 deep plan。
6. 调用 `IntentRouter.route()`；只捕获 `ValueError` 并转成 `parameter_error`。
7. 构建 research capability routes。
8. 才开始写 artifact 和调用 provider。

`src/smart_search/service.py:1267-1339`

前三种早退均不会写 `00-plan.json` 或 `summary.json`。

### 10.2 minimum profile

默认 `SMART_SEARCH_MINIMUM_PROFILE="standard"`；允许 `standard`、`off`。`standard` 必须满足：

```text
main_search
docs_search
web_fetch
```

`off` 的 required 列表为空。`web_search` 和 `vertical_search` 不属于 minimum profile。`src/smart_search/config.py:15-27`、`src/smart_search/service.py:1801-1820`

capability 配置链为：

```text
main_search: xai-responses → openai-compatible
web_search: zhipu → zhipu-mcp → tavily → firecrawl
docs_search: context7 → exa
web_fetch: tavily → jina → zhipu-mcp-reader → firecrawl
vertical_search: anysearch
```

`src/smart_search/service.py:164-172`、`src/smart_search/service.py:1532-1594`

因此 `research()` 要求有 `main_search` 配置，即使其运行时阶段本身不调用 main-search provider。预检发生在 plan、路由和 artifacts 之前。`src/smart_search/service.py:1287-1312`

### 10.3 `plan_intent_signals` 如何进入 IntentRouter

research 固定调用：

```python
await IntentRouter(config).route(
    question,
    validation_level="balanced",
    allow_remote=True,
    plan_intent_signals=plan["intent_signals"],
)
```

`src/smart_search/service.py:1314-1320`

rules route 对 plan signals 的消费为：

- `docs_api_intent` 可令 `docs_intent=true`；
- `locale_domain_scope=="china"` 可令中文当前意图成立；
- `recency_requirement in {"recent","current"}` 可令 web-current 成立；
- `known_url` 可令 fetch intent 成立；
- 其余 plan signals 通过 `setdefault` 合并进 router 的 `intent_signals`。`src/smart_search/intent_router.py:118-188`

router mode：

- `off`：直接返回空路由，不运行 rules；
- `rules` 或 `allow_remote=False`：只返回 rules；
- `hybrid`：先 rules，再按配置串行尝试 embeddings 和 classifier；远程组件未配置或异常会记录在 route 的 `degraded/degraded_reason`，保留 rules 结果。`src/smart_search/intent_router.py:340-487`

classifier 只能输出 capability；provider 选择会被忽略。其 `web_search` 增补还受 rules 的时效、严格验证、交叉验证、风险或 fetch signal 约束。`src/smart_search/intent_router.py:202-213`、`src/smart_search/intent_router.py:443-465`

当 router mode 为 `off` 时，`_research_capability_routes()` 会用该空 `route_result` 的 `docs_intent/web_current_intent/fetch_intent` 覆盖 planner 对应信号，因而这三项变成 false。`src/smart_search/intent_router.py:374-380`、`src/smart_search/service.py:751-756`

---

## 11. research route signals 与 capability routes

### 11.1 `_research_route_signals()`

初始 signals：

| 字段 | 来源 |
|---|---|
| `docs_api_intent` | plan signals 加本地 rules |
| `official_low_noise_intent` | `DEEP_EXA_DISCOVERY_KEYWORDS` |
| `current_or_locale_intent` | rules 的 `web_current_intent` |
| `known_url` | rules 的 `fetch_intent` |
| `pdf_or_arxiv_intent` | `pdf`、`arxiv`、`论文`、`paper`、`.pdf` |
| `js_heavy_intent` | `js-heavy`、`javascript`、`dynamic`、`动态页面`、`浏览器渲染`、`登录页`、`cloudflare`、`screenshot`、`ocr`、`扫描` |
| `vertical_intent` | rules 的 vertical signal |
| `claim_risk` | plan，默认 `medium` |
| `cross_validation_need` | plan，默认 `normal` |
| `raw_query` | question 的小写文本 |

`src/smart_search/service.py:151-163`、`src/smart_search/service.py:726-741`

有 `route_result` 时覆盖：

- `docs_api_intent`
- `current_or_locale_intent`
- `known_url`
- `vertical_intent`：router signal 为真，或 `required_capabilities` 含 `vertical_search`

其余本地/plan signals 不被覆盖。`src/smart_search/service.py:751-756`

vertical rules 词来自 capability catalog，覆盖 CVE/security、finance、legal、academic/paper、repo/codebase、gaming guide、travel itinerary 及对应中文词。`src/smart_search/intent_catalog.py:116-126`

### 11.2 capability provider 链

| capability | route 选择 |
|---|---|
| `web_search` | 当前/地区意图：`zhipu, zhipu-mcp, tavily, firecrawl`；否则：`tavily, firecrawl, zhipu, zhipu-mcp` |
| `docs_search` | 默认 `context7, exa`；只有 official-low-noise 且不是 docs/API intent 时改为 `exa, context7` |
| `web_fetch` | JS-heavy：`firecrawl, tavily, jina, zhipu-mcp-reader`；PDF、arXiv、已知 URL：`jina, tavily, zhipu-mcp-reader, firecrawl`；其余使用配置顺序 |
| `vertical_search` | 仅 vertical intent 时为 `anysearch`，否则空；标记 `experimental=true` |

未配置的 provider 会先被删除。`src/smart_search/service.py:710-723`、`src/smart_search/service.py:779-809`

`routing_decision` 顶层包括：

```text
signals
fallback_mode
route_policy_version
invalid_provider_overrides
capabilities
intent_router_mode
required_capabilities
intent_signals
confidence
router_engines_used
degraded
degraded_reason
reasons
```

后八项在存在 `route_result` 时从其结果复制。每个 capability 至少记录 `providers`、`reason`；vertical 另有 `experimental`。`src/smart_search/service.py:758-811`

### 11.3 preferred/disabled overrides

直接配置键：

```text
SMART_SEARCH_RESEARCH_PREFERRED_PROVIDERS
SMART_SEARCH_RESEARCH_DISABLED_PROVIDERS
```

解析规则为逗号分隔、trim、小写、去重、保序、忽略空值。`src/smart_search/config.py:580-597`

路由层处理顺序：

1. 只承认 `PROVIDER_PROFILES` 中的 provider；
2. 未知值写入 `invalid_provider_overrides`；
3. 删除 disabled；
4. 删除不支持该 capability 的 provider；
5. 按 preferred CSV 顺序把允许的 provider 前置；
6. 其余 provider 保持原顺序追加。

`src/smart_search/service.py:682-707`

实际调用 helper 后还存在以下组合语义：

- web search 的 provider CSV 会被 `_parse_provider_filter()` 转成集合；helper 再按固定配置顺序 `zhipu → zhipu-mcp → tavily → firecrawl` 过滤。因此 `routing_decision.web_search.providers` 的重排顺序不直接成为 helper 的迭代顺序；disabled 通过集合成员过滤仍生效。`src/smart_search/service.py:1823-1833`、`src/smart_search/service.py:2053-2073`
- fetch helper 会把 `preferred_order` 中的 provider放前面，然后把所有其余已配置 fetch provider 追加回来。因此 route 层删除的 disabled fetch provider在 `auto` 下仍可能出现在后续执行列表；如果 route 列表为空，helper 直接使用全部已配置 fetch provider。`src/smart_search/service.py:1983-2004`
- docs 阶段直接遍历 route list，顺序和 disabled 过滤会直接生效。`src/smart_search/service.py:1361-1407`
- vertical 阶段用 route list 判断是否启用，但调用 helper 时未传 providers/fallback；当前只有 `anysearch`。`src/smart_search/service.py:1446-1458`、`src/smart_search/service.py:2205-2279`

---

## 12. `research()` 阶段控制流

### 12.1 `known_url_fetch`

- 对 query 中提取出的全部 URL 按出现顺序逐个执行。
- URL 提取会清理末尾标点，但不去重。
- 每个 URL 调 `_run_web_fetch_fallback()`；失败不会中断后续 URL。
- 成功产生 evidence 和 Markdown artifact；失败增加 `sq1` gap。
- 不存在成功后 break。`src/smart_search/intent_router.py:86-92`、`src/smart_search/service.py:1341-1360`

`stage_results`：

```json
{
  "stage": "known_url_fetch",
  "url": "...",
  "ok": true,
  "provider_attempts": []
}
```

### 12.2 `docs_discovery`

仅当 `docs_api_intent=true`。

无 docs provider 时增加 gap，随后阶段继续。`fallback=off` 只取 route 的第一个 provider；`auto` 遍历完整 route。`src/smart_search/service.py:1361-1367`

Context7 路径：

1. `context7_library(question, question)`。
2. 成功且有 results：
   - 立即记录一次成功 attempt；
   - 立即写一条成功 `docs_discovery` stage；
   - 取第一项 `id`；
   - 调 `context7_docs(library_id, question)`。
3. docs 有 content：
   - 再记录一次成功 attempt；
   - 直接生成 `source_type="docs"` evidence；
   - 写 `docs-context7.md`；
   - `break` 整个 docs-provider loop。
4. docs 无正文或失败：
   - 记录 `empty/error` attempt；
   - `fallback=off` 时 `break`；
   - `auto` 时 `continue` 到下一个 provider。
5. library 请求失败或为空时记录 attempt；循环自然进入下一个 provider。`src/smart_search/service.py:1367-1397`

Context7 的 stage success 表示 library resolution 成功；后续 docs retrieval 即使失败，该 stage 记录仍保留。

Exa 路径：

1. `exa_search(..., num_results=5, include_highlights=True)`。
2. `ok` 且规范化后 sources 非空：
   - 记录成功 attempt；
   - 添加 discovery sources；
   - 添加 stage；
   - `break`。
3. 否则记录 `empty/error` attempt，继续 provider loop。`src/smart_search/service.py:1398-1407`

成功 stage shape：

```json
{
  "stage": "docs_discovery",
  "provider": "context7 | exa",
  "ok": true,
  "result_count": 1
}
```

`research()` 没有调用仓库中另一个 `_run_docs_search_fallback()` helper；docs fallback 在函数内部手写。`src/smart_search/service.py:1361-1407`、`src/smart_search/service.py:2115-2202`

### 12.3 `web_discovery`

触发条件：

```text
current_or_locale_intent
OR cross_validation_need == "high"
OR (evidence_items 为空 AND discovery_sources 为空)
```

同时必须不是：

```text
query 含 URL AND fallback == "off"
```

`src/smart_search/service.py:1409-1413`

有 route provider 时调用 `_run_web_search_fallback(question, count=5, ...)`。helper 对 provider 串行尝试，首个返回非空规范化 sources 的 provider立即结束；empty/error 才尝试下一个。无 provider 时追加 gap。`src/smart_search/service.py:1414-1427`、`src/smart_search/service.py:2053-2112`

stage shape：

```json
{
  "stage": "web_discovery",
  "ok": true,
  "result_count": 5,
  "provider_attempts": []
}
```

即使 sources 为空也会记录该 stage，只是 `ok=false`、`result_count=0`。

### 12.4 Exa 补强

触发条件必须全部成立：

```text
fallback != "off"
official_low_noise_intent == true
docs_search route 中包含 exa
discovery_sources 中尚无 provider == "exa"
```

成功且 sources 非空时记录一次 `docs_search/exa/ok` attempt 并追加 discovery sources；provider 返回失败时记录 error attempt；返回 `ok=true` 但 sources 为空时不追加 attempt。该阶段不写 `stage_results`。`src/smart_search/service.py:1429-1444`

### 12.5 `vertical_discovery`

触发条件：

```text
vertical_intent == true
AND vertical_search route providers 非空
```

调用为 `_run_vertical_search_fallback(question)`，没有传 route providers 或当前 fallback mode。当前 helper 只有 AnySearch，首次成功、失败或异常都会立即返回。`src/smart_search/service.py:1446-1458`、`src/smart_search/service.py:2205-2279`

stage shape：

```json
{
  "stage": "vertical_discovery",
  "provider": "anysearch",
  "ok": true,
  "result_count": 0,
  "provider_result": {}
}
```

`result_count` 只计算可提升为 discovery source 的规范 HTTP(S) URL。URL 为空的 structured result 可以保留在 `provider_result` 中，但不会进入 `discovery_sources` 或 evidence。`content`、`raw_content`、`raw_result` 会从 compact provider result 移除，description 最多 300 字符。`src/smart_search/providers/anysearch.py:18-41`、`src/smart_search/service.py:581-589`

### 12.6 candidate selection 与 `candidate_fetch`

`_select_candidate_urls()`：

- 不排序；
- 保留 `discovery_sources` 的追加顺序；
- 跳过空 URL；
- 跳过 `context7:` URL；
- 按 URL 原始字符串精确去重，不做 URL canonicalization；
- 达到 limit 立即 break；
- helper 默认 limit 5，`research()` 显式使用 6。`src/smart_search/service.py:875-886`、`src/smart_search/service.py:1460`

随后：

1. 从已有 evidence URL 建 `fetched_urls`。
2. 按 candidate 顺序逐个执行。
3. URL 为空或已在 `fetched_urls` 中时 `continue`，不会补取第七个 candidate。
4. 每个新 URL 重新计算 fetch order。
5. 成功后加入 evidence，但不会停止后续 candidate。
6. `fallback=off` 的单 URL fetch 失败会产生专门 gap；`auto` 不产生逐 URL gap。
7. artifact 文件编号使用 candidate 在最多六项列表中的 index，因此被 `continue` 跳过时可能出现编号空缺。`src/smart_search/service.py:1460-1485`

stage shape：

```json
{
  "stage": "candidate_fetch",
  "url": "...",
  "ok": true,
  "provider_attempts": []
}
```

---

## 13. fallback 机制

### 13.1 `_run_web_fetch_fallback()`

配置顺序：

```text
tavily → jina → zhipu-mcp-reader → firecrawl
```

有 `preferred_order` 时先按其排序，再追加其余所有已配置 provider。`fallback="off"` 在追加完成后截取第一项。provider 串行执行：

- 成功且正文非空：记录 `ok` 并立即 return；
- 正文空：记录 `empty`，继续；
- provider 返回分类错误或抛异常：记录 `error/empty`，继续；
- 耗尽：返回 `None, attempts`。

`src/smart_search/service.py:1983-2050`

### 13.2 `_run_web_search_fallback()`

先建立固定配置顺序：

```text
zhipu → zhipu-mcp → tavily → firecrawl
```

`providers` 参数只作为集合过滤器；`fallback="off"` 再截第一项。首个非空 sources 立即 return；empty/error 继续；耗尽返回空列表。`src/smart_search/service.py:2053-2112`

### 13.3 `_run_vertical_search_fallback()`

当前只有 AnySearch。成功即返回，包括 “provider ok 但规范 URL sources 为空”；provider 失败或异常也立即返回对应结构化结果，没有第二个 provider。`src/smart_search/service.py:2205-2279`

### 13.4 `fallback=off` 与 `auto` 的运行差异

| 行为 | `auto` | `off` |
|---|---|---|
| known URL fetch | 同 capability 内继续尝试 provider | 每个 URL 只尝试 fetch helper 的第一项 |
| docs provider | 遍历 route | 只取 route 第一项 |
| web discovery provider | empty/error 后继续 | 第一项后停止 |
| query 有 URL 时 web discovery | 可按普通条件触发 | 无条件禁止 |
| Exa official 补强 | 可触发 | 禁止 |
| candidate 数量 | 最多 6，逐个 fetch | 仍最多 6；每个 candidate 只尝试首 provider |
| candidate 单 URL fetch 失败 gap | 不立即添加逐 URL gap | 添加 `fetch failed with fallback off: ...` |
| vertical | helper 默认 `auto`；当前仅 AnySearch | research 未把 `off` 传入 helper；当前仍仅 AnySearch |

`src/smart_search/service.py:1341-1485`

### 13.5 `fallback_used`

`fallback_used` 不是参数值的回显。它把所有 attempts 按 capability 分组；同一 capability 中前后 provider/model identity 变化即为 true。跨 capability 的 provider 变化不算 fallback。该判断跨阶段、跨 URL 累积，不按单次 fetch 重置，因此同 capability 在不同阶段或不同 URL 上使用不同 provider 也会得到 true。`src/smart_search/service.py:601-621`

`providers_used` 只收集 `status=="ok"` 的 provider，按首次出现顺序去重。`src/smart_search/service.py:592-598`

attempt 基础字段固定为：

```text
capability
provider
status
error_type
error
elapsed_ms
result_count
```

可附加 credential pool 的 `key_index/credential_rotated`，或 vertical 的 `operation/tool/experimental`。`src/smart_search/service.py:364-397`

Credential Pool 只在 `rate_limited` 或 `quota_exhausted` 时轮换到下一个 credential；其他失败直接返回当前结果。`src/smart_search/credential_pool.py:157-199`

---

## 14. evidence 模型

### 14.1 `_research_evidence_item()`

固定字段：

```text
id
url
title
provider
source_type
subquestion_id
content
content_len
verified
```

构造规则：

- `id = "e" + sha1(url + "\n" + provider + "\n" + title)[:12]`
- `title` 为空时回退为 URL
- `source_type` 默认 `fetched_page`
- `content_len = len(content or "")`
- `verified = bool(content and content.strip())`

ID 不包含 content、source type 或 subquestion。`src/smart_search/service.py:814-834`

三类 evidence 输入：

| 来源 | `source_type` | `subquestion_id` | title |
|---|---|---|---|
| known URL fetch | `fetched_page` | `sq1` | URL |
| Context7 docs | `docs` | `sq2` | `library_id` |
| candidate fetch | `fetched_page` | candidate 的 `subquestion_id`，通常为空 | discovery title，空则 URL |

`src/smart_search/service.py:1349-1357`、`src/smart_search/service.py:1380-1389`、`src/smart_search/service.py:1475-1483`

discovery snippets 本身不会进入 evidence。只有成功的 fetch/read 正文才生成 evidence。

### 14.2 citations

citations 按 evidence 顺序生成，按 URL 精确去重；每项只有：

```text
url
title
provider
```

多个 evidence item 可以存在，但同 URL 只产生一条 citation。`src/smart_search/service.py:837-850`

---

## 15. gap 的全部产生点

| 产生点 | 精确 `reason` | 其他字段 |
|---|---|---|
| minimum profile 失败 | `minimum profile is missing required capabilities` | `subquestion_id=""` |
| known URL fetch 失败 | `failed to fetch known URL: {url}` | `subquestion_id="sq1"`, `url` |
| docs intent 无 provider | `no configured docs_search provider for docs/API evidence` | `subquestion_id="sq2"` |
| web discovery 无 provider | `no configured web_search provider for discovery` | `subquestion_id=""` |
| candidate fetch 在 off 下失败 | `fetch failed with fallback off: {url}` | `subquestion_id=""`, `url` |
| 全程无 evidence | `no fetched/read evidence items were produced` | `subquestion_id=""` |
| 已有 evidence，但无 known URL、有 candidates，且 candidate fetch 没产生新 evidence | `discovery produced candidates but no new fetch evidence converged` | `subquestion_id=""` |

`src/smart_search/service.py:1287-1303`、`src/smart_search/service.py:1358-1359`、`src/smart_search/service.py:1363-1366`、`src/smart_search/service.py:1426-1427`、`src/smart_search/service.py:1483-1490`

以下失败没有专门的即时 gap：

- 已配置 docs provider 全部失败或为空；
- 已配置 web-search provider 全部失败；
- Exa 补强失败；
- vertical discovery 失败；
- `auto` 下某个 candidate URL fetch 失败。

这些情况可能通过最终 “无 evidence” 或 “未收敛” gap 体现。gaps 是追加式列表，没有后续按 URL 或 subquestion 删除、闭合已有 gap 的步骤。

### 15.1 covered、gap_status、stop_reason

```python
covered = bool(evidence_items)
```

这不是逐 subquestion coverage；只要至少有一个 evidence item 即为 true。`src/smart_search/service.py:1492`

| 条件 | `gap_check.status` | `stop_reason` | 顶层 `ok` | 顶层 `degraded` |
|---|---|---|---|---|
| 有 evidence，gaps 空 | `closed` | `evidence_converged` | true | false |
| 有 evidence，gaps 非空 | `degraded` | `degraded_with_gaps` | true | true |
| 无 evidence | `failed` | `provider_exhausted` | false | true |

`src/smart_search/service.py:1492-1521`

顶层 `degraded` 只取 `bool(gaps)`，不吸收 `routing_decision.degraded`。IntentRouter 的远程组件降级保留在 `routing_decision` 内。

minimum-profile 早退的 `gap_check` 只有 `status` 和 `gaps`，没有 `stop_reason`。非法 fallback 和 router `ValueError` 早退没有 `gap_check`。`src/smart_search/service.py:1276-1330`

---

## 16. `_evidence_only_synthesis()` 的确切输出

无 evidence 时返回单行：

```text
未能为 `{question}` 获取可引用的页面正文证据。本次 research 已停止在降级状态，未对缺证据的结论做断言。
```

`src/smart_search/service.py:853-858`

有 evidence 时格式为：

```text
Research result for: {question}

Evidence-backed findings:
1. {title-or-url} ({provider})
   Evidence excerpt: {excerpt}
   Source: {url}
2. ...

Unverified gaps:
- {subquestion_id}: {reason}
```

规则：

- evidence 逐项编号；
- content 先把连续空白规范为单个空格；
- excerpt 取规范化 content 的前 360 字符；
- excerpt 为空时省略 `Evidence excerpt` 行；
- `Source` 始终输出；
- 只有 gaps 非空时才输出 `Unverified gaps`；
- 最终 `.strip()`，不保留尾部换行。`src/smart_search/service.py:859-872`

该函数不调用 LLM，也不从 discovery snippet 生成新结论；`content` 与 `final_answer` 使用同一字符串。`src/smart_search/service.py:1494-1510`

---

## 17. `stage_results` 完整过程面

`stage_results` 没有统一 schema，所有 append 点如下：

| `stage` | 字段 |
|---|---|
| `known_url_fetch` | `stage`, `url`, `ok`, `provider_attempts` |
| `docs_discovery` / Context7 | `stage`, `provider="context7"`, `ok=true`, `result_count` |
| `docs_discovery` / Exa | `stage`, `provider="exa"`, `ok=true`, `result_count` |
| `web_discovery` | `stage`, `ok`, `result_count`, `provider_attempts` |
| `vertical_discovery` | `stage`, `provider="anysearch"`, `ok`, `result_count`, `provider_result` |
| `candidate_fetch` | `stage`, `url`, `ok`, `provider_attempts` |

`src/smart_search/service.py:1347`、`src/smart_search/service.py:1373`、`src/smart_search/service.py:1405`、`src/smart_search/service.py:1425`、`src/smart_search/service.py:1450-1458`、`src/smart_search/service.py:1470`

不进入 `stage_results` 的过程包括：

- minimum profile；
- plan 构造和 IntentRouter；
- docs provider 的失败/empty；
- 无 provider gap；
- Exa 补强；
- candidate selection；
- gap check；
- synthesis；
- artifact 写入。

这些过程仍可能通过 `provider_attempts`、`routing_decision`、`gaps` 或 artifacts 可见。

---

## 18. `research()` 返回 JSON

### 18.1 正常完成路径的完整顶层字段

```text
ok
error_type
error
mode
query_mode
question
budget
research_plan
routing_decision
stage_results
discovery_sources
vertical_discovery
final_answer
content
citations
evidence_items
gap_check
provider_attempts
providers_used
fallback_used
degraded
route_policy_version
evidence_dir
minimum_profile_ok
capability_status
elapsed_ms
```

`src/smart_search/service.py:1496-1527`

固定或派生语义：

- `mode="deep_research_execution"`
- `query_mode="research"`
- `budget` 为规范化值
- `research_plan` 是嵌入的完整 deep plan
- `discovery_sources` 是候选来源，不代表 evidence
- `vertical_discovery` 为 compact AnySearch provider result 或 `null`
- `content == final_answer`
- 有 evidence 时 `ok=true`，即使 `gap_status="degraded"`
- 无 evidence 时：
  - `error_type="evidence_error"`
  - `error="research could not obtain fetched evidence"`
- `route_policy_version="research-router-v1"`

### 18.2 discovery source shape

`_normalize_source_results()` 至少保留：

```text
url
provider
```

并按源数据可选保留：

```text
title
description
published_date
source
```

没有 URL 的普通 discovery result 会被删除。`src/smart_search/service.py:558-578`

### 18.3 早退 shape

非法 fallback 或 router `ValueError`：

```text
ok
error_type
error
question
mode
route_policy_version
elapsed_ms
```

`src/smart_search/service.py:1276-1285`、`src/smart_search/service.py:1321-1330`

minimum profile 失败：

```text
ok
error_type
error
question
mode
minimum_profile_ok
capability_status
final_answer
citations
evidence_items
gap_check
provider_attempts
fallback_used
degraded
route_policy_version
evidence_dir
elapsed_ms
```

该 shape 没有 `query_mode`、`budget`、`research_plan`、`routing_decision`、`stage_results`、`content`、`providers_used`。`src/smart_search/service.py:1287-1310`

### 18.4 CLI 渲染

`research --format markdown`：

```text
# Research Report

Question
Status
Route policy
Evidence dir
Fallback used
Degraded

## Answer
...

## Citations      # 仅非空时

## Gaps           # 仅非空时
```

`src/smart_search/cli.py:994-1023`

`--format content` 只输出 `content` 加换行。`--format json` 输出完整 result。`src/smart_search/cli.py:1077-1087`、`src/smart_search/cli.py:1235-1240`

CLI exit code：

- `ok=true` → 0
- `parameter_error` → 2
- `config_error` → 3
- `evidence_error` → 4
- 其他 runtime/parse → 5

因此 “有 evidence 但 degraded” 仍退出 0；无 evidence 的正常执行路径退出 4。`src/smart_search/cli.py:1295-1310`

---

## 19. artifacts 落盘全集

### 19.1 自动写入文件

| 时机 | 文件名 | 内容 |
|---|---|---|
| 路由完成、provider 调用前 | `00-plan.json` | 完整 deep plan |
| known URL fetch 成功 | `{index:02d}-fetch-{provider}.md` | 正文 |
| Context7 docs 成功 | `docs-context7.md` | docs content |
| candidate fetch 成功 | `fetch-{index:02d}-{provider}.md` | 正文 |
| 最终 result 构造完成 | `summary.json` | 完整 research result |

`src/smart_search/service.py:1339`、`src/smart_search/service.py:1356-1357`、`src/smart_search/service.py:1388-1389`、`src/smart_search/service.py:1482-1483`、`src/smart_search/service.py:1527-1529`

没有单独自动写入：

```text
web-discovery.json
exa-discovery.json
vertical-discovery.json
provider-attempts.json
gaps.json
final-answer.md
```

这些数据只位于 `summary.json` 或返回 JSON 中。

plan 中的 `01-search.json`、`02-context7-library.json` 等 `steps[].output_path` 也不会被 `research()` 执行或写入；它们只是嵌入 `00-plan.json` 的离线计划命令。

### 19.2 写入格式

`_write_research_artifact()`：

- 首次写入时 `mkdir(parents=True, exist_ok=True)`；
- 字符串按 UTF-8 原样写入；
- 非字符串使用 `json.dumps(..., ensure_ascii=False, indent=2)` 后 UTF-8 写入；
- 固定文件名在同一 evidence dir 再次运行时由 `write_text()` 覆盖。`src/smart_search/service.py:889-900`

`deep` 本身不调用该 writer。只有显式 CLI `--output` 会把渲染后的 plan 写到用户指定文件；计划中的 evidence 文件要由后续执行者运行 steps 才产生。`src/smart_search/cli.py:2594-2600`、`src/smart_search/service.py:4969-4972`

---

## 20. 执行模型、budget 与超时

### 20.1 串行性

`research()` 内没有 `asyncio.gather`、TaskGroup 或 batch 并发：

- router embeddings、classifier 顺序执行；
- known URLs 顺序执行；
- docs library、docs content 顺序执行；
- capability fallback provider 顺序执行；
- web、Exa、vertical 阶段顺序执行；
- candidate URLs 顺序执行；
- artifacts 同步写入。

`src/smart_search/intent_router.py:400-469`、`src/smart_search/service.py:1312-1529`

`deep` 只返回有序 steps。planner 不执行它们，也不规定 agent 必须并发或串行调度；skill 引导按步骤消费。

### 20.2 runtime budget

`research()` 的 budget：

- 传给 `build_deep_research_plan()`；
- 写入 `research_plan.intent_signals.breadth_depth_budget`；
- 回显到顶层 `budget`；
- 不改变 runtime 的阶段数量；
- 不改变 web count 5、Exa count 5、candidate limit 6；
- 不提供时间额度或整体 deadline。

`src/smart_search/service.py:1312-1313`、`src/smart_search/service.py:1361-1485`、`src/smart_search/service.py:1501-1504`

因此 quick 对嵌入 plan 做裁剪，但 `research(..., budget="quick")` 仍运行同一套 live 阶段。

### 20.3 超时

`research` CLI 和函数没有总 timeout 参数，也没有整体 `wait_for`。总耗时为实际串行调用、provider retry 和 credential rotation 的累计。最终只记录 `elapsed_ms`。`src/smart_search/service.py:1267-1529`

主要请求超时：

| 路径 | as-is timeout |
|---|---|
| IntentRouter embedding/classifier | `INTENT_ROUTER_TIMEOUT_SECONDS`，默认 8 秒，每个远程调用分别使用 |
| Exa、Context7、Zhipu、Zhipu MCP、Jina、AnySearch | 对应 `*_TIMEOUT_SECONDS`，默认 30 秒；通常为 connect 6、read 配置值、write 10 |
| Tavily extract | 固定 HTTP 60 秒 |
| Tavily search | 固定 HTTP 90 秒 |
| Firecrawl search | 固定 HTTP 90 秒 |
| Firecrawl scrape | 固定 HTTP 90 秒；payload `timeout=60000` ms |

`src/smart_search/config.py:15-23`、`src/smart_search/config.py:615-642`、`src/smart_search/config.py:762-833`、`src/smart_search/config.py:897-898`、`src/smart_search/intent_router.py:525-569`、`src/smart_search/service.py:2282-2451`

`TAVILY_TIMEOUT_SECONDS` 属性默认 30，但 research 实际使用的 Tavily search/extract helper采用上述固定 90/60 秒。`src/smart_search/config.py:614-616`、`src/smart_search/service.py:2288`、`src/smart_search/service.py:2328`

Exa、Context7、Zhipu 使用 `retry_max_attempts + 1` 次的 retry；默认 `SMART_SEARCH_RETRY_MAX_ATTEMPTS=3`，即最多 4 次，并使用随机指数等待。Firecrawl scrape 的空正文循环次数是 `retry_max_attempts`，默认 3。`src/smart_search/providers/exa.py:192-207`、`src/smart_search/providers/context7.py:217-233`、`src/smart_search/providers/zhipu.py:130-142`、`src/smart_search/service.py:2409-2445`、`src/smart_search/config.py:375-385`

---

## 21. research 相关配置与 policy version

直接影响 research 编排的配置：

| 配置 | 默认/语义 |
|---|---|
| `SMART_SEARCH_MINIMUM_PROFILE` | `standard`；可为 `standard/off` |
| `SMART_SEARCH_RESEARCH_PREFERRED_PROVIDERS` | provider CSV，按 capability 重排 |
| `SMART_SEARCH_RESEARCH_DISABLED_PROVIDERS` | provider CSV，路由层删除 |
| `SMART_SEARCH_INTENT_ROUTER` | 默认 `hybrid`；可为 `hybrid/rules/off` |
| `INTENT_EMBEDDING_*` | hybrid embedding route |
| `INTENT_CLASSIFIER_*` | hybrid classifier route |
| `INTENT_ROUTER_TIMEOUT_SECONDS` | 默认 8 |
| 各 provider key/URL/timeout | 决定 configured provider 与单请求 timeout |
| `SMART_SEARCH_RETRY_MAX_ATTEMPTS` | 默认 3 |
| `SMART_SEARCH_RETRY_MULTIPLIER` | 默认 1 |
| `SMART_SEARCH_RETRY_MAX_WAIT` | 默认 10 |

`src/smart_search/config.py:15-53`、`src/smart_search/config.py:54-93`

CLI 的 `research --fallback` 不读取 `SMART_SEARCH_FALLBACK_MODE`；CLI 默认值直接传入函数。`SMART_SEARCH_FALLBACK_MODE` 是其他搜索入口使用的通用配置。`src/smart_search/cli.py:3157-3159`、`src/smart_search/service.py:1267-1275`

策略版本：

```text
RESEARCH_ROUTE_POLICY_VERSION = "research-router-v1"
```

消费位置只有：

- `routing_decision.route_policy_version`
- 正常结果顶层 `route_policy_version`
- 三类结构化早退结果

它不参与条件分支。`src/smart_search/service.py:149`、`src/smart_search/service.py:758-763`、`src/smart_search/service.py:1277-1329`、`src/smart_search/service.py:1522`

---

## 22. skill / agent 引导层

### 22.1 主 skill 的分流指引

`skills/smart-search-cli/SKILL.md` 要求 agent：

- 普通检索使用 `search`；
- live Deep Research 使用 `research`；
- `research` 不传 caller capability declaration；
- 显式 deep/multi-source 请求才读取 `deep-research-mode.md`；
- claim-level evidence 必须来自 fetched page content，而不是 discovery candidate。`skills/smart-search-cli/SKILL.md:14-21`

跨分支 invariant 进一步规定：

- 高风险或时效事实先 fetch；
- `research` 可自动执行 domain-less AnySearch Vertical Discovery；
- URL-less structured vertical result保留在 `vertical_discovery`，不作为 source/evidence。`skills/smart-search-cli/SKILL.md:50-55`

### 22.2 deep/manual execution 指引

`deep-research-mode.md` 明确教 agent：

1. `smart-search deep` 是 offline planner。
2. `smart-search research` 是 live executor。
3. 手动模式先运行 `deep --format json`。
4. 检查 `intent_signals`、`decomposition`、`capability_plan`。
5. 执行所有 planned `search` 和适用的专项 steps。
6. 对关键 URL 执行 `fetch`。
7. 执行 `gap_check`；缺正文证据时继续 fetch，或标成 unverified。`skills/smart-search-cli/references/deep-research-mode.md:33-50`

其 evidence boundary 为：

- `primary_sources`、`extra_sources` 只是候选；
- fetched page text 才能支持 claim；
- final answer 应包含 fetched evidence、unverified candidates 和关键命令。`skills/smart-search-cli/references/deep-research-mode.md:50`

live 命令示例：

```powershell
smart-search research "question" --budget deep --fallback auto \
  --evidence-dir "<evidence-dir>" --format json --output "research.json"
```

引导文本将 `auto` 描述为同 capability fallback，将 `off` 描述为每个 capability 只尝试第一个 selected provider。`skills/smart-search-cli/references/deep-research-mode.md:160-170`

agent 被要求消费的 research 输出包括：

```text
final_answer
citations
evidence_items
gap_check
provider_attempts
fallback_used
degraded
route_policy_version
evidence_dir
```

并禁止把 unfetched discovery candidates 当作 proof。`skills/smart-search-cli/references/deep-research-mode.md:168-183`

`cli-core.md` 还列出了 deep 和 research 的完整稳定输出字段。`skills/smart-search-cli/references/cli-core.md:79-80`

### 22.3 引导文本与源码同时存在的表面

- skill reference 的 allowed step tool 文本列出 8 项：`search`、`exa-search`、`exa-similar`、`zhipu-search`、`context7-library`、`context7-docs`、`fetch`、`map`。源码 `DEEP_ALLOWED_TOOLS` 为 13 项，额外含五个 `zhipu-mcp-*`；当前 generator 实际生成 7 项，不生成 `map` 或 `zhipu-mcp-*`。`skills/smart-search-cli/references/deep-research-mode.md:138-142`、`src/smart_search/service.py:65-79`
- reference schema 列出 `locale_domain_scope="mixed"`、`claim_risk="low"`，并写复杂 plan 可有 2～6 个子问题；当前 planner 不产生 `mixed/low`，无 URL deep 当前补到 4。`skills/smart-search-cli/references/deep-research-mode.md:63-71`、`skills/smart-search-cli/references/deep-research-mode.md:140`、`src/smart_search/service.py:996-1025`、`src/smart_search/service.py:1161-1176`
- 引导文本将 `fallback=off` 表述为第一个 selected provider；源码在 docs 路径符合该描述，web/fetch 的实际执行还受到各 fallback helper 重建顺序的影响。`skills/smart-search-cli/references/deep-research-mode.md:168`、`src/smart_search/service.py:1983-2112`

公开 skill 树 `skills/smart-search-cli/` 与打包镜像 `src/smart_search/assets/skills/smart-search-cli/` 由 regression test 要求文本完全同步。`tests/test_regression.py:320-333`

`docs/agents/` 下没有 smartsearch deep/research 的调用或输出消费指引；唯一的 `research` 是 issue 类型标签。`docs/agents/issue-tracker.md:36-43`

README 的用户层说明同样把 `deep` 定义为拆解后由用户/AI 分步执行，把 `research` 定义为完整 live evidence flow。`README.zh-CN.md:202-249`

---

## 23. 测试与内建 smoke 体现的契约

单元测试明确覆盖：

- planner 不调用 live provider；市场 query 得到高风险、Zhipu、fetch-before-claim，且不无条件加入 Exa。`tests/test_service.py:365-397`
- 默认 evidence dir 位于平台临时目录，所有 step 的 command/output path 一致；无 supplemental capability 时使用 `--capabilities none`。`tests/test_service.py:400-417`
- deep docs query 至少四个 decomposition，Context7 在 Exa 之前；非 official docs 不加 Exa。`tests/test_service.py:419-460`
- URL-first 的第一 step 是 fetch；quick 最多四步并保留 fetch，所有 step 都引用有效 subquestion。`tests/test_service.py:463-503`
- 即使配置 hybrid remote router，deep planner 也不调用 embedding/classifier。`tests/test_intent_router.py:356-374`
- route 结果中的 docs、Zhipu、Jina、Firecrawl、AnySearch 优先顺序，以及 override capability boundary。`tests/test_service.py:543-593`
- `fallback_used` 只检查同一 capability 内的 provider identity 变化。`tests/test_service.py:595-606`
- staged research 只有 fetched body 进入 evidence/citation，并生成 `summary.json`。`tests/test_service.py:609-638`
- discovery candidates 全部 fetch 失败时，`ok=false`、citations/evidence 为空、gap status 为 `failed`。`tests/test_service.py:641-672`
- known URL 加 `fallback=off` 时只尝试首 fetch provider，并跳过 web discovery。`tests/test_service.py:675-695`
- docs 加 `fallback=off` 时不运行 supplemental Exa。`tests/test_service.py:697-729`
- vertical discovery 是无 domain/sub-domain 的调用；URL-less structured result保留在 `vertical_discovery`，不进入 discovery/evidence；无 AnySearch credential 时不运行。`tests/test_service.py:2187-2242`
- CLI 原样传递 budget、evidence dir、fallback；Markdown/content renderer 保留 answer 与 citations。`tests/test_cli.py:612-680`

`smart-search smoke --mock` 还直接调用 planner 和 route helpers，内建 current-market、docs、claim-verification、URL-first、provider routing、same-capability fallback 等 cases。其局部 `deep_allowed_tools` 集合为 skill 文本中的 8 项。`src/smart_search/service.py:4501-4519`、`src/smart_search/service.py:4670-4857`

`smart-search regression` 在源码 checkout 中运行包含上述测试的 pytest 列表；打包环境缺少测试文件时回退到 mock smoke。`src/smart_search/cli.py:2873-2895`

---

## 两模式分工对照表

| 维度 | `deep`：agent 编排 | `research`：引擎编排 |
|---|---|---|
| CLI | `deep` / `dr` | `research` / `rs` |
| 默认 budget | `standard` | `deep` |
| 核心入口 | `build_deep_research_plan()` | `async research()` |
| 执行性质 | 同步、离线、只生成计划 | 异步函数、live provider 执行 |
| minimum profile | 不校验；只在 `preflight` 中给出 `doctor` 指引 | provider 调用前强制 `validate_minimum_profile()` |
| IntentRouter | 只用本地 `build_rules_route()` 辅助 signals，不调用远程 route | 复用 plan signals，再调用 `IntentRouter.route(..., allow_remote=True)` |
| planner 共享 | 自身即 planner | 首先调用同一个 planner，完整嵌入 `research_plan` |
| 是否执行 plan steps | 否 | 否；runtime 使用 plan signals 和独立阶段代码 |
| known URL | planner 只围绕第一个 URL 建 2 个 subquestions、3 个 steps | 按 query 出现顺序抓取全部 URL |
| decomposition | 生成 subquestions 和 capability plan | 回显 planner decomposition；runtime 不按 subquestion 循环执行 |
| vertical search | planner 不声明 `vertical_search`，不生成 AnySearch step | IntentRouter 选中 vertical 后可自动执行 AnySearch |
| budget 影响 | quick 裁剪 subquestions/steps；影响 search `extra_sources` | 只影响嵌入 plan；不裁剪 runtime stages、candidate limit 或 provider 次数 |
| provider route | plan 中写现有 CLI command | `_research_capability_routes()` 按配置、signals、override 生成 route |
| fallback | 不执行；后续 agent按各 command 行为处理 | `auto/off` 控制 docs、web、fetch，并影响 Exa 补强和 known-URL web-discovery |
| 阶段模型 | 输出有序 `steps[]`，实际调度属于 agent/用户 | 固定串行阶段：known URL → docs → web → Exa → vertical → candidates |
| evidence | `evidence_policy="fetch_before_claim"`，由执行者落实 | 只有 fetch/read 正文生成 `evidence_items` 和 citations |
| gap | 输出规则和 `unsupported_claim_action` | 运行时产生结构化 gaps、三态 status 和 stop reason |
| synthesis | 不执行 | `_evidence_only_synthesis()`，不调用 LLM |
| 自动 artifacts | 无；只有显式 `--output` | `00-plan.json`、成功 fetch/docs Markdown、`summary.json` |
| 默认 evidence dir | 平台临时目录下的 timestamp/slug，仅写入 plan | 复用同一目录规则，并在执行时创建目录 |
| 完成状态 | `ok=true` 的 plan | evidence 非空即 `ok=true`；无 evidence 为 `evidence_error` |
| route policy version | plan 不含 | `research-router-v1` |
| 主要消费方 | AI agent 或用户读取 `steps`、执行命令、做 gap check | CLI 调用者读取最终 answer、evidence、gaps、attempts 与 artifacts |