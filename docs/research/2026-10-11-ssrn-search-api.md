# SSRN browser 检索改用原站接口的实测（2026-10-11）

环境：OpenCLI 1.8.6（Browser Bridge 扩展 1.0.24）、用户自己的 Chrome、macOS，机器时区 UTC+8。可行性部分用临时探测命令和 `forager-ssrn/3` 的页面 adapter 对照，验收部分用仓库构建的 forager 与 `forager-ssrn/4` adapter。生产会话参数为 `--window background --site-session ephemeral --keep-tab false`。决策见 ADR 0025。

## 接口

结果页 `papers.ssrn.com/searchresults.cfm` 的数据来自：

```
GET https://api.ssrn.com/papers/v1/papers/search/advanced
    ?text=<查询词>&text_fields=<范围>&search_mode=<fuzzy|boolean>
    &authors=<作者文本>&date=<日期预设>&sort_by=<排序或空串>&page=<原生页码>
```

成功响应（节选，一条论文）：

```json
{"total": 10000, "papers": [{
  "id": 3447702,
  "title": "Unraveling <em>Momentum's</em> <em>Moments</em>",
  "authors": [{"id": 3105613, "full_name": "Sunil Teluja"}],
  "approved_date": "2019-09-07T19:00:38.000Z",
  "revision_date": null,
  "snippets": ["… <em>momentum</em> …"],
  "page_count": 27, "downloads": 261, "affiliations": "…",
  "publication_status": "UNDER REVIEW", "abstract_type": "WPS",
  "reference": "", "is_approved": true, "is_paid": false
}]}
```

## 可行性

| 问题 | 结果 |
|---|---|
| 是否需要凭据 | 不需要。`credentials: 'omit'` 的普通 GET 返回 200。原站页面自己也不带 cookie 请求，带 cookie 时 CORS 失败 |
| 是否需要打开 SSRN 页面 | 不需要。命令声明 `navigateBefore: false` 且不调用 `goto` 时，标签页停在 `about:blank`（origin `null`）。接口的 CORS 允许任何来源，请求返回 200 与 50 条结果 |
| 浏览器之外 | curl 带浏览器 UA 请求返回 403 和 Cloudflare 验证页（`cf-mitigated: challenge`），因此仍须经由浏览器。该验证页是 `text/html`，不带 `Access-Control-Allow-Origin` |
| 拦截时能否读到状态 | 能。OpenCLI 的 `page.evaluate` 在空白标签页中跨源请求不带 CORS 头的响应（以 `https://www.google.com/` 下不存在的路径实测，404 `text/html`）时，`fetch` 正常返回，`response.type` 为 `basic`，状态与正文都可读，没有因 CORS 失败。因此接口被 Cloudflare 拦截时，adapter 读到的是 403 或 503 加 HTML，以 77 结束，而不是网络错误。这依赖 OpenCLI 1.8.6 的执行环境 |
| 耗时 | 接口请求 0.25–1.2 秒；页面读取约 2.5–3.2 秒 |
| 内容一致性 | 同一结果页 50 条的 ID、标题（去掉 `<em>`）、作者、片段逐条相同。与页面 adapter 的逐组对照见下节 |
| 边界 | 无结果为 200 `{"total":0,"papers":[]}`；非法参数为 400 JSON；超出 10,000 条上限的第 201 页为 500 JSON；`total` 最大为 10000 |
| 节奏 | 当天约 25 次接口请求和 40 次页面检索（约 4.5 秒一次），没有出现验证或拦截 |
| 日期 | 页面上的 Posted 是 `approved_date` 按浏览器本地时区格式化的结果，例如 `2019-09-07T19:00:38Z` 在 UTC+8 显示为 `08 Sep 2019`。同一篇 `ssrn:2042750` 在结果页显示 `20 Apr 2012`，在详情页显示 `19 Apr 2012` |
| null 字段 | 接口对没有值的字段给 null，而不是省略：title 范围检索的 `snippets` 全为 null；抽查的两组各 50 条中 `revision_date` 全为 null |
| 标记与实体 | 标题和片段中出现过 `<em>`、`<p>`、`<span>`、`<i>`，以及 `&amp;`、`&nbsp;`、`&#x3b;` |

## 与页面 adapter 的对照

同一组条件先由 `forager-ssrn/3` 的页面 adapter 读取结果页，等 5 秒再由探测命令请求接口，比较两边的 ID 序列。逐条 ID 序列当时没有保存，下表只保留比较结论。

| 条件（查询词、范围、模式、作者、日期、排序、页码） | 页面 | 接口 | 条数 | ID 序列 |
|---|---|---|---|---|
| `dual momentum`，title-abstract-keywords，fuzzy，—，all_time，相关性，1 | 2.5 秒，1 to 50 of 10000 | 0.8 秒，200，total 10000 | 50 / 50 | 不同 |
| `momentum AND (crash OR reversal)`，title，boolean，—，all_time，downloads-desc，2 | 2.5 秒，51 to 60 of 60 | 0.9 秒，200，total 60 | 10 / 10 | 相同 |
| `carbon tax`，title-abstract-keywords-fulltext，fuzzy，—，last_year，approval_date-asc，1 | 3.2 秒，1 to 50 of 10000 | 1.2 秒，200，total 10000 | 50 / 50 | 不同 |
| `dual momentum`，title-abstract-keywords，fuzzy，Antonacci，all_time，title-asc，1 | 2.5 秒，1 to 4 of 4 | 0.7 秒，200，total 4 | 4 / 4 | 相同 |
| `zzqxjvkwplm qqzzyyxx`，title-abstract-keywords，fuzzy，—，all_time，相关性，1 | 2.4 秒，无结果 | 0.6 秒，200，total 0 | 0 / 0 | 相同 |

两组不同的原因在接口自身：对同一请求连续发两次，顺序就不同，相关性排序甚至有 1 条被替换；页面只是显示它当时收到的接口结果。

## 验收

以下请求经完整的 forager CLI → route → OpenCLI → Chrome 执行，配置只启用 `ssrn_browser`。每次请求都由 route 核对请求地址中的全部条件与页码，以及返回条数与 `total`。耗时约 5 秒是 route 的访问节奏（每 5 秒一个 OpenCLI 命令）；单独一次检索为 1.15 秒。

| 用例 | CLI 条件 | 结果 | 前几条 ref 与 posted |
|---|---|---|---|
| 范围：标题 | `dual momentum --scope title --limit 3` | 3 条，有下一页 | ssrn:2041429 18 Apr 2012；ssrn:2042750 19 Apr 2012 |
| 范围：全文 | `dual momentum --scope full-text --limit 3` | 3 条，有下一页 | ssrn:3447702 07 Sep 2019；ssrn:2042750 19 Apr 2012 |
| Boolean | `momentum AND crash --mode boolean --limit 3` | 3 条，有下一页 | ssrn:2578296；ssrn:3092546 |
| 作者 | `momentum --author Antonacci --limit 5` | 4 条，没有下一页 | ssrn:2042750；ssrn:1833722 |
| 日期预设 | `momentum --date last-month --limit 3` | 3 条，posted 均在 2026-09 | ssrn:7466186 15 Sep 2026 |
| 下载量降序、升序 | `momentum --sort downloads [--order asc]` | 各 3 条 | 降序首条 ssrn:1585517；升序首条 ssrn:1319878 |
| 发布日期降序、升序 | `momentum --sort posted [--order asc]` | 各 3 条 | 降序首条 10 Oct 2026；升序首条 22 May 1995 |
| 标题升序、降序 | `momentum --sort title [--order asc]` | 各 3 条 | 升序首条 “0DTEs: …”；降序首条为非拉丁文字标题 |
| 实体 | `"risk & return" --scope title --limit 5` | 5 条，标题无标记与实体残留 | ssrn:1540994 |
| 无结果 | `zzqxqzzxqvv` | 0 条，退 0，无 cursor | — |
| 第 2 页 | `dual momentum --limit 50` 的 cursor | 50 条，有下一页 | ssrn:3759017 |
| 上限末页 | offset 9950 的 cursor（`momentum`） | 50 条，`next_cursor` 为 null | — |
| doctor deep | `doctor --provider ssrn_browser` | `ok: true` | — |
| smoke | `smoke --live` | C22（search）通过 | — |

## 页面要求人工验证时的接口表现

同日 smoke 中，C22（search）于 03:35:31 UTC 通过，C23（fetch）的三次尝试都失败（03:39:37 结束），随后单独运行的 `platform ssrn fetch ssrn:2042750` 以 Auth 失败（`SSRN security verification did not clear`），说明详情页进入了需要人工勾选的验证。没有处理验证，接着：

1. 03:41:07 运行 `platform ssrn search "dual momentum" --limit 2`，接口检索正常返回 `ssrn:3447702`（07 Sep 2019）与 `ssrn:965765`（27 Feb 2007）；
2. 紧接着再次运行同一 fetch，仍以 Auth 失败。

因此至少在这一次页面级验证中，接口检索不受影响。这只是一次观察，不保证 Cloudflare 以后不会直接拦截接口；接口被拦截时 adapter 以 77 立即结束，route 报 Auth。
