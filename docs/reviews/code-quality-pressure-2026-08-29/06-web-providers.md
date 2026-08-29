# Web Search、Web Fetch 与 Map provider

审查范围：`src/providers/supplemental.rs`、`web_fetch.rs`、`tavily_map.rs` 及 fetch/map 集成测试，并追踪 engine/net/type 边界。

## Findings

### [高严重度，高置信度] 截断恢复把 provider 传输包装冒充 Normalized Fetch Content

- 位置：`src/providers/web_fetch.rs:194`、`src/providers/web_fetch.rs:279`、`src/net.rs:108`
- Fact：完整 DTO 解码失败且响应达到 4 MiB 时，无条件改走 `decode_truncated`；三个 provider 找不到目标键时都返回整个 JSON 前缀。共享 `json_string_prefix` 只找第一个同名文本键，不表达 Jina `data.content`、Tavily `results[0].raw_content`、Firecrawl `data.markdown` 的结构路径。多 MiB 单行 JSON 会通过通用 thin gate。
- Impact：传输 envelope、元数据或错误形状可被稳定误标为正文。raw fallback 也绕过 provider 解码后的安全边界：`FetchOutcome.url` 会脱敏，但 content 作为正文豁免不脱敏，超大 2xx envelope 中的敏感 URL 可进入 stdout/tee。
- Judgment：传输 DTO 与领域正文的信息隐藏失效；浅层通用 helper 无法承载 provider 路径不变量，raw fallback 删除了“完成 provider 解码后才进入质量门”的边界。
- Evidence：截断 reader 返回 UTF-8 前缀；不完整 JSON 触发 fallback；缺目标键返回 raw body；thin gate 不拒绝多 MiB 单行；engine 将其直接装入 content。现有测试只覆盖正文键在前缀内的合法正例。
- Recommendation：让截断解码返回 `Result`/`Option`；只有明确提取到 provider-owned 正文路径才成功，找不到则 Runtime 并 fallback。删除三个 `unwrap_or_else(|| body.to_owned())`。路径感知恢复应属于 provider adapter，而非通用 net helper。
- Verification：覆盖目标字段不在前缀、前置无关同名键、envelope URL canary；均不得返回 raw JSON或泄漏。保留正文前缀可恢复和恢复后仍薄的现有正例。

### [中严重度，高置信度] Map DTO 把畸形响应与合法空站点折叠为同一成功状态

- 位置：`src/providers/tavily_map.rs:171`
- Fact：`base_url`、`results`、`response_time` 全部 `#[serde(default)]`；`{}`、error envelope 或缺核心字段的 2xx JSON 都能成为成功 `MapOutcome`，results 字符串也未验证 HTTP(S) URL。
- Impact：上游 schema 漂移/错误 envelope 被静默解释为合法空站点，调用方无法区分协议失败；重试和 attempt 诊断也被绕过。
- Judgment：HTTP boundary DTO 让非法状态可表示，把“协议有效且空”与“缺失协议字段”压成同一值。
- Evidence：合法空结果测试明确提供 `base_url` 与 `results: []`，无需依赖默认；现有 fixture 表明 `response_time` 可能可省略，这是只保留该字段可选的反证。
- Recommendation：将 `base_url`/`results` 设为必填；`response_time` 按正式契约使用 `Option<f64>` 或局部默认。边界验证 base/result 为带 host 的 HTTP(S) URL。
- Verification：2xx `{}`、缺核心字段、非字符串元素、非 HTTP(S) result 均 Runtime；显式空数组仍成功。

## Open Questions

- Tavily Map 正式 wire contract 是否允许缺失 `response_time`？答案不影响 `base_url/results` 必填结论。
- 若正文键在 4 MiB 前尚未出现，是否真的需要成功恢复？需要先定义路径明确且永不返回 envelope 的流式契约。

## Notes

- WebSearch/WebFetch 的 tag dispatch 偏离“一 provider 一 struct”，但当前未证明拆分能净减少概念或维护成本，因此本切面不重复立项。
- Tavily Map 保持唯一 concrete operation，没有虚构 seam，符合 YAGNI。
- deadline、attempt timeout、retry 与 credential rotation 统一委托共享执行器，未发现所有权分裂。
- Web Fetch 质量门由 engine 唯一拥有；除 raw fallback 外分层清晰。
- provider wire 测试与行为矩阵比例总体合理，主要缺口正是上述边界失败状态。

## 主 Agent 点验

- 已复读 capped reader、浅层 `json_string_prefix`、三个 raw fallback、thin gate、FetchOutcome 投影，以及 Map 全默认 DTO/显式空结果测试；两项结构机制均可复现。

## Thermo Pressure Pass

- deletable-complexity：三个 raw-body fallback 可净删除，不增加抽象层。
- growth/cohesion：目标文件低于 1,000 行且总体内聚；No finding。
- spaghetti/model：No additional finding。
- boundaries/types：传输 envelope 与领域正文共享 String 状态；Map 缺字段失败与合法空结果无法区分。
- canonical-ownership：路径感知截断恢复应归 provider adapter；其余质量门有明确 owner。
- concurrency/atomicity：provider chain 有序、fan-out 有界保序，无共享可变状态；No finding。
- behavior-safety：缺 target-key-absent/wrong-key/canary 与 2xx malformed Map 矩阵。

## 最终 Disposition

- Truncated fetch 返回 raw envelope：**accepted-deferred (P3)**。路径感知修复需要一套手写不完整 JSON parser，且只在超过 4 MiB 后截断并缺少正确正文路径时生效；该候选实现已删除，保持 `HEAD` 行为，后续如有真实需求应独立设计流式 decoder。
- Tavily Map 畸形 DTO 变空成功：**resolved**。核心字段必填且 URL 必须是带 host 的 HTTP(S)，合法空 results 仍保留。

本切面 1 条 resolved，1 条 accepted-deferred (P3)。
