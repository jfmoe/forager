# SSRN 搜索质量实测：Crossref 与 browser

2026-09-28 13:37:38–13:40:20 UTC，在同一台机器上执行 30 个查询、60 次搜索。全程只调用 `platform ssrn search`，没有调用 fetch、打开论文详情或下载 PDF。

本轮结果：Crossref 更快，且多数结果直接带摘要；browser 多命中一个预先指定的论文 ID，并提供原站发布日期。两条路线返回的候选差异很大，不能将其中一条视为另一条的等价替代。

## 数据与方法

- [完整搜索响应](ssrn-2026-09-28/search-comparison.json)、[汇总](ssrn-2026-09-28/summary.json)、[采集脚本](ssrn-2026-09-28/search-comparison.py)、[统计脚本](ssrn-2026-09-28/summarize.py)。
- 查询集沿用 [2026-09-26 的实验](2026-09-26-ssrn-route-comparison.md)：20 个金融、法律、经济主题，10 个已知论文查询。每次最多取前 20 条，不翻页。
- 使用仓库已有的 `target/debug/forager`（版本输出 0.6.0，支持 SSRN），而非 PATH 中尚无 SSRN 子命令的安装版本。
- 每次通过 `FORAGER_PLATFORMS__SSRN__ORDER` 固定为一条路线，禁止两条路线之间 fallback。按查询交替执行顺序：偶数查询 Crossref 先运行，奇数查询 browser 先运行；所有命令串行。
- browser 使用 OpenCLI 1.8.6、现有 Chrome，设置 `OPENCLI_SITE_SESSION=persistent` 与 `OPENCLI_WINDOW=foreground`。会话在实验前已成功读取过论文，本轮不是冷启动试验。
- 每次命令预算 120 秒。延迟为进程启动到退出的墙钟时间，包含限速与命令启动开销；p95 使用最近秩法。browser 的单并发和至少 5 秒操作间隔保持不变。
- 采集脚本在 browser 返回 Auth 时暂停后续 browser 请求，保留失败记录并继续 Crossref。本轮未触发该条件。
- 预设目标 ID 沿用前次实验，没有通过额外 fetch 重新核验。命中只按指定 SSRN ID 判定；相同标题的不同 ID 不自动视为同一条目。

## 已知论文命中与排序

“未命中”仅表示目标 ID 未进入前 20，不能据此断言数据库没有收录。

| 查询 | 目标 ID | Crossref 排名 | browser 排名 |
|---|---|---:|---:|
| betting against beta | 2049939 | 4 | 9 |
| gross profitability premium | 1598056 | 未命中 | 5 |
| quality minus junk | 2312432 | 1 | 1 |
| value and momentum everywhere | 2174501 | 2 | 2 |
| time series momentum | 2089463 | 未命中 | 未命中 |
| five-factor asset pricing model | 2287202 | 1 | 未命中 |
| fact fiction momentum investing | 2435323 | 1 | 2 |
| factor momentum and the momentum factor | 3014521 | 1 | 2 |
| dual momentum risk premia harvesting | 2042750 | 1 | 1 |
| momentum has its moments | 2041429 | 未命中 | 1 |

| 指标（10 个查询） | Crossref | browser |
|---|---:|---:|
| 首位命中 | 5/10 | 3/10 |
| 前 5 命中 | 7/10 | 7/10 |
| 前 10 命中 | 7/10 | 8/10 |
| 前 20 命中 | 7/10 | 8/10 |
| MRR@20 | 0.575 | 0.481 |

MRR@20 为每个查询目标排名倒数的平均值，未命中计 0。browser 在这个小样本里命中范围略大，Crossref 将命中的目标排得更靠前。两条路线合并后可命中 9/10，但本轮没有实现合并或重排。

`value and momentum everywhere` 在两条路线中的首条均为同标题的 `ssrn:1363476`，预设目标 `ssrn:2174501` 位列第二；这说明按指定 ID 评分比按论文概念评分更严格。没有读取详情，因此不判断这两个 ID 的版本或内容关系。

## 主题搜索的标题匹配

取 20 个主题查询的前 5 条，共 200 个结果位置。同一查询下相同标题去重后有 172 个标题，随机排列并隐藏路线、排名、摘要与片段，进行模型标题盲评。它不是人工标注，也不是论文正文相关性评估。评分为：2 分表示标题直接覆盖核心主题；1 分表示部分相关或信息不足；0 分表示标题明确偏离。`private credit` 按私募信贷资产类别理解。

评估只消费 [标题输入](ssrn-2026-09-28/title-review-input.json)，[逐条评分与理由](ssrn-2026-09-28/title-ratings.json) 可用于复核。相同标题使用同一评分，再映射回各路线的实际结果位置；因此同标题不同 ID 仍占不同结果位置。

| 前 5 条标题匹配（每条路线 100 个位置） | Crossref | browser |
|---|---:|---:|
| 直接覆盖主题 | 95 | 89 |
| 部分相关或标题信息不足 | 4 | 11 |
| 明确偏离 | 1 | 0 |

按每个查询的直接匹配数比较：Crossref 在 4 个查询领先，browser 在 1 个查询领先，15 个查询相同。这个差异不足以证明整体检索质量优劣，只说明本样本中 browser 没有呈现全面的标题匹配优势。

| 查询 | Crossref 直接匹配数 / 5 | browser 直接匹配数 / 5 |
|---|---:|---:|
| dual momentum | 5 | 3 |
| merger arbitrage | 5 | 4 |
| private credit | 4 | 5 |
| insider trading enforcement | 5 | 3 |
| venture capital returns | 4 | 2 |

具体例子：

- `dual momentum`：browser 前两条为 *Unraveling Momentum's Moments* 和 *Momentum, Size and Value Factors versus Systematic Co-Moments in Stock Returns*，标题涉及动量，但没有明确双动量；Crossref 前五条标题均明确包含双动量。标题不足不能推出论文内容不相关。
- `private credit`：Crossref 首条是 *Corruption, trade credit, and bank credit in private firms*，标题指向私人企业的贸易与银行信贷，偏离本次采用的私募信贷含义；browser 前五条标题都直接涉及 private credit。
- `venture capital returns`：browser 第 2–4 条是三个不同 ID、同名 *Efficient Venture Capital Market* 的条目。标题没有显示回报主题；没有读取详情，不能断言三条正文重复。

## 结果重合度

按每个查询的 SSRN ID 集合比较前 20 条：

- 30 个查询的重合条目次数合计 186，每个查询中位数 6，范围 1–15。
- Crossref 独有 414 次，browser 独有 403 次。这是按查询累计的条目次数，不是全局去重论文数。
- 两边每个单页内均无重复 ID。跨查询去重后，Crossref 有 592 个 ID，browser 有 582 个 ID。
- 低重合不等于一方结果差；需要结合查询相关性判断。原站与 Crossref 的索引和排序并不等价。

## 搜索结果的信息量

| 字段或深度 | Crossref（600 条） | browser（589 条） |
|---|---:|---:|
| 非空摘要 | 415（69.2%） | 0 |
| 非空片段 | 0 | 589（100%） |
| 作者 | 600 | 589 |
| 发布日期 | 600，仅年份 | 589，完整年月日 |
| 实际深度 | abstract 415；metadata 185 | snippet 589 |

browser 的 `dual momentum risk premia harvesting` 只返回 9 条，其他查询均返回 20 条。少于 limit 不自动算失败。

摘要和片段均由搜索响应直接提供，没有额外 fetch。对于只搜索后就要筛选文献的流程，Crossref 的摘要是实际优势；browser 的原站 Posted 日期更适合按发布日期做判断。两类日期含义不同，不用它们直接比较“谁收录更新”。

## 延迟与可用性

| 指标 | Crossref | browser |
|---|---:|---:|
| 成功 | 30/30 | 30/30 |
| 失败 | 0 | 0 |
| p50 | 1.58 秒 | 3.79 秒 |
| p95 | 2.31 秒 | 5.08 秒 |

browser 延迟包含访问节奏等待，不能将两列之差全部归因于网页加载。全程约 2 分 42 秒。

本轮没有记录到 Auth 或其他失败，但没有逐次采集挑战页状态，也没有设置临时会话对照组。因此，这组结果不能证明全程没有短暂验证、持久会话降低了挑战概率，或 browser 可以长期无人值守。

## 适用范围

这是单台机器、单个已可用浏览器会话、30 个英文查询的现场样本。已知论文集集中于资产定价，不代表整个 SSRN；没有完整相关论文集合，不能计算主题搜索的真实召回率。未评估正文质量、下载成功率、修订版本一致性或新论文收录延迟。

默认检索优先 Crossref 有合理依据：延迟较低，搜索结果通常已有摘要。需要原站候选或精确 Posted 日期时，browser 有增量价值。当前平台 order 的语义是失败后补位，不会在 Crossref 搜索成功后再搜索 browser 并合并结果；若目标是提高发现范围，需要显式搜索两条路线、按 SSRN ID 去重，再比较候选。
