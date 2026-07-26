# 1. 功能取舍清单

权威来源：[#53 Resolution](https://github.com/jfmoe/smartsearch/issues/53)。基线：`main @ 11ac647`（0.7.1）。后续票对本清单的修订见文末「修订留痕」。

## 砍掉

1. **Zhipu 全家**：`zhipu.py`、`zhipu_mcp.py`、`zhipu-search` 与 5 个 `zhipu-mcp-*` 命令、全部 config 键、skill 文档段落。唯一实质损失是中文时效补强变弱——接受降级，不找替代（中文时效大头由主搜索 grok 覆盖）。
2. **中文补强意图路由**：`zh_current_intent`、`locale_domain_scope=china` 信号与词表；web_search 补强链固定 `[tavily → firecrawl]`，不按语言/地域重排。时效信号保留（驱动 research 是否补跑 web discovery，不驱动 provider 排序）。
3. **Intent routing 三层**（rules 词表 / embeddings / classifier）、`route`、`route-calibrate`、embedding presets、标定管线（约 350 行）。
4. **`deep` 独立命令**：计划能力收进 `research` 本体。
5. **自研 skill 子系统**：`skills install/status/update/clear`、`skill_installer.py`、`skill_sync.py`、Automatic Skill Sync、`setup` 的 skill 步骤 → 改用 `npx skills add jfmoe/forager`（vercel-labs/skills）。代价（已接受）：版本同步改手动（skill 文档标注适配版本）。连带：ADR-0001 废止；CONTEXT.md 四术语废止（见第 6 章）。
6. **`diagnose`**（探测并入 `doctor --provider`）；**`regression`**（自测归 cargo test / CI，行为基线由验收契约接任）。
7. **`anysearch-extract`**（无消费方）、**`anysearch-batch`**（循环 `anysearch search` 可替代）。
8. **`model` 命令**（`config` 全覆盖）。

## 新增 / 重构

9. **分类器 LLM**：独立配置（自有 url/keys/model，任意模型），替代三层路由。
   - skill 调用（有 caller）：search 按 Caller Capability Declaration（权威）；research 由 caller 注入计划（`--plan`，见第 2 章）。
   - 裸调用：分类器为 search 判定能力集合；为 research 产出 intent_signals + 子问题分解（URL 提取保留为确定性解析）。
   - openai-compatible 通道专职 main search fallback，不得复用于分类/计划。
   - 降级：分类器未配置时 search 优雅降级为默认 Web 链；research 裸调用退 3（config_error）。分类器**已配置但失败**的降级语义见第 4 章（含 research 侧固定最小降级 plan，第 5 章 H8）。
   - Intent Routing Catalog 的「能力身份 + 语义例句」保留，作为分类器 prompt 与 skill 契约共同来源。
10. **Provider Credential Pool 升级为唯一凭据体系**：8 provider 全入池（含 xai、openai_compatible），单凭据是池的退化情形；配置面统一为 `keys` 真数组（第 3 章）；轮换触发条件按 provider 声明。
11. **Search Result Journal 增强**：记录面从终态业务结果扩展到执行过程（provider 尝试链、耗时、分类器决策、错误分类），修订 ADR-0002（第 6 章）。

## 保留（概念平移）

12. 其余全部保留：
    - **8 个 provider**：exa、tavily、firecrawl、jina、context7、anysearch、openai-compatible、xai-responses。
    - **主搜索管线语义**：主链 `[xai-responses → openai-compatible]`、`--capabilities`、`--validation`、fallback 模式、`--extra-sources`、来源规范化。
    - **`research` 一体化入口**（吸收计划能力；证据目录约定降格为 skill 文档约定）。
    - **直连命令六件套**：fetch、map、exa search、exa similar、context7 library、context7 docs。
    - **AnySearch 验收面瘦身版**：`anysearch search`（双形态）+ `anysearch domains` + Verified Domain Manifest，晋升通道完整。
    - **doctor**（吸收 diagnose 探测）、**smoke**（内容重定义见第 5 章）。
    - **config path/list/set/unset**、**setup**（配置面收缩见第 3 章）。
    - **openai-compatible model 断路器**：连续 2 败熔断 600s；进程内（第 4 章）。
    - Capability Seam 词汇表（`--capabilities` 校验、分类器 prompt、skill 契约共源）。

## 修订留痕（后续票对 #53 的修订）

- **minimum profile 门禁不迁移**（[#57 补充决议 R1](https://github.com/jfmoe/smartsearch/issues/57#issuecomment-5078205945)）：原第 12 条把 minimum profile 门列入管线语义基线，经重新评估砍除。继任机制为 **capability 缺口自报**——同一判定谓词（某 seam 的 order 链 ∩ 有凭据者 = ∅）从「拦死」改「自报」：结果 JSON `capability_gaps` 字段 + stderr 警告一行。字段形状见第 4 章，验收基线相应调整见第 5 章。
- **空结果语义切分**（#59 补充决议② B1）：退 5（evidence_error）只属于证据管线；直连 search 类命令的合法空结果集 = Ok / 退 0。
