# Rust 工具型 CLI 的安装与分发地图

> 调查日期：2026-07-30。
>
> 范围：面向终端用户和 Agent 的跨平台 Rust CLI；覆盖用户如何安装、维护者如何产出与投放、以及发布信任边界。本文只引用 Rust/Cargo、平台包管理器、GitHub、Apple、Microsoft 与相应工具项目的一手资料。
>
> 说明：“常见”不表示市场份额排序（本调查未以下载量或星数作推断），而是指各生态的官方标准入口或 Rust CLI 中反复可组合的基础渠道。**事实**来自链接；标为“工程判断”的内容是基于这些事实的建议。

## 执行摘要

**事实。** `cargo install` 是 Cargo 管理本地已安装二进制 crate 的官方命令，默认从 crates.io 取包并在本机编译；它也能从 Git、路径或其它 registry 安装。[Cargo Book](https://doc.rust-lang.org/cargo/commands/cargo-install.html) 明确了这一行为和安装根目录。预编译二进制通常由项目维护者上传至 GitHub Release，再由直接下载、`cargo-binstall` 或操作系统包管理器消费；GitHub Release 本身可以附加二进制资产。[GitHub Releases 文档](https://docs.github.com/en/repositories/releasing-projects-on-github/about-releases)

**工程判断。** 对新工具型 Rust CLI，最小且实用的组合不是“一次接入所有商店”，而是：

1. 发布 crates.io，提供 `cargo install --locked <crate>` 这个锁定依赖版本的源码兜底入口；
2. 同一版本发布 GitHub Release 的预编译归档与 `SHA256SUMS`，覆盖 Linux x86_64、macOS Intel/Apple Silicon、Windows x86_64；
3. 让 `cargo-binstall <crate>` 成为 Rust 用户的可选快路径（先在实际 Release 上验证其识别结果）；
4. 早期不做 Homebrew core、WinGet、Chocolatey、Deb/RPM 发行仓库、npm/PyPI 包装器和自更新器。

这四项互补：Cargo 解决“有 Rust 环境且想从源码构建”，Release 解决“无需 Rust 的快速安装”，binstall 发现并下载后者；它们不是三套必须互斥的发布物。系统包管理器则是在产品稳定、目标用户明确后增加的原生入口。

## 先分清两张地图

```
维护者：源码 ──构建/签名──> Release 归档、校验和、包
                              │
用户： cargo install <── crates.io（源码）
      cargo binstall <── Release 归档（预编译）
      brew/scoop/winget/... <── 各自的包定义与审核/同步
      下载解压 <── GitHub Release
```

`dist`、`release-plz`、`cross` 和 GitHub Actions 位于上图的维护者一侧：它们自动化版本、构建或上传，并不是用户安装渠道。`dist` 自己也把能力划为构建（计划、二进制、安装器）与分发（托管、发布、公告）两部分。[dist README](https://github.com/axodotdev/cargo-dist) `release-plz` 则自动化版本递增、changelog、registry 发布与 GitHub/Gitea/GitLab Release。[release-plz README](https://github.com/release-plz/release-plz)

## 渠道地图与用户体验

| 渠道 | 用户典型命令 / 体验 | 主要用户 | 与其它渠道的关系 | 维护成本 |
| --- | --- | --- | --- | --- |
| crates.io + Cargo | `cargo install --locked <crate>`；本机编译 | Rust 开发者、需自审构建的人 | 源码兜底；不替代预编译包 | 低：发布 crate、维护 lockfile/MSRV |
| `cargo-binstall` | `cargo binstall <crate>`；优先预编译，缺失时回退构建 | 已装 Rust/Cargo、在意安装速度者 | 消费 crates.io 元数据 + Release，不替代两者 | 低增量：规范命名/元数据并持续产物覆盖 |
| GitHub Release 直下 | 下载对应 `.tar.gz`/`.zip`，验 SHA-256，解压到 `PATH` | 无 Rust 的开发者、Agent、其它安装器 | 是预编译资产的事实源；可被脚本/包管理器复用 | 中：目标矩阵、归档、校验和、签名 |
| Shell / PowerShell 脚本 | 一个安装命令，脚本选择目标、验包、放入目录 | 初次使用者 | 是 Release 的薄客户端，不应另建产物源 | 中：跨 shell、代理、权限、失败恢复 |
| Homebrew / Scoop / WinGet | `brew install ...`、`scoop install ...`、`winget install ...` | macOS/Linux 开发者、Windows 用户 | 原生体验；通常仍下载上游 Release | 每个渠道各一份包定义与更新链 |
| AUR、Deb/RPM、Nix | `pacman` 前端、`apt`/`dnf`、`nix profile install`/`nix run` | 特定 Linux 发行版和受管环境 | 与 Release 并存；系统/声明式安装优先 | 中到高：打包规范、构建与安全更新责任 |
| npm / PyPI 包装器 | `npm i -g ...`、`pipx install ...` 等 | 已经以 Node/Python 工具链为入口的用户 | 只是附加入口，不替代原生 Rust 分发 | 高：第二生态、运行时与二进制映射 |
| 源码仓库 / 容器 | `cargo install --git ...`、`cargo install --path .`、`docker run ...` | 贡献者、可复现构建/自动化 | 补充入口，不是桌面用户的首选 | 低到中：镜像维护、漏洞修复、文档 |

### 1. crates.io + `cargo install`：官方源码安装基线

**事实。** `cargo install <crate>` 默认以 crates.io 为源、构建可执行目标、将可执行文件写入安装根的 `bin`；也支持 `--git`、`--path` 和 `--registry`。Cargo 默认忽略随包附带的 `Cargo.lock`，而 `--locked` 会强制使用它，适合要求确定依赖解析的场景。[Cargo Book：`cargo install`](https://doc.rust-lang.org/cargo/commands/cargo-install.html)

**用户体验。**

```sh
cargo install --locked <crate>
# 贡献者或未发布版本
cargo install --path . --locked
cargo install --git https://github.com/<org>/<repo> --locked
```

**工程判断。** 这是 Rust 用户最小、最稳定的源码入口，应保留；但它要求 Rust 工具链和 C/系统构建依赖，耗时受目标机与依赖图影响，不能作为面向所有终端用户或 Agent 的唯一入口。发布 CLI crate 时应保留可安装的二进制 target、准确声明 `rust-version`，并在安装说明中优先写 `--locked`。

### 2. `cargo-binstall`：Rust 用户的预编译快路径

**事实。** `cargo-binstall` 读取 crates.io 的 crate 信息，查找其 `repository` 指向的 Release/资产；会尝试第三方 artifact host、备选目标，最后回退到 `cargo install`。它的 README 将其定位为通常可替代 `cargo install` 的命令，并允许维护者用 `Cargo.toml` metadata 显式描述资产位置；签名验证是“初始且有限”的支持，用户可用 `--only-signed` 拒绝未签名包。[cargo-binstall README](https://github.com/cargo-bins/cargo-binstall)

```sh
cargo binstall <crate>
# 自动化时明确同意，仍建议固定版本
cargo binstall --no-confirm <crate>@<version>
```

**工程判断。** 这是发布预编译 Release 后最便宜的增量入口，但不是 Cargo 内建功能，也不能替代直接下载页。把它写入安装说明前，应对每个承诺 target 运行一次真实安装；若资产命名不能被自动识别，再添加其文档定义的 metadata。不要以“它会回退”为理由漏发主流 target。

### 3. GitHub Releases：无 Rust 环境的基本预编译入口

**事实。** GitHub Release 是基于 tag 的发布对象，可包含 release notes 和二进制资产；Release 资产可由浏览器和 API 下载。[GitHub：About releases](https://docs.github.com/en/repositories/releasing-projects-on-github/about-releases) [GitHub：管理 Release](https://docs.github.com/en/repositories/releasing-projects-on-github/managing-releases-in-a-repository)

**用户体验。** 提供一个清楚的 Release 页面，按平台下载、验证哈希，再把二进制放入 `PATH`。示例中的占位符应替换为真实 tag 和文件名：

```sh
curl -LO https://github.com/<org>/<repo>/releases/download/<tag>/<tool>-<target>.tar.gz
curl -LO https://github.com/<org>/<repo>/releases/download/<tag>/SHA256SUMS
shasum -a 256 -c SHA256SUMS --ignore-missing
tar -xzf <tool>-<target>.tar.gz
```

**工程判断。** Release 是其它多数渠道可共同复用的“二进制真源”。每项资产要有稳定且可预测的名称、版本、目标三元组、校验和和变更说明。不要把 `latest` 下载链接当作自动化或企业部署的唯一来源；部署说明应支持固定 tag/版本。

### 4. 安装脚本与自动更新：便利层，不是信任替代品

**事实。** Shell 和 PowerShell 一行安装器是许多工具采用的机制；例如 `cargo-binstall` 的官方 README 同时提供 shell、PowerShell、手动归档和源码安装方法。[cargo-binstall README](https://github.com/cargo-bins/cargo-binstall)

**工程判断。** 脚本应只是从固定 Release 下载已校验的归档并安装，不应成为另一个编译/托管系统。推荐“下载脚本 + 验证脚本的校验和/签名 + 显式执行”，而不是把网络内容直接管道给 shell；PowerShell 同理。脚本必须可选择版本、打印目标 URL、支持无交互模式、在失败时不破坏既有可执行文件。

GitHub Release **不提供应用程序自动更新策略**；它只提供版本化资产。因此“自动更新”需要另选机制：包管理器的升级命令、显式 `self update`，或定时自动更新。`cargo-binstall` 本身建议再次执行安装命令升级；其 README 把批量更新交给 companion tool `cargo-update`。[cargo-binstall README](https://github.com/cargo-bins/cargo-binstall) 对面向 Agent 的网络工具，默认静默自更新会改变执行代码与供应链版本，建议只提供显式、可固定版本的更新操作；企业环境交给管理员的包管理器策略。

## 平台和生态包管理器

以下渠道都值得知道，但不是“首版全部接入”的清单。

| 渠道 | 事实与典型入口 | 何时值得做 | 主要代价 / 边界 |
| --- | --- | --- | --- |
| Homebrew tap / core | Tap 是 Formula/Cask 的 Git 仓库；用户可 `brew tap <org>/tap` 后 `brew install <formula>`。官方文档区分 tap 与 Homebrew/core 的接受标准。[Taps](https://docs.brew.sh/Taps) [Acceptable Formulae](https://docs.brew.sh/Acceptable-Formulae) | macOS 开发者是主群体时，先维护自有 tap；满足 core 规范且已有稳定用户需求后再申请 core | core 是上游项目审核和政策约束，不是自动镜像；tap 仍需维护 formula、哈希和每版更新 |
| Scoop | Scoop 以 bucket（Git 仓库）提供 manifest，用户 `scoop bucket add` / `scoop install`；官方项目维护创建 bucket 的说明。[Scoop：Creating a bucket](https://github.com/ScoopInstaller/Scoop/wiki/Creating-a-bucket) | Windows 开发者优先、希望用户态安装时 | 自有 bucket/主 bucket 的清单、哈希和版本同步；不是 Windows 企业管控的默认入口 |
| WinGet | 客户端通过 manifest 安装包，典型命令为 `winget install <id>`；Microsoft 文档定义 manifest 和提交流程。[WinGet package manifests](https://learn.microsoft.com/windows/package-manager/winget/package/) | Windows 普通用户、IT/终端管理流程要求 WinGet 时 | manifest 审核、安装器类型与静默安装行为；需保持 Release URL/哈希对应 |
| Chocolatey | 包以 NuGet 包形式发布，可包装安装脚本/上游安装器；官方提供创建与发布包指南。[Chocolatey packaging](https://docs.chocolatey.org/en-us/create/create-packages/) | 已有 Chocolatey 企业基础设施的明确客户 | 不应因“Windows”泛化而必做；额外的包脚本、审核和安全更新责任 |
| AUR / Arch | AUR 是 Arch 的社区驱动包仓库；`PKGBUILD` 描述构建包的方法，官方 Arch 文档强调 AUR 包由用户维护、应审阅 PKGBUILD。[AUR](https://wiki.archlinux.org/title/Arch_User_Repository) [PKGBUILD](https://wiki.archlinux.org/title/PKGBUILD) | Arch 用户明确提出，或社区愿意维护 | AUR 不等同官方仓库；最好接受社区维护包而非承诺官方支持，且不能把 AUR 的构建脚本当作信任边界 |
| Deb/RPM 与 apt/dnf/yum 仓库 | Debian 与 RPM 都有正式打包规范；`apt`、`dnf`/`yum` 是消费各自仓库的客户端。[Debian New Maintainer's Guide](https://www.debian.org/doc/manuals/maint-guide/) [RPM Packaging Guide](https://rpm-packaging-guide.github.io/) | 需要系统级部署、离线镜像、企业基线或发行版集成时 | 最高：每发行版/版本的包、依赖、仓库签名、密钥轮换、更新与 CVE 响应；首版通常不划算 |
| Nix / nixpkgs | Nix 的声明式包定义可被 `nix run`、profile 等方式消费，nixpkgs 有自己的贡献、构建与更新流程。[Nix manual：`nix run`](https://nix.dev/manual/nix/latest/command-ref/new-cli/nix3-run.html) [nixpkgs contributing](https://nixos.org/manual/nixpkgs/stable/#chap-quick-start) | Nix 用户或需要可复现环境的团队 | Nix expression、上游审查与更新节奏；适合社区/用户贡献，未有需求前不必抢先维护 |

**工程判断。** Homebrew tap、Scoop、WinGet 是增长期最有性价比的三项，选择应服从实际平台用户而不是“渠道齐全”。AUR、Nix 更适合接纳社区贡献；Deb/RPM 自建仓库、Chocolatey 则应以企业要求为前置条件。Homebrew core 与官方 Linux 发行版仓库并非普通的 Release 自动上传目标，不能承诺“每次 tag 自动即时可用”。

### npm / PyPI 作为二进制包装器

**事实。** npm 的 `package.json` 中 `bin` 字段把命令名映射到可执行脚本。[npm `bin` 文档](https://docs.npmjs.com/cli/v11/configuring-npm/package-json#bin) Python 包可用 `project.scripts` 声明 console-script entry point。[PyPA entry points specification](https://packaging.python.org/en/latest/specifications/entry-points/)

**工程判断。** 只有当目标用户已把 Node 或 Python 当作唯一/主要工具入口，且包装器能实质降低接入摩擦时，才值得发布 npm/PyPI 包。包装器可按平台下载 Release 二进制，或把 Rust CLI 嵌入包内；前者要维护下载 URL、哈希、代理/离线行为和每个平台映射，后者要维护多份大资产。两者都会引入第二 registry、第二套发布凭据/权限、运行时依赖和同版本同步问题。它不是把 Rust CLI “变成 npm/Python CLI”，也不应成为原生二进制安全验证的绕过路径。

### 源码和容器是补充入口

**事实。** Cargo 官方支持 `cargo install --path` 和 `cargo install --git`。[Cargo Book：`cargo install`](https://doc.rust-lang.org/cargo/commands/cargo-install.html) OCI 容器镜像可作为命令运行环境；Docker 官方文档把 GitHub Actions 中构建和发布镜像作为一条独立发布路线。[Docker：GitHub Actions](https://docs.docker.com/build/ci/github-actions/)

**工程判断。** 源码安装适合贡献者、开发版和受审计构建；容器适合 CI、隔离执行和不愿写入宿主机 `PATH` 的 Agent。容器不是常规本机 CLI 的替代：要处理挂载、凭据、网络、镜像 tag 固定、镜像扫描和基础镜像补丁。若发布镜像，应把它作为与二进制同一 tag 的额外资产，而非唯一路径。

## 多平台产物与安全注意

### 建议的首发矩阵

| 平台 | 首发建议 | 为什么 / 注意 |
| --- | --- | --- |
| Linux x86_64 | `x86_64-unknown-linux-gnu` **或** `x86_64-unknown-linux-musl`，先选一个并公开兼容承诺 | GNU 产物依赖所选 glibc 基线；musl 往往减少 libc 兼容面，但不是“任何 Linux 都必然可运行”，需实测 DNS、TLS、动态依赖与目标发行版。若同时发两种，安装器必须避免错误选择。 |
| Linux aarch64 | 有 ARM Linux 用户证据后增加 GNU 或 musl 对应产物 | 不因能交叉编译就承诺支持；在真实 ARM runner/环境测主路径。 |
| macOS | `x86_64-apple-darwin` 与 `aarch64-apple-darwin` | 用 macOS runner 原生构建并测试；通用二进制可后置，先保证两个明确产物。 |
| Windows | `x86_64-pc-windows-msvc` | 主流 Windows Rust 目标；ARM64 仅在有需求后加。发布 `.zip`，文档说明解压和 PATH。 |

**事实。** Cargo 的 target 由目标三元组选择，`cargo install --target` 使用 `<arch><sub>-<vendor>-<sys>-<abi>` 格式。[Cargo Book：target option](https://doc.rust-lang.org/cargo/commands/cargo-install.html) GitHub Actions 的 matrix 可以用一个 job 定义产生多操作系统/变量组合的 job。[GitHub Actions matrix](https://docs.github.com/en/actions/using-jobs/using-a-matrix-for-your-jobs) `cross` 是容器化交叉编译/测试工具，需 Docker 或 Podman，且项目明确列出各 target 的支持与限制。[cross README](https://github.com/cross-rs/cross)

**工程判断。** 最小矩阵应当是 Linux x86_64、macOS x86_64/aarch64、Windows x86_64，并逐项在对应 OS 运行 `--version`、`--help` 和一次真实网络主路径。交叉编译是构建手段而不是兼容性证明；尤其 macOS 应在 macOS runner 构建，Linux 的 glibc/musl 选择要以支持策略和实测为准。

### 完整性、签名与供应链

- **校验和（工程判断）：** 每个 Release 发布由 CI 生成的 `SHA256SUMS`，安装器在执行前验证。版本化 URL、校验和和 Release note 应同属一个 tag；不能只依赖 HTTPS 或“latest”。
- **来源证明（事实）：** GitHub Actions 支持为构建产物创建和验证 artifact attestation，用于把产物与构建工作流关联。[GitHub artifact attestations](https://docs.github.com/en/actions/security-for-github-actions/using-artifact-attestations-to-establish-provenance-for-builds) **工程判断：** 增长期将归档、校验和和容器镜像纳入 provenance，并固定 Action 的可信 revision、收紧 release workflow 权限。
- **macOS（事实）：** Apple 的 Developer ID 直接分发路径要求先签名；以 Developer ID 分发的现代 macOS 软件还需要公证，notarization 服务会检查恶意内容与代码签名问题，并在通过后生成 ticket。[Apple：notarizing macOS software](https://developer.apple.com/documentation/security/notarizing_macos_software_before_distribution) **工程判断：** 若要给普通用户提供低摩擦体验，应采用 Developer ID 签名与公证，并把它们放进发布 CI。未签名 macOS 二进制不应承诺为无摩擦体验。
- **Windows（事实）：** Microsoft 的 SignTool 用于签名、时间戳和验证文件/包；SmartScreen 同时评估发布者信誉与文件哈希信誉，因此有效签名能提供稳定发布者身份，却不能保证新二进制立即不再告警。[SignTool](https://learn.microsoft.com/windows/win32/seccrypto/signtool) [SmartScreen reputation for Windows app developers](https://learn.microsoft.com/windows/apps/package-and-deploy/smartscreen-reputation) **工程判断：** 对直接下载的 `.exe`/安装器，Authenticode 代码签名是企业/高信任场景的必需投入之一；须在发布计划中预留证书、密钥保护、时间戳与信誉积累成本。
- **自动更新（工程判断）：** 自动更新器必须验证签名/哈希、固定/可审计更新源、抵抗回滚与中间人、提供关闭和版本固定；否则它把一次下载风险变成持续远程代码执行通道。对 CLI/Agent 默认采用“检查并提示，显式升级”，企业版本由包管理器或管理员策略更新。

## 构建与发布工具的正确位置

| 工具 | 它负责什么 | 它不负责什么 | 适用判断 |
| --- | --- | --- | --- |
| GitHub Actions | CI runner、matrix、测试、构建、上传 | 不自动决定产品应发布到哪些包管理器 | 首发足够；用 matrix 分别在目标平台构建/验证 |
| `dist`（原 cargo-dist） | 生成归档、安装器、manifest 和 release CI；可上传资产/包 | 不是用户通过的单一安装命令，也不替代平台签名策略 | 渠道增多、手写 release workflow 已成负担时采用；先评估生成配置是否符合本项目需求 |
| `release-plz` | 版本、changelog、release PR、cargo registry 发布、创建 Release | 不构建完整跨平台二进制矩阵 | 版本节奏稳定、希望自动化 release PR 时与 Actions/`dist` 互补 |
| `cross` / cross-rs | 以容器提供交叉工具链和库 | 不发布资产、不证明目标机兼容性 | 需要 Linux 多架构/musl 构建时使用；保留真实 target 测试 |

这些工具可组合而非互斥：例如 `release-plz` 决定版本/标签，GitHub Actions 运行测试，`dist` 或自写 workflow 构建并上传 Release，`cross` 仅参与部分 Linux target。首版并不需要四者同时引入。

## 分阶段推荐

### 阶段一：新工具的最小可行发布组合

**工程判断。**

1. 可公开发布源码时：crates.io + `cargo install --locked <crate>`；写清 Rust 最低版本和系统依赖。
2. 同一 tag 的 GitHub Release：四项首发平台归档、`SHA256SUMS`、简短安装页、固定版本示例。
3. 验证 `cargo-binstall <crate>` 能发现所有首发产物，再将其列为 Rust 用户的推荐快装命令；失败时保留 Cargo 与手动下载。
4. 用 GitHub Actions matrix 在各目标 OS 构建/冒烟测试；初期可手写清晰 workflow，不因“以后也许有十个渠道”过早引入发布编排器。

技术上最小可以只有 `cargo publish`；但对于无 Rust 环境的终端用户/Agent，只有源码安装不够。因此上面是本报告推荐的**实用最小**组合。

### 阶段二：有稳定用户后的增长组合

**工程判断。** 在支持负担确实出现后，接入 `dist` 或维护等价的自有 Actions workflow；版本/changelog 痛点明确时加入 `release-plz`。优先按用户系统数据选择：macOS 开发者多则自有 Homebrew tap，Windows 用户多则 Scoop 或 WinGet（也可两者），Nix/Arch 由社区贡献为先。对渠道维护引入一条验收规则：**每次上游 tag 都能在新环境安装、校验并运行 `--version`**。若做 shell/PowerShell 安装器，安装器只消费已发布且校验过的 Release 资产。

### 阶段三：企业或高信任场景

**工程判断。** 增加 macOS Developer ID 签名与公证、Windows Authenticode 签名与时间戳、校验和签名/密钥轮换说明、GitHub artifact attestation、SBOM/依赖审计和固定版本下载。按客户环境提供 WinGet/Chocolatey、Deb/RPM、私有 registry 或受控镜像仓库；不要强制内建自动更新。发布流水线应最小权限、保护发布 tag、隔离签名密钥，并保留可复现的源到产物证据。

## 适用于本仓库的建议

本仓库现有的 Rust 生态选型报告已经把 `dist`、`release-plz`、`cross` 和 GitHub Release 列为候选发布流水线。本报告补充的结论是：它们应服务于下面这个优先级，而不应替代用户入口设计。

1. **在 Rust CLI 首版落地时，优先 GitHub Release + 校验和 + crates.io/Cargo 源码兜底。** 这是面向终端用户和 Agent 的最低覆盖面：无需让非 Rust 用户安装编译器，也保留可审计构建路径。
2. **把 `cargo-binstall` 作为推荐的可选入口，而不是唯一入口。** 先在真 Release 检验资产命名、`repository` metadata 与四个平台；它恰好把 Cargo 用户导向已有 Release 资产。
3. **首版先不做 Homebrew core、AUR 官方维护、Nix、Scoop、WinGet、Chocolatey、Deb/RPM 仓库、npm/PyPI 包装器与应用内自动更新。** 这些并非错误渠道，而是在未掌握目标平台、发布节奏和签名能力前，维护成本超过新增价值。若有明确 Windows 用户，增长期优先评估 WinGet/Scoop；若有 macOS 用户，优先自有 Homebrew tap，不直接以 core 为里程碑。
4. **首版未必需要 `dist` 和 `release-plz` 同时引入。** 先用可读的 GitHub Actions matrix 产出和验证资产；当版本发布与多渠道同步成为可观负担时，`release-plz` 解决版本/changelog，`dist` 解决多产物/安装器编排，二者互补。`cross` 仅按 Linux 多 target 的实际需要加入。
5. **签名按风险升级。** 首版至少校验和 + 受保护的 release workflow；若要承诺 macOS/Windows 的普通用户无阻安装或进入企业，签名、公证、证书和 provenance 不能后补为一句文档。

## 来源与局限

### 一手来源索引

- Rust/Cargo：[cargo install](https://doc.rust-lang.org/cargo/commands/cargo-install.html)、[publishing](https://doc.rust-lang.org/cargo/reference/publishing.html)。
- GitHub：[Releases](https://docs.github.com/en/repositories/releasing-projects-on-github/about-releases)、[Actions matrix](https://docs.github.com/en/actions/using-jobs/using-a-matrix-for-your-jobs)、[artifact attestation](https://docs.github.com/en/actions/security-for-github-actions/using-artifact-attestations-to-establish-provenance-for-builds)。
- Rust 分发工具：[cargo-binstall](https://github.com/cargo-bins/cargo-binstall)、[dist](https://github.com/axodotdev/cargo-dist)、[release-plz](https://github.com/release-plz/release-plz)、[cross](https://github.com/cross-rs/cross)。
- 包管理器：[Homebrew taps](https://docs.brew.sh/Taps)、[Scoop buckets](https://github.com/ScoopInstaller/Scoop/wiki/Creating-a-bucket)、[WinGet manifests](https://learn.microsoft.com/windows/package-manager/winget/package/)、[Chocolatey](https://docs.chocolatey.org/en-us/create/create-packages/)、[Arch AUR](https://wiki.archlinux.org/title/Arch_User_Repository)、[Debian packaging](https://www.debian.org/doc/manuals/maint-guide/)、[RPM packaging](https://rpm-packaging-guide.github.io/)、[Nix](https://nix.dev/manual/nix/latest/command-ref/new-cli/nix3-run.html)。
- 平台安全：[Apple notarization](https://developer.apple.com/documentation/security/notarizing_macos_software_before_distribution)、[Microsoft SignTool](https://learn.microsoft.com/windows/win32/seccrypto/signtool)、[SmartScreen reputation for Windows app developers](https://learn.microsoft.com/windows/apps/package-and-deploy/smartscreen-reputation)。

### 局限与待落地验证

- 调查日为 2026-07-30；各包管理器的审核政策、工具能力和支持目标会变化。实施前应复查当前官方文档，尤其是 `dist` 生成的配置与包管理器提交要求。
- 没有把渠道星数、下载量或搜索结果当作“最常见”的证据，也没有对本仓库尚未发布的 Rust 二进制假定具体 crate 名、目标支持或签名账户。
- Linux 的 glibc/musl 选择、每个 target 的网络/TLS 行为、`cargo-binstall` 实际识别、Homebrew/WinGet manifest、签名和公证都必须在首个真实 Release 中端到端验证；本报告不把构建成功等同于用户机兼容。
