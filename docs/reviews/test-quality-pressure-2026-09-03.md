# 测试质量压力审查

审查日期：2026-09-03

审查目标：当前仓库全部 Rust 自动化测试、测试夹具和测试运行入口。主要判断标准是测试是否对行为变化敏感、对结构变化不敏感，以及每个测试是否提供独立的回归保护或文档价值。

## 范围与方法

| 模块 | 测试数 | 主要文件 |
|---|---:|---|
| Research / evidence / core | 93 | `tests/research.rs`、`src/core/*`、`src/evidence/*` |
| Search / main providers | 100 | `tests/search.rs`、classifier、xAI、OpenAI-compatible、shared |
| 专用 provider | 84 | AnySearch、Exa、Context7、Tavily Map |
| Web Fetch | 19 | `tests/fetch.rs` 与三家 provider fixture |
| CLI / config / setup / preflight | 74 | 六个 integration target 与 config/CLI unit tests |
| Doctor / smoke / harness | 43 | doctor、smoke、watchdog、fixture deadline、support |
| 基础单元测试 | 27 | net、credentials、types、redact、catalog |
| Release / skill contract | 19 | release scaffolding、skill contract |
| **总计** | **459** | **124 unit + 335 integration** |

审查采用 Full Review 加 Thermo Pressure Pass。逐模块 reviewer 只提出候选；以下结论由主 Agent 重新读取生产实现、规格、ADR、测试和反证后裁定。未以覆盖率百分比或文件行数直接产生 finding。

## Findings

### 行为 oracle 与 false-green

- [高，高置信度] `src/core/attempt_trace.rs:215` 归因真值表由被测实现生成期望值
  - Fact：测试调用私有 `error_priority` 计算 `terminal_kind` 的期望值。
  - Impact：优先级实现和测试 oracle 可以同步出错，Tier 0 终态归因仍保持绿色。
  - Judgment：Change-Detector Test。
  - Evidence：架构规格独立规定 `Network < Timeout < RateLimited < QuotaExhausted < Auth < Parameter < Runtime < Quality < Evidence`；现有 integration tests 只覆盖少数组合。
  - Recommendation：测试内使用规格顺序作为独立 oracle，保留 9x9 穷举。
  - Verification：互换任意两种错误优先级时测试必须失败。
  - Disposition：接受，本轮修复。

- [高，高置信度] `src/core/attempt_trace.rs:265`、`src/core/chain.rs:810` “最后一项”语义没有可区分输入
  - Fact：两个测试分别只放入一个成功 attempt、一个待标记 attempt。
  - Impact：去掉反向遍历或把 `last_mut` 改为 `first_mut` 仍可全绿。
  - Judgment：测试名覆盖的行为大于输入实际保护的行为。
  - Evidence：生产实现明确使用 `.rev()` 和 `last_mut()`；其他测试不区分同一 seam/step 内的首尾项。
  - Recommendation：各加入两个可区分 attempt，并同时断言未被选择的项保持不变。
  - Verification：方向 mutation 必须失败。
  - Disposition：接受，本轮修复。

- [高，高置信度] `src/capabilities/providers/xai.rs:435` 手工重建请求而未经过生产装配
  - Fact：测试直接构造 `ResponsesRequest`，没有调用 `send_once`、`search` 或 `probe`。
  - Impact：真实 ModelProbe 的 role、input、instructions 或 tools 接错时仍可全绿。
  - Judgment：Change-Detector / false-green。
  - Evidence：普通 Search 的真实请求已由 `tests/search.rs` 部分覆盖；`tests/doctor.rs` 已有真实 xAI probe fixture，是更合适的替代保护。
  - Recommendation：增强真实 doctor probe 的结构化 request 断言后删除手工装配单测。
  - Verification：修改真实 probe body 任一关键字段时 doctor fixture 必须失败。
  - Disposition：接受，本轮修复。

- [高，高置信度] `tests/fetch.rs:193` UTF-8 边界用例实际截在 ASCII 区域
  - Fact：JSON 前缀未计入填充长度，`EUR` 多字节字符位于 4 MiB cap 之后。
  - Impact：删除 UTF-8 安全回退仍可通过。
  - Judgment：错误夹具导致虚假覆盖。
  - Evidence：该测试仍保护截断恢复和 diagnostic，因此应修正而不是删除。
  - Recommendation：按响应前缀字节数放置多字节字符，并精确断言正文结尾和 replacement character。
  - Verification：任意字节截断 mutation 必须失败。
  - Disposition：接受，本轮修复。

- [高，高置信度] `tests/fetch.rs:13` HTML 密度门没有独立正向拒绝用例
  - Fact：现有 thin HTML 都先命中 `<200` 长度线；250 字符单行只在 PDF 豁免场景出现。
  - Impact：删除 `unique_lines <= 3 && chars < 500` 整条规则仍可全绿。
  - Judgment：关键等价类缺口。
  - Evidence：`is_thin` 是纯决策逻辑，适合局部真值表；PDF integration 保留接线保护。
  - Recommendation：在 engine 单测中用同一 250 字符正文对照 HTML 拒绝与 PDF 接受。
  - Verification：删除密度判定只应打红 HTML case。
  - Disposition：接受，本轮修复。

### 请求、配置与安全边界

- [中，高置信度] `tests/context7.rs:53`、`:117` 未验证 MCP arguments
  - Fact：成功测试只检查 tool 名，不检查 `libraryName`、`libraryId`、`query` 的结构和取值。
  - Impact：字段名、嵌套或值对调时 fixture 仍返回预制成功响应。
  - Judgment：provider wire contract 的请求半边缺失。
  - Recommendation：复用 `request_json` 精确断言 `/params/name` 和 `/params/arguments`。
  - Verification：交换或漏发字段时测试失败。
  - Disposition：接受，本轮修复。

- [中，高置信度] `tests/tavily_map.rs:12` 未保护合法边界 `1/5` 与 `1/500`
  - Fact：成功路径只覆盖内部值 2/10，非法路径覆盖 0/6 和 0/501。
  - Impact：合法范围被误收窄时全绿。
  - Judgment：边界等价类缺口。
  - Recommendation：用两个具名 case 精确断言合法端点原样进入 JSON body。
  - Verification：收窄任一上下界时对应 case 失败。
  - Disposition：接受，本轮修复。

- [中，高置信度] `tests/anysearch.rs:846` 的候选 fixture 大部分字段没有消费者
  - Fact：测试只消费 domain、sub-domain、provenance 和 parameter schema；valid request/result 与两类 error 样例没有进入任何 transport 路径。
  - Impact：未消费字段可以任意漂移，却仍被文档描述为已版本化的 transport evidence。
  - Judgment：Mystery Guest / false confidence。
  - Recommendation：将 fixture 收窄为实际受保护的 schema artifact，并同步把文档中的 transport 声明降为 schema 声明；真实请求/错误/结果留给独立 live acceptance。
  - Verification：fixture 身份或 schema 变化会使 manifest contract 失败，其他未验证行为不再被声称已覆盖。
  - Disposition：接受，本轮修复。

- [高，高置信度] `src/infra/config/schema.rs:575` 只按字段类型抽查 schema wiring
  - Fact：47 条 `path -> get/get_mut` 绑定由宏参数分别声明，同类型字段可互换；现有测试只抽查六个类型代表。
  - Impact：`config list/set/env` 可自洽地显示或修改错误字段，而 runtime 读取真实字段。
  - Judgment：配置唯一真相源的 wiring 缺口。
  - Evidence：路径集合测试只证明名称齐全，不证明名称绑定正确。
  - Recommendation：每个 leaf 使用唯一 sentinel，分别验证 getter 与 mutator 最终落到 leaf.path；替换两个代表性测试。
  - Verification：互换任意两个同类型 binding 时失败。
  - Disposition：接受，本轮修复。

- [高，高置信度] `tests/config_permissions.rs:8` 等权限测试受 umask 偶然值影响
  - Fact：Unix 用例从新目录/文件开始，没有先构造 0777/0666 的既有宽权限对象。
  - Impact：删除“重新收紧已有对象”的逻辑后，在 umask 077 环境仍可全绿。
  - Judgment：安全边界 false-green。
  - Recommendation：夹具显式创建宽权限目录、config 和 lock，再执行 primitive 与真实 CLI 写路径。
  - Verification：删除 reassert chmod 时稳定失败。
  - Disposition：接受 Unix 修复；Windows ACL 只由远端 CI 验证。

- [中，高置信度] `tests/setup.rs:151` 六个 supplemental provider 只验证三个写入
  - Fact：向导遍历六家 provider，但测试对 Context7、Tavily、Firecrawl 输入为空且无断言。
  - Impact：三条 prompt-to-path 映射错误时全绿。
  - Judgment：矩阵不完整，不是矩阵过度。
  - Recommendation：一次向导输入六个唯一 canary 并逐项断言。
  - Verification：任一 provider path 对调时定位到具体 provider。
  - Disposition：接受，本轮修复。

- [中，高置信度] `src/infra/types.rs:1241` 与 `src/capabilities/net.rs` 错误分类没有独立完整表
  - Fact：AttemptErrorKind 测试只断言结果不是 Config；HTTP status 只抽查少数状态。
  - Impact：认证、重试、轮换、序列化名和 family 可错误映射而保持绿色。
  - Judgment：类型穷尽不等于语义正确。
  - Recommendation：用显式表验证 attempt-to-error、name、serde、family、retry、rotate；status 表覆盖所有等价类和大小写 quota。
  - Verification：逐项 mapping mutation 只打红对应 case。
  - Disposition：接受，本轮修复。

- [高，高置信度] `src/capabilities/credentials.rs:258` 持久化游标的局部安全网不足
  - Fact：唯一 unit test 只覆盖取消后的 mutex 生命周期；文件事务主要依赖 Exa integration 间接保护。
  - Impact：首次/轮询、provider 隔离、key 数缩减取模、坏状态复位和文件权限的失败定位差。
  - Judgment：高风险文件事务缺少快速、局部行为测试。
  - Evidence：integration tests 已覆盖跨进程共享、锁忙和单 provider 损坏，因此不重复整套 E2E。
  - Recommendation：为 `claim_persistent_index` 增加三个聚焦测试：轮询/隔离、损坏/缩容、私有权限与无凭据内容。
  - Verification：定向单测及重复并行运行。
  - Disposition：接受，本轮修复。

### Harness、确定性与测试结构

- [中，高置信度] `tests/support/mod.rs:57` 预设响应 fixture 无法发现第 N+1 个请求
  - Fact：listener 收满 N 个连接后立即关闭；`finish_all().len() == N` 是构造保证，不是 exactly-once 证明。
  - Impact：被生产代码忽略错误的额外计费请求可以 false-green。
  - Judgment：Shallow Fixture API / non-predictive interaction test。
  - Recommendation：收满预设响应后保持 listener 到 `finish_all()`，记录并拒绝额外请求；完成时断言实际数量。
  - Verification：增加一个“成功后额外请求且忽略结果”的 fixture 自测。
  - Disposition：接受，本轮修复。

  完整套件启用精确计数后立即暴露两处旧 Mystery Guest：research 测试用 listener 关闭隐式模拟 fetch 失败，smoke P2 测试只为 9 个既定 probe 中前三个配置响应。两处均已改为显式完整响应序列；未改生产请求数。

- [中，高置信度] `tests/command_watchdog.rs:12` 只证明 panic，不证明 child 被 kill
  - Fact：删除 `kill/wait`、直接 panic 时现有测试仍绿。
  - Impact：测试进程可遗留孤儿和 pipe reader。
  - Judgment：Resource Lifecycle 的 surviving mutation。
  - Recommendation：helper 先写 started marker，延后写 survived marker；watchdog 返回后确认 started 存在且 survived 永不出现。
  - Verification：删除 kill mutation 时测试稳定失败。
  - Disposition：接受，本轮修复。

- [中，中高置信度] `tests/search.rs:1666`、`:2077` 用 2 秒 sleep 对 3 秒 budget 推断并发
  - Fact：没有同步屏障证明所有请求同时到达。
  - Impact：机器负载可混入正确性判断。
  - Judgment：值得保留的并发行为使用了不稳定 oracle。
  - Recommendation：增加“全部连接到达后统一释放”的同步 fixture，结果顺序另行断言。
  - Verification：串行 mutation 必须确定性失败。
  - Disposition：接受，本轮修复。

- [中，高置信度] `src/core/chain.rs:422` 测试模块大于生产实现
  - Fact：约 679 行测试与 421 行实现同处 1100 行文件。
  - Impact：核心链实现导航成本高，新增测试继续扩大单文件。
  - Judgment：Growth/Cohesion；行为仍属于同一 owner，不应拆成多个 Cargo target。
  - Recommendation：原样移动到 sibling `chain_tests.rs`，保留模块名和私有访问。
  - Verification：测试枚举名称和数量不变。
  - Disposition：接受，本轮修复。

### 可删除的负价值与重复测试

- [中，高置信度] `tests/release_scaffolding.rs:22` 源码行数 ratchet 是结构 change-detector
  - Fact：一行无行为变化也会失败；压缩格式又可掩盖职责增长。
  - Impact：把压力审查的“1000 行触发调查”错误机械化成代码测试。
  - Recommendation：净删除该测试和专用 helper/常量，架构判断回到 review。
  - Disposition：接受，本轮修复。

- [中，高置信度] `tests/exa_search.rs:352`、`:418`、`:473` 重复共用 execute 路径
  - Fact：Search/Similar 共享 deadline、HTTP error 与空结果 decode；operation 分支只决定 endpoint/body/target/字段投影。
  - Evidence：Similar golden path 已保护其 endpoint/body/output；Search 分别保护空结果、Auth、deadline；Similar rotation 保留 operation attribution。
  - Recommendation：删除三项 Similar 重复 case。
  - Verification：运行 Exa target；保留的 Similar golden/rotation 与 Search 边界均通过。
  - Disposition：接受，本轮修复。

- [中，高置信度] `tests/anysearch.rs:878` 只断言源码 manifest 文件没有被修改
  - Fact：生产实现只用 `include_str!` 读取，仓库中没有写路径。
  - Impact：测试不杀任何现存实现 mutation，并耦合源文件状态。
  - Recommendation：删除；保留 manifest status/output 契约。
  - Disposition：接受，本轮修复。

- [低，高置信度] 多处断言或测试归属没有额外 mutation 价值
  - Fact：`tests/preflight.rs:49` 期望值自引用；`tests/fetch.rs:110` 在已精确断言 Quality 后再断言非 Evidence；catalog 测试手写标准迭代器 lookup；release 全文件 `contains("npm")`；engine 测试直接测试 net 的 `slice_budget`。
  - Recommendation：删除死断言；把 `slice_budget` 真值表移回 net；不新增 wrapper。
  - Disposition：接受，本轮修复。

### Release 与静态契约

- [中高，高置信度] `tests/release_scaffolding.rs:315` 用 shell/PowerShell substring 冒充制品行为验证
  - Fact：注释、echo 或不可达分支包含关键字时测试仍绿。
  - Impact：PR CI 对正式发布 gate 提供错误信心。
  - Judgment：Testing the Wrong Thing。
  - Evidence：正式 release workflow 会真实执行验证块；workflow 图、matrix、env wiring 仍是合法结构契约。
  - Recommendation：本轮删除命令 substring，测试改名为只声明结构 wiring；后续把命令提取为可执行脚本并用 fixture archive 验证。Windows 行为仍需远端 CI。
  - Verification：结构测试继续保护 needs/matrix/env/upload；不得再声称验证 shell 语义。
  - Disposition：部分接受；去除 false confidence，本轮不做跨平台发布脚本迁移。

- [中，高置信度] `tests/release_scaffolding.rs:203` cargo-dist 版本有两个 owner
  - Fact：workflow 下载 URL硬编码 0.31.0，测试不与 `dist-workspace.toml` 比较。
  - Impact：升级配置但漏生成 workflow 时全绿。
  - Recommendation：从 dist config 读取版本并构造期望 URL。
  - Verification：单改配置版本时测试失败，重新生成 workflow 后恢复。
  - Disposition：接受，本轮修复。

- [中，高置信度] `tests/skill_contract.rs:8` “installable” 只验证目录和文件存在
  - Fact：frontmatter 缺失、非法或 `name` 改错仍可通过。
  - Judgment：浅结构断言；frontmatter 是机器契约，不是正文文案。
  - Recommendation：只解析 YAML frontmatter 并断言 `name == forager`，不检查提示词措辞。
  - Verification：正文改写不失败，frontmatter 破坏必须失败。
  - Disposition：接受，本轮修复。

## 降级、驳回与延期

- `model_breaker_closes_after_the_six_hundred_second_cooldown`：事实成立，但 600 秒是内部策略常量。拒绝新增“常量等于常量”的测试；本轮重命名为实际保护的 `expired_breaker_state_is_evicted`。可控时钟只在冷却策略成为外部契约时再引入。
- Fetch 的 1.1s/1s、6.2s/6s deadline 测试：存在调度裕量风险，但未观察到 flake；单纯继续拉长 sleep 会拖慢套件而不提高因果性。本轮不改，精确时间边界应在未来随可控 Tokio clock 接缝处理。
- Credential cancellation 的 50ms 负向窗口：事实成立，但增加测试专用生产接缝或 Loom 的成本超过当前收益；保留现状并列为 P3。
- Raw request String 全面 typed 化：已有 `request_json` 可用于关键契约；机械迁移数百处没有足够 P2 证据，驳回批量改造。
- `search.rs` / `research.rs` 超大：两者仍各自围绕单一 CLI workflow，拆成更多 integration target 会增加编译/link 成本；仅因行数拆分被驳回。
- Release gate 可执行脚本化：方向正确，但涉及 Unix/Windows 制品安装协议和 workflow 迁移，本轮只移除虚假 substring 证明，脚本化保留为独立发布工程任务。

## 高价值后续缺口

以下缺口证据成立，但不与本轮删减/false-green 修复混在同一批次：classifier 完整 wire schema、OpenAI streaming 在 partial 后 malformed 的真实 HTTP 路径、research Vertical Discovery、journal 递归凭据脱敏、Firecrawl 截断恢复与 nested error、AnySearch 未知 domain status、Context7 malformed candidate、MCP 纯决策函数局部真值表。它们应按生产风险和真实历史缺陷排序，而不是为追求覆盖率一次性补齐。

## Thermo Pressure Pass

- Checked：deletable-complexity / growth / spaghetti / boundary / canonical-ownership / concurrency-atomicity / behavior-safety。
- Deletable complexity：Exa 三个跨 operation 重复、AnySearch 不可变源码断言、行数 ratchet、死断言可净删除。
- Growth/cohesion：只移动 `chain.rs` 内联测试；`search.rs`、`research.rs` No finding。
- Spaghetti/model：未发现测试参数化中隐藏不同分支的条件逻辑。
- Boundaries/types：关键 request 和 config schema wiring 需要结构化、独立 oracle；不推动全量 typed fixture。
- Canonical ownership：`slice_budget` 测试回归 net owner；xAI 请求契约回归真实 doctor fixture。
- Concurrency/atomicity：并发证明改用同步 fixture；真实 deadline 与 cancellation 精确时钟延期。
- Behavior safety：优先修正 false-green 与安全边界，再删重复；没有为降低测试数牺牲独立行为。

## 修复结果

本轮已完成报告中所有标记为“接受，本轮修复”的项目，并对实现后的 diff 做主线程复核。当前共有 457 个静态测试声明；本机实际运行 455 个，另外 2 个为 Windows 条件测试。

- 净删除负价值测试：Exa Similar 的空结果/Auth/deadline 三项、AnySearch manifest 不变测试、源码行数 ratchet、xAI 手工请求装配测试。
- 合并或归位：四个 attempt summary 边界测试合为两个具名 case 表；`slice_budget` 真值表移到 net；setup 的 subcommand inference 并入 clap 元数据契约。
- 强化独立 oracle：错误优先级、最后成功/最后 attempt、ErrorKind/status、47 个 config leaf、Context7/Tavily/Jina wire、UTF-8 截断、HTML/PDF 密度、六 provider setup、Unix 权限收紧。
- 强化 harness：sequence/parallel fixture 现在拒绝额外请求；新增连接屏障替代两处 sleep 推断；watchdog 会验证 child 未存活到 marker 时点；sender 断开时 listener 会退出。
- 修正测试资产：AnySearch fixture 收窄为真实受保护的 schema artifact，文档同步降级声明；acceptance manifest 的三个迁移/重命名引用已更新。
- 改善结构：`src/core/chain.rs` 从 1100 行降为 424 行，原 19 个测试保持名称不变移至 `src/core/chain_tests.rs`。

精确请求计数首次跑完整套件时暴露两个旧 Mystery Guest：

- `research_returns_evidence_exit_five_when_discovery_cannot_be_fetched` 以前依赖 listener 关闭来隐式模拟第二个 `/extract` 失败，现已显式提供 search 成功与 fetch 失败响应，并断言两个路径。
- `live_smoke_p2_does_not_write_evidence_into_the_configured_journal` 以前只为 9 个既定 classifier probe 配置前三个响应，现已完整配置 P2/C04 响应序列。

最终验证：

- `cargo fmt --all -- --check`：通过。
- `cargo clippy --all-targets --all-features --locked -- -D warnings`：通过。
- `cargo test --all-targets --all-features --locked --no-fail-fast -- --quiet`：通过，455 passed、0 failed、0 ignored。
- `git diff --check`：通过。

Windows 权限行为按项目约定只由 Windows CI 证明；本次未声称本地 Windows 验证成功。
