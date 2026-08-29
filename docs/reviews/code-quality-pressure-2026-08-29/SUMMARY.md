# 全仓代码质量压力审查总验收

日期：2026-08-29

## 结论

本轮以 9 个模块/垂直切面报告和 1 个独立横切复核报告完成全仓审查。模块报告共给出 34 个 finding 条目；按允许交叉的审查方式，其中包含跨切面的重复观察，不代表 34 个互斥根因。

按最终“质量重构为主、常见 bug 修复为辅、不扩建罕见防御”的口径重新裁决后：25 条已修复并验证，9 条 accepted-deferred (P3)。本轮 scope 内仍需处理的 P0-P2 为 0；超过 4 MiB 后畸形截断、artifact 中途文件系统失败、OAuth owner 被强杀等罕见问题明确保持 `HEAD` 行为，没有包装成“已修复”。

## 报告与处置

| 报告 | 切面 | Findings | Resolved | Accepted P3 |
|---|---|---:|---:|---:|
| [01](01-cli-app.md) | CLI、应用编排、输出 | 4 | 2 | 2 |
| [02](02-config-types-classifier.md) | 类型、配置、分类器 | 5 | 5 | 0 |
| [03](03-engine-research.md) | Engine、Research evidence | 4 | 3 | 1 |
| [04](04-main-search-providers.md) | 主搜索与 provider seam | 3 | 2 | 1 |
| [05](05-auxiliary-providers.md) | AnySearch、Context7、Exa | 3 | 3 | 0 |
| [06](06-web-providers.md) | Web Search、Fetch、Map | 2 | 1 | 1 |
| [07](07-infrastructure.md) | Net、journal、敏感状态 | 2 | 2 | 0 |
| [08](08-test-architecture.md) | 测试架构与可重构性 | 5 | 3 | 2 |
| [09](09-js-release-automation.md) | Kimi JS、发布自动化 | 6 | 4 | 2 |
| **合计** |  | **34** | **25** | **9** |

[10-architecture-crosscut.md](10-architecture-crosscut.md) 负责根因归并与反证检查；[11-scope-closure.md](11-scope-closure.md) 记录最终范围收口、删减依据、复杂度变化与完整门禁。

## 主要保留改动

- 类型与 owner：引入 `FallbackPolicy`、`AttemptDisposition`、`AttemptTarget` 和严格 `ResearchPlan`；Main Search request/provider 构造归位，`doctor` 等归约点统一消费 authoritative disposition。
- Research：分离 fetch identity 与多子问题 coverage；known URL 位于 discovery cap 之外；普通失败只在终态生成 gap；classifier chronology 在持久化前完整。
- Provider/config/output：Context7 正文不再按 redirect 文案解释；核心 wire 字段和 Map URL 在 adapter 边界验证；inline-table 编辑、唯一 order 与 preflight tee 契约统一。
- Journal：attempt 创建者独占 model/endpoint 事实；journal 保留完整 answer，只对受保护字段应用 canonical redaction。
- 测试质量：删除 Markdown、CHANGELOG 和 production 源码扫描；CLI 调用汇入薄 direct-child watchdog；fixture read 有上界。
- Kimi/release：保留最小时钟、认证、gateway、token/file 和输出测试缝；失败 envelope 先分类后写白名单文件；release target 由机器清单和 producer 生成，workflow 测试使用结构化 YAML。

## 主动删减的过度防御

- 删除手写截断 JSON 结构 parser。
- 删除 artifact 临时文件序列、fsync/rename、已提交前缀与失败后二次 manifest 协议。
- 删除 Kimi Unix helper/Windows named-pipe 锁、kill/reap、alias identity 与异常生命周期测试。
- 删除 Unix process group、Windows suspended Job Object、Drop/attach cleanup 和三路 I/O 状态机。

收口前 tracked diff 为 `+3437/-1496`，最终为 `+2165/-1418`；另将两个 untracked 压力测试从 726 行降至 319 行，合计撤出约 1,679 行新增实现和专用测试。

## 复杂度验收

这不是 LOC 减少型重构。最终 production Rust 仍为 `+808/-427`，tracked tests 为 `+1216/-913`，另有 393 行新的 release/Node/watchdog 资产。

降低的是维护复杂度：裸字符串和成功哨兵减少，非法状态进入类型，重复 owner 和文本解析测试被删除，release/provider/research 的同步修改面缩小。增加的是显式领域模型与行为安全网。结论是：**结构与状态复杂度下降，代码量上升；删掉罕见防御后，新增复杂度与普通路径收益基本匹配。**

## 最终门禁

| 命令 | 结果 |
|---|---|
| `cargo fmt --all -- --check` | 通过 |
| `git diff --check` | 通过 |
| `node --test skills/kimi-datasource/scripts/kimi-datasource.test.mjs` | 8 passed，0 failed |
| `cargo clippy --all-targets --all-features --locked -- -D warnings` | 通过，0 warnings |
| `cargo test --all-targets --all-features --locked --no-fail-fast -- --quiet` | 25 targets，427 passed，0 failed |

关键定向安全网：watchdog 2/2、doctor 19/19、release scaffolding 17/17、fetch 16/16、research 44/44。Windows CI 继续执行 Node 契约和 direct-child watchdog；本地结果只陈述本机实际运行。

## 验证期追加修复

- Transport 成功后被质量/证据门拒绝时，`ProviderAttempt::mark_failed` 统一更新 disposition、kind 和 message。
- Known URL evidence 不再占用 discovery cap。
- Candidate wave 仅永久剔除已满额归属，暂时受 reservation 阻塞时整条 candidate 延后。
- `doctor` 不再使用 `error_kind.is_none()` 推断成功，改为显式 `AttemptDisposition::Succeeded`。

## Accepted P3

- `app.rs` 仍是多职责汇合点；只有在能按完整职责簇移动时再拆分。
- Registry/runtime/doctor/smoke 的 canonical ownership 与 compile-time support matrix 尚未完全统一。
- HTTP fixture 仍以 raw wire 为主 API；后续按实际结构断言需求增量增加 typed accessor。
- 配置锁缺少 barrier/hook 的 lost-update characterization；重构锁范围前再补。
- Release plan 的 `announcement_tag`/`app_version` 仍在隔离 jobs 中重复解码。
- 多归属 candidate 因 reservation 可能多等待一波，是保证确定性 cap 的有界取舍。
- 超过 4 MiB 后截断且 envelope 缺正确正文路径时，Web Fetch 仍保留基线 raw fallback。
- Artifact 中途写失败仍可能留下未索引的已写文件；未引入部分提交恢复协议。
- Unix OAuth 目录锁在 owner 被强杀后可能 stale，Windows 仍沿用基线 no-op；未引入跨平台生命周期锁。

以上延期项均有明确触发条件和独立立项边界；它们没有混入本轮“代码质量修复已完成”的结论。
