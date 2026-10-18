# 退役 Vertical Search Capability 与 AnySearch

forager 删除 Vertical Search Capability 及其唯一供应方 AnySearch，包括 `vertical_search` 能力、`anysearch` 子命令（别名 `as`）、`providers.anysearch` 与 `capabilities.vertical_search` 配置、Verified Domain Manifest，以及 doctor、smoke、setup 中的对应条目。search 与 research 的能力集合只剩 Documentation Search、Web Search 与 Web Fetch。旧的能力声明、配置键、环境变量与子命令按未知输入报错，不保留别名或兼容层。

## 依据

本机 2026-09-19 至 2026-10-18 的 journal（443 次 search）与 agent 会话记录显示，这项能力调用频繁，但贡献很低：

- 152 次 search（34%）带上了 `vertical_search`；在分类器生成的 92 个计划中，有 61 个选择了它。
- 结果只进入 `extra_sources`，不参与主回答。默认每次取 1 条，平均每次多出 1.27 条来源；其中 38 次没有结果，8 次返回的是 "No relevant results found." 占位条目。193 条结果中，只有 5 个 URL 出现在主回答里。
- 在会话中被反复引用的结果，多数是 `ssrn.com/`、`csindex.com.cn/` 这类网站首页。学术来源已由 arXiv、SSRN、Google Scholar 平台覆盖（ADR 0019）。
- `anysearch search/domains` 在会话记录中从未被实际调用；Verified Domain Manifest 始终为空，域级结构化搜索从未启用。
- 这项能力贯穿 CLI、配置、分类器、research、doctor、smoke、setup、发布验证与规格文档，任何相关改动都要连带维护这一整条链路。

## 不保留空的能力槽位

只删 AnySearch、保留没有供应方的 `vertical_search`，会让分类器、research 计划和文档继续提供一个永远无法覆盖的选项，每次选中它都只会产生能力缺口。将来如果需要垂直检索，以新的 ADR 重新引入，成本更低。

## 不改为自动路由到平台

原本被分类器路由到 vertical 的查询，现在由 Documentation Search 与 Web Search 覆盖。分类器不会自动选择平台：ADR 0019 规定平台只通过显式命令调用，自动路由到平台属于另一项决策。

## 重新评估的条件

- 出现 Web Search 与现有平台都无法覆盖、并且反复出现的垂直检索需求；
- 或某个垂直检索供应方提供稳定且经过验收的域级契约，其结果能被主回答或 research 证据实际采用。
