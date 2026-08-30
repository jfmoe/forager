# 发布自动化

审查范围：GitHub Actions、Cargo/dist/release 配置及对应 scaffolding 测试。

## Findings

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

- 五目标 allowlist 是否刻意独立于 cargo-dist plan？可保留独立审批语义，但仍需单一机器清单生成/校验。

## Notes

- `release.yml` 是 dist 生成物，未因体积/重复生成步骤单独立项。
- workflow 权限声明与 custom job permissions 分工未发现结构问题。
- Cargo/release-plz/rust-toolchain 未发现独立 finding。

## 主 Agent 点验

- 已确认 release plan/target 多点复制及 YAML 文本测试，两项证据均可复现。

## Thermo Pressure Pass

- deletable-complexity：No finding。
- growth/cohesion：No finding。
- spaghetti/model：No finding。
- boundaries/types：opaque release plan。
- canonical-ownership：release target/plan 多点所有权。
- concurrency/atomicity：No finding。
- behavior-safety：YAML 文本测试。

## 最终 Disposition

- Release plan/target 无 canonical owner：**accepted-deferred (P3)**。Target allowlist、matrix 与记录已统一由机器清单和 fail-closed producer 生成；仅 `announcement_tag`/`app_version` 仍在隔离 job 中重复解码，保留为小型 schema 维护债。
- Release 测试模拟 YAML 文本：**resolved**。Workflow 使用结构化 YAML 解析，target producer 作为可执行资产接受行为测试。

本切面最终无仍需处理的 P0-P2 finding。
