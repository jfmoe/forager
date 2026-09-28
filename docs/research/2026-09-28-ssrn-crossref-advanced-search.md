# SSRN Crossref 高级搜索验收

检索时间（UTC）：2026-09-28T14:21:31Z。关联 [#175](https://github.com/jfmoe/forager/issues/175)。

使用当前工作区构建的 `target/debug/forager`，每次通过环境变量 `FORAGER_PLATFORMS__SSRN__ORDER='["ssrn_crossref"]'` 固定路线；未修改用户配置。所有论文调用均为 search，没有 fetch、详情页或 PDF 请求。

## 结果

| 用例 | search 参数 | 退出码 | 返回 ref |
|---|---|---|---|
| title_author_abstract | `'dual momentum' --scope title --author Antonacci --has-abstract --limit 1` | 0 | ssrn:2042750 |
| title_author_abstract_page2 | `--cursor <首个用例返回的 next_cursor>` | 0 | ssrn:1833722 |
| publication_range | `momentum --published-from 2020-01-01 --published-to 2024-12-31 --sort published --order asc --limit 3` | 0 | ssrn:3689223, ssrn:3722540, ssrn:3553682 |
| created_range | `momentum --created-from 2020-01-01 --created-to 2024-12-31 --sort created --limit 3` | 0 | ssrn:5053270, ssrn:5057525, ssrn:5018430 |
| updated_range | `momentum --updated-from 2024-01-01 --updated-to 2025-12-31 --sort updated --limit 3` | 0 | ssrn:5879622, ssrn:5989250, ssrn:5986082 |
| bibliographic_type | `'dual momentum' --scope bibliographic --type journal-article --sort citations --limit 3` | 0 | ssrn:1553430, ssrn:3071506, ssrn:1738315 |
| affiliation | `finance --affiliation Harvard --limit 3` | 0 | 空结果 |
| orcid | `bubble --orcid 0009-0006-6981-0361 --limit 3` | 0 | ssrn:6385902, ssrn:6187955 |
| funder | `science --funder 10.13039/100000001 --limit 3` | 0 | 空结果 |

9 次 CLI 调用均由 `ssrn_crossref` 返回成功。标题、作者、摘要组合的第二页继续返回另一篇论文；发表时间升序的三个样例均保留 `2020` 的年份精度；首次登记时间降序样例均位于指定区间。更新时间与引用量没有为了验收增加到 CLI 输出，因此对这两种排序的证明是上游接受请求及 HTTP fixture 的精确映射，不能仅凭 CLI 输出独立重建其排序依据。

## 覆盖限制

- ORCID 样例来自同日 SSRN 前缀的搜索响应；`has-orcid:true` 的上游计数为 389723。指定 `0009-0006-6981-0361` 后检索 `bubble` 返回两篇记录。
- `finance` 加 Harvard 机构条件为空；这不证明该机构没有相关论文，只说明这次元数据查询没有命中。
- SSRN 前缀 `has-funder-doi:true` 搜索计数为 0；NSF 的 OFR DOI `10.13039/100000001` 筛选返回空结果。接口接受该过滤字段，但当前样本没有正向命中可验证，不能把元数据缺失当作原论文未获资助。
- 搜索条件会改变召回，不承诺精确短语匹配。上述数量与排名随上游数据变化；不是检索质量或完整召回率评估。

## 来源与离线证明

- [Crossref REST 过滤器](https://www.crossref.org/documentation/retrieve-metadata/rest-api/rest-api-filters/)：日期、摘要、类型、ORCID 与资助机构语义。
- [当前 API 定义](https://api.crossref.org/swagger-docs)与[类型列表](https://api.crossref.org/types)：查询接口及注册文献类型。
- `tests/ssrn_search.rs` 使用真实 CLI 和本地 HTTP fixture，验证编码、组合条件、分页、无效请求零网络及不支持路线跳过。类型与支持矩阵规则另有进程内测试。
