# SSRN 两条 route 的检索对比

实测日期：2026-09-26（UTC 约 14:20–14:25）。对比对象是 `ssrn_crossref`（匿名 Crossref REST）和 `ssrn_browser`（经本机 OpenCLI 1.8.6 驱动用户的 Chrome，读取 SSRN 原站）。本文只陈述观测结果，不给出总体成功率，也不据单台机器、单日的结果推断长期稳定性。

原始数据：[route-comparison.json](ssrn-2026-09-26/route-comparison.json)（每次运行的条目、延迟与错误）、[route-comparison-freshness.json](ssrn-2026-09-26/route-comparison-freshness.json)（近期论文的 Crossref 核对）。采集脚本：[route-comparison.py](ssrn-2026-09-26/route-comparison.py)。

## 方法

- **查询集**：20 个金融、法律、经济方向的主题查询，以及 10 个已知论文查询（查询词是用户可能输入的简称，目标 SSRN id 事先核对过）。近期论文不单独出题，而是从主题查询结果中取 Posted 日期在近 30 天内的论文。
- **运行方式**：用 `FORAGER_PLATFORMS__SSRN__ORDER` 把 order 固定为单条 route，每个查询依次跑两条 route，串行执行，`--limit 20`。浏览器 route 每次只读取一个 50 条的原生结果页，所以 20 条来自同一页。
- **延迟**：forager 进程从启动到退出的墙钟时间，包含限速等待。已知的等待间隔是 Crossref 每秒 1 次、浏览器每 5 秒 1 次。两条 route 交替运行，前一条 route 的耗时通常已经覆盖了本 route 的间隔，因此这组数据几乎不含限速等待。连续调用同一条浏览器 route 时，每次至少间隔 5 秒。
- **新鲜度**：近 30 天指 2026-08-27 及以后。浏览器条目的日期是 SSRN 结果卡片上的 Posted 日期（原站事实）。Crossref 条目的 `published` 只有年份，不能用来判断近 30 天，因此单独列出，只用浏览器卡片核对其中能对上的条目。

## 结果

### 已知论文的召回

| 查询 | 目标 | Crossref 名次 | 浏览器名次 |
|---|---|---|---|
| betting against beta | 2049939 | 4 | 9 |
| gross profitability premium | 1598056 | 未进前 20（Crossref 没有该 DOI） | 5 |
| quality minus junk | 2312432 | 1 | 1 |
| value and momentum everywhere | 2174501 | 1 | 2 |
| time series momentum | 2089463 | 未进前 20 | 未进前 20 |
| five-factor asset pricing model | 2287202 | 1 | 未进前 20 |
| fact fiction momentum investing | 2435323 | 1 | 2 |
| factor momentum and the momentum factor | 3014521 | 1 | 2 |
| dual momentum risk premia harvesting | 2042750 | 1 | 1（该页只有 9 条结果） |
| momentum has its moments | 2041429 | 未进前 20（Crossref 标题为 "Managing the Risk of Momentum"） | 1 |

- 前 10 召回：Crossref 7/10，浏览器 8/10；前 20 召回相同。
- 两条 route 漏掉的论文不同。Crossref 漏掉的两篇，一篇没有 DOI 记录，另一篇的 Crossref 标题与 SSRN 当前标题不同。浏览器漏掉 Fama-French 五因子论文，这时原站按模糊检索排序。

### 前 20 条的重合度

- 30 个查询的重合 id 共 184 条；每个查询的中位数为 6 条，范围 1–15 条。
- 只在 Crossref 出现的有 416 条，只在浏览器出现的有 405 条。
- 两条 route 的结果大部分不同，不能互相替代作为召回来源。

### 字段覆盖

| 字段 | Crossref（600 条） | 浏览器（589 条） |
|---|---|---|
| 摘要 | 416（69%） | 0（检索结果只有片段） |
| 片段 | 0 | 588 |
| 作者 | 600 | 589 |
| 日期精度 | 全部只有年份 | 全部为完整日期（Posted） |
| 条目深度 | `abstract` 416，`metadata` 184 | `snippet` 588，`metadata` 1 |

浏览器 route 的 fetch 能读取详情页的完整摘要，以及 Posted、Last revised、Date Written（例如 `ssrn:3014521`：Posted 7 Aug 2017，Last revised 20 Mar 2021，Date Written December 9, 2020）。

### 新鲜度（近 30 天）

- **浏览器**：589 条中 9 条（9 篇不同论文）的 Posted 日期在近 30 天内。
- **这 9 篇在 Crossref 中的情况**：8 篇有 DOI 记录，`crossref_created` 与 SSRN Posted 日期相差 0–1 天。另一篇 `ssrn:3692151` 是旧 id，Posted 日期为 2026-09-16，Crossref 没有它的记录。
- **Crossref 的 2026 年条目**：共 75 篇，`published` 只有年份。其中 26 篇能在浏览器结果中找到 Posted 日期，这 26 篇里 6 篇在近 30 天内。其余 49 篇没有独立核实的日期，不计入新鲜度。

### 失败类型与延迟

| | Crossref（30 次） | 浏览器（30 次） |
|---|---|---|
| 失败 | 0 | 0 |
| 延迟 p50 | 2.8 秒 | 2.6 秒 |
| 延迟 p95 | 3.9 秒 | 3.0 秒 |

同一天较早时，浏览器 route 曾因 SSRN 显示需要人工勾选的 Cloudflare 验证而以 Auth 失败。当时每次 attempt 等到读取截止点才结束，约 80 秒。用户在 Chrome 中手动通过验证后才完成本次实验。这一失败没有计入上表。

## 结论

- 两条 route 的已知论文召回接近，但漏掉的论文不同，前 20 条大部分不重合。在 order `[ssrn_crossref, ssrn_browser]` 下，浏览器 route 只在 Crossref 失败或缺摘要时补位，不会合并两边的结果。
- Crossref 的优势是检索结果自带摘要（69%）、不依赖本机浏览器、没有站点验证风险。它的日期只到年份，也覆盖不到没有 DOI 或 DOI 标题已过时的论文。
- 浏览器 route 能提供原站排序、完整的 Posted 日期和详情页日期字段，检索结果只有片段。它依赖本机 OpenCLI 和已通过站点验证的 Chrome，站点验证升级时需要人工处理。
- 对近期论文，Crossref 的 DOI 登记与 SSRN 发布基本同步（8 篇中相差 0–1 天）。但 Crossref 只给年份，调用方无法据此判断论文是不是近 30 天发布的。
