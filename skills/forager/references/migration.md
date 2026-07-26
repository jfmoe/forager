# Skill 切换

同时发现 `smart-search-cli` 与 `forager` 时，`smart-search-cli` 是应删除的残留。两个
Skill 不能并存：旧 Skill 会继续生成已经废止的命令与计划。

先删除旧 Skill，再安装新 Skill：

1. 列出曾保存旧 Skill 的每个 Skill Container。
2. 从每个 container 删除名为 `smart-search-cli` 的目录。例如，默认 Agents container
   使用 `rm -rf ~/.agents/skills/smart-search-cli`；其他 container 只删除对应的同名子目录。
3. 运行 `npx skills add jfmoe/forager`，选择 `forager`。
4. 再次检查所有 container：应只发现 `forager`，不应发现 `smart-search-cli`。

此切换不修改 forager 的 provider 配置。Skill 安装由 `npx skills` 管理，CLI 不维护安装
目标，也不在启动时同步 Skill。
