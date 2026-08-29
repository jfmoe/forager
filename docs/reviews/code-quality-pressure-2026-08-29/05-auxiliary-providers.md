# AnySearch、Context7 与 Exa provider

审查范围：`src/providers/anysearch.rs`、`context7.rs`、`exa.rs` 及对应集成测试，并追踪 MCP/engine/research/type 边界。

## Findings

### [高严重度，高置信度] Context7 会把普通文档正文误判为 library ID 重定向

- 位置：`src/providers/context7.rs:176`、`src/providers/context7.rs:396`
- Fact：`Context7Operation::decode` 在区分 Library/Docs 操作前，对所有 MCP 文本调用 `redirect_target`。文本启发式只要求正文任意位置含 `redirect`，然后取任意首个以 `/` 开头的词作为目标。合法正文 `Redirect requests from /old to /new.` 会确定性返回 Runtime。
- Impact：`context7 docs` 退出 4；research 的 `DocsSearch::read` 同样变成证据 gap。HTTP redirect、路由和代理文档是常见技术内容，不是纯理论碰撞。
- Judgment：协议适配器把任意正文数据误当控制状态，且让 Library/Docs 共享不成立的解码前置条件。
- Evidence：`redirectedLibraryId` 等结构化字段是可靠信号，现有测试只覆盖该正例，没有正文负例；宽泛文本分支既未锚定官方句式也未限定操作。
- Recommendation：保留 structuredContent 的明确重定向字段，删除宽泛正文扫描。若上游确有 text-only 重定向，必须以真实 fixture 固化完整锚定句式，并仅在适用操作解析。
- Verification：docs fixture 返回 `Redirect requests from /old to /new.` 时 content 原样成功；structured `redirectedLibraryId` 仍 Runtime、无重试且 ID 正确。

### [中严重度，高置信度] Exa 缺失 `results` 的成功响应被降格为合法空结果

- 位置：`src/providers/exa.rs:336`
- Fact：`ExaResponse.results` 使用 `#[serde(default)]`；`{}` 或 HTTP 200 `{"error":"..."}` 都解码为空 Vec，只有显式类型错误才 Runtime。
- Impact：直连 Exa 退出 0 返回空结果，丢失协议错误；自动 DocsSearch 虽会 fallback，却把该成功 attempt 改写为 Evidence，而非保留真实 Runtime。
- Judgment：typed JSON 边界把合法空集合与缺失必需 container 合并，违反 parse-at-boundary。
- Evidence：现有测试只证明显式 `{"results":[]}` 应成功；本地规格与 fixture 未证明缺失字段合法。
- Recommendation：令 `results` 必填；若 Exa 有 HTTP 200 错误包装，单独 typed decode 并映射错误。
- Verification：保留显式空数组成功；新增 `{}`/error wrapper 为 Runtime，并确认 docs fallback 保留 Exa Runtime attempt。

### [中严重度，高置信度] Provider Acceptance Operation 被记录成 `vertical_search` Capability Seam

- 位置：`src/providers/anysearch.rs:68`、`src/providers/anysearch.rs:184`
- Fact：Domain Discovery、显式域搜索和自动 Vertical Discovery 共用 `execute_tool`，该函数无条件写 `seam: vertical_search`。只有无域 adapter 才是真正的自动 Vertical Search seam；CONTEXT 明确 Domain Discovery 属于 Provider Acceptance Operation。
- Impact：verbose/journal/attempt 聚合把验收流量计入能力流量；未来按 seam 做可用性、成功率或路由审计会得出错误结论。
- Judgment：acceptance operation 与 capability 身份被 stringly-typed seam 合并，形成模型边界泄漏。
- Evidence：outcome 自身正确标记 `operation: domain_discovery`，engine 也只通过 adapter 自动调用无域搜索，故未上调为高严重度；测试没有检查 verbose attempt 身份。
- Recommendation：把 attempt 目标建模为 Capability 与 Provider Operation 的互斥类型，或增加独立 operation 维度；direct wrapper 与 VerticalSearch adapter 分别赋予身份。
- Verification：`anysearch domains --verbose` 与显式未验证域搜索不得产生 `vertical_search` seam；自动无域 discovery 仍应产生该 seam；journal/manifest 分别校验 operation/capability 集合。

## Open Questions

- Context7 官方 text-only 重定向的精确 wire 句式是什么？无真实 fixture 前不应保留宽泛兼容。
- 是否有外部 journal/telemetry 消费者把 `ProviderAttempt.seam` 当封闭 capability 维度？这决定 AnySearch schema 迁移成本。
- Exa 是否正式允许 HTTP 200 省略 `results`？当前仓库没有证据。

## Notes

- Context7 provider-owned read、typed locator 与 research 调用保持清晰，没有把正文重新送入 WebFetch。
- Exa candidate 白名单投影、Context7 typed library locator、AnySearch URL-less structured candidate 符合规格。
- AnySearch manifest/Markdown 多形态解析虽复杂，但与窄解析规格相符，未找到可安全删除的分支。
- 三家均委托共享 retry/rotation 执行器，未发现局部顺序漂移。
- 未报告 formatter/lint 或样式问题。

## 主 Agent 点验

- 已复读 Context7 全操作前置重定向与宽泛文本启发式、Exa `#[serde(default)]`、AnySearch 共用 `execute_tool` 的固定 seam，并核对 CONTEXT/adapter 边界；三项证据均可复现。

## Thermo Pressure Pass

- deletable-complexity：Context7 宽泛正文扫描可直接删除，structured redirect 保留行为。
- growth/cohesion：三个 provider 内部总体内聚；No finding。
- spaghetti/model：No additional finding。
- boundaries/types：Exa 缺失必需字段被合成空成功；AnySearch operation/capability 身份混合。
- canonical-ownership：MCP transport/retry/deadline/error mapping 已集中；Context7 read 有明确 owner。
- concurrency/atomicity：目标 provider 无 fan-out 或事务状态；No finding。
- behavior-safety：主要投影、rotation、cap 和 read ownership 已覆盖，缺少上述三项负例。

## 最终 Disposition

- Context7 正文误判 redirect：**resolved**。只接受具名结构化重定向字段，正文中的 redirect 文案保持普通内容。
- Exa 缺失 `results` 变空成功：**resolved**。核心 container 必填，显式空数组与畸形 2xx 响应由测试区分。
- AnySearch acceptance operation 错标 capability seam：**resolved**。显式 provider operation 与自动 `vertical_search` target 分开记录。

本切面 3 条 finding 全部关闭。
