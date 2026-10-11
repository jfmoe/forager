# 合入与发版

## 合入 main

- 分支经 PR 合入：CI 的 `quality`、`test`、`windows-permissions` 全绿后，运行 `gh pr merge <N> --rebase --delete-branch`。用 rebase 合并是为了保留每个 Conventional Commit，release-plz 按提交生成 changelog。
- GitHub 的 rebase 合并会重写提交哈希。合并后先确认本地与远端的树一致，再运行 `git pull --rebase`，本地同内容的提交会被跳过。

## 发版

版本号和 changelog 由 release-plz 生成，`CHANGELOG.md` 不手改。`release-plz.yml` 只能手动触发，每次发版要触发两次：

1. 运行 `gh workflow run release-plz.yml --ref main`，等这次 run 结束。它会开一个 `chore: release vX.Y.Z` PR，核对其中的版本号和 changelog。
2. 等这个 release PR 的检查通过，运行 `gh pr merge <N> --squash --delete-branch`。release PR 一律 squash 合并。
3. 运行 `git pull --rebase`，然后再次触发 `release-plz.yml`。这一次它会打 tag，创建草稿 Release，并触发 `release.yml`（cargo-dist 构建）。用 `gh run list --workflow release.yml --limit 1` 和 `gh run watch <ID> --exit-status` 等构建结束。

`gh release view vX.Y.Z --json isDraft` 显示 `isDraft` 为 `false` 时，发版完成：`release.yml` 只在各平台制品都通过校验后才公开 Release。

## 收尾

发版后依次完成以下步骤，全部完成才算结束任务：

1. **更新本机 CLI**：本机的 forager 是 cargo-dist 安装器装到 `~/.cargo/bin` 的，回执在 `~/.config/forager/forager-receipt.json`。用同一个安装器更新：

   ```sh
   curl --proto '=https' --tlsv1.2 -LsSf \
     https://github.com/jfmoe/forager/releases/download/vX.Y.Z/forager-installer.sh | sh
   ```

   然后确认 `forager --version` 是新版本。`cargo install --path .` 不能代替安装器：目标位置已有不归 cargo 管理的同名二进制，它会拒绝覆盖。验证未发布的改动时，直接运行 `./target/debug/forager`。
2. **更新 skill**：运行 `npx skills add jfmoe/forager -g -s forager -a '*' -y`。不支持全局安装的 agent 会报失败，这是预期结果。
3. **更新 OpenCLI adapter**：本次发版改过的 `skills/forager/opencli/<name>`，按 [`opencli.md`](opencli.md) 重新安装，并用 `diff -r` 确认装上的和 skill 里的一致。
4. **live 端到端测试**：用真实配置运行 `forager smoke --live`。默认时限 420 秒，命令的工具超时要设得更长。逐个用例报告状态。消息以 `not started:` 开头的 `skipped` 表示时限用完，加大 `--timeout` 后重跑，不要当成 provider 故障。
5. **清理构建产物**：运行 `cargo clean -p forager`。forager 自身的测试二进制和增量编译目录会不断累积，这条命令只删 forager 自己的产物，第三方依赖的编译结果保留。
