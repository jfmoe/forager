# 测试架构与可重构性

审查范围：全部 `tests/**/*.rs` 与 `src/**` 内 `#[cfg(test)]` 模块，共 336 个 integration tests、92 个 unit tests。

## Findings

### [高严重度，高置信度] CLI 测试驱动没有进程级 watchdog

- 位置：`tests/support/mod.rs:321`、`tests/support/mod.rs:157`
- Fact：共享 `RunEnvironment` 直接调用 `Command::output()`/`wait_with_output()`，没有超时、kill 与 reap。AnySearch/Context7 还各自复制了一套同构无界 runner。fixture join 和已接受 socket 的阻塞读取也无可靠外层时限。
- Impact：被大量测试保护的 hard deadline/异步取消一旦回归，测试不会快速失败，而会挂到 CI job 的 20 分钟上限；具体 case 与剩余失败丢失。
- Judgment：Resource Lifecycle / Erratic Test。harness 没有比被测 deadline 更外层、更可靠的生命周期边界；重复 runner 使修复容易漏面。
- Evidence：CI job timeout 只能杀整批，不会形成可诊断单测失败，因此不能替代 per-process watchdog。
- Recommendation：在唯一共享 command driver 中加入固定测试级 watchdog，超时后 kill、wait/reap 并报告 argv/stdout/stderr；AnySearch/Context7 合并到该 owner。socket read 与 fixture join 同样设上界。
- Verification：用永不退出的 harness 自测证明上界内 kill/reap；partial-request fixture 停住时 join 也必须明确失败。

### [中严重度，高置信度] HTTP fixture 以 raw wire String 作为主 API

- 位置：`tests/support/mod.rs:157`
- Fact：`finish`/`finish_all` 只返回 String/Vec<String>，没有 method、path、headers、JSON body 的结构表示。11 个文件有大量 `contains`、`starts_with`、手工 CRLF 切割。
- Impact：断言只能证明字节片段存在，不能证明字段层级、header 唯一性或 method/path 组合；wire 格式变化造成批量无行为修改，错误嵌套又可能 false-green。
- Judgment：Shallow Fixture API / Primitive Obsession / Over-specified Test。
- Evidence：少数协议测试确实需要 raw wire，因此 raw 应保留为 escape hatch，而不是删除。
- Recommendation：引入 `CapturedRequest { method, path, headers, body, raw }`，提供 `json_body()` 和不区分大小写 header 查询；普通 contract 默认走 typed API。
- Verification：迁移各 transport 代表测试，并用错误层级、重复 header、错误 path 的 mutation 证明断言会失败。

### [中严重度，高置信度] 并发配置写测试用 30ms sleep 代替同步

- 位置：`tests/config_commands.rs:305`
- Fact：测试持有 `.config.lock`，spawn 两个进程后仅 sleep 30ms 就释放锁，没有证明两个子进程都到达锁等待点。
- Impact：慢启动机器上测试可退化为顺序写；锁被删除、提前释放或读写边界移动时仍可能通过，无法可靠保护 read-modify-write 无丢失不变量。
- Judgment：Erratic / Non-predictive Concurrency Test；sleep 不是 barrier。
- Evidence：spawn 提高重叠概率但不建立 happens-before；项目其他测试已有显式锁探测模式。
- Recommendation：在更低 config-edit 接缝用 barrier/latch 控制 writer 同时到达“已读取、待提交”边界；integration 层保留锁超时/CLI wiring 测试。
- Verification：高负载和单线程调度下重复稳定；临时移除锁或缩小锁范围时必须稳定失败。

### [中严重度，高置信度] Rust 测试冻结文档与历史 CHANGELOG 措辞

- 位置：`tests/skill_contract.rs:25`、`tests/release_scaffolding.rs:143`
- Fact：多条测试读取 Markdown、压平空白后检查成组短语；CHANGELOG 测试永久扫描已发布版本文案。它们不执行 CLI 或 skill 行为。
- Impact：同义改写、段落移动、翻译会破坏 Rust CI，语义错误只要保留关键词仍可 false-green。
- Judgment：Change-Detector / Testing the Wrong Thing，并违反仓库“不要给文档、提示词写单元测试”的明确规则。
- Evidence：Clap 公共面、JSON 示例可解析、安装结构等机器契约仍有测试价值，不在删除范围；workflow/Cargo 属于可执行交付政策，也不在范围。
- Recommendation：删除纯文案 substring 与历史 CHANGELOG 文案测试；保留安装目录、JSON schema/example、Clap/public-surface 等机器可判定 contract。文案完整性进入 review/release checklist 或真实 skill 验收。
- Verification：不改变语义的文案改写不再影响 Rust 测试；skill 安装与调用验收仍通过。

### [中严重度，高置信度] 通过扫描 production 源码验证 `include_str!` 拼写

- 位置：`tests/skill_contract.rs:416`
- Fact：测试读取 `src/classifier.rs`，断言精确包含某个 `include_str!` 相对路径。已有 unit test 验证编译后的 vocabulary identity/order，另有真实 classifier HTTP 请求将 prompt 与 asset 做结构等值比较。
- Impact：移动 constant、改用 `concat!`、生成时嵌入或调整路径都会在行为不变时失败；该扫描又不证明运行时真正消费内容。
- Judgment：Change-Detector Test，落在错误层级。
- Recommendation：删除 production 源码文本扫描和精确 include 语法断言，保留编译 asset 与真实请求行为测试。
- Verification：机械改写 include 表达但请求 prompt 不变时，行为测试继续通过。

## Open Questions

- 是否有 CLI integration test 挂到 job timeout 的历史记录？只影响 watchdog 排期，不改变结构事实。
- 是否存在纯文档测试的书面例外？当前最近的 AGENTS 是绝对禁止。

## Notes

- `search.rs`/`research.rs` 虽大但主要按 CLI 行为场景平铺，未按行数报 Growth/Cohesion。
- provider config/expected JSON/AAA 片段多数服务不同场景，按 DAMP 保留，未把表面相似误报为 DRY。
- 真实延迟测试以命令 timeout 证明并发/deadline 是合理 offline-E2E 取舍；只报告缺 happens-before 的 30ms 测试。
- 整体 black-box CLI 安全网较强，未发现必须阻断当前重构的公共行为完全无保护。

## 主 Agent 点验

- 已复读共享/重复 runner、fixture join/read、30ms 并发测试、全部纯文案测试、Clap/JSON 机器 contract 与 classifier 实际请求测试；五项证据及安全删除边界均可复现。

## Thermo Pressure Pass

- deletable-complexity：纯文档 substring、CHANGELOG 文案和 classifier 源码扫描可净删除。
- growth/cohesion：大型测试文件按场景平铺；No finding。
- spaghetti/model：未发现隐藏不同 setup/assertion 的条件化参数测试。
- boundaries/types：raw request String 是浅 fixture 边界。
- canonical-ownership：command execution/isolation 有 `RunEnvironment` owner，但 AnySearch/Context7 保留同构副本。
- concurrency/atomicity：30ms sleep 无 happens-before；其余候选未发现响应错配。
- behavior-safety：CLI runner 缺外层 watchdog 是主要缺口。

## 最终 Disposition

- CLI 测试 watchdog：**resolved**。全部普通 CLI 调用进入共享 runner；direct child 受 deadline、kill/wait/reap 约束，stdout/stderr 并行 drain，跨平台 hung-child 行为测试为 2/2。后代继承 pipe、超大未消费 stdin 与 spawn 后 panic cleanup 明确作为罕见 P3 残余，不保留进程树框架。
- Raw wire fixture API：**accepted-deferred (P3)**。多数调用只用于同步、计数或协议级 wire 断言；当前没有足以支持数百处迁移的 P2 行为风险，后续按需增加 typed accessor 并保留 raw escape hatch。
- 30ms 并发配置测试：**accepted-deferred (P3)**。非确定性测试已删除，确定性锁争用测试保留；若未来重构锁范围，应先补 barrier/hook 的 lost-update characterization。
- Markdown/CHANGELOG 文案测试：**resolved**。纯文案扫描已删除，结构化机器契约保留。
- Classifier source/include 扫描：**resolved**。源码文本扫描已删除，由真实请求中的 vocabulary 等值测试替代。

本切面最终无仍需处理的 P0-P2 finding。
