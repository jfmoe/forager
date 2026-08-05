# 2. CLI 接口

权威来源：[#56 Resolution](https://github.com/jfmoe/smartsearch/issues/56) 及其[补充决议](https://github.com/jfmoe/smartsearch/issues/56#issuecomment-5076846003)。

## 定名

项目 / binary 名 **forager**（脱离上游 fork 网络的独立身份；crates.io / GitHub 撞名验证通过）。

## 命令面（11 顶层）

```
forager search QUERY [--capabilities CSV|none] [--model ID] [--extra-sources N]
                     [--validation fast|balanced|strict] [--fallback auto|off]
                     [--timeout 180] [--format json|markdown|content] [--output FILE] [--verbose]
forager research QUERY [--plan FILE|-] [--budget quick|standard|deep]
                       [--evidence-dir DIR] [--fallback auto|off] [--timeout 600] [...]
forager fetch URL [--timeout N] [...]
forager map URL [--instructions S] [--max-depth 1] [--max-breadth 20] [--limit 50] [--timeout 150] [...]
forager exa search QUERY [--num-results 5] [--search-type neural|keyword|auto]
                         [--include-text] [--include-highlights] [--start-published-date D]
                         [--include-domains ...] [--exclude-domains ...] [--category NAME] [...]
forager exa similar URL [--num-results 5] [...]
forager context7 library NAME [QUERY] [...]
forager context7 docs LIBRARY_ID QUERY [...]
forager anysearch search QUERY [--domain D --sub-domain S] [--sub-domain-params JSON] [--max-results 5] [...]
forager anysearch domains [DOMAIN] [...]
forager doctor [--provider PROVIDER] [--timeout 30] [--format json|markdown]
forager smoke [--live] [...]
forager config path|list|set|unset
forager setup [--non-interactive] [--lang zh|en]
```

- **分界规则**：点名 provider 的命令按 provider 分组嵌套（exa/context7/anysearch）；操作语义 + fallback 链的按操作命名保持顶层（fetch、map）。裸动词＝智能管线，provider 前缀＝旁路直连。
- **别名六槽**（全部 visible_alias）：`s`=search、`f`=fetch、`rs`=research、`c7`=context7、`as`=anysearch、`ls`=config list。关闭 clap `infer_subcommands`。

## 输出与退出码

- `--format json(默认)/markdown/content` 三态；**content 收窄**到四个正文命令（search/fetch/context7 docs/research），per-command ValueEnum 在解析层强制。doctor 默认 json。
- `--output FILE` 为 **tee 语义**（写文件 + stdout 照常）。写失败＝非零终态退 3，stdout JSON 照常输出并标注写失败（#59 H15）；与 journal 旁路（非致命）区分。
- 退出码：`0` 成功（含直连命令的合法空结果）；`2` 参数错（clap 天然 + 坏 plan + `config set` 非法路径）；`3` config_error（含未知文件键、未知 `FORAGER_*` env、web_fetch 空链、`--output` 写失败）；`4` transport 族终态；`5` content 族终态（quality/evidence；**evidence_error 由 4 改 5**）。`1` 空缺；panic 101 不拦，为非契约异常出口。
- **默认 stdout 瘦载荷**（#59 I0-6 连带修订本票）：成功＝结果本身；失败＝`error_kind` + 一行 message + attempts 计数摘要（total/by_kind/providers，非全文）+ 精简 capability_gaps + `journal_ref`（nullable，写失败置 null 并附 `journal_status`）。全量 `provider_attempts` 移出默认 stdout、只落 journal；`--verbose` 为 inline 全量逃生阀。载荷上限与截断规则见第 5 章 M18。
- **fetch 成功载荷**：`content` 只包含 provider 无关的 Markdown 正文；provider attempts 与 diagnostic 保持在各自字段/输出通道，不混入正文。URL 与 PDF 共享 `web_fetch` 链和失败语义，`--output` 仍是 tee。

## search 参数清理

- **砍** `--providers`（链序权威归配置）、`--platform`（伪过滤器）、`--stream/--no-stream`（持久开关走配置键，临时覆盖走 env）。
- 留 `--model`、`--extra-sources`、`--validation`、`--fallback`。
- **`--timeout` 横切**所有网络命令：search 180 / research 600 / doctor 探测 30 / fetch、map 补齐。

### `--timeout` 语义（补充决议 A2）

整条 CLI 命令的 **hard deadline**：实现可为单次 attempt 设更短上限，但所有重试、fallback 与探针共享总预算；超时结果保留已完成 attempts（journal 全量 + stdout 摘要，#59 B2）。预算保留规则（保证 fallback 可达）见第 4 章。

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

## 契约③：`doctor --provider`

两档：`doctor` 浅检全体（掩码配置 + 凭据存在 + 可达性 + 过宽权限报告 + config list 同构生效值块）；`--provider NAME` 深探单体，值域＝8 provider 编译期 enum。8 个 provider 均执行凭据有效性 + 最小活体调用；openai-compatible 额外保留 stream/no-stream 双形状判定。

## 收尾

- `smoke [--live]`：默认离线档；内容定义见第 5 章。
- `--sub-domain-params` 保留内联 JSON 单对象 + serde 严格校验。
- `setup` 只留 `--non-interactive`/`--lang`；键面见第 3 章。
