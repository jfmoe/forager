# 代码质量修复范围收口

日期：2026-08-29

## 裁决口径

最终工作树以 `HEAD` 为行为基线。本轮只保留两类变化：

1. 能减少重复知识、非法状态、跨层推断或测试结构耦合的代码质量重构；
2. 普通输入、正常 provider 链或公开 CLI 路径可到达的缺陷修复。

仅由强杀、主机崩溃、超大且不读 stdin、后代继承管道、artifact 写到一半失败，或超过 4 MiB 后截断且 envelope 畸形等条件触发的机制，不作为本轮 bug 修复继续扩建。

## 保留：代码质量重构

- `ResearchPlan`、`FallbackPolicy`、`AttemptDisposition`、`AttemptTarget` 把裸字符串和 `Option<error_kind>` 哨兵替换为显式领域状态；`doctor` 等生产归约点统一消费 authoritative disposition。
- Main Search request 与 provider 构造回到 provider owner；journal 不再下游猜测 model/endpoint，redaction 只由 canonical 字段策略负责。
- Research 将 fetch identity、多个 subquestion coverage、终态 gap 与 classifier chronology 分开建模。
- TOML inline-table 编辑共享同一遍历语义；测试删除 Markdown、CHANGELOG 和 production 源码文本扫描。
- CLI 测试命令统一进入一个薄 runner；release target 进入机器清单，workflow 测试改为结构化 YAML。
- Kimi 把时钟、gateway、token/file 和输出决策形成最小测试缝，不拆分单文件 CLI，也不增加通用框架。

## 保留：常见路径 bug 修复

- Config set/unset 对合法 inline table 不再假成功；provider order 拒绝重复项。
- Preflight 的 config/plan 错误继续遵守 JSON tee/output 契约。
- Attempt 在质量门失败后同时更新 disposition、kind 与 message；breaker skip 与 operation target 不再伪装成 timeout/capability。
- 多子问题共享 URL 只抓取一次但保留全部 coverage；known URL 不占 discovery cap；普通候选失败只在终态生成 gap。
- Context7 正文不再按 redirect 文案解释；Exa、Tavily Map、Web Fetch 的常规 DTO 核心字段在 adapter 边界校验。
- Kimi 的公开 stdin JSON 路径、单次 401 retry、失败 envelope 先分类后写文件及文件白名单得到行为覆盖。

## 主动退出本轮：罕见防御性机制

- 删除截断 JSON 的手写结构解析状态机，恢复 `HEAD` 的既有截断行为；不在本轮解决“4 MiB 后畸形 envelope”问题。
- 删除 research artifact 的临时文件序列、fsync/rename、已提交前缀与失败后二次 manifest 协议；保留简单写入和既有错误返回。
- 删除 Kimi 的 Unix helper 内核锁、Windows named pipe、deadline/kill/reap、alias identity 和异常退出测试；恢复 `HEAD` 的简单锁语义。
- 删除测试 runner 的 Unix process group、Windows suspended Job Object、Drop/attach cleanup 与三路 I/O 状态机；只保留 direct-child deadline、kill/wait 和 stdout/stderr drain。

这些不是“已修复”；它们是刻意保持基线行为的 P3 残余。若将来有真实故障频率或明确安全要求，应独立立项并以更小的系统原语解决。

## 复杂度验收

- 收口前的 tracked diff 为 `+3437/-1496`；收口后为 `+2165/-1418`，删除 1,272 行待提交增量。
- 两个 untracked 压力测试从 726 行降至 319 行；合计从候选修复中撤出约 1,679 行新增实现与专用测试。
- 最终 diff 仍不是 LOC 减少型重构：production Rust 为 `+808/-427`，tracked tests 为 `+1216/-913`，另有 393 行新的 release/Node/watchdog 资产。
- 降低的是维护复杂度：非法状态、重复 owner、字符串哨兵、文本解析测试和跨文件同步点减少；增加的是显式领域类型与行为安全网。代码量增加，但概念边界比收口前和 `HEAD` 更清楚。

## 最终验证

| 命令 | 结果 |
|---|---|
| `cargo fmt --all -- --check` | 通过 |
| `git diff --check` | 通过 |
| `node --test skills/kimi-datasource/scripts/kimi-datasource.test.mjs` | 8 passed |
| `cargo clippy --all-targets --all-features --locked -- -D warnings` | 通过，0 warnings |
| `cargo test --all-targets --all-features --locked --no-fail-fast -- --quiet` | 25 targets，427 passed |

三名原 reviewer 对删减后的 Rust、JS/release 与测试基础设施分别复审；除 `doctor` 的旧成功哨兵外均为 `No finding`。该哨兵已改为 `AttemptDisposition::Succeeded`，并由 `doctor` 19 项测试和完整门禁验证。
