# Platform retrieval

A platform command retrieves platform-native items: a typed ref, the platform's metadata, and a
stated content depth. Check [`platform-vocabulary.json`](platform-vocabulary.json) first. A
platform absent from it has no platform command; follow
[`direct-retrieval.md`](direct-retrieval.md) or the cost ladder in `SKILL.md` for it instead (for
example, `x.com` links keep the authenticated-client guidance).

## Choose the branch

- **Known item** (the user gave a platform URL or ref, or forager returned one): run
  `forager platform <id> fetch 'REF_OR_URL'`. Prefer it over `forager fetch` for the same link; it
  returns the versioned ref, metadata, and the configured full-text route.
- **Platform-only results, or platform options** (category, author, title, date range, sort): run
  `forager platform <id> search QUERY`. Set only options the request states or clearly implies.

`forager platform <id> <op> --help` is the syntax authority; see the platform section of
[`cli.md`](cli.md) for output fields and exit codes.

## Read full text

`fetch` defaults to `--depth full_text`: it writes the body to a Markdown file and stdout carries
only metadata and `content_url`, `content_provider`, `content_path`, `content_len`.

1. Read the stdout metadata first.
2. List the headings with `grep -n '^#' CONTENT_PATH`, then read only the line ranges you need.
3. For several papers, hand each `content_path` to a subagent that returns only conclusions and
   citations.

Use `--depth abstract` when the abstract answers the request. Use `--format content` only when the
body must enter context or a pipe directly.

## Consume results

- Search results are candidates at `abstract` depth, not evidence of the paper body.
- Pass a platform URL or ref exactly as the user gave it or as forager returned it. Cite and page
  with the ref, URL, and cursor that forager returned. Construct no ref or URL yourself.
- State the content depth behind each claim: `abstract` or `full_text`, and for full text whether
  `content_url` is the HTML or the PDF.
- **arXiv evidence**: cite the versioned ref (`arxiv:2401.04088v1`); an abstract is not the full
  text; make no claim about authors, affiliations, or categories beyond the returned metadata.

## Recover

- Exit 2 naming an unsupported option: retry once without that option and tell the user which
  condition you relaxed.
- A `--cursor` failure: drop the cursor and search again from the first page.
- Exit 3, or an `auth` error: go to Diagnose or configure in `SKILL.md`.
- Exit 5 on full-text fetch (both HTML and PDF too thin): report it; use `--depth abstract` only
  when the abstract still answers the request, and say so.

Platform retrieval is complete when the requested items or content are read at the depth the answer
needs, or a terminal failure is reported with its recovery.
