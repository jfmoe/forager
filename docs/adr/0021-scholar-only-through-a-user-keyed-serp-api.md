# 谷歌学术只经第三方 SERP API 接入、key 由用户自备、以免费额度为前提

forager 的第三个平台 `scholar`（Google Scholar）只经第三方 SERP API 接入，首条也是唯一一条 route 是 `serpapi`（SerpApi 的 Google Scholar 引擎）。key 由用户自己注册并配置在 `providers.serpapi.keys`；forager 不附带 key，不代付额度，默认使用场景是 SerpApi 免费计划（2026-10-07 为每账号每月 250 次搜索）。设计与实测证据见 `docs/design/2026-10-07-scholar-serpapi-route.md`，平台契约见规格第 7 章「Google Scholar」。

## 为什么不直接访问谷歌学术

谷歌学术没有官方公共检索 API，官方帮助明确不提供批量访问，并要求自动化软件遵守 robots.txt，而 robots.txt 禁止访问 `/scholar`（`docs/research/2026-09-25-scholarly-agent-access.md`）。自建抓取因此不能作为稳定的基础设施；有文档的第三方 SERP API 已经提供结构化结果，包括被引数与版本聚合。所以第一期不做自建抓取，也不做浏览器 route。以后如果加入 `scholar_browser`，它和 SSRN 的浏览器 route（ADR 0020）一样，只能由用户手动加入 order。

## 为什么 key 由用户自备

- SERP API 按次计费，forager 是本地 CLI，没有服务端可以安全保管共享 key，也不应替用户承担费用或条款责任。
- key 进入现有 Provider Credential Pool（ADR 0005），不新增凭据机制：多 key 轮询、`run out of searches` 的 429 识别为 QuotaExhausted 后同一请求内换 key、其他 429 为 RateLimited 也换 key。
- 默认 order 含 `serpapi`，但没有 key 时 route 视为未配置：平台命令飞行前退 3，消息点名 `providers.serpapi.keys`；普通 `search` 与 `research` 永不调用它。
- 文档只说明"每个 key 对应一个账号的额度"，不建议用多注册免费账号的方式扩大额度，因为这可能被认定为滥用免费计划。

## 为什么以免费额度为前提

每次成功的搜索（包括合法空集与不存在的 cluster）都消耗用户额度，额度很小，因此接口按"少花额度"设计：

- search 的 `--limit` 默认取满页 20：每页无论多少条都计 1 次。
- fetch 只取一页 cluster，不自动翻页；只提供 `metadata` 深度，正文交给目标平台或 Web Fetch。
- 不持久化配额状态：额度用尽的 key 轮到时先收到一次不计费的 429 再换 key，代价只是一次往返。
- 普通 `doctor` 只对端点发一次不带 key 的 GET（不计费）；只有显式的 `doctor --provider serpapi` 与 live smoke（C24–C26）会消耗额度，smoke 只在配置了 key 时运行。

## 影响

- provider 按供应商命名为 `serpapi`，不叫 `scholar_serpapi`：同一账号的额度与吞吐不分引擎，以后接入 SerpApi 的其他引擎可以复用同一 key 池与凭据游标。
- key 只能放在 URL 查询参数里，因此 route 先去掉 reqwest 错误中的 URL，并在 route 自产消息进入 attempt 前按凭据值脱敏；输出不投影 `search_metadata`、`search_parameters` 与分页链接。
- 结果质量受第三方解析与谷歌学术自身限制：作者名可能缩写或截断，cluster 身份部分依赖未文档化的 `result_id` 编码，cluster 的第一个版本不一定是正式出版版本。这些限制写在规格第 7 章与 skill 的平台 reference 中。
- SerpApi 停止服务或条款变化时，Scholar 平台随之不可用；平台不会 fallback 到其他平台。
