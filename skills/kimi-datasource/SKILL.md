---
name: kimi-datasource
description: "Query Kimi structured data sources through a bundled CLI with Kimi Code OAuth. Use for stock quotes, fundamentals, technical indicators and A/H/US markets; World Bank or IMF macro data; Chinese company or legal records; arXiv or Scholar papers; Wind or Gildata queries; SEC filings or S&P fundamentals; and datasource authentication."
---

# Kimi datasource

Resolve this Skill directory and use the absolute path to `scripts/kimi-datasource.mjs` as `CLI`.
Require Node.js >= 18.17 and report an incomplete installation if the script is missing. Use Kimi
Code OAuth only; ask the user to run `/login` in Kimi Code when credentials are unavailable.

## 1. 这个 skill 提供什么能力

本 skill 后面挂了 12 个外部数据源。每一行的"数据源名"就是传给 CLI `desc` 和 `call` 的 source。

| 能力域 | 数据源名 | 典型问题 |
|---|---|---|
| **A股 / 港股 / 美股 行情和财务** | `stock_finance_data` | "茅台现在多少钱"、"宁德时代 2024 年财报"、"腾讯股东"、"杭州的人工智能股票" |
| **Yahoo Finance 全球金融** | `yahoo_finance` | "苹果分析师评级"、"AAPL 期权链"、"苹果前十大机构股东" |
| **世界银行历史宏观** | `world_bank_open_data` | "中国历年 GDP"、"印度通胀率"、"各国人口增长对比" |
| **中国企业工商信息** | `tianyancha` | "字节跳动股东"、"比亚迪司法风险"、"宁德时代专利" |
| **arXiv 论文预印本** | `arxiv` | "找 RAG 综述"、"下载 2406.xxxxx" |
| **Google Scholar 学术搜索** | `scholar` | "Hinton 最新论文"、"transformer 综述高引文献" |
| **中国法律法规 / 司法案例** | `yuandian_law` | "民法典关于居住权的规定"、"帮我查劳动合同解除的相关法条"、"找几个不当得利的判例" |
| **Wind 万得（A股/基金/债券/宏观）** | `wind` | "茅台今天的分钟线"、"十年期国债收益率走势"、"基金净值查询" |
| **IMF 国际宏观（汇率 / CPI / 预测）** | `imf` | "美元兑人民币汇率"、"各国 GDP 增速预测"、"全球通胀率对比" |
| **恒生聚源智能筛选** | `gildata` | "筛选净利润增速超 30% 且 ROE 大于 15% 的股票"、"基金经理筛选" |
| **美股 SEC 披露文件** | `sec_edgar` | "特斯拉 10-K 年报"、"苹果 10-Q 季报"、"Form 4 内部人交易"、"13F 机构持仓" |
| **S&P Capital IQ 美股基本面** | `sp_data` | "苹果分析师一致预期"、"美股估值比率对比"、"竞争对手关系" |

### 选源原则

1. **用户点名了数据源** → 直接用指定的源。
2. **没点名** → 按能力域从上表选最匹配的一个；结合下面的"能力边界参考"和用户问题的深度、范围自行判断。
3. **一次简单查询只选一个数据源**，不要并行读取其他源的 desc。选定的源成功返回且已经覆盖用户问题后，立即回答；不要为了补充字段、重新格式化或交叉验证继续调用其他 API。只有用户明确要求跨源对比时，才能查询第二个数据源。

### 能力边界参考（客观事实，选源时考虑）

- `yahoo_finance` 的外汇历史最多 2 年；`imf` 提供长期的汇率、CPI、GDP 预测和国际收支序列
- `stock_finance_data` 的行情是实时/收盘快照；分钟级分时序列在 `wind`（另有基金、债券、国债收益率）
- 股东 / 机构持仓：`yahoo_finance`、`sec_edgar`（13F）、`sp_data`（S&P 标准化持有人）都覆盖，口径和深度不同
- `world_bank_open_data` 是 50 年以上的历史宏观序列；要 IMF 的预测值用 `imf`
- `gildata` 的查询输入是自然语言条件（选股 / 选基金 / 基金经理筛选），`tianyancha` 是企业工商档案
- `wind` 的 `indexes`/`indicators` 参数要求 Wind 原生字段名；PE/PB/ROE/总市值这类常用字段先调 `wind_search_fields` 映射（支持别名和中文，一次查一个），不要硬猜字段名

**不支持的能力**：通用 Web 搜索 / 实时新闻。问到这类问题，告诉用户当前数据源不覆盖。

## Call from the cached schema

1. Read only `references/desc-<source>.md` for the selected source.
2. Select the API and copy its parameter names, JSON types, formats, defaults, and limits exactly.
3. Run one call and stop when its result answers the request.

Use one parameter form; omit both to send `{}`:

```sh
node "$CLI" call SOURCE API --json '{"query":"RAG","max_results":5}'
node "$CLI" call SOURCE API --json-file /tmp/params.json
```

Pass required output paths as absolute paths under the exact key named in the description. Before a
stock call, verify the ticker and suffix (`.SH`, `.SZ`, `.BJ`, `.HK`, or `.US`) with a current lookup
or user confirmation. For `tianyancha`, resolve an unknown registered company name through its
search API first.

### Refresh a stale reference

Require schema evidence before refreshing: the reference is missing, the user requests the latest
schema, the requested capability is absent, or the backend contradicts a documented API or
parameter. Authentication, quota, empty-data, and runtime failures are not schema evidence.

Run `desc <source> --quiet` into a sibling temporary file. After a successful nonempty response,
move it over `references/desc-<source>.md`, reread the reference, rebuild the parameters, and retry a
failed data call once. Keep the existing reference if discovery fails.

## Keep large results in files

For lists, time series, screening, downloads, or calculations, supply the required absolute output
path. Start with `data_preview`; inspect only file shape and the smallest needed rows or fields. Keep
the complete file on disk and put only supporting excerpts in context and the final answer.

Mixed A-share/HK results may use `_a.csv` and `_hk.csv` siblings instead of the requested unsuffixed
path.

`arxiv read_paper` accepts no output path; the CLI emits its content as Markdown. After
`download_paper`, redirect the quiet output to a temporary `.md` file and inspect only the needed
sections:

```sh
node "$CLI" call arxiv read_paper --json '{"paper_id":"2004.10934"}' --quiet \
  > /tmp/arxiv-paper.md
```

Treat a `file://` URI from `download_paper` as a backend path, not a local file.

## Limits and failures

- For `stock_finance_data`, send at most three tickers to a real-time API or ten to a historical API;
  split larger requests into batches.
- Let the CLI perform its single OAuth refresh and retry after HTTP 401. Report a repeated failure
  without replaying the call.
- Report inconsistent schemas and backend tracebacks as service defects with available request and
  tool-call identifiers.
- Add “AI 生成，不构成投资建议” after financial data in Chinese, or an equivalent notice in the
  user's language.
