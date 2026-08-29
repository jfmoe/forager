# 类型模型、配置子系统与分类器

审查范围：`src/types.rs`、`src/classifier.rs`、`src/config/**` 及对应 config、preflight、setup、skill-contract 测试。

## Findings

### [中严重度，高置信度] 合法 inline table 进入“可读但不可改”状态，`unset` 还会假成功

- 位置：`src/config/edit.rs:290`
- Fact：严格加载接受合法 TOML inline table，定位逻辑也能在其中找到错误；但 `set_document_path` 和 `remove_document_path` 下钻时只接受普通 `Table`。前者报“不是 table”，后者静默返回，`unset_file_value` 随后仍写回原文并报告成功。
- Impact：合法配置不能通过承诺的修复通道修改；`config unset retry.max_attempts` 可退出 0 却没有删除值。
- Judgment：加载模型与编辑模型不一致，配置边界的信息隐藏失效。
- Evidence：普通 table 的 set/unset 有测试，inline table 只有错误定位测试。即使项目有意不支持 inline table 编辑，也必须一致、显式拒绝，不能假成功。
- Recommendation：让 set/unset 共用支持 `TableLike`/inline table 的路径遍历；删除返回值明确区分“已删除、值不存在、结构不支持”。
- Verification：分别对合法 inline table 执行 set/unset，断言退出码、最终 TOML 与注释保留，并覆盖普通 table/dotted-key 回归。

### [中严重度，高置信度] capability order 允许重复 provider，运行时把重复项当额外 fallback

- 位置：`src/config/validate.rs:40`、`src/config/runtime.rs:636`、`src/engine.rs:100`
- Fact：`Rule::CapabilityOrder` 的编辑与完整校验只检查非空和 registry 支持关系，不检查唯一性；runtime 为每个数组元素创建一个 `SeamEntry`，engine 随后逐项执行。
- Impact：如 `order = ["tavily", "tavily"]`，同一 provider 可被调用两次，绕过 RetryPolicy 的重试配额语义、重复计费，并扭曲预算与 attempt 归因。
- Judgment：provider order 是有序身份集合，不是重试 DSL；缺少唯一性约束让配置层复制了重试职责。
- Evidence：`search.backends` 已显式要求唯一；项目另有独立 RetryPolicy。未发现 capability order 重复项测试。
- Recommendation：在共享 `CapabilityOrder` 校验中拒绝重复 provider，并同时作用于文件、env 和 `config set`。
- Verification：文件/env 重复项退 3，`config set` 退 2；合法链仍保持配置顺序且每个 provider 至多出现一次。

### [中严重度，高置信度] `ResearchPlan` 的“严格解析”不是类型不变量

- 位置：`src/types.rs:186`
- Fact：`ResearchPlan`/`ResearchSubquestion` 公开字段并直接派生 `Deserialize`；版本、非空 decomposition、唯一 ID、非空 reason 和 capability 去重只存在于 `pub(crate) parse_json`。外部 crate 可以直接反序列化出违反这些约束的公开类型。
- Impact：库消费者或未来调用方可构造 version 2、空 decomposition、重复 ID 等非法状态并交给后续逻辑；“严格解析”的文档承诺无法由类型保证。
- Judgment：parse-but-still-validate / anemic model；Schema v1 知识分散在 serde 形状、`parse_json`、分类器 JSON Schema 和测试手工断言之间。
- Evidence：CLI caller plan 与分类器主路径当前显式走 `parse_json`，所以现有 CLI 主路径受保护，风险不升为高严重度。
- Recommendation：用私有 raw serde 形状和单一 `TryFrom<RawResearchPlan>`，或为 `ResearchPlan` 实现严格 `Deserialize`；收窄可破坏不变量的字段并提供只读访问器/具名构造器。
- Verification：直接 `serde_json::from_str::<ResearchPlan>` 必须拒绝错误版本、空 decomposition、重复或空白 ID、空白 reason，同时保留 capability 保序去重规则。

### [中严重度，高置信度] fallback policy 以裸字符串横穿多层

- 位置：`src/config/schema.rs:24`、`src/config/runtime.rs:102`、`src/app.rs:77`、`src/engine.rs:617`、`src/research.rs:59`
- Fact：闭集 `auto|off` 由字符串和 allowlist 表达，runtime、app、engine、research 继续传递或比较裸字符串；多个执行点使用“等于 off，否则即 auto”的隐式默认。
- Impact：策略增加或改名需要横跨多层同步修改；漏改会静默采用 auto 行为而不是编译失败。
- Judgment：primitive obsession + duplicated knowledge + shotgun surgery；策略身份缺少 canonical owner。
- Evidence：CLI 与配置边界会拒绝未知用户输入，因此当前风险主要是变更成本和新增内部消费者，不是已证实的外部非法值路径。
- Recommendation：引入单一 `FallbackPolicy` enum，在配置与 CLI 边界构造，后续只传枚举并删除重复 allowlist/字符串判断。
- Verification：保留 `auto/off` CLI 与配置行为测试，并穷举证明 main、supplemental、research 的变体语义一致。

### [低严重度，高置信度] 测试通过扫描 Rust 源码锁定 `include_str!` 写法

- 位置：`tests/skill_contract.rs:429`
- Fact：测试读取 `src/classifier.rs` 并断言包含精确的 `include_str!(...)` 文本。真实 classifier HTTP 请求已经另有测试，把实际 system prompt 与安装包 vocabulary JSON 做完整值比较。
- Impact：等价地移动常量、构建期嵌入或提取 helper 会在行为不变时打破测试。
- Judgment：Change-Detector Test，耦合实现语法而非可观察契约。
- Evidence：保留 asset 可解析性和实际请求体等值测试即可继续检测漂移。
- Recommendation：删除源码文本匹配，不替换为另一种源码扫描。
- Verification：改变 vocabulary 的加载方式但保持请求体不变时，行为测试应继续通过。

## Open Questions

- inline table 是否被明确排除在 `config set/unset` 支持面之外？当前无此契约，且 `unset` 假成功无论如何都需修复。
- capability order 重复项是否曾被设想为重试语法？若是，它与 RetryPolicy、attempt 和预算的现有所有权冲突。
- `ResearchPlan` 是否承诺为外部库 API？若不是，可优先收窄字段可见性；若是，应优先修复严格反序列化。

## Notes

- `types.rs` 超过 1,000 行，但规格明确把跨层形状集中到零 IO 类型层；未按行数机械报错。
- `SCHEMA` 已较好统一 leaf 路径、编辑类型、验证、视图与模板说明；未把 schema 与 Rust 配置结构并存本身判为重复。
- capability vocabulary 的身份与顺序已有值级漂移测试，未发现内容漂移。
- 未报告 rustfmt/clippy 可机械发现的问题。

## 主 Agent 点验

- 已复读 inline-table 遍历、CapabilityOrder 校验/runtime 投影、ResearchPlan 反序列化、fallback 字符串消费者和 skill-contract 源码扫描；五项事实均可复现。
- reviewer 原报告将 CLI fallback 的一处定位写为 `src/main.rs`；实际 CLI schema 所有权在 `src/app.rs`，本报告已校正。

## Thermo Pressure Pass

- deletable-complexity：源码扫描断言可净删除。
- growth-and-cohesion：`types.rs`/`runtime.rs` 仍分别围绕纯类型与配置投影，No additional finding。
- spaghetti-and-model：重复 provider 使声明式 order 意外成为隐式重试控制流。
- boundaries/types/wrappers：inline TOML 编辑、ResearchPlan 不变量、fallback primitive obsession。
- canonical-ownership：ResearchPlan 验证与 fallback 身份分散；SCHEMA/vocabulary 其余部分有明确 owner。
- concurrency/atomicity：配置锁覆盖读改写、临时文件/fsync/rename，No finding。
- behavior-safety：inline set/unset 和重复 order 缺行为测试；源码扫描属于已有行为覆盖之外的结构锁定。

## 最终 Disposition

- Inline table 编辑：**resolved**。普通 table 与 inline table 共用 `TableLike` 路径；set/unset 行为测试已覆盖。
- Capability order 重复 provider：**resolved**。统一校验拒绝重复项，并覆盖文件、环境变量与 `config set` 三个边界。
- `ResearchPlan` 类型不变量：**resolved**。字段私有化，构造器与自定义 `Deserialize` 共用严格校验。
- Fallback policy 裸字符串：**resolved**。运行时统一使用 `FallbackPolicy`，字符串只停留在配置/CLI 解析边界。
- Classifier `include_str!` 源码扫描：**resolved**。实现语法断言已删除，保留 asset 与真实请求体的机器契约。

本切面 5 条 finding 全部关闭。
