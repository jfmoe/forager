---
status: accepted
---

# Record search results and execution in a local journal

forager maintains a Search Result Journal as its persistent observability surface. The journal is enabled by default and records one terminal entry for each completed Default Search Invocation, whether it succeeds or fails.

## Decision

- `journal.enabled` defaults to `true`.
- The default directory follows XDG state and resolves to `~/.local/state/forager/journal` when `XDG_STATE_HOME` is unset.
- Each entry has two surfaces. The result surface may contain the query, full answer, normalized sources, and research citations and evidence items. The execution surface may contain the plan summary, provider attempts, terminal attribution, deadline budget, classifier duration, and capability gaps.
- Persisted fields follow an allowlist. Provider attempts may include provider, seam, error kind, HTTP status, duration, credential index, retry and rotation counts, a redacted error message truncated to 500 characters, model, endpoint host, and circuit-breaker events.
- The journal never persists request or response headers, request bodies, raw response bodies, credentials in any form, classifier prompt text, or tool traces.
- URLs and error messages pass through the shared redactor before persistence.
- The application layer owns the terminal writer and writes once for both successful and failed outcomes. A journal write failure emits a non-fatal diagnostic and does not change the command result or exit code.
- `journal.retention_days` defaults to `30`; `0` retains entries indefinitely.

## Consequences

Full queries, answers, sources, and execution facts are intentionally persisted for review and diagnosis, so directory and file permissions must remain user-only. Debug output stays on stderr and is not a second persistent log.
