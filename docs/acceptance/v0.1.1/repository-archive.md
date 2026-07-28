# v0.1.1 repository archive

Date: 2026-07-28 Asia/Shanghai

This record completes the final two steps of the retirement order in
[`docs/spec/forager/06-migration.md`](../../spec/forager/06-migration.md). The prerequisite local
entrypoint and npm retirement work was completed in
[#23](https://github.com/jfmoe/forager/issues/23).

## Authority boundary

The transferred Wayfinder issues preserve the Python-era decision trail only. They are not a
second forager specification and do not authorize new implementation work. The sole current
specification remains [`docs/spec/forager/`](../../spec/forager/).

All ten transferred issues are closed, have no actionable triage label, have a `[历史档案]` title
prefix, and contain an explicit comment that resolves any conflict in favor of the repository
specification.

## Wayfinder migration

The pre-transfer inventory contained one open Map, nine closed direct children, 32 comments, and
seven blocking relationships. GitHub only transfers open issues, so each child was reopened for
transfer and immediately closed again in forager. One archival-boundary comment was added to every
transferred issue, bringing the post-transfer comment count to 42 without losing any of the 32
original comments.

| smartsearch | forager | Original comments | Current comments |
|---|---|---:|---:|
| [#52](https://github.com/jfmoe/smartsearch/issues/52) | [#29](https://github.com/jfmoe/forager/issues/29) | 0 | 1 |
| [#53](https://github.com/jfmoe/smartsearch/issues/53) | [#30](https://github.com/jfmoe/forager/issues/30) | 1 | 2 |
| [#54](https://github.com/jfmoe/smartsearch/issues/54) | [#31](https://github.com/jfmoe/forager/issues/31) | 2 | 3 |
| [#55](https://github.com/jfmoe/smartsearch/issues/55) | [#32](https://github.com/jfmoe/forager/issues/32) | 1 | 2 |
| [#56](https://github.com/jfmoe/smartsearch/issues/56) | [#33](https://github.com/jfmoe/forager/issues/33) | 4 | 5 |
| [#57](https://github.com/jfmoe/smartsearch/issues/57) | [#34](https://github.com/jfmoe/forager/issues/34) | 4 | 5 |
| [#58](https://github.com/jfmoe/smartsearch/issues/58) | [#35](https://github.com/jfmoe/forager/issues/35) | 6 | 7 |
| [#59](https://github.com/jfmoe/smartsearch/issues/59) | [#36](https://github.com/jfmoe/forager/issues/36) | 7 | 8 |
| [#60](https://github.com/jfmoe/smartsearch/issues/60) | [#37](https://github.com/jfmoe/forager/issues/37) | 5 | 6 |
| [#61](https://github.com/jfmoe/smartsearch/issues/61) | [#38](https://github.com/jfmoe/forager/issues/38) | 2 | 3 |

The migrated Map reports nine direct children, all completed. Every child reports #29 as its
parent. The seven pre-transfer blocking relationships retained their direction and now resolve as:

- #33 blocks #36 and #37.
- #34 blocks #37.
- #35 blocks #37.
- #38 blocks #36 and #37.
- #36 blocks #37.

Anonymous HTTP checks returned 200 for the old #52 URL after it redirected to forager #29, the
migrated Map itself, and the current specification.

## smartsearch archive

The smartsearch README terminal notice was committed and pushed as
[`4e76689`](https://github.com/jfmoe/smartsearch/commit/4e76689). It directs current development,
documentation, and issue work to forager while describing smartsearch as the Python-era
implementation and decision archive.

The public guide issue
[`jfmoe/smartsearch#62`](https://github.com/jfmoe/smartsearch/issues/62) points to forager, the
historical Map, and the current specification. GitHub's pinned-issues query returned #62 as the
repository's pinned issue. Anonymous HTTP checks returned 200 for both the archived repository and
the guide issue before and after archival.

Immediately before archival, neither the active shell nor a new login shell resolved a
`smart-search` command. The repository was archived rather than deleted. The final repository
checks reported:

- `jfmoe/smartsearch`: `archived=true`, `disabled=false`.
- `jfmoe/forager`: `archived=false`, issues enabled, and the owner retains push and issue-write
  permissions.

The archive operation is reversible through GitHub repository settings; the Python implementation,
decision history, releases, and redirected issue URLs remain readable.
