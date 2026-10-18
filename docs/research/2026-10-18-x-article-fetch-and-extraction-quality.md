# X 文章的读取方式，以及 Jina 与 Tavily 的抓取质量

核验日期：**2026-10-18**。本机 forager 0.14.0 与未发布的 #232 构建；Web Fetch 请求直接调用三家的官方端点，请求参数与 forager 相同（[`probe.py`](x-article-fetch-2026-10-18/probe.py)）。逐次请求的状态、耗时、长度、标志内容计数与短摘录见 [`summary.json`](x-article-fetch-2026-10-18/summary.json)；文章属于个人作者，仓库不保存全文。

## 结论

1. **`platform x fetch` 不读 X 文章（Articles）。** `XRef` 只有 `post` 一种，`/<handle>/article/<id>`、`/i/article/<id>` 与 `/status/<id>/article` 都在飞行前退 2。文章 id 可以当作帖子 id：`platform x fetch x:<文章 id>` 两篇都返回了文章正文，但这是模型转述，代码块、表格与部分编号列表缺失。
2. **`forager fetch <文章 URL>` 能读到全文，交付者是 Tavily。** Firecrawl 云端对 x.com 一律返回 403 `we do not support this site`，与请求参数无关；默认链因此落到 Tavily。
3. **X 文章上 Jina 的结果最好。** 两篇文章中，Jina 保留了 Tavily 的全部内容，另外保留了完整的表格、正确换行的代码块、发布日期与 Tavily 缺失的句子。Tavily 把代码块压成重复的一行、清空了表格单元格，同一 URL 两次返回的版本不同。
4. **Jina 也能读 X 帖子页的逐字原文**，并带父帖与部分回复，可用于核对 `platform x fetch` 转述的正文。
5. **一般网页上两家各有短板，没有全面胜出者。** Jina 召回更高、结构保留更好，但带入大量导航与页脚；Tavily 正文更干净，但会静默丢掉代码块与表格内容。唯一带统计的独立基准上 Tavily 综合分更高，领先来自杂项排除。

## X 文章实测

样本：本机 journal 中出现过的两篇文章。

- A：`https://x.com/cjtong_X/article/2070043847749771285`，含代码块与表格的技术长文。
- B：`https://x.com/HelloMrYEE/article/2041336912842227756`，以段落和列表为主。

| 路径 | A | B |
| --- | --- | --- |
| Firecrawl（`onlyMainContent` 开或关） | 403，0.6 秒 | 未测（同站点） |
| Tavily `basic` | 11529 字符，0.8 秒 | 3184 字符 |
| Tavily `advanced` | 与 `basic` 逐字相同 | 与 `basic` 逐字相同 |
| Jina 默认 | 9130 字符，3.6 秒 | 3324 字符，4.2 秒 |
| Jina `x-engine: browser` | 与默认相同 | 与默认相同 |
| `platform x fetch x:<id>` | 4424 字符，模型转述 | 2826 字符，模型转述 |

文章 A 的逐项对照：

| 内容 | Tavily | Jina | `platform x fetch` |
| --- | --- | --- | --- |
| 订阅日志引用块 | 有（另一次 forager 调用拿到的 7221 字符版本缺失） | 有 | 有 |
| 代码块（如 `sing-box version`） | 每行重复、换行丢失，压成一行 | 原样，换行与缩进正确 | 大多缺失 |
| "最终状态"表格 | 结构在，表头与两行单元格为空 | 完整 | 缺失 |
| "解码出来的内容让我愣住了""网络配置原封不动"两句 | 缺 | 有 | 部分 |
| 发布日期 | 缺 | 有 | — |

文章 B：Jina 多出发布日期与最后一句，其余一致。

其他观察：

- Tavily 的 `extract_depth` 由 forager 固定为 `basic`，但 `advanced` 在两篇上结果相同，没有可改善的配置。
- Firecrawl 的拦截来自服务端的站点黑名单（`firecrawl/firecrawl` 的 `apps/api/src/scraper/WebScraper/utils/blocklist.ts`，2026-10-17 的 main）：云端黑名单存于数据库，只有 Firecrawl 给团队设置 `unblockedDomains` 才放行；自托管版只在 `USE_DB_AUTHENTICATION` 开启且未设 `DISABLE_BLOCKLIST` 时加载黑名单。自托管能否在未登录时渲染 x.com 未验证。
- Jina 读帖子页（`/status/<id>`）返回 4–5 千字符：标题为"`<作者> on X: "<原文>"`"，正文逐字，并附父帖与部分回复，开头有登录与注册链接。
- 单次覆盖 Web Fetch 链顺序可用环境变量：`FORAGER_CAPABILITIES__WEB_FETCH__ORDER='["jina"]' forager fetch '<文章 URL>'` 实测由 `jina` 交付。

## 一般网页：基准与社区讨论

### 量化基准

| 来源 | 独立性 | Jina | Tavily Extract |
| --- | --- | --- | --- |
| [arlen/bench web extraction](https://arlenkumar.com/bench/leaderboards/web-extraction)：WCXB 语料 150 页，20% 人工复核 | 个人维护；厂商审阅尚未完成 | 综合 0.54；关键句召回 0.59；杂项排除 0.26；覆盖率 99.3% | 综合 0.62；关键句召回 0.54；杂项排除 0.68；覆盖率 98.7% |
| [primitive-bench eval-extraction](https://github.com/primitive-bench/primitive-bench/blob/64f15963/packages/eval-extraction/README.md)：关键词留存 | 与上一项同一作者 | 约 100% | 约 100% |
| [WebCrawlerAPI](https://webcrawlerapi.com/benchmarks/scrape-quality) | 厂商；只测 Jina | F1 0.184，召回高，成功率 86.8%；低 F1 归因于返回整页导航 | — |
| [TinyFish](https://www.tinyfish.ai/blog/how-we-evaluate-fetch-quality-for-ai-agents)：45 个 URL | 厂商；只测 Tavily | — | 成功 36/45；一篇 Daily Mail 文章返回 4.6 万字符、正文占 17% |

arlen/bench 按页面类型的综合分：Tavily 在 6 类领先，"服务"类两家接近（文章 0.69 对 0.59，文档 0.64 对 0.52）。综合分权重为标题 0.2、关键句召回 0.5、杂项排除 0.3；Jina 召回更高，失分来自杂项。该基准没有公开 Jina 的请求头。[Kanopy Labs 的对比文章](https://kanopylabs.com/blog/firecrawl-vs-jina-reader-vs-tavily-ai-data-extraction)没有公开方法，且把 Tavily 当作搜索片段而没有测试 Extract，不采用。

### 社区讨论

- **Jina Reader。** HN 上它常被推荐为"在 URL 前加 `r.jina.ai/`"的默认做法；抱怨集中在 JS 很重的页面（[hn:46934568](https://news.ycombinator.com/item?id=46934568)、[reader#1148](https://github.com/jina-ai/reader/issues/1148)）、反爬站点（[reader#1158](https://github.com/jina-ai/reader/issues/1158)，维护者表示不对抗验证码）与偶发空正文（[reader#105](https://github.com/jina-ai/reader/issues/105)）。[reader#1237](https://github.com/jina-ai/reader/issues/1237)（2026-03 起，未关闭）报告大陆网络无法直连。[SanityWebEval](https://github.com/lemon07r/SanityWebEval) 因按 token 计费较贵而默认不测 Jina。X 上有人分享用 Jina 读 X 页面（[x:2035369626188816868](https://x.com/i/status/2035369626188816868)），与本次实测一致。
- **Tavily Extract。** 公开讨论大多关于搜索；官方社区在 Discord，无法抓取。官方文档与 skill 要求检查 `failed_results`，因为 HTTP 200 不代表每个 URL 都成功；[claude-code#7349](https://github.com/anthropics/claude-code/issues/7349) 报告 basic 模式不执行 JS，动态图片只得到 blob 地址。
- 本仓库此前的实测与此一致：[2026-09-26 对比](2026-09-26-firecrawl-tavily-fetch-quality.md)中 Tavily 丢掉 Python 教程全部 35 个代码块，HN 讨论串输出为原文的 2.63 倍；ADR 0018 记录 Jina 在 arXiv HTML 上只成功 3/8。

## 未覆盖

- X 文章只测了两篇；Jina 是否对所有文章都优于 Tavily 未验证。
- 独立的量化数据只来自一位作者的两个项目。
- Reddit 上的讨论无法读取：Jina 对 reddit.com 返回网络安全拦截页，与 #217 的记录相同。
- Jina 的 `x-respond-with: readerlm-v2` 是否减少杂项、实际计费倍数，未测试。
