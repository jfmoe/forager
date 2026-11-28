# What every platform shares

- **Refs and URLs**: pass a platform URL, ref or cursor exactly as the user gave it or forager
  returned it, single-quoted. Build no ref, ID or URL yourself from a title, slug or snippet.
- **Options**: set a search filter or sort only when the task states or clearly implies it.
- **One platform**: a platform command returns only that platform's items and never falls back to
  another; a general question still goes through the branch the entry chose.
- **Empty is not an error**: an empty `items` with exit 0 means nothing matched.

## Content depth

Every item carries `depth`, the content it actually holds. State the depth behind each claim.

- `metadata`: the item's fields only; it supports claims about title, author, date and counts.
- `snippet`: an excerpt the search showed, never the abstract or the body.
- `abstract`: the abstract or description the publisher wrote.
- `full_text`: the body, written to a file.
- `thread`: a post with its discussion, written to a file.

## Read a file result

A `full_text` or `thread` fetch writes the body to a Markdown file; stdout carries the metadata
with `content_path`, `content_len` and, for a page read from elsewhere, `content_url`.

1. Read the stdout metadata first.
2. List the headings with `grep -n '^#' CONTENT_PATH`, then read only the line ranges you need.
3. For several files, hand each `content_path` to a subagent that returns only conclusions and
   citations.

`--format content` prints the body instead of writing a file; use it only when the body must enter
context or a pipe directly. `--content-dir DIR` chooses where the file goes.

## Pages

`next_cursor` continues a search: pass it alone with `--cursor`, without the query, options or
`--limit`, because it restores the whole request. `null` means no page follows; a platform file
says when that does not mean the results are complete.

## Recover

- Exit 2 naming an option the route does not support: retry once without it and tell the user
  which condition you dropped. Any other exit 2: fix the input the message names.
- A cursor that fails: drop it and search again from the first page.
- Exit 3, or an `auth` failure on a keyed route: read [`diagnostics.md`](../diagnostics.md). Exit
  3 because an opt-in route is off: follow the opt-in rule in the entry.
- `timeout`: the deadline includes waiting for the platform's request pace; retry once with a
  larger `--timeout`.
- A failure the platform file does not explain, or a result that looks wrong: read
  [`feedback.md`](../feedback.md).

A platform task is complete when the items or content it needs are read at the depth the answer
rests on, or a failure is reported with what was tried.
