# CLI、应用编排与输出边界

审查范围：`src/app.rs`、`src/main.rs`、`src/lib.rs`、`src/doctor.rs`、`src/smoke.rs`，以及对应 CLI、doctor、setup、smoke、preflight、release-scaffolding 测试。

## Findings

### [高严重度，高置信度] JSON 飞行前错误丢失 `--output` tee

- 位置：`src/main.rs:19`、`src/main.rs:380`、`src/app.rs:378`
- Fact：`main` 在消费 `Cli` 前只保留 `uses_json_preflight_errors()` 布尔值。普通飞行前 `Err` 和 `CommandOutput::SearchPreflight` 都不再携带 `output`，最终绕过唯一的 `apply_tee` 入口。
- Impact：成功解析 `--format json --output FILE` 后，如果配置或 research plan 在飞行前失败，stdout 会返回 JSON，但目标文件不会创建；目标不可写时也不会产生约定的退出码 3 或 `output_status=failed`。
- Judgment：CLI 边界把仍有契约意义的输出状态降格并丢失，违反 `docs/spec/forager/02-cli.md` 的命令级 tee 语义。
- Evidence：`tests/preflight.rs` 覆盖 JSON 飞行前错误但未覆盖 `--output`。反证是 tee 可能被有意限定到飞行后，但规格未声明例外。
- Recommendation：在进入 `app::run` 前保留飞行前输出上下文，并让 JSON 飞行前结果统一经过 `apply_tee`；不要复制另一套文件写入逻辑。
- Verification：补真实二进制测试，覆盖可写目标镜像 stdout、不可写目标退出 3 并标注失败，以及 stdout 始终是单个 JSON 对象。

### [中严重度，高置信度] provider registry 没有实际拥有 doctor probe，smoke 又复制多套映射

- 位置：`src/providers/mod.rs:365`、`src/doctor.rs:197`、`src/smoke.rs:43`
- Fact：`ProviderRegistration` 只有身份、能力、操作和凭据标志；deep doctor 另设八分支构造与探测。live smoke 又在 case 列表、provider 映射、outage 证据、配置判定和命令构造中重复 case→provider/operation 知识。
- Impact：新增或调整 provider/case 时需要同步多处语义映射；现有集合相等测试只能证明 ID 集合一致。
- Judgment：canonical ownership 缺失导致 shotgun surgery，并偏离 `docs/spec/forager/04-architecture.md` 的 F10。
- Evidence：`ProviderId` 的穷尽 match 和逐 provider 测试提供了编译期/运行期反证，因此不升为高严重度。
- Recommendation：让 registry 拥有可执行 doctor probe 描述或构造入口；让 typed live-case definition 成为 case 映射的单一事实源。保留规格清单与运行 registry 的独立一致性门。
- Verification：断言每个 provider 唯一映射到 probe、每个 live case 唯一映射到执行/config/outage 语义，并运行 doctor、smoke 测试。

### [中严重度，高置信度] `app.rs` 是多轴变更汇合点

- 位置：`src/app.rs:37`
- Fact：该文件同时拥有 CLI schema、context 构造、search/research 编排、命令分派、smoke 网络探针、doctor Markdown、交互 setup 和错误输出模型。
- Impact：输出、setup UX、live acceptance 或搜索编排任一变化都需要理解同一 2,300 行模块，且与 `main.rs`、`doctor.rs`、`smoke.rs` 的所有权边界模糊。
- Judgment：有职责与历史证据支持的 Divergent Change，不是单纯的大文件问题；也偏离架构规格对极薄 app 组合层的约束。
- Evidence：平坦且穷尽的命令 match 本身可审计；逐命令抽 helper 会制造浅包装，因此不建议按行数机械拆分。
- Recommendation：如批准结构重构，按完整职责簇移动 setup workflow、smoke probe 和 doctor renderer，保留平坦 app dispatch。
- Verification：相关 CLI/doctor/setup/smoke E2E 契约在纯移动后应保持不变。

### [中严重度，高置信度] 单元测试锁死 CHANGELOG 文案

- 位置：`tests/release_scaffolding.rs:142`
- Fact：`breaking_output_release_documents_the_complete_caller_migration` 解析 `CHANGELOG.md` 并用三组字符串包含断言固定大量措辞，不执行代码或可执行配置。
- Impact：合法的文案和结构调整会让代码套件失败，却不能证明 CLI 契约正确。
- Judgment：Testing the Wrong Thing，且违反仓库“不为文档、提示词写单元测试”的明确规则。
- Evidence：同文件其余测试至少验证 Cargo/TOML/workflow/binary；该测试可整段删除而不损失代码行为保护。
- Recommendation：删除该测试，不以另一种文案断言替换；迁移说明完整性留给 release checklist/人工审查。
- Verification：运行 `cargo test --test release_scaffolding` 和完整套件。

## Open Questions

- `--output` 是否有意限定为飞行后结果？现有 CLI 规格支持命令级 tee，未发现例外依据。
- F10 的 registry-owned doctor probe 是否仍是有效架构决定？如不再有效，应先修订规格。

## Notes

- `main.rs` 与 `smoke.rs` 超过 1,000 行本身不是 finding。前者主要维持 bin 输出所有权；后者的 deadline、child ownership、pipe draining 与 case 执行仍属同一生命周期。
- 未发现证据充分的并发/原子性重构机会；现有 smoke 实现明确拥有 child，并并行排空 stdout/stderr。
- 未报告 rustfmt/clippy 可机械发现的问题。

## 主 Agent 点验

- 已复读 tee 控制流、provider registry、doctor probe、CHANGELOG 测试及 CLI/架构规格，四项证据均可复现。
- 动态验证留到统一修复与最终测试批次。

## Thermo Pressure Pass

- Checked：deletable-complexity / growth-cohesion / spaghetti-model / boundaries-types-wrappers / canonical-ownership / concurrency-atomicity / behavior-safety。
- 可删除复杂度：CHANGELOG 文案测试可净删除。
- 增长与内聚：`app.rs` 有语义证据支持 divergent change；未按行数机械立项。
- 边界：preflight tee 丢失输出上下文。
- canonical ownership：doctor/smoke 映射分散。
- concurrency/atomicity：No finding。
- 其余生产模式、wrapper 与并发候选未找到可靠、更小且安全的替代。

## 最终 Disposition

- JSON preflight `--output`：**resolved**。`main` 保留 preflight 输出上下文并统一进入 tee；可写与不可写目标均有二进制测试。
- Registry/doctor/smoke canonical ownership：**accepted-deferred (P3)**。Identity metadata 与 typed live cases 已有 owner，doctor probe 仍是独立穷尽分支；当前属于变更成本债，不是已观察到的行为错误。
- `app.rs` divergent change：**accepted-deferred (P3)**。职责汇合仍在，但按完整职责簇拆分的风险高于本轮收益，且没有剩余 P0-P2 后果。
- CHANGELOG 文案测试：**resolved**。纯文案断言已删除，可执行发布契约测试保留。

本切面最终无仍需处理的 P0-P2 finding。
