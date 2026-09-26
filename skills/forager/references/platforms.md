# Platform retrieval

A platform command retrieves platform-native items: a typed ref, the platform's metadata, and a
stated content depth. Check [`platform-vocabulary.json`](platform-vocabulary.json) first. A
platform absent from it has no platform command; follow
[`direct-retrieval.md`](direct-retrieval.md) or the cost ladder in `SKILL.md` for it instead (for
example, `x.com` links keep the authenticated-client guidance).

## Choose the branch

- **Known item** (the user gave a platform URL, ref, or SSRN DOI, or forager returned one): run
  `forager platform <id> fetch 'REF_OR_URL'`. Prefer it over `forager fetch` for the same link; it
  returns the platform ref, metadata, and the content its routes can read.
- **Platform-only results, or platform options** (category, author, title, date range, sort): run
  `forager platform <id> search QUERY`. Set only options the request states or clearly implies.

`forager platform <id> <op> --help` is the syntax authority; see the platform section of
[`cli.md`](cli.md) for output fields and exit codes.

## Content depth

Every item carries `depth`, the content it actually holds: `metadata` (bibliographic fields only,
`abstract: null`), `snippet` (a search excerpt, never the abstract), `abstract`, or `full_text`.

- **arXiv** `fetch` defaults to `--depth full_text`; `--depth abstract` skips the body.
- **SSRN** `fetch` defaults to `--depth metadata` and includes the abstract when a route has one.
  `--depth abstract` requires an abstract and fails with exit 5 when none is available. SSRN full
  text is not available: `--depth full_text` exits 2.

## Read full text

A full-text fetch writes the body to a Markdown file and stdout carries only metadata and
`content_url`, `content_provider`, `content_path`, `content_len`.

1. Read the stdout metadata first.
2. List the headings with `grep -n '^#' CONTENT_PATH`, then read only the line ranges you need.
3. For several papers, hand each `content_path` to a subagent that returns only conclusions and
   citations.

Use `--depth abstract` when the abstract answers the request. Use `--format content` only when the
body must enter context or a pipe directly.

## Consume results

- Search results are candidates at `abstract` or `metadata` depth, not evidence of the paper body.
- Pass a platform URL or ref exactly as the user gave it or as forager returned it. Cite and page
  with the ref, URL, and cursor that forager returned. Construct no ref or URL yourself.
- State the content depth behind each claim: `metadata`, `abstract`, or `full_text`, and for full
  text whether `content_url` is the HTML or the PDF. A `metadata` item supports claims about its
  title, authors, and date only.
- **arXiv evidence**: cite the versioned ref (`arxiv:2401.04088v1`); an abstract is not the full
  text; make no claim about authors, affiliations, or categories beyond the returned metadata.
- **SSRN evidence**: cite the ref (`ssrn:2042750`) and its abstract-page `url`; SSRN refs have no
  version. `published` is the date the route reports, at its own precision (often only a year);
  `crossref_created` is when Crossref registered the DOI, never the SSRN posting date. Use only the
  URLs forager returns; SSRN download links are never derived from the abstract ID.

## Recover

- Exit 2 naming an unsupported option: retry once without that option and tell the user which
  condition you relaxed.
- A `--cursor` failure: drop the cursor and search again from the first page.
- Exit 3, or an `auth` error: go to Diagnose or configure in `SKILL.md`.
- Exit 5 on full-text fetch (both HTML and PDF too thin): report it; use `--depth abstract` only
  when the abstract still answers the request, and say so.
- Exit 5 on an SSRN `--depth abstract` fetch: no route had the abstract. Fetch at the default
  `metadata` depth and state that the abstract is unavailable.
- "SSRN paper not found in Crossref": the paper may still exist on SSRN; say that Crossref has no
  record of it rather than that the paper does not exist.

Platform retrieval is complete when the requested items or content are read at the depth the answer
needs, or a terminal failure is reported with its recovery.
