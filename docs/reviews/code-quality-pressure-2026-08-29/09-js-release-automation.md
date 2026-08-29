# Kimi 数据源 CLI 与发布自动化

审查范围：`skills/kimi-datasource/scripts/kimi-datasource.mjs`、GitHub Actions、Cargo/dist/release 配置及对应 scaffolding/skill-contract 测试。

## Findings

### [高严重度，高置信度] 技能目录机器契约已与当前仓库分叉

- 位置：`tests/skill_contract.rs:10`
- Fact：测试枚举 `skills/` 全部一级目录，却精确断言只有 `forager`；当前 HEAD 实际跟踪 `forager` 与 `kimi-datasource`。CI 会运行该测试。
- Impact：当前完整测试套件必然失败；新增 skill 没有进入库存契约，安全网与仓库现实已经分叉。
- Judgment：已经实现的 Fragile/Change-Detector Test，而非潜在缺口。
- Evidence：目录数量已足以证明失败，不依赖文件系统迭代顺序。
- Recommendation：把库存断言改为无序集合并纳入两个 skill，分别验证 frontmatter、入口脚本和必要资产；不能只放宽为“包含 forager”。
- Verification：运行该精确测试及完整套件。

### [高严重度，高置信度] 935 行 JS CLI 没有可执行安全网，也没有隔离关键副作用的测试缝

- 位置：`.github/workflows/ci.yml:17`、`skills/kimi-datasource/scripts/kimi-datasource.mjs:197`
- Fact：CI 只有 Rust checks；仓库无 JS test 入口。OAuth 刷新、网络、锁、文件写入、时钟、随机数、环境变量和进程输出直接绑定全局依赖。
- Impact：401 单次重试、token rotation、撤销凭据、timeout、脱敏、返回文件白名单和 CLI 输出契约可在 CI 绿色时退化，且难以手工稳定覆盖。
- Judgment：Behavior Safety Net 缺失，Functional Core / Imperative Shell 的决策边界没有形成。问题不是测试数量，而是关键编排不能由普通数据稳定驱动。
- Evidence：脚本导出少数纯函数，但 refresh/request/file/output 等关键路径私有且硬连全局依赖。单文件便携是合理约束，不能替代行为网。
- Recommendation：使用 Node 内建 `node:test`；先给 OAuth/请求编排注入最小依赖对象，或提取纯 transition/planning 函数，再在 CI 加测试。可先在单文件内建立 seam，无需立即跨文件拆分。
- Verification：覆盖临近过期、强制刷新竞争、重复 401、非法响应、脱敏、成功/失败文件写入及 text/json/quiet 输出。

### [中严重度，高置信度] OAuth 刷新锁在异常终止后没有 owner/recovery 协议

- 位置：`skills/kimi-datasource/scripts/kimi-datasource.mjs:285`
- Fact：通过 `mkdir(lockPath)` 获得目录锁，只有正常 `finally` 会删除；EEXIST 只等待 15 秒后失败。锁无 PID、nonce、租约、mtime 检查或 stale 回收。
- Impact：进程获锁后被强杀或主机崩溃会永久阻塞所有后续刷新，直到人工删目录。
- Judgment：Resource Lifecycle 异常终止路径未闭合；所有权只覆盖 JS finally，不覆盖进程生命周期。
- Evidence：正常/异常 throw 会 finally 释放，这是反证；SIGKILL/断电不会。Windows 禁用锁也不能解决 Unix stale lock。
- Recommendation：记录可验证 owner/nonce，只回收确认死亡的 owner；或使用由内核在进程退出时释放的锁。不要仅加固定 TTL，避免误删仍活动的长刷新。
- Verification：子进程获锁后被强制终止，下一进程安全恢复；两个活动进程不能同时刷新。

### [中严重度，中置信度] 工具语义成功前已提交返回文件

- 位置：`skills/kimi-datasource/scripts/kimi-datasource.mjs:725`
- Fact：`invokeTool` 先 `writeResponseFiles`，随后 `extractToolText` 才检查 `is_success === false` 并抛错。
- Impact：2xx 但工具层失败且带 files 时，文件已落盘而调用报失败；错误返回不暴露 writtenFiles 且无 rollback，形成半应用状态。
- Judgment：副作用提交顺序违反“先分类结果、后执行效果”的原子边界。
- Evidence：代码没有强制失败 envelope 不带 files；若后端有正式保证可降级，但仓库契约未表达该不变量。
- Recommendation：先解析/分类 response 为 success/error，再只对 success 执行计划好的文件写入；保留单文件写失败转 warning 的既有策略。
- Verification：`{is_success:false, files:[...]}` 不得创建文件；成功 envelope 仍按白名单写入。

### [中严重度，高置信度] release plan 与目标集合没有单一规范 owner

- 位置：`.github/workflows/release-artifact-gate.yml:4`、`dist-workspace.toml:17`
- Fact：同一 opaque plan 在多个 job 重复解析 tag/version；五目标集合分别存在于 dist config、checksum loop、matrix、Windows 分支、release-gate.json 和测试。
- Impact：plan schema/target 变化扇出到多处 YAML/TOML/test；漏改可能漏验资产、记录错误 attestation，或在发布后段才失败。
- Judgment：高风险发布协议的 Duplicated Knowledge / Shotgun Surgery。
- Evidence：独立 allowlist 有审计价值，cargo-dist 生成物也可能要求重复；问题在于副本没有单一可执行契约校验。
- Recommendation：增加 decode-plan job，一次验证并输出 tag/version/artifact matrix；其他 job 只消费输出。必须保留 allowlist 时，建立一个机器可读 release-target manifest，由 gate/记录/test 共用。
- Verification：plan fixture 增删 target 时只改规范源，matrix/checksum/verified_targets 同步；缺字段在 decode job 立即失败。

### [中严重度，高置信度] 发布测试以文本位置和 substring 模拟 Actions 语义

- 位置：`tests/release_scaffolding.rs:221`
- Fact：job 顺序使用 find/rfind，trigger/permissions/steps/targets 大量使用 contains，自写 parser 依赖固定缩进；TOML 使用了解析器但 YAML 没有。
- Impact：关键字符串移到注释可能 false-green；等价 YAML 重排会 false-red。测试既可能漏回归，也妨碍生成 workflow 的无害重排。
- Judgment：Fragile Test / Change-Detector，锁文本实现而非 needs 图、trigger、permissions 和步骤归属。
- Evidence：便宜 guard 能发现部分意外删除，但不足以保护发布语义。
- Recommendation：用 YAML 结构解析断言 job graph/输入/权限/trigger；把 Unix/PowerShell gate 核心脚本抽为可执行资产并做行为测试。生成的 release.yml 另做 pinned dist 版本再生成后的语义漂移检查。
- Verification：关键字符串移入注释必须失败；仅重排等价 job 声明应通过；删除 announce 对 gate 的 needs 必须失败。

## Open Questions

- 后端是否保证 `is_success:false` 永远不含 files？只影响 finding 4 的置信度/修复边界。
- 单文件、零依赖 JS 是否不可变约束？不影响增加测试 seam 的结论。
- 五目标 allowlist 是否刻意独立于 cargo-dist plan？可保留独立审批语义，但仍需单一机器清单生成/校验。

## Notes

- JS 单文件对消费者形成窄 CLI，有“便携深模块”的合理反证，未按行数要求拆分。
- OAuth guard-clause 控制流仍可追踪，除锁生命周期和测试缺口外未要求正式状态机。
- `release.yml` 是 dist 生成物，未因体积/重复生成步骤单独立项。
- workflow 权限声明与 custom job permissions 分工未发现结构问题。
- `agents/openai.yaml`、Cargo/release-plz/rust-toolchain 未发现独立 finding。

## 主 Agent 点验

- 已确认 HEAD 的两个 skill 与必败断言、无 JS test/CI 入口、目录锁生命周期、文件写入先于语义分类、release plan/target 多点复制及 YAML 文本测试；六项证据均可复现。

## Thermo Pressure Pass

- deletable-complexity：No finding。
- growth/cohesion：单文件体积本身不构成 finding，测试 seam 缺失有实际后果。
- spaghetti/model：认证 guard-clause 可追踪；No finding。
- boundaries/types：opaque release plan、失败响应先写文件。
- canonical-ownership：release target/plan 多点所有权。
- concurrency/atomicity：stale refresh lock 与失败 envelope 半应用副作用。
- behavior-safety：失效 skill contract、无 JS tests、YAML 文本测试。

## 最终 Disposition

- Skill inventory 契约分叉：**resolved**。无序精确集合纳入两个 tracked skills，并验证必要入口资产。
- JS CLI 无行为安全网：**resolved**。关键时钟、认证、gateway、token/file 与输出边界已形成测试缝；Node 内建测试覆盖 8 个正常边界行为并接入 Ubuntu/Windows CI。
- OAuth stale directory lock：**accepted-deferred (P3)**。内核 helper、Windows named pipe、kill/reap、alias identity 与异常退出测试属于罕见生命周期防御，已全部删除并恢复 `HEAD` 的简单锁语义。
- 失败 envelope 先写文件：**resolved**。先分类工具语义，再执行成功文件计划；白名单测试同时覆盖允许文件、同目录非法文件与跨目录非法文件。
- Release plan/target 无 canonical owner：**accepted-deferred (P3)**。Target allowlist、matrix 与记录已统一由机器清单和 fail-closed producer 生成；仅 `announcement_tag`/`app_version` 仍在隔离 job 中重复解码，保留为小型 schema 维护债。
- Release 测试模拟 YAML 文本：**resolved**。Workflow 使用结构化 YAML 解析，target producer 作为可执行资产接受行为测试。

本切面最终无仍需处理的 P0-P2 finding。
