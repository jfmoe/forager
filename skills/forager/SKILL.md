---
name: forager
description: "Search current web and X/Twitter sources with the local forager CLI. Use for known-URL retrieval, site mapping, official/API documentation lookup, vertical discovery, source-backed fact checking, deep research, or forager configuration and diagnostics."
---

# forager

Use the local `forager >=0.1.0` CLI as the execution layer. The CLI owns provider selection,
ordering, credential rotation, and same-capability fallback.

Treat a command as complete only at terminal exit. When the runner yields a handle, poll that handle
until it exits before reading output, diagnosing, or retrying. Main search legitimately runs 60
seconds or longer on complex queries; keep the default `--timeout`, give any outer tool timeout more
headroom than it, and do not abandon the run early.

If `forager` is missing or below the required version, report the observed command or version and
stop this Skill branch.

## Ordinary search

Use ordinary search for most requests. Read
[`references/ordinary-search.md`](references/ordinary-search.md) and follow it through completion.

## Research

Use research only when the request requires decomposition, multi-source verification, or explicitly
asks for deep research. Read [`references/research.md`](references/research.md) and follow it
through completion.

## Direct retrieval

For a task limited to reading a known URL or PDF, use `forager fetch URL --format json`. For site
structure discovery, use `forager map URL --instructions "GOAL" --format json`.

When a URL requires authentication and cannot be fetched directly, such as `x.com`, use another
available tool with authenticated access, such as a browser, to retrieve its content.

Commands under `exa`, `context7`, and `anysearch` bypass capability routing. Use them when the user
requests that provider or the operation exists only as a direct command.

Complete this branch when it returns the requested content or page set, or route its observed
failure to Diagnose or configure.

## Diagnose or configure

For a configuration error, use `forager config path` and `forager config list`; repair a parseable
document with `forager config set` or `forager config unset`, and edit the reported path when its
TOML is malformed.

For provider authentication, availability, or connectivity failures, use
`forager doctor --provider PROVIDER --format json`. Complete this branch when the reported
diagnostic and recovery command are actionable.

## CLI reference

Read [`references/cli.md`](references/cli.md) only for exact command syntax, non-routine commands,
or diagnosis and recovery details. Treat `forager <command> --help` as the final parsing authority.
