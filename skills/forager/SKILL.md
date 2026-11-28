---
name: forager
description: "Search current web and X/Twitter sources with the local forager CLI. Use for known-URL retrieval, platform items such as arXiv, SSRN, or Google Scholar papers, Xiaohongshu (小红书) notes, X posts, Hacker News discussions, YouTube videos, Polymarket prediction markets, and Reddit discussions, site mapping, official/API documentation lookup, source-backed fact checking, deep research, or forager configuration and diagnostics."
---

# forager

Use the local `forager` CLI as the execution layer. The CLI owns provider selection, ordering,
credential rotation, and same-capability fallback. If `forager` is not installed, report it and stop
this skill run. If a command or option this skill names is missing, the installed forager is older
than this skill: report the observed `forager --version` and ask the user to upgrade forager.

## Run commands to completion

Treat a command as complete only at terminal exit. When the runner yields a handle, poll that handle
until it exits before reading output, diagnosing, or retrying. Main search can run for 60 seconds or
longer; keep its default `--timeout` and give the outer runner more headroom.

This step is complete when the selected command has a terminal exit and its full result is
available.

## Route on the cost ladder

When the request names a platform, supplies a platform URL or ref, or needs platform-native items,
read [`references/platforms.md`](references/platforms.md) first; it decides whether a platform
command serves the request and covers its direct and search branches.

Route every request through `direct retrieval → ordinary search → multi-source investigation`.
Choose the cheapest branch that can complete the request. Run one eligible branch, then escalate
only when the request shape or an observed shortfall shows that branch cannot finish the task. A
multi-source investigation is the most expensive branch.

- For a known URL, PDF, site map, or provider-direct operation, read
  [`references/direct-retrieval.md`](references/direct-retrieval.md) and follow it through
  completion.
- For a request one live search can complete, read
  [`references/ordinary-search.md`](references/ordinary-search.md) and follow it through
  completion.
- For a request that requires decomposition or multi-source verification, orchestrate the
  investigation yourself. Split the question into parts; run `forager search` for a part that
  needs discovery, `forager fetch` for each source that may answer it, and the platform commands
  for platform-native items. Judge each source on whether it bears on the question, not by how
  many sources there are. Cite only fetched content, and disclose each part no source answered.
  Stop when further searches are unlikely to change the answer, the task budget is reached, or
  access is blocked, and report what remains open.

Routing is complete when exactly one branch matches the request shape or the preceding branch has
returned an observable reason to escalate.

## Diagnose or configure

For a configuration error, use `forager config path` and `forager config list`; repair a parseable
document with `forager config set` or `forager config unset`, and edit the reported path when its
TOML is malformed.

For a provider's configuration, prerequisites, or key validity, use
`forager doctor --provider PROVIDER --format json`; it sends no billed request and opens no page.
For connectivity of every configured provider, use `forager doctor --format json`. To see which
providers have been failing in the user's real use, including failures a fallback hid, use
`forager doctor --health --format json`; it reads only local records and sends nothing. To check
against the real service that a provider's operations still work, use
`forager doctor --live --provider PROVIDER`, but only when the user asks for it: its requests can
spend quota, and for a route in the `attended` tier, naming it is the user's consent to open pages
of a logged-in site or put the egress address at risk.

Diagnosis is complete when the reported cause and recovery command are actionable.

## CLI reference

Read [`references/cli.md`](references/cli.md) only for exact command syntax, non-routine commands,
or diagnosis and recovery details. Treat `forager <command> --help` as the final parsing authority.
