# Web Fetch 默认顺序改为 Firecrawl → Jina → Tavily，Jina 无可用 key 时匿名运行

Web Fetch 的默认顺序从 `Firecrawl → Tavily → Jina` 改为 `Firecrawl → Jina → Tavily`。本记录只替换 ADR 0018 中的默认顺序；Firecrawl、Tavily 与 Jina 的请求配置、薄正文门、截断、链预算的平均切分，以及"不存在按内容类型或域名的顺序"的规则都不变。直接 fetch、research 取证、search-side Web Fetch 与平台全文仍共用同一条链。

同时，Jina 的凭据改为可选：`providers.jina.keys` 为空时以匿名方式请求，有 key 但凭据池以认证、额度或限流失败告终时，再匿名请求一次（Anonymous Fallback）。凭据池沿用现有规则：额度与限流时换用其他 key，认证失败不换 key，直接结束。

## 依据

- Firecrawl 失败（不支持的站点、超时、额度耗尽、正文过薄）后，第二家决定交付的正文。forager 交付原文证据，静默丢失内容比多带页面杂项危害更大。
- 2026-10-18 的 X 文章实测（`docs/research/2026-10-18-x-article-fetch-and-extraction-quality.md`）：Firecrawl 云端对 x.com 一律返回 403；两篇文章中 Jina 保留了 Tavily 的全部内容，另外保留了完整表格、换行正确的代码块与 Tavily 缺失的句子。Tavily 把代码块压成重复的一行、清空表格单元格，同一 URL 两次返回的版本不同。
- 2026-09-26 的对比（`docs/research/2026-09-26-firecrawl-tavily-fetch-quality.md`）中 Tavily 丢掉了 Python 教程全部 35 个代码块。
- 独立基准（arlen/bench web extraction）中 Jina 的关键句召回高于 Tavily，失分来自杂项。
- Jina Reader 支持不带 key 的调用（官方限额每 IP 每分钟 20 次，带 key 为 500 次）。2026-10-18 实测匿名读取 `doc.rust-lang.org` 成功。此前没有 key 的 Jina 被当作未配置而跳过，key 全部失效或额度用尽时 Jina 直接失败，而匿名调用本可以读到正文。

## 匿名兜底的规则

- 只有注册为"凭据可选"的 provider 使用匿名兜底；目前只有 Jina。凭据可选的 provider 恒视为已配置，因此默认 order 下即使没有任何 Web Fetch key，`forager fetch`、research 与平台全文也不会在飞行前以"没有已配置的 provider"退 3。
- 只有凭据池以 Auth、QuotaExhausted 或 RateLimited 终止时才匿名再试；网络错误、超时、参数错误与薄正文与 key 无关，不触发。每次 Jina 执行最多一次匿名请求，匿名 429 不重试、交给链上的下一家，不为匿名请求设限速。
- 匿名请求不发送 `Authorization`，其余请求头与带 key 时相同。attempt 以 `anonymous: true` 标明。
- Jina 因滥用防护拒绝匿名访问某个域名时返回 403 `AbuseAlleviationError`（2026-10-18 匿名读取 x.com 时出现），共享 status mapper 把它记为 Parameter，而不是认证失败。

## 被接受的代价

- Firecrawl 整体不可用（额度耗尽、限流、故障）时，Jina 成为一般网页的实际首选，而独立基准上 Jina 的综合分低于 Tavily（杂项多）。
- ADR 0018 的实测中 Jina 在 arXiv HTML 上只成功 3/8，arXiv 全文在 Firecrawl 失败后可能多花一次 Jina attempt 才到 Tavily。
- Jina 按 token 计费，页面杂项也计费。
- Jina 在大陆网络无法直连（jina-ai/reader#1237）。这类网络中每次落到 Jina 都多一次失败的 attempt，并占用一份链预算。
- 匿名请求共享每 IP 的限额，并可能被 Jina 按域名临时封禁；保留可用的 Jina key 更可靠。
- live smoke 的 Jina 用例（C07）仍只在有 key 时运行，并且只有带 key 的请求读到正文才算通过；匿名路径只由离线 fixture 覆盖。`doctor --provider jina` 有 key 时同样只认带 key 的请求，key 失败后匿名请求即使成功也报告 key 的失败。
- 显式设置过 `capabilities.web_fetch.order` 的配置保持原值。此前 `forager setup` 生成的模板都显式写入了旧顺序，这些用户需要手动修改才能得到新默认值。

## 重新评估的条件

- 出现 Jina 交付了错误正文却通过薄正文门的实例；
- 或 Firecrawl 失败后的 Jina 结果被证实普遍劣于 Tavily。
