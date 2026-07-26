# 6. 仓库迁移、退役链与文档修订

权威来源：[#61 Resolution](https://github.com/jfmoe/smartsearch/issues/61)。ADR/CONTEXT 修订内容与随迁 manifest 由 #57/#59 补充决议指定归本章。

## 迁移方案

- **git 历史：全新起点**。`jfmoe/forager` 从 Rust 首 commit 开始，零历史随迁；Python 历史、npm 发布史、fork 残留由本仓 archive 承担可考性。
- **旧仓作废方式：archive（非 delete）**。archive 前：README 顶部替换为终告（项目已重写为 forager、迁往 jfmoe/forager、本仓为 Python 时代决策与实现档案）+ pin 一条指路 issue。
- **时序**：规格定稿（本文档）→ 立即新建 `jfmoe/forager`（首 commit＝Cargo 脚手架 + CONTEXT.md + ADR 状态标注）→ Rust 开发全程在新仓、流水线随开发搭建 → 验收按第 5 章在新仓执行、本仓冻结只读 → 通过后走退役链。开发期两仓并存、无双向同步。

## 退役链（唯一权威顺序）

1. 正式 GitHub Release + 干净安装 + doctor 验证（含第 5 章 H10 制品门）。
2. skill 先删旧后装新（同一清单步骤）：`rm -rf ~/.agents/skills/smart-search-cli` + 按旧 Skill Installation Preference 记录的 container 集合逐一清理 → `npx skills add jfmoe/forager`。新 skill 目录名与 `name` 为 `forager`（不同名使残留可检测）；声明**最低** forager 版本（不锁精确版本）；文档写明「同时发现两者时 smart-search-cli 为残留应删除」。发布顺序 binary 先 skill 后。
3. npm `@jfmoe/smart-search` 发最后一个 patch（README/postinstall 指向 forager）→ `npm deprecate` 全版本，不 unpublish。
4. 全局 CLAUDE.md 改指向 forager。
5. Wayfinder 地图 `gh issue transfer` #52 及全部子票迁至 forager（旧仓自动重定向）；transfer 后做数量/链接/关系校验（M20）。
6. 本仓 README 终告 + pin 指路 issue → archive。archive 前验证本机旧 `smart-search` 命令已不可达（H16）。

## 建仓随迁资产 manifest（M20）

首 commit 随迁（它们是开发输入，非验收后迁移）：

| 资产 | 随迁处理 |
|---|---|
| `CONTEXT.md` | 按下述「死词清理」修订后随迁；旧仓 issue 引用全部改为全限定 URL |
| `docs/adr/0001`（skill 安装偏好） | 标 **superseded**（自研 skill 子系统砍，改 npx skills）存档 |
| `docs/adr/0002`（Search Result Journal） | 按下述三条修订 |
| `docs/adr/0003`（caller 声明能力） | 已标 **superseded by ADR-0004**（文件现状），存档随迁；#61 票文「0003 仍有效」为误记，以文件状态为准 |
| `docs/adr/0004`（caller 声明权威） | 有效随迁；第 2 章 A1 为其 research 豁免区的首次权威定义，加交叉引用 |
| `docs/adr/0005`（凭据池） | 按下述修订/替代 |
| `docs/anysearch-verified-domain-manifest.md` | **按 forager 命令面修订后随迁**：删除 Batch Discovery、AnySearch Extraction 及旧 `ANYSEARCH_API_KEY`/`ANYSEARCH_LIVE_ACCEPTANCE`/`smart-search smoke` 表述，live 验收指向第 5 章 C14–C16；域晋升通道本体保留 |
| 本规格 `docs/spec/forager/` | 随迁（开发唯一输入） |
| 两份 research 文档（选型、痛点清单） | 随迁入 `docs/research/` |
| `rust-toolchain.toml`、`#![forbid(unsafe_code)]` | 脚手架照办件，见下 |

## ADR 修订内容

- **ADR-0002 修订三条**：① `journal.enabled` 默认翻转为 **true**（file logging 死后 journal 是唯一持久观测面）；② 目录迁 **XDG state**（`~/.local/state/forager/journal`）；③ 记录面增强为**结果 + 执行过程双面**，附字段白名单契约（第 4 章）——原「不是调试消息流」边界放宽，但「不持久化请求头、原始请求/响应体、工具 trace」依然有效。
- **ADR-0005 修订/替代**：八 provider 全入池；凭据唯一形状＝TOML `keys` 真数组；废除 `KEY`/`KEYS` 双形态、「KEYS 覆盖 KEY」优先级与 JSON 字符串数组表述。

## CONTEXT.md 死词清理

- skill 子系统词条：Agents Skill Target、Skill Container、Skill Installation Preference、Automatic Skill Sync。
- AnySearch 词条：Batch Discovery、AnySearch Extraction（随 #53 砍除与第 5 章 extraction 裁决删除，含 Web Fetch 词条中的相关排除文案）。
- 三层路由词条：Intent Routing Catalog 等（「能力身份 + 语义例句」内容保留，转为分类器 prompt 与 skill 契约共同来源的新词条）。
- Search Result Journal 定义随增强更新（结果 + 过程双面）。
- 路由文档换成声明指南（caller capability declaration + plan 注入流程）。

## 新仓脚手架照办件（会话拍板补录，随规格记录）

- **Rust 工具链版本策略**：不设 MSRV 承诺（GitHub Releases 分发预编译二进制，无源码编译下游）；新仓放 `rust-toolchain.toml` 钉住建仓日最新 stable，本机与 CI 由它统一（仅防环境漂移，不构成兼容性承诺）；升级＝改一行版本号 + CI 通过即合。
- **零 unsafe**：全程 HTTP/JSON 业务，无 FFI/性能特例；crate 级声明 `#![forbid(unsafe_code)]`，编译器强制。
- 发布流水线：dist（原 cargo-dist）+ release-plz 在新仓随开发首建（#54 调研为输入），无迁移物。
