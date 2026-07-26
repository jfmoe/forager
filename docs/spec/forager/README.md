# forager Rust 重构规格

本目录是 Wayfinder 地图 [#52](https://github.com/jfmoe/smartsearch/issues/52) 的最终产出：把全部已决内容汇编为一份可执行的重构规格。编码执行是地图之外的独立 effort，在新仓库 `jfmoe/forager` 进行。

## 文件划分

| 章 | 文件 | 内容 | 权威来源票 |
|---|---|---|---|
| 1 | [01-scope.md](01-scope.md) | 功能取舍清单（砍/新增/保留）与后续修订留痕 | [#53](https://github.com/jfmoe/smartsearch/issues/53) |
| 2 | [02-cli.md](02-cli.md) | CLI 接口：命令面、参数、输出、退出码、三个契约 | [#56](https://github.com/jfmoe/smartsearch/issues/56) |
| 3 | [03-config.md](03-config.md) | 配置格式、全键面、config/setup 命令、**旧键→新键映射表** | [#57](https://github.com/jfmoe/smartsearch/issues/57) |
| 4 | [04-architecture.md](04-architecture.md) | 架构骨架：模块、trait、错误模型、net、凭据池、journal | [#58](https://github.com/jfmoe/smartsearch/issues/58) |
| 5 | [05-acceptance.md](05-acceptance.md) | 验收契约（Tier 0/1、四层测试、live 矩阵）与切换步骤 | [#59](https://github.com/jfmoe/smartsearch/issues/59) |
| 6 | [06-migration.md](06-migration.md) | 仓库迁移、退役链、随迁 manifest、ADR/CONTEXT 修订、脚手架照办件 | [#61](https://github.com/jfmoe/smartsearch/issues/61) |

各章以对应票的 Resolution 及其补充决议为权威来源；票间冲突已按「后票修订前票」原则在汇编时消解。**规格优先规则的边界**：规格与票文冲突时，仅当该差异属于显式留痕的汇编裁定——即 [#60 Resolution](https://github.com/jfmoe/smartsearch/issues/60#issuecomment-5082058705) 列出的悬置件定稿、各章「修订留痕」条目、或行文中标注「以文件/规格为准」的更正——才以规格为准；**未留痕的冲突视为汇编缺陷**，应回溯票文修正规格。

## 核心基调（地图 Notes 摘录）

- 完全替换 Python，不长期双实现并行；CLI 与配置格式重新设计。
- 项目更名 **forager**；开发直接在新仓库 `jfmoe/forager` 干净环境进行，本仓冻结为只读参照，验收通过后走退役链、本仓 archive。
- 分发走 GitHub Releases（dist + release-plz），放弃 npm。
- 价值排序：分发与启动体验 + 类型安全 + Agent 长期迭代下的架构稳定性；运行时吞吐不是目标。

## 研究输入

- [Rust CLI 生态选型](https://github.com/jfmoe/smartsearch/blob/research/rust-cli-ecosystem/docs/research/rust-cli-ecosystem-selection.md)（#54）：clap 4 derive、tokio + reqwest(rustls)、figment + serde、thiserror/anyhow/miette、dist + release-plz；aichat 为架构参考。
- [Python 架构痛点 13 条](https://github.com/jfmoe/smartsearch/blob/research/python-arch-pain-points/docs/research/python-arch-pain-points.md)（#55）：三大根因——未类型化 dict 契约、缺失单一真相源、全局可变状态。第 4 章逐条给出消解落点。
