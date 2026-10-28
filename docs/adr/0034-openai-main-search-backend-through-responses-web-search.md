# 主搜索新增 openai 供应方：经 Responses API 的 web_search 检索

forager 在 Main Search 下新增供应方 `openai`：经 OpenAI Responses API（`POST <url>/responses`，SSE）与固定的 `web_search` 工具执行主搜索。`url` 可以指向官方 API，也可以指向 CLIProxyAPI 等说同一协议的网关。`search.backends` 的合法值扩为 `{xai, openai}`，默认值改为 `["xai", "openai"]`。没有 key 的 `openai` 不算已配置，链上跳过，因此不配置它的用户行为不变；把顺序改为 `["openai", "xai"]` 就能让它成为首选。

这一决定取代 [ADR-0031](0031-retire-openai-compatible-main-search-backend.md) 中的两条：「`search.backends` 合法值只剩 `xai`」与「主搜索没有 backend 级 fallback」。ADR-0031 其余部分不变：同端点模型链、model 断路器、SSE→HTTP 传输回退与同故障域 `config_warnings` 都不恢复。

## 依据

ADR-0031 的重新评估条件之一是「出现与 xai 故障域独立（不同 endpoint 或不同模型提供方）、并经过验收的主搜索候选」。`openai` 满足这一条：

- 上游模型提供方是 OpenAI 而不是 xAI，与 xai 的网关也是不同的 endpoint。
- 2026-10-28 经维护者自建的 CLIProxyAPI 网关实测：`POST /v1/responses` 调用 `gpt-6-luna`，带 `stream: true` 与 `tools: [{"type": "web_search"}]`，约 12 秒完成，过程中有 2 轮 `web_search_call`；`output_text` 带 `url_citation` 标注，SSE 事件序列（`response.output_text.delta`、`response.completed` 等）与 xAI Responses API 一致。
- CLIProxyAPI 源码（`router-for-me/CLIProxyAPI` 2026-10-27 的 main 分支）显示，它的 Codex 执行器原样透传 Responses 请求，接受 `web_search`、`web_search_preview` 与 `web_search_preview_2025_03_11` 三种工具类型，只强制 `store: false` 与 `stream: true`，并删除 `max_output_tokens`、`temperature` 等字段。forager 不发送这些被删除的字段。

截至 2026-10-28，本机近 30 天的 journal 共有 274 次 search，其中 2 次主搜索失败：一次是 3 个连续的快速 network attempt，每个不到 0.5 秒；另一次是 30 秒的 runtime 失败。两次失败后剩余的 deadline 都足够让另一个独立的 backend 接住。

## 共用 Responses 传输

xai 与 openai 说同一个协议，因此原先 xai 私有的 Responses 传输（请求体、SSE 读取、`response.completed` 提取、终态失败分类、`output_text` 与 `url_citation` 提取）上移为 providers 层按协议命名的共享模块。失败消息中的服务名由调用方给出，协议行为不随调用方变化，协议层的修复只需改一处。主搜索的执行路径（按请求种类构造请求、经凭据池执行、规范化、组装结果）同样只有一份，provider 只提供端点、模型、工具项与追踪参数。X 平台的 `x_search` 适配仍只归 xai。

## `--model` 只作用于链首

`--model` 原先会被交给链上每一个 backend。只有 xai 时这没有区别；有两个 backend 后，`--model grok-4.5` 会在 fallback 时被发给 openai，导致参数错误。现在 `--model` 只交给本次主搜索链实际运行的第一个 backend（跳过没有凭据的条目之后的链首），之后接手的 backend 使用各自配置的模型。

## 请求面固定

`openai` 只开放 `url`、`keys` 与 `model`。工具固定为 `web_search`，因为 `x_search` 是 xAI 专有的工具；不开放 `tools`、`reasoning.effort` 或 `web_search` 的可调参数，等有实际需求再加。与 xai 一样没有 `timeout` 键：首选 backend 可以用掉全部剩余预算（[ADR-0007](0007-primary-first-timeout-budget.md)），fallback 用剩下的部分。

OpenAI 在引用 URL 末尾追加 `utm_source=openai`。forager 只去掉这一个精确参数，保留 URL 的其他部分；它与 ChatGPT 网页的 `utm_source=chatgpt.com` 由同一条共享规则处理，不做通用的 `utm_*` 剥离。

## 已接受的局限

- **静默挂起会饿死 fallback**：首选 backend 静默挂起时，每个 attempt 最多等待 60 秒的共享读超时，加上 `retry.max_attempts` 次重试，可能耗尽默认 180 秒 deadline，使 fallback 没有机会运行。这一路径在上述 journal 中没有出现过。等 journal 显示确有发生，再考虑为 fallback 预留预算或在后面还有 backend 时减少首选的重试次数。
- **故障域的不确定性**：维护者本机 xai 网关与 CLIProxyAPI 网关的域名都解析到本地代理的 fake-IP，无法判断两者是否部署在同一主机上。如果是同一主机，主机级故障会让两个 backend 同时失败；上游服务的故障仍然独立。
- **成本**：一次带 `web_search` 的简单查询在 `gpt-6-luna` 上消耗约 1.56 万 token（含推理）；更复杂的查询耗时与额度消耗更高。默认模型 `gpt-6.1-terra` 未实测。
- **默认模型名**：`gpt-6.1-terra` 是 CLIProxyAPI 下 Codex 账号的模型名，直接指向 `api.openai.com` 的用户需要按官方 API 可用的模型修改 `providers.openai.model`。
- **网关注入的工具**：CLIProxyAPI 会为非免费账号自动注入 `image_generation` 工具。这是网关行为，forager 不处理。

## 不做的事

- 不把 `openai` 作为 X 或任何平台的 route，也不接入 classifier。
- 不恢复同故障域 `config_warnings`：`openai` 的协议与默认 endpoint 都不同于 xai，配到同一 endpoint 与模型需要用户刻意为之。
