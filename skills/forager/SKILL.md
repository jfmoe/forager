---
name: forager
description: "Search current web and X/Twitter sources with the local forager CLI. Use for known-URL retrieval, site mapping, official/API documentation lookup, vertical discovery, source-backed fact checking, deep research, or forager configuration and diagnostics."
---

# forager

Use the local `forager >=0.3.0` CLI as the execution layer. The CLI owns provider selection,
ordering, credential rotation, and same-capability fallback. If the command is missing or below the
required version, report the observed command or version and stop this skill run.

## Run commands to completion

Treat a command as complete only at terminal exit. When the runner yields a handle, poll that handle
until it exits before reading output, diagnosing, or retrying. Main search can run for 60 seconds or
longer; keep its default `--timeout` and give the outer runner more headroom.

This step is complete when the selected command has a terminal exit and its full result is
available.

## Route on the cost ladder

Route every request through `direct retrieval → ordinary search → research`. Choose the cheapest
branch that can complete the request. Run one eligible branch, then escalate only when the request
shape or an observed shortfall shows that branch cannot finish the task. Research is the most
expensive branch.

- For a known URL, PDF, site map, or provider-direct operation, read
  [`references/direct-retrieval.md`](references/direct-retrieval.md) and follow it through
  completion.
- For a request one live search can complete, read
  [`references/ordinary-search.md`](references/ordinary-search.md) and follow it through
  completion.
- For a request that requires decomposition or multi-source verification, read
  [`references/research.md`](references/research.md) and follow it through completion.

Routing is complete when exactly one branch matches the request shape or the preceding branch has
returned an observable reason to escalate.

## Diagnose or configure

For a configuration error, use `forager config path` and `forager config list`; repair a parseable
document with `forager config set` or `forager config unset`, and edit the reported path when its
TOML is malformed.

For provider authentication, availability, or connectivity failures, use
`forager doctor --provider PROVIDER --format json`.

Diagnosis is complete when the reported cause and recovery command are actionable.

## CLI reference

Read [`references/cli.md`](references/cli.md) only for exact command syntax, non-routine commands,
or diagnosis and recovery details. Treat `forager <command> --help` as the final parsing authority.
