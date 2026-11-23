# Record every search invocation in a local journal

## Decision

forager keeps a Search Result Journal, enabled by default, as its only persistent record of what a
search did. Every Default Search Invocation that reaches a terminal state, success or failure,
writes one entry with two surfaces: the result (query, full answer, sources) and the execution (plan
summary, provider attempts, terminal attribution, budget, capability gaps).

- Persisted content follows an allowlist. Credentials in any form, request and response headers,
  request bodies, raw response bodies and tool traces are never written; URLs and messages pass
  through the shared redactor first. The journal directory and its files are readable by the user
  only, because full queries and answers are deliberately kept.
- The application layer writes the entry once, after the terminal state is known. A journal
  failure is a non-fatal diagnostic and never changes the command's result or exit code.
- Each entry is its own exclusively created file, synced before anything else happens, and its
  path is the entry's identity. Retention deletes only regular files whose names fully match the
  journal's own naming pattern, so it cannot remove neighbouring data.
- When no valid journal configuration exists yet, forager reports the journal as unavailable
  instead of guessing a default location.

## Why

- Reviewing and diagnosing a search later needs the full query and answer next to the attempts
  that produced them; a summary or a debug stream loses exactly the facts a review asks about.
- One file per invocation needs no cross-process append lock, no partial-line recovery and no
  separate record identifier: concurrent invocations never touch the same file.
- Strict name ownership makes retention safe in a directory the user may share with other data.

## Rejected alternatives

- **Daily JSONL partitions with an append lock.** Concurrent processes would contend for one
  file, a crash could leave a partial line that every reader must recover from, and addressing a
  record would need an identifier beyond its location.
- **Off by default.** The record would be missing exactly when a user first needs to review a
  surprising result.
- **Persisting debug logs as a second record.** Debug output stays on stderr; two persistent
  records would disagree about what happened.

## Revisit when

- A consumer needs to query across entries at a volume that per-file storage makes slow.
- An entry is found to persist something sensitive that the allowlist did not anticipate.
