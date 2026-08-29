# 搜索引擎与 Research Evidence Pipeline

审查范围：`src/engine.rs`、`src/research.rs`、`tests/search.rs`、`tests/research.rs`、`tests/research_error.rs`、`tests/fixture_deadline.rs`，并追踪 app/provider/type 调用关系。

## Findings

### [中严重度，高置信度] Context7 候选读取失败被提前固化为终态 gap

- 位置：`src/research.rs:698`
- Fact：`fetch_candidate` 对 `Context7Library` 读取失败无条件返回 `ResearchGap`；调用方立即写入 `research_gaps`。之后即使同一子问题的 Web 候选成功，该 gap 也不会撤销。普通 URL 候选失败仅在 `known_url` 时提前生成 gap。
- Impact：同一子问题先遇到 Context7 失败、随后取得有效替代证据时仍会虚报覆盖缺口，引发不必要披露或 refetch。
- Judgment：候选级失败与子问题终态 gap 被混为一体，且状态语义被 locator/provider 形状决定；违反 ADR-0012 的“普通候选失败只记 attempt，耗尽且无证据才记 gap”。
- Evidence：known URL 是 ADR 明确的提前 gap 例外；现有 Context7 测试只覆盖它是唯一候选且最终无证据的场景。
- Recommendation：所有非 known URL 失败只返回 attempt/失败事实；候选处理结束后统一按子问题生成终态 gap。需要保留 Context7 原因时使用候选失败记录。
- Verification：同一子问题声明 docs+web，Context7 read 失败而 Web Fetch 成功时保留 attempts/evidence 且 gap closed；只有 Context7 且失败时仍生成单一终态 gap。

### [中严重度，中高置信度] locator 去重同时丢掉跨子问题覆盖归属

- 位置：`src/research.rs:178`、`src/research.rs:246`
- Fact：`Candidate` 把 fetch 身份 `locator` 与覆盖身份 `subquestion_id` 绑在一起；fetch 阶段用全局 `seen_locators`，第二个子问题遇到同一 locator 时被静默丢弃，不计 attempt、不进入 unconsumed candidates，也不关联已取得的证据。
- Impact：计划顺序决定哪个子问题独占证据；后一个子问题得到“attempting 0 candidate URLs”的虚假 gap。
- Judgment：canonical evidence identity 与 coverage association 的所有权错误合并。网络抓取依赖 locator，覆盖记账依赖 locator+subquestion，两者需要分离。
- Evidence：ADR-0008 要求同一 URL 只抓一次，ADR-0012 同时要求按子问题记账。当前测试没有覆盖多个子问题发现同一普通候选。
- Counter-evidence：若产品明确一条证据只能归属一个子问题，单归属可以有意；但 loser 候选也不应从 attempt/unconsumed 语义中消失。
- Recommendation：wave 组装时合并相同 locator 的所有子问题归属；成功后为每个归属完成覆盖，失败则所有归属保持未覆盖，不增加请求数。
- Verification：两个子问题发现同一 URL 时只发一次 fetch，但两者都有明确覆盖或未消费语义；错误路径也不得出现虚假 0-attempt。

### [中严重度，高置信度] Recovery Manifest 在 classifier attempts 合并之前写入

- 位置：`src/research.rs:432`、`src/research.rs:458`、`src/app.rs:807`
- Fact：`research::execute` 在返回前写 `summary.json`；`ResearchContext` 等 execute 返回后才 `prepend_classifier_context`。
- Impact：bare research 的 stdout verbose 与 journal 含 classifier attempt，而 Recovery Manifest 永远缺失它；classifier 降级时 manifest 只有 `plan_source`，没有错误种类、模型、rotation 或耗时链。
- Judgment：完整运行 chronology 没有单一 canonical owner，持久化时点早于 app 完成状态；违反 ADR-0015 对 manifest 包含 provider attempts 的约定。
- Evidence：现有 manifest attempt 测试使用 caller plan 或不检查 classifier；journal 只能提供部分反证，不能替代独立恢复制品。
- Recommendation：把 classifier attempts/diagnostic 作为 `ResearchRequest` 初始执行上下文传入，让 research 从完整 chronology 开始；不要由 app 在持久化后再补写。
- Verification：bare research 成功、classifier-degraded 成功、evidence terminal 均读取 summary，断言 attempts 以 classifier 开头并与 stdout/journal 顺序一致。

### [中严重度，高置信度] artifact 中途失败留下半提交目录，却返回空恢复索引

- 位置：`src/research.rs:120`、`src/research.rs:378`、`src/research.rs:922`、`src/research.rs:998`
- Fact：plan→逐 evidence→candidates→summary 均直接 `fs::write` 到目标目录。第 N 个写入失败时，先前文件已落盘或覆盖；`runtime_error` 却清空 `evidence_items`，且不会尝试生成 recovery manifest。
- Impact：复用调用方指定的 `evidence_dir` 时可留下新旧混合目录；已成功写入的新证据不会出现在错误载荷或 manifest 中，调用方只能猜测目录状态。
- Judgment：共同构成恢复单元的 artifact set 只实现了半套提交语义。问题是 rollback/已提交清单，不是并发或 backpressure。
- Evidence：现有测试覆盖根目录不可写和最后 summary 写失败，没有覆盖第 2 个 evidence 或 candidates 中途失败。
- Recommendation：明确选择一种语义：在 invocation 临时目录写完后整体提交；或每文件原子写入，并在后续失败时生成列出已提交 evidence 的 Recovery Manifest。不能继续“留下文件但返回空 inventory”。
- Verification：预建 `02-evidence.md` 同名目录稳定触发第二项写失败；断言要么无新 artifact 提交且旧文件不变，要么错误 manifest 明确列出已提交第一项；另测 candidates 写失败。

## Open Questions

- 相同 locator 是否允许同时覆盖多个子问题？ADR 同时要求唯一抓取和按子问题记账，但未明确公开的多归属形状。
- Recovery Manifest 的 `provider_attempts` 是否包括 plan classifier？stdout/journal 已将其视为完整 chronology，ADR 文义也支持纳入。
- artifact 写失败采用全有或全无，还是可恢复的部分提交？当前实现没有定义。

## Notes

- `engine.rs` 的 PrimaryFirst/SlicedEven、fallback-off、最终 attempt 归因、顺序 fan-out 与上限符合 ADR-0007/0008，未发现证据充分的问题。
- fan-out 都由 `buffered(...).collect()` 或 `join_all` 拥有并等待完成，没有 fire-and-forget；共享 `Deadline` 为绝对时点，未发现无界背压或半途丢弃。
- `research::execute` 混合四阶段，但长度本身不是 finding；上述状态后果说明后续修复应按阶段显式化状态，而不是机械抽 helper。
- 大型集成测试主要通过真实 CLI、HTTP fixture、文件 artifact 和可观察顺序验证行为，未按文件长度报错。
- 未报告 rustfmt/clippy 可机械发现的问题。

## 主 Agent 点验

- 已复读 Context7 错误分支、全局 locator 去重、classifier attempt 补写时序、artifact 写入及 ADR-0012/0015；四项证据均可复现。
- 动态接缝测试留到统一修复批次。

## Thermo Pressure Pass

- deletable-complexity：No finding。
- growth/cohesion：`research::execute` 跨四阶段是 concern，实际后果已由 findings 具体化。
- spaghetti/model：候选失败、终态 gap、fetch identity、coverage association 状态边界不清。
- boundaries/types：`Candidate { locator, subquestion_id }` 无法表达单次抓取的多覆盖归属。
- canonical-ownership：manifest 早于完整 attempt chronology 构造。
- concurrency/atomicity：fan-out ownership/顺序/上限可靠；问题在合并后的覆盖归属和 artifact rollback。
- behavior-safety：缺少四项 finding 对应的行为接缝；无 blocker。

## 最终 Disposition

- Context7 失败提前固化 gap：**resolved**。非 known 候选失败只记 attempt，终态统一按覆盖生成 gap；替代 Web 证据关闭 gap 的测试已覆盖。
- Locator 去重丢跨子问题归属：**resolved**。抓取 identity 与 `subquestion_ids` 分离，同一 locator 一次抓取可覆盖多个子问题。
- Manifest chronology 晚补 classifier attempts：**resolved**。Classifier chronology 在 research 执行与 artifact 持久化前作为初始上下文传入。
- Artifact 半提交却清空恢复索引：**accepted-deferred (P3)**。修复候选曾引入临时文件、fsync/rename、已提交前缀和失败后二次 manifest 协议；复审确认它只服务中途文件系统故障，复杂度高于本轮代码质量目标，已恢复 `HEAD` 的简单写入语义。

本切面 3 条 resolved，1 条 accepted-deferred (P3)。
