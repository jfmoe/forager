# Firecrawl PDF `pageMarkers` 固定启用的小基准

> 核验日期：2026-09-26 UTC。对象为 Firecrawl Cloud `/v2/scrape` 和 forager 的 Firecrawl Web Fetch 请求。本文区分 Firecrawl 官方文档、实际响应与由此作出的工程判断；所有计时均为脚本观察到的单次 HTTP 请求耗时。

## 结论

**不建议给所有 Firecrawl Web Fetch 请求固定加入 `parsers:[{"type":"pdf","mode":"auto","pageMarkers":true}]`。建议仅在 PDF 需要较完整的正文、可用表格或页码线索时有条件采用，并给长 PDF 足够的客户端等待预算。`timeout:60000` 这一 Firecrawl 请求体参数暂不调整。**

实测的两份 HTML 在加入该参数后 Markdown **逐字节相同**，响应均为每次 1 credit；因此 HTML 兼容性不是主要障碍。主要取舍在 PDF：Mixtral 和 Llama 3 的表格明显改善，IRS 的分散正文锚点增加，但 Llama 3 的处理从 4.136 秒增至 31.959 秒。forager 当前请求体写 60 秒，Firecrawl provider 的本地单次等待却默认 **30 秒**（[请求构造](../../src/capabilities/providers/web_fetch.rs)、[配置默认值](../../src/infra/config/schema.rs)）。**由代码与耗时推断**，若全局启用该参数而保持默认本地等待时间，本次 Llama 3 增强档会先被本地超时截断；直接增大 API 的 `timeout:60000` 无法解决此问题。实际 CLI 端到端结果未在本基准中测量。

## 请求与判定口径

两档均向 `https://api.firecrawl.dev/v2/scrape` 发送 `url`、`formats:["markdown"]`、`onlyMainContent:true`、`timeout:60000`、`maxAge:0`；增强档只多下列字段：

```json
{"parsers":[{"type":"pdf","mode":"auto","pageMarkers":true}]}
```

`maxAge:0` 排除 Firecrawl 顶层缓存命中，**不建议据此改动生产缓存策略**。官方说明它会走完整抓取流程，可能更慢且更易失败。[Scrape 缓存说明](https://docs.firecrawl.dev/features/scrape#caching-and-maxage)。官方文档说明 PDF 默认 `mode:auto`，`pageMarkers` 在 Markdown 中插入页码注释且不另收费；页码可因跨页内容合并而跳号，不能当作严格的逐页数组。[Parse 的 PDF 参数与页码标记](https://docs.firecrawl.dev/features/parse#pdf-options)。

词数统一按 Markdown 空白分词（`str.split()`，与先前 Mixtral 研究的 `wc -w` 数值一致）；正文完整度按 [assess.py](firecrawl-pdf-parsers-2026-09-26/assess.py) 从指定页首尾抽取并归一化的锚点命中率。扫描件用九个人工文字锚点。Markdown 表格行数包含表头和分隔行，**本身不代表单元格正确**。明显错字专项检查 `Mixral`，另人工核对 Llama 3 Table 2 的表头。原 PDF 的页数和 SHA-256 见[样本来源](firecrawl-pdf-parsers-2026-09-26/sources.json)。

## 两轮逐样本对比

表中每档依次列出“词数；锚点；Markdown 表格行；耗时；`creditsUsed`；超时”。`—` 表示 HTML 不适用正文锚点。Mixtral 来自[先前 Firecrawl/Tavily 研究](2026-09-26-firecrawl-tavily-fetch-quality.md)的已保存 `maxAge:0` 成对响应，其余来自本基准；本轮**没有重跑 Mixtral**。所有列出的响应均为 HTTP 200、`success:true`，都未观察到 API 超时。两轮结果已按来源合入[结果汇总](firecrawl-pdf-parsers-2026-09-26/results.json)与[锚点评估](firecrawl-pdf-parsers-2026-09-26/assessments.json)。

| 样本 | 默认 | 加 `pageMarkers` | 可核对的质量差异 |
| --- | --- | --- | --- |
| [Llama 3](https://arxiv.org/pdf/2407.21783)，92 页 | 49,229；7/8；181；4.136 s；92；否 | 63,811；7/8；696；31.959 s；92；否 | Table 2 默认表头误写 `Genma`、`GPT-4(0128)`，且首行重复；增强档表头与原 PDF 一致，抽查 MMLU 数值对齐。均缺第 3 页脚注锚点；未逐格核验整篇表格。 |
| [Bitcoin 白皮书](https://bitcoin.org/bitcoin.pdf)，9 页 | 3,410；8/8；0；3.212 s；9；否 | 3,581；8/8；0；6.081 s；9；否 | 两档所抽查首尾正文齐全；增强档有 8 个页间标记，未发现可核对的 Markdown 表格。 |
| [OCRmyPDF 扫描件](https://raw.githubusercontent.com/ocrmypdf/OCRmyPDF/main/tests/resources/ccitt.pdf)，1 页 | 715；7/9；0；3.239 s；1；否 | 715；7/9；0；10.923 s；1；否 | 两档 Markdown 逐字节相同，均漏页脚地址与电话两个锚点；单页无页间标记。 |
| [IRS Publication 583](https://www.irs.gov/pub/irs-pdf/p583.pdf)，28 页 | 15,659；5/8；151；3.414 s；28；否 | 18,308；7/8；225；12.686 s；28；否 | Table 1 的 10 组问答两档都有；增强档补回第 1、15 页的两个首尾锚点，仍缺第 1 页一个锚点。未逐格核验其余表格。 |
| [Mixtral v1](https://arxiv.org/pdf/2401.04088v1)，13 页 | 4,330；7/8；0；2.345 s；13；否 | 8,486；8/8；239；11.395 s；13；否 | 默认有 10 处 `Mixral`、Table 2 无可用表格；增强档 0 处该错字，Table 2 的 7 行 × 14 列共 98/98 单元格与[核对基准](firecrawl-tavily-2026-09-26/table2-checks.json)一致。 |
| [Python 教程 HTML](https://docs.python.org/3/tutorial/datastructures.html) | 4,319；—；0；3.285 s；1；否 | 4,319；—；0；2.799 s；1；否 | Markdown 32,202 字节，两个条件的 SHA-256 相同。 |
| [Mixtral HTML](https://arxiv.org/html/2401.04088v1) | 5,696；—；91；3.331 s；1；否 | 5,696；—；91；2.778 s；1；否 | Markdown 39,358 字节，两个条件的 SHA-256 相同。 |

HTML 两组的字节相等由本机原始 `.md` 比较确认，不只根据词数判断；对应 SHA-256 分别为 `5a537e501424e38c728764696d736c1b74beecf552286ea1769987e933262310` 和 `7c4a2fb8831f33893b1d4e30e178d15bb630032e8987737038ac05b4ec3a5006`。这些结果支持**这两个 HTML 样本**在该请求参数下输出与响应 credit 不变，不能证明所有 HTML 都相同。

## 延迟、额度与局限

本轮只补缺失组合：**12 次新增调用**，其中 11 次取得 HTTP 200；扫描件默认档首次遇 TLS EOF，未收到 HTTP 响应，随后补跑成功。成功响应的 `creditsUsed` **合计 172**，低于 220 上限；无响应的尝试没有该字段，故 172 是响应报告的使用量，未与账户账单核对。本轮没有再请求已成功的 Llama 3 默认档，也没有重跑 Mixtral。首轮使用的一个凭据已耗尽，产生 402、429；这些失败未纳入质量比较。原始账本保留全部 26 次历史与新增尝试，新增逐次记录在本机 `~/.local/share/forager/research/firecrawl-pdf-parsers-20260926/attempts/`。

五组 PDF 成对响应的 `creditsUsed` 各自相等，且均等于 PDF 页数；这与官方 Parse 页的每页 1 credit、页码标记不加价一致。不过官方 [Billing](https://docs.firecrawl.dev/billing) 将 PDF 写为基础抓取费之外每页再加 1 credit，与本轮响应数值的口径不完全一致；本文只报告实际响应字段，不推断最终账单。增强档实际标记数分别为 Llama 3 的 79、Bitcoin 的 8、扫描件的 0、IRS 的 23、Mixtral 的 10，不能把标记数当作页数。`pageMarkers` 的标记本身每个约增 4 个 `cl100k_base` token；较大的词数变化主要来自解析结果差异，**不能归因于注释开销**。显式 PDF parser 与默认请求可能经过不同的服务端处理路径，基准无法证明仅是 `pageMarkers` 开关导致质量改善。

Llama 3 增强档在 60 秒 API timeout 内成功，因此没有证据要求提高请求体的 `timeout:60000`；但 31.959 秒已超过 forager 默认的 30 秒 provider 等待上限。若要对长 PDF 实际启用该选项，应另行验证并设置足够的 provider 与命令总等待预算，且保留重试、回退空间。官方也提醒大型或扫描 PDF 可能需要更长等待时间。[Parse 限制说明](https://docs.firecrawl.dev/features/parse#considerations)。本基准样本少、均为英文，扫描件只有一页；锚点与局部表格检查不覆盖全部公式、图、阅读顺序、OCR 错误，也不估计全网成功率。

## 复现与制品

脚本和汇总数据在 [`firecrawl-pdf-parsers-2026-09-26/`](firecrawl-pdf-parsers-2026-09-26/)；PDF、API 响应、Markdown 与账本保存在上述本机原始目录。Mixtral 的旧原始 Markdown 位于 `~/.local/share/forager/research/firecrawl-tavily-20260926/`。运行脚本从本机配置读取第二个 Firecrawl 凭据，仅在进程内使用，不写入制品。`run` 只补本基准仍缺失的成功组合，新增调用最多 16 次、响应报告的新增 credits 超过 220 即停止；连续调用至少间隔 7 秒，429 等 60 秒后只重试一次，402 立即停止。以下命令在已有原始目录上重算汇总；`run` 当前不会重复任何成功组合，若日后出现缺失组合才会发起付费请求。

```bash
uv run --with requests==2.34.2 --with PyMuPDF==1.28.2 --with tiktoken==0.14.0 \
  python docs/research/firecrawl-pdf-parsers-2026-09-26/bench.py run
uv run --with requests==2.34.2 --with PyMuPDF==1.28.2 --with tiktoken==0.14.0 \
  python docs/research/firecrawl-pdf-parsers-2026-09-26/bench.py summarize
uv run --with requests==2.34.2 --with PyMuPDF==1.28.2 --with tiktoken==0.14.0 \
  python docs/research/firecrawl-pdf-parsers-2026-09-26/assess.py
```
