# SSRN browser 高级搜索验收（2026-09-28）

环境：仓库构建的 forager、OpenCLI 1.8.6、Chrome Browser Bridge，adapter 契约 `forager-ssrn/3`。全部操作为 search；没有打开论文详情、调用 fetch 或下载 PDF。

## 原站能力核验

核验来源为[原站搜索页](https://papers.ssrn.com/searchresults.cfm?term=dual+momentum)及页面加载的[官方搜索脚本](https://cdn.ssrn.com/ssrn-advanced-search-mfe/static/js/main.js)。该脚本的搜索请求仅构造 text、text_fields、search_mode、sort_by、page、authors、date。

| 条件 | 结论 |
|---|---|
| 搜索范围 | title、title-abstract-keywords、title-abstract-keywords-fulltext；全文范围仍只返回卡片 |
| 匹配 | Fuzzy、Boolean；原站 Help 明列 AND、OR、NOT、括号，组合表达式现场成功 |
| 作者 | Author(s) 独立文本框，Enter 提交 authors；没有身份选择控件。仅标题+Boolean 的 dual momentum 查询中，Antonacci 返回 ssrn:2042750，Gary Antonacci 返回空，不承诺精确全名匹配 |
| 日期 | all_time、last_week、last_month、last_3_months、last_6_months、last_year、last_2_years、last_3_years |
| 排序 | relevance；downloads、approval_date、title 各升降序。相关性无反向选项 |
| 机构 | 页面无独立控件，搜索请求没有相应字段；browser 不支持 |
| 文献类型 | 页面无独立控件，搜索请求没有相应字段；browser 不支持 |
| 有摘要 | 页面无独立控件，搜索请求没有相应字段；browser 不支持 |
| 任意起止日期 | 只有日期预设，无自定义日期控件或请求字段；browser 不支持 |

## 真实搜索

以下 18 次请求经完整 forager CLI → route → OpenCLI → Chrome 执行。验收用临时 command wrapper 把会话 flag 改为 persistent/foreground/keep-tab，以便验证会话残留；没有更改持久配置，生产默认仍为 ephemeral/background/关闭 tab，默认 deadline 与跨进程限速保持不变。每次响应同时检查控件、结果页 URL、已完成原站请求的全部条件以及页码和结果范围。

| 用例 | CLI 条件（默认 limit=10） | 实际范围/状态 | 首条 ref |
|---|---|---|
| author | `dual momentum --scope title --mode boolean --author Antonacci` | Displaying results 1 to 1 of 1 | ssrn:2042750 |
| default_after_author | `dual momentum` | Displaying results 1 to 50 of 10000 | ssrn:3447702 |
| fulltext | `dual momentum --scope full-text --mode boolean` | Displaying results 1 to 50 of 10000 | ssrn:2042750 |
| boolean_operators | `(momentum OR reversal) AND portfolio NOT quantum --scope title --mode boolean` | Displaying results 1 to 35 of 35 | ssrn:1349701 |
| all-time | `momentum --date all-time --sort relevance --order desc` | Displaying results 1 to 50 of 10000 | ssrn:3447702 |
| last-week | `momentum --date last-week --sort posted --order desc` | Displaying results 1 to 50 of 124 | ssrn:7526838 |
| last-month | `momentum --date last-month --sort posted --order asc` | Displaying results 1 to 50 of 605 | ssrn:7362406 |
| last-3-months | `momentum --date last-3-months --sort downloads --order desc` | Displaying results 1 to 50 of 1893 | ssrn:2801856 |
| last-6-months | `momentum --date last-6-months --sort downloads --order asc` | Displaying results 1 to 50 of 3481 | ssrn:6741338 |
| last-year | `momentum --date last-year --sort title --order asc` | Displaying results 1 to 50 of 5526 | ssrn:6875234 |
| last-2-years | `momentum --date last-2-years --sort title --order desc` | Displaying results 1 to 50 of 8751 | ssrn:5867404 |
| last-3-years | `momentum --date last-3-years --sort relevance --order desc` | Displaying results 1 to 50 of 10000 | ssrn:5701713 |
| switch_sort_asc | `dual momentum --scope title --mode boolean --sort posted --order asc` | Displaying results 1 to 14 of 14 | ssrn:581323 |
| switch_sort_desc | `dual momentum --scope title --mode boolean --sort posted --order desc` | Displaying results 1 to 14 of 14 | ssrn:7436501 |
| empty | `zzqxforagernomatch827392 --scope title --mode boolean` | no_results | — |
| page1 | `momentum --scope title --mode boolean --sort downloads --limit 50` | Displaying results 1 to 50 of 1675 | ssrn:2042750 |
| page2 | `上一页 cursor；恢复 title、boolean、downloads desc、limit 50` | Displaying results 51 to 100 of 1675 | ssrn:741244 |
| page3 | `上一页 cursor；恢复 title、boolean、downloads desc、limit 50` | Displaying results 101 to 150 of 1675 | ssrn:517822 |

所有命令退出 0。默认查询恢复空作者、Fuzzy、默认范围、All Time、Relevancy；同查询 Posted 升序与降序的首条分别为 ssrn:581323 与 ssrn:7436501。三个连续原生页分别显示 1–50、51–100、101–150。空查询命中的是原站 `No results.` 提示，且已完成请求仍匹配完整条件。

逐次实际状态见[验收记录](ssrn-browser-advanced-2026-09-28.json)。

## 证据边界

搜索结果会随收录变化；10,000 是页面显示上限，不代表精确全库数量。本次证明参数实际到达原站、状态与卡片对应，不独立证明全文索引覆盖率、作者姓名归一化或每个相关性分数。原站控件或请求变化时状态校验会失败，需要重新核验。

独立 agent-browser 会话首次打开被站点显示 Content Blocked，因此停止该会话请求；OpenCLI 的用户 Chrome 会话瞬时安全验证自行通过。没有自动点击验证或绕过访问控制。

另一次直接使用生产默认 ephemeral/background/keep-tab=false 的 CLI 验收成功返回 `ssrn:2042750`（dual momentum、title、boolean、Antonacci、limit 1），没有通过 wrapper 修改会话行为。
