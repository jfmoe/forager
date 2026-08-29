# 全仓架构与代码质量横切复核

本报告独立点验前九份模块报告，将其归并为根因级 finding，并裁决重复、降级和不成立候选。

## Findings

### [高严重度，高置信度] Provider 边界不能可靠区分正文、控制状态、协议错误与调度跳过

- 位置：`src/providers/context7.rs:176`、`web_fetch.rs:194`、`exa.rs:336`、`tavily_map.rs:171`、`openai_compatible.rs:149`、`anysearch.rs:184`、`src/types.rs:387`
- Fact：正文启发式可被解释为 redirect；截断 decode 找不到正文时返回 raw envelope；核心 wire container 可被 default 成空成功；breaker skip 编码成 Timeout；acceptance operation 编码成 capability seam。
- Impact：合法正文失败、envelope 冒充正文、协议漂移被当空成功、零请求归因 Timeout、验收流量被统计为自动能力。
- Judgment：同一类 state/type boundary 失败。外部 wire 状态被压成 String、空 Vec、`Option<ErrorKind>` 和裸 seam 字符串，adapter 没形成深模块。
- Evidence：都可由普通成功 envelope 到达。合法显式空结果、structured redirect 和正文键已出现的截断恢复均应保留，因此不是要求删除合法协议差异。
- Recommendation：净删除宽泛正文扫描/raw fallback；核心字段必填；截断恢复由路径感知 provider decoder 返回 Result/Option；attempt 增 disposition；target 建模为 capability/operation 互斥类型。不要造万能 response wrapper。
- Verification：见模块报告 04-06 的负例矩阵。

### [中严重度，高置信度] 输入已校验，但不变量没有进入运行时类型

- 位置：`src/types.rs:186`、`src/config/validate.rs:40`、`src/config/edit.rs:290`、`src/config/schema.rs:24`
- Fact：ResearchPlan 严格性只在 crate-private parser；capability order 不唯一；inline table 可读不可改且 unset 假成功；fallback policy 横穿多层为裸字符串。
- Impact：未来入口可构造非法 plan；重复 provider 绕过 retry ownership；合法配置假成功；策略变体修改需散点同步。
- Judgment：Parse-but-still-validate、Primitive Obsession、duplicated policy knowledge。
- Evidence：当前 CLI 主路径大多受边界校验保护，因此维持中严重度。
- Recommendation：raw serde DTO+TryFrom 或严格 Deserialize；FallbackPolicy enum；有序唯一 order；set/unset 共用 TableLike 遍历。无需泛化 typestate。
- Verification：见模块报告 02。

### [中高严重度，高置信度] Research 把抓取身份、覆盖、终态 gap、chronology 与 artifact commit 混在同一可变流程

- 位置：`src/research.rs:68`、`src/research.rs:178`、`src/research.rs:285`、`src/research.rs:378`、`src/research.rs:432`、`src/app.rs:807`
- Fact：locator 与单一 subquestion 绑定；global dedup 丢第二归属；Context7 普通失败提前变终态 gap；manifest 写后才补 classifier attempts；artifact 逐文件直写最终目录，中途失败却清空 inventory。
- Impact：计划顺序决定覆盖；替代证据成功仍虚报 gap；三个观测面 chronology 不一致；evidence dir 可半提交且不可恢复。
- Judgment：问题不在 fan-out，而在结果合并后的领域状态与持久化提交单元。
- Evidence：`buffered`/`join_all` 保序并拥有 future，Deadline 是绝对时点；不建议重写并发模型。
- Recommendation：canonical locator + coverage associations；非-known 失败只产 failure fact，终态统一记账；classifier chronology 作为 prelude；明确全有或全无/可恢复部分提交语义。无需通用 workflow framework。
- Verification：见模块报告 03。

### [中高严重度，高置信度] 终态输出与观测事实在下游被补丁式重建

- 位置：`src/main.rs:19`、`src/main.rs:380`、`src/journal.rs:17`、`src/journal.rs:254`、`src/journal.rs:368`
- Fact：preflight 丢 output context 并绕过 tee；journal 给所有空 model/endpoint 回填主搜索值；typed result 转 Value 后递归清洗所有字符串；research chronology 同样在持久化后补写。
- Impact：preflight tee 契约失效；aux attempt 被伪造归因；journal 无法忠实保留 full answer，也无法显式决定新字段 redaction。
- Judgment：终态事实缺少 single typed owner，形成 Tell Don't Ask 与 untyped boundary。
- Recommendation：统一 OutputContext/tee；attempt 创建者独占事实；typed journal DTO + 字段级 canonical redactor；research chronology 在持久化前完整。
- Verification：见模块报告 01、03、07。

### [中严重度，高置信度] 架构规格声明 canonical owner，但 provider/app 仍由多套穷尽 match 维护

- 位置：`src/providers/xai.rs:22`、`src/providers/mod.rs:365`、`src/config/runtime.rs:566`、`src/doctor.rs:197`、`src/smoke.rs:43`、`src/app.rs:473`
- Fact：seam request 归 xAI；registry、runtime、constructor、doctor probe、smoke mappings 各自维护；app 同时拥有 schema、context、编排、setup、doctor/smoke 与 dispatch。
- Impact：新增 provider/case 或调整 UX/输出需多点同步，F10 无法由编译器证明。
- Judgment：Canonical Ownership、Dependency Inversion、Divergent Change；不是“大文件/有 match 即坏”。
- Evidence：穷尽 match 提供编译期保护；MainSearch/DocsSearch 有真实异构；tag-driven Web adapter 尚未证明拆分能净减复杂度，因此不机械照 ADR 拆 wrapper。
- Recommendation：立即移动 seam request；先决定 F10 是否继续有效；扩展 typed LiveCaseDefinition；app 只移动完整职责簇并保留平坦 dispatch。
- Verification：见模块报告 01、04。

### [高严重度，高置信度] 测试体系同时“必红”和“关键盲区过大”

- 位置：`tests/skill_contract.rs:10`、`tests/skill_contract.rs:25`、`tests/support/mod.rs:321`、`tests/config_commands.rs:305`、`.github/workflows/ci.yml:17`
- Fact：skill inventory 与 tracked tree 确定性冲突；Rust tests 冻结 Markdown/CHANGELOG/源码拼写；CLI runner/fixture 无 case-level watchdog；raw wire 是 fixture 主 API；并发测试用 sleep；Kimi JS 无测试/CI。
- Impact：当前完整套件必红，同时 deadline/child lifecycle/OAuth/401/文件/输出等关键副作用仍可在其他 checks 绿色时回归。
- Judgment：Behavior Safety Net、Resource Lifecycle、Fragile Test、shallow fixture API。
- Evidence：Rust 黑盒 CLI 覆盖总体较强，CI 20 分钟 timeout 可终止整批；但不能替代单 case kill/reap 或 JS 行为网。
- Recommendation：先修库存契约；删除文案/源码扫描；Node tests 入 CI；共享 runner 加 watchdog；CapturedRequest typed API；并发锁测试使用 barrier。
- Verification：见模块报告 08、09。

### [中高严重度，高置信度] Kimi OAuth 与返回文件缺 crash-safe ownership/commit 顺序

- 位置：`skills/kimi-datasource/scripts/kimi-datasource.mjs:285`、`:599`、`:648`、`:725`
- Fact：目录锁只有正常 finally 删除，无 stale owner recovery；工具响应在判断语义失败前写 files。
- Impact：异常终止可永久阻塞 refresh；失败调用可留下调用方不可见的半应用文件。
- Judgment：Resource Lifecycle 与副作用提交顺序错误。
- Evidence：普通 throw 会 finally，后端也可能保证失败不带 files，因此不升高严重度。
- Recommendation：内核锁或 PID+nonce/确认死亡回收；先 parse/classify，后执行 success 文件计划。
- Verification：见模块报告 09。

## Prior-Report Adjudication

- 合并：01+04 的 provider ownership；02 的 plan/config policy；03 的四项 research 状态；04-06 的 provider boundary；01+03+07 的 terminal observation；01/02/08/09 的测试问题；09 的 Kimi lifecycle。
- 降级：`app.rs` 体积只作 divergent-change 证据；tag-driven provider 只作待决架构问题；preflight tee 从单项高降为横切中高；release target 多副本降为 concern；YAML substring 优先级低于必败 inventory、JS 零覆盖与 watchdog。
- 丢弃：不按 1,000 行拆 main/smoke/types/net/tests；不取消 MainSearch/DocsSearch/VerticalSearch traits；不造万能 provider DTO；不重写 Rust fan-out；不改 credential claim、journal retention 或 smoke child 生命周期；不报告机械 lint。

## Repair Order

1. 恢复安全网：skill inventory、删除错误层级测试、watchdog、Kimi Node tests/CI。
2. 修 provider data/control boundary：Context7、Web Fetch raw envelope、核心 wire 字段。
3. 修 Kimi commit 顺序与 stale lock。
4. 引入 attempt disposition/typed target，再删 journal fact 回填并修 AnySearch target。
5. 统一 terminal context：preflight tee、typed journal redaction、research chronology。
6. 修 research locator/coverage/gap/artifact commit。
7. 收紧 ResearchPlan/FallbackPolicy/order/inline-table invariants。
8. 最后处理 registry/app 架构；先决策 ADR，再移动完整职责簇。

## Thermo Pressure Pass

- deletable-complexity：Context7 text redirect、Web Fetch raw fallback、journal fact 回填、文案/源码扫描测试可净删除。
- growth/cohesion：app 有 divergent change；Kimi/net/types 体积本身 No finding。
- spaghetti/model：research 与 provider data/control 状态需要显式模型。
- boundaries/types：Findings 1、2、4。
- canonical-ownership：Findings 4、5。
- concurrency/atomicity：Research artifacts、Kimi lock/files 有 finding；Rust fan-out/credentials/journal retention/smoke lifecycle No finding。
- behavior-safety：Finding 6；Rust 主搜索/retry/deadline/主要 CLI 已有较强安全网。

## 主 Agent 点验

- 已逐份点验 01-09 的关键路径；本报告对合并、降级和丢弃结论与实际代码/ADR 一致。

## 最终 Disposition

- Provider data/control/protocol boundary：**resolved-common / accepted-rare**。Context7、Exa、Tavily Map、常规 Web Fetch DTO 与 attempt disposition/target 已关闭；仅超过 4 MiB 后截断且 envelope 畸形的 raw fallback 保持 `HEAD` 行为，列为 P3。
- Runtime/config invariants：**resolved**。ResearchPlan、FallbackPolicy、唯一 order 与 inline-table 编辑进入统一类型或边界。
- Research identity/gap/chronology/commit：**resolved-common / accepted-rare**。多归属 candidate、终态 gap 与初始 chronology 形成一致状态流；artifact 中途文件系统失败的提交恢复协议不纳入本轮，列为 P3。
- Terminal observation 下游补丁重建：**resolved**。Preflight tee、attempt 自有归因、journal answer policy 与 research chronology 均在持久化前完成。
- Canonical owner/app architecture：**accepted-deferred (P3)**。Seam request 已归位；registry/runtime/doctor/smoke 多 owner、compile-time matrix 与 `app.rs` divergent change 仍是结构债，但无已观察到的 P0-P2 后果。
- 测试“必红”与关键盲区：**resolved-common / accepted-rare**。Inventory、文案测试、Node 安全网与共享 direct-child watchdog 已关闭普通路径；raw-wire fixture、deterministic lost-update、后代/Drop cleanup 留作 P3。
- Kimi lock/file commit：**resolved-common / accepted-rare**。失败响应先分类后写文件已关闭；SIGKILL/主机崩溃后的 stale lock recovery 保持 `HEAD` 行为，列为 P3。

范围收口复审补充：删除截断 JSON parser、artifact commit/recovery 协议、Kimi 内核锁状态机和跨平台 process-tree watchdog 后，三名原 reviewer 均确认常见路径保留完整；唯一残余类型一致性项 `doctor` 已改为消费 `AttemptDisposition::Succeeded`。

横切复核最终无本轮仍需处理的 P0-P2；罕见生命周期与文件系统故障均明确记录为 accepted-deferred P3，而非声称已修复。
