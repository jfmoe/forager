# 共享基础设施与敏感状态边界

审查范围：`src/net.rs`、`credentials.rs`、`journal.rs`、`redact.rs`、`secure_fs.rs`、`attempt_log.rs` 及直接消费者/相关测试。

## Findings

### [中严重度，高置信度] Journal 为缺失字段伪造主搜索归因

- 位置：`src/journal.rs:17`、`src/journal.rs:254`
- Fact：`SearchRecord` 携带调用级主搜索 model/endpoint；`build_record` 序列化完整 attempt 链后，对所有缺少这些字段的 attempt 无差别回填主搜索值。Supplemental Web Search/Web Fetch 正确产生 `model: None, endpoint_host: None`，却在 journal 中被改写；真正的主搜索 attempt 已在创建处写入自身事实。
- Impact：Tavily/Firecrawl/Jina 等 auxiliary attempt 会被错记为使用主搜索模型与 endpoint，污染诊断与统计。任何新 seam 使用 None 表示“不适用”都会继承错误事实。
- Judgment：Tell Don't Ask / canonical ownership 违背。`ProviderAttempt` 应独占 attempt 事实，journal 不应从调用级上下文二次推导。
- Evidence：类型已把字段建模为可空；现有 journal 测试只覆盖主搜索 attempt。旧 synthetic attempt 可能是回填动机，但当前循环不限制 main_search/provider，None 比错误默认更忠实。
- Recommendation：删除全链补写及仅为该推断存在的 `SearchRecord.endpoint_host/model` plumbing；若某个 synthetic main attempt 需要默认值，在其构造点显式写入。
- Verification：含主搜索与 Tavily/Web Fetch 的 journal 中，主 attempt 保留真实 model/endpoint，auxiliary attempt 不出现这些字段或记录自身 endpoint。

### [中严重度，高置信度] 序列化后的递归清洗抹平字段级 redaction 策略

- 位置：`src/journal.rs:107`、`src/journal.rs:368`
- Fact：journal 先构造无类型 `serde_json::Value`，再递归重写每一个字符串，无法区分必须脱敏的 source URL/attempt message 与明确豁免的 answer。`redact_journal_text` 又重复实现 `redact.rs` 已有的 URL+credential 组合规则。
- Impact：journal 不再忠实记录完整 answer；任何新增字符串字段都会在没有显式策略决定时自动被清洗。高风险 redaction 知识分散在 `redact.rs`、`CredentialPool::redact` 和 journal。
- Judgment：untyped boundary + canonical ownership 问题。字段语义在转换为 Value 后丢失，policy 无法由类型/构造器表达。
- Evidence：ADR-0002 要求完整 answer，ADR-0006 将 answer body/fetched content 设为豁免，仅 URL/错误等受保护面脱敏。现有 canary 测试反而把“stdout 保留、journal 删除”这一冲突固化。blanket sanitizer 有 defense-in-depth 价值，但不能替代已存在的 allowlist 与字段策略。
- Recommendation：使用 typed journal DTO，或在构造每个字段时应用具名脱敏；仅对 URL/title/error/diagnostic 等受保护字段调用 canonical helper。统一 `redact_secrets`、`redact_urls`、`redact_protected_text`，删除序列化后全对象递归改写。
- Verification：`journal.result.answer` 与 stdout answer 一致；source/title/attempt message 仍脱敏；research evidence 正文继续不被 journal 重复持久化。

## Open Questions

- ADR-0006 的 answer exemption 是否明确包含 Search Result Journal？ADR-0002 的 full answer 与当前实现支持“包含”。
- non-main attempt 的 `endpoint_host` 是不适用时省略，还是由 provider 写真实 host？两种都优于回填主搜索 host。

## Notes

- `net.rs` 虽大但形成有价值的深模块；Client/status/response policy/caps/SSE/MCP 有稳定共享语义，No finding。
- credential claim 的 async mutex、`spawn_blocking`、owned guard、文件锁/fsync/rename 与取消所有权符合 ADR；No finding。
- journal 独占文件、完整 JSON、`sync_all` 后 retention，严格 filename ownership；未发现半记录或误删结构问题。
- `secure_fs` 的 Unix mode/Windows owner ACL 边界清晰；未把 Windows-only 本地未运行列为风险。
- `attempt_log` 是从 `ProviderAttempt` 派生的只读安全投影，不构成重复可变领域状态。

## 主 Agent 点验

- 已复读 `SearchRecord` 回填循环及 provider attempt 创建处，确认 auxiliary facts 会被伪造；已复读递归 sanitizer、共享 redactor、canary 测试与 ADR-0002/0006，确认字段语义在 Value 阶段丢失。

## Thermo Pressure Pass

- deletable-complexity：attempt 补写层及相关调用级 plumbing 可删除。
- growth/cohesion：net 是深模块，其余文件职责集中；No finding。
- spaghetti/model：No finding。
- boundaries/types：无类型递归 sanitizer 丢失字段策略。
- canonical-ownership：ProviderAttempt 创建者应独占事实；redaction 规则应集中。
- concurrency/atomicity：credential claim 与 journal 写入/retention 具有明确 owner、顺序和 rollback 语义；No finding。
- behavior-safety：缺主+aux journal attribution 断言，现有 canary 测试固化了 ADR 冲突。

## 最终 Disposition

- Journal 伪造 auxiliary model/endpoint：**resolved**。Attempt 创建者独占事实，journal 不再从调用级主搜索上下文补写空字段。
- Untyped recursive redaction 抹平 answer policy：**resolved**。统一 credential redactor 显式保护字段，并在 journal 中保留完整 answer；answer 与受保护字段的行为均有测试。

本切面 2 条 finding 全部关闭。
