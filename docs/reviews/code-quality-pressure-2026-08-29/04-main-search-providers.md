# Provider seam 与主搜索 provider

审查范围：`src/providers/mod.rs`、`execution.rs`、`shared.rs`、`xai.rs`、`openai_compatible.rs`，以及主搜索集成测试和 engine/net/runtime/type 调用链。

## Findings

### [中严重度，高置信度] registry 的“编译期支持矩阵”实际由手写表和运行时分支共同维护

- 位置：`src/providers/mod.rs:106`、`src/providers/mod.rs:373`、`src/config/runtime.rs:566`
- Fact：`REGISTRY` 手写 provider/capability 字符串，配置验证读取该表；runtime assembly 和 builder 又各自独立 match。Tavily/Firecrawl 的 Web Search 实际是同一个 `SupplementalSearch`，通过 provider 字符串分支表达；Web Fetch 也用一个带 `ProviderId` tag 的 adapter 表达三家差异。
- Impact：registry 不是身份、seam 支持和构造入口的唯一权威；表、runtime、builder 与内部 tag dispatch 可以独立漂移。新增 provider 需要同步修改多套知识。
- Judgment：canonical ownership 分裂和虚假动态 dispatch，偏离架构规格“一 provider 一 struct、trait impl 构成支持矩阵、registry 唯一拥有构造入口”的边界。
- Evidence：现有 fixture projection 只比较 registry 字符串与 manifest，不能证明 trait/runtime/builder 一致。MainSearch/DocsSearch 确有不同 concrete types，问题不等于否定所有动态 dispatch。
- Recommendation：先确认是否继续遵守现有规格。若遵守，让每家 provider 拥有具体 seam adapter，registry descriptor 导出唯一构造入口，共享传输循环留作内部 helper；若保留 tag-driven 单实现，则删除无收益的 trait object 并同步修订规格。
- Verification：双向证明每个 registry `(provider,seam)` 有且仅有一个 constructor，且不存在未登记 constructor；新增支持不再改独立 allowlist。

### [中严重度，高置信度] 主搜索 seam 的请求类型由 xAI 具体 adapter 所有

- 位置：`src/providers/xai.rs:22`、`src/providers/mod.rs:91`、`src/providers/openai_compatible.rs:21`
- Fact：`SearchRequest` 定义在 `xai.rs`，经 `providers::mod` 重导出后成为 `MainSearch` trait 输入；OpenAI-compatible 直接横向 import xAI 模块。
- Impact：seam 抽象依赖具体 provider，xAI 成为 model/fallback 等主搜索策略字段的意外 owner；新增主搜索 provider 也必须依赖 xAI。
- Judgment：Dependency Inversion 方向错误，并违反 provider 间禁止横向 import 的架构纪律。
- Evidence：两个主搜索实现消费同一请求形状，而 xAI 已另有自己的 wire DTO，说明该类型属于 seam，不属于 xAI 协议。
- Recommendation：更名并移动为 seam-owned `MainSearchRequest`，放在 `providers::mod` 或跨层纯类型层；各 provider 只拥有 wire DTO。
- Verification：编译期确认 provider 模块无横向 import；现有 xAI role-array、OpenAI HTTP/SSE、model override/fallback 行为保持不变。

### [中严重度，高置信度] breaker 跳过的模型被伪装为一次 Timeout 失败

- 位置：`src/providers/openai_compatible.rs:149`、`src/providers/openai_compatible.rs:349`、`src/types.rs:387`
- Fact：breaker-open 模型被加入 `attempts`，记录 `error_kind: Timeout`、`duration_ms: 0`、无 transport/status，并带 `breaker_event: open`。全模型熔断时，最终错误直接从该伪失败取得。`ProviderAttempt` 只有 success/failure 二态，无法表达 skipped。
- Impact：零网络请求会被归因为 Timeout；journal/attempt 汇总把调度跳过记成 provider failure，engine 的最终 attempt 归因可被该假 Timeout 决定。
- Judgment：缺失可表达状态导致非法编码；附加 `breaker_event` 不能修复 `error_kind` 与终态归因仍按二态解释的问题。
- Evidence：breaker 测试覆盖跳过与事件，但未断言 skipped 的 `error_kind`、终态或 journal 语义。Timeout 作为“暂不可执行”近似值与 attempt 的事实语义冲突。
- Recommendation：为 attempt 增加显式 disposition（success/failed/skipped），或把 breaker/budget skip 放入独立调度记录；终态归因必须忽略 skipped。不能临时用 `error_kind=None`，因为现有消费者会解释为成功。
- Verification：全模型 breaker-open、零 HTTP 请求时 journal 明确为 skipped，terminal attribution 不由伪 Timeout 产生；部分 skip 后真实失败/成功的顺序也需覆盖。

## Open Questions

- `ProviderAttempt` 是扩展 disposition，还是把 skipped 迁移到独立调度事件？这涉及 journal 兼容性。
- 是否继续以架构规格的“一 provider 一 struct / trait impl support matrix / registry constructor owner”为现行约束？当前实现只部分遵守。

## Notes

- 主搜索 transport、normalizer、rotation/retry 和 primary-first deadline 有较强行为网，未发现额外问题。
- xAI completion 后仍读取 raw SSE 尾部符合 ADR-0006 的完整协议 cap，不作为 finding。
- MainSearch/DocsSearch 动态 dispatch 有真实异构变体；VerticalSearch 虽单实现但 Capability Seam 是明确领域边界，未按实现数量报 YAGNI。
- `openai_compatible.rs` 较大但仍围绕单协议 adapter、model/transport fallback 与 breaker 内聚。
- 未报告 rustfmt/clippy 可机械发现的问题。

## 主 Agent 点验

- 已复读 registry/runtime/builder 三套映射、WebSearch/WebFetch tag dispatch、SearchRequest 所有权、breaker skip 构造与 engine terminal attribution；三项证据均可复现。

## Thermo Pressure Pass

- deletable-complexity：MainSearch/DocsSearch trait object 有真实异构消费者；No finding。
- growth/cohesion：较大文件保持协议内聚；No finding。
- spaghetti/model：SSE/HTTP/model/transport fallback 阶段可追踪；No additional finding。
- boundaries/types：seam request 的具体-provider 所有权泄漏。
- canonical-ownership：registry、runtime 与 builder 不是单一编译期模型。
- concurrency/atomicity：请求/rotation/retry/fallback 严格顺序，breaker 锁不跨 await；No finding。
- behavior-safety：主要 transport 接缝已覆盖，缺口集中在 registry 一致性和 skipped attempt 归因。

## 最终 Disposition

- Registry compile-time support matrix：**accepted-deferred (P3)**。Builder 已集中，但 runtime 的 seam/provider 投影仍由穷尽 match 维护；当前编译期枚举与配置测试足以把它降为结构债。
- Main search request 由 xAI 所有：**resolved**。`MainSearchRequest` 已移到 provider seam owner，具体 adapter 只保留 wire DTO。
- Breaker skip 伪装 Timeout：**resolved**。Attempt 现在显式区分 disposition 与 target；跳过不再带伪造错误种类，也不会参与失败归因。

本切面最终无仍需处理的 P0-P2 finding。
