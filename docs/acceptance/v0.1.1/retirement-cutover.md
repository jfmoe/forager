# v0.1.1 retirement cutover

Date: 2026-07-28 Asia/Shanghai

This cutover follows the retirement order in
[`docs/spec/forager/06-migration.md`](../../spec/forager/06-migration.md). The prerequisite
switch gate was opened by issue
[#22](https://github.com/jfmoe/forager/issues/22#issuecomment-5097912743) against forager v0.1.1.

## Skill replacement

The Python-era installation preference at `~/.config/smart-search/config.json` recorded these three
Skill Containers:

- `~/.agents/skills`
- `~/.claude/skills`
- `~/.hermes/skills`

`smart-search-cli` was removed from all three before running:

```console
npx skills add jfmoe/forager -g --agent codex claude-code hermes-agent --skill forager -y --copy
```

The post-install filesystem check found zero `smart-search-cli` directories and one `forager`
directory in each recorded container. `npx skills list -g --json` reported the installed `forager`
Skill with source `jfmoe/forager`.

## npm retirement release

The final Python distribution patch is
[`@jfmoe/smart-search@0.7.2`](https://www.npmjs.com/package/@jfmoe/smart-search/v/0.7.2), built from
commit
[`85a8d76`](https://github.com/jfmoe/smartsearch/commit/85a8d76d7f0d8290c54b7e6cf1e59329fa001c2c).
Its README and postinstall output direct users to `jfmoe/forager` and
`npx skills add jfmoe/forager`. GitHub Actions run
[`30314141922`](https://github.com/jfmoe/smartsearch/actions/runs/30314141922) published the npm
package through trusted publishing and created the
[`v0.7.2` GitHub Release](https://github.com/jfmoe/smartsearch/releases/tag/v0.7.2). The official
npm registry reported `latest=0.7.2`.

The full published range, from `0.2.0-beta.1` through `0.7.2`, was deprecated on the official npm
registry with `Deprecated: use forager instead — https://github.com/jfmoe/forager`. A per-version
registry query confirmed the same message on all 14 versions. No version was unpublished.

## Local entrypoint and Agent guidance

The v0.1.1 Release installer installed an arm64 Mach-O binary at `~/.cargo/bin/forager` without
changing shell profiles. The active PATH resolved that binary and it reported `forager 0.1.1`.

The global Codex Agent guidance now prefers `forager`. A retirement scan found no remaining
`smart-search`, npm installation, routing command, or provider-native environment-variable
references in the global Codex and Claude guidance.

The local Hermes npm package, its generated binary link, and the user-level forwarding link were
removed. Both the active shell and a new login shell reported no `smart-search` command.

回退：仓库 archive 可逆、npm 旧版可重装。
