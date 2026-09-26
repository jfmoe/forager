# X 社区的学术检索项目推荐：Google Scholar、SSRN、arXiv

检索日期：2026-09-25（America/Los_Angeles）。重点检索 2025–2026 年的 X 原帖，核对原帖正文、项目链接和项目文档；日期按 UTC 记录。平台 API、CLI 和源码的基础核验见[接入调研](2026-09-25-scholarly-agent-access.md)。

## 结论

**有明确的项目推荐和使用自述，但三个渠道的证据强度不同。**本次最值得跟进的是多源 `paper-search-mcp`、arXiv 的 `arxiv-mcp-server` 和 alphaXiv MCP。Google Scholar 可确认 `scholarly` 的使用自述和 SerpApi 的接入建议；SSRN 没有找到有说服力的专用抓取项目使用反馈。

“使用自述”指发帖者声称使用过，不代表本文复现了其结果；“社区转荐”指推荐项目但未给出具体测试；“项目发布”是维护者或供应商自己的公告。本文不据账号身份猜测商业关系，也不把点赞、转发或 GitHub stars 当成功率。

```text
X 原帖与回复 → 确认实际项目 → 核对源码或官方文档 → 判断是否值得试用
```

## 推荐项目与原帖证据

| 渠道 / 项目 | 已读取的 X 证据 | 证据性质 | 判断 |
| --- | --- | --- | --- |
| 多源：[openags/paper-search-mcp](https://github.com/openags/paper-search-mcp) | [Priyanshu Priyank，2026-08-08](https://x.com/PriyanshuP1405/status/2086010803157975091) 表示已使用数日，适合寻找量化、ML/DL 文献；[同作者回复](https://x.com/PriyanshuP1405/status/2086010895751434514)给出仓库 | 使用自述，已核对回复短链接指向 | 与投资研究需求最贴近的使用信号；支持 CLI、Skill、MCP，但各数据源仍须单独验收 |
| 同上 | [Quant Morales，2026-04-16](https://x.com/Quant_Morales/status/2044830683399598393) 推荐在 Claude 内检索和下载论文，举例机器学习因子择时；[回复](https://x.com/Quant_Morales/status/2044841266815369346)明确链接仓库 | 量化研究场景转荐；不能把“刚发现”升级成长期使用验证 | 确认有与本任务相近的社区关注，不证明 SSRN 或 Scholar 抓取可靠 |
| 同上 | [GitHubDaily，2025-09-01](https://x.com/GitHub_Daily/status/1962478042342678612) 推荐统一检索 arXiv、PubMed、Google Scholar 等来源 | 项目介绍与转荐；短链接已解析到该仓库 | 提供中文社区线索；帖文对 PDF 下载的总体描述不能套用每个来源 |
| arXiv：[blazickjp/arxiv-mcp-server](https://github.com/blazickjp/arxiv-mcp-server) | [Bryan，2026-08-07](https://x.com/bryann2k_dev/status/2085808726783795251) 明确写出项目名和作者 Joe Blazick，推荐在 coding agent 中查询、筛选和读取论文 | 社区转荐；短链接已解析到该仓库 | 适合现成的本地 MCP 阅读流程；项目还有真正的 Skill 文件 |
| arXiv：[alphaXiv MCP](https://www.alphaxiv.org/docs/mcp) | [Yehez，2026-09-13](https://x.com/YehezGun/status/2099112358039912481) 用印尼语表示该 MCP 帮助自己写论文时做系统性文献阅读 | 使用自述；属于效率体验，不是系统综述质量验证 | 比只见发布公告更有依据，值得加入语义发现与阅读候选 |
| 同上 | [Elliot Arledge，2026-03-24](https://x.com/elliotarledge/status/2036276405391204641) 表示正用 alphaXiv MCP 制作深度学习性能、训练稳定性和硬件相关 skills | 具体 agent 工作流的使用自述 | 说明它已被用于为 agent 构建研究知识，而不只是网页浏览 |
| Google Scholar：[scholarly](https://github.com/scholarly-python-package/scholarly) | [Sarvesh Gharat，2026-09-22](https://x.com/SarveshGharat12/status/2102484886317826113) 澄清自己用的是 scholarly Python 库来获取 Scholar 信息 | 使用自述 | 确认有用户使用；原帖没有给出规模、错误率或持续运行结果 |
| Google Scholar：[SerpApi Scholar](https://serpapi.com/google-scholar-api) | [Andrey Kruglyak，2026-06-24](https://x.com/theuniverseson/status/2069772430642495532) 建议通过 SerpApi 的 Scholar engine 获取结果，并提醒作者去重问题 | 接入建议；原帖未明确给出个人部署记录 | 可作为工程方案线索。帖中关于“多数人”的说法没有统计依据，不采纳为市场份额结论 |
| SSRN 专用项目 | 使用 `SSRN scraper`、`SSRN API`、`SSRN MCP` 等检索，未取得可核验的近期专用项目使用推荐 | 证据缺口 | 本次不能给出“X 社区验证过的 SSRN 专用抓取器”名单；不等于此类项目不存在 |

## alphaXiv MCP：值得新增评估的候选

[alphaXiv 官方于 2026-03-17 发布 MCP](https://x.com/askalphaxiv/status/2034003206217601375)，宣传关键词、向量和多轮论文检索。它是 alphaXiv 提供的第三方服务，不是 arXiv 官方 API。

当前[官方 MCP 文档](https://www.alphaxiv.org/docs/mcp)确认：

- 服务入口为 `https://api.alphaxiv.org/mcp/v1`，使用 Streamable HTTP；支持 OAuth 2.1，也支持账号创建的 API key。
- `discover_papers` 将关键词、语义问题与多轮检索结合，用于发现相关论文。
- `get_paper_content` **默认可能返回 AI 生成的中间报告**；要读取原文，应显式指定 `fullText: true`。
- `answer_pdf_queries` 返回与问题相关的页级正文；另有 GitHub 文件读取等研究工具。
- 研究工具会消耗账号的 assistant quota；文档没有给出可用于本次比较的完整免费额度与价格数字。

本次只核实原帖和文档，没有登录、安装或运行 alphaXiv MCP，也没有测量它对量化金融论文的召回质量。已有使用自述主要来自学术和深度学习场景，不能直接外推金融覆盖。

**建议**：把它作为“用研究问题发现文献、按问题取原文”的候选，与 arXiv 官方字段检索和本地阅读工具比较。若目标是精确、可复现地列出某分类或作者论文，官方 API 仍是清楚的起点。

## 社区推荐不能替代的技术边界

### 多源推荐不等于 SSRN 已可稳定抓取

`paper-search-mcp` 当前源码包含 SSRN connector，但读取的是网页入口，README 将 SSRN 标为受 403 影响、下载与阅读尽力支持。上述量化用户的原帖并未声称验证了 SSRN；将二者拼接成“用户证明 SSRN 可用”会超出证据。[SSRN 实现](https://github.com/openags/paper-search-mcp/blob/808e462a824ce6b26fdccbed352b4bf47d7b84cb/paper_search_mcp/academic_platforms/ssrn.py)、[项目能力表](https://github.com/openags/paper-search-mcp#platform-capability-matrix)

接入调研中的实际结果仍需保留：Crossref 成功取得 SSRN 双动量论文的元数据，同篇 SSRN 页面直接请求返回 403；`paper-search` 的 Crossref 查询成功，但一次 arXiv 查询返回空数组且无结构化错误。X 的推荐没有消除这些限制。

### “arxiv mcp”不能自动归到某个仓库

[Vishant Shah 的 2026-09-09 原帖](https://x.com/VishantShah10/status/2097578467206513079)表示自己常用 arXiv MCP 配合 Claude Code 探索想法，但实际阅读时改用 NotebookLM，原因是阅读界面体验不好。该帖没有给出具体项目地址，因此本文只将其视为对这类工作流的反馈，不作为 `blazickjp/arxiv-mcp-server` 的正面或负面评价。

### 有人在用 scholarly，不代表无需处理阻断

`scholarly` 维护者明确提醒部分 Scholar 查询可能被阻断；SerpApi 文档也明确其 Scholar 服务是搜索结果抓取。X 用户的使用自述不会使这些封装变成 Google 官方 API。[scholarly README](https://github.com/scholarly-python-package/scholarly)、[SerpApi 文档](https://serpapi.com/google-scholar-api)

## 针对投资文献工作的试用顺序

1. **统一 CLI / MCP：`paper-search-mcp`。**社区中有量化研究场景和使用自述，优先测试 arXiv、Crossref；对 Scholar、SSRN 分开验收。
2. **arXiv 本地读取：`blazickjp/arxiv-mcp-server`。**适合论文下载、章节阅读和本地缓存，社区帖子明确对应到该仓库。
3. **语义发现与按问题阅读：alphaXiv MCP。**有使用自述，且官方文档确认了所需能力；先测量化金融覆盖、原文获取和额度成本。
4. **Scholar 特有的排序、引用链和版本：SerpApi；Python 抓取试验可参考 scholarly。**本次 X 证据支持“有用户使用或建议”，不支持两者成功率排名。
5. **SSRN：继续以 Crossref 元数据加原始页面/作者版本为起点。**没有足够的 X 证据将专用 scraper 提升为首选。

本次是项目发现与证据核验，不是 X 全站普查，也不是性能基准。公开抓取得到的原帖正文与部分回复足以支持表中有限判断，但无法保证评论覆盖完整、没有遗漏项目，或发帖者不存在未披露关系。
