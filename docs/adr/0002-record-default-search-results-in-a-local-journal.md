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
- Each invocation writes one complete JSON object plus a trailing newline to an exclusive `search_result_<nanos>_<pid>_<seq>.json` file and calls `sync_all` before retention cleanup. The unique path string is the opaque record identity exposed as `journal_ref`; there is no separate `record_id`.
- The journal does not use daily JSONL partitions, cross-process append locks, or partial-line recovery.
- Retention uses the file modification time and owns only regular files whose names fully match `search_result_<nanos>_<pid>_<seq>.json`, with three non-empty ASCII decimal fields. Other JSON or JSONL files, approximate names, directories, and links are ignored.
- Retention cleanup is best-effort after the new record has been written and synchronized. A cleanup warning does not change the command result or exit code.
- A preflight failure is journaled only after a valid, enabled journal configuration is available and the failure belongs to a recordable invocation. When configuration is invalid or unavailable, JSON output reports `journal_ref: null` and `journal_status: "unavailable"` without inferring defaults or creating the default journal directory.
- `journal.retention_days` defaults to `30`; `0` retains entries indefinitely.

## Consequences

Full queries, answers, sources, and execution facts are intentionally persisted for review and diagnosis, so directory and file permissions must remain user-only. Independent files avoid shared append and record-addressing coordination, while strict filename ownership prevents retention from deleting neighboring application data. Debug output stays on stderr and is not a second persistent log.
