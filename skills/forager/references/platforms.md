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
- **Xiaohongshu (小红书) notes**: only `forager platform xiaohongshu search QUERY` exists so far;
  there is no Xiaohongshu fetch yet. For a xiaohongshu.com link, give the user the link (or a
  result's `access_url`) to open; `forager fetch` cannot read it, because the page needs the
  user's login.
- **Papers citing a known paper** (follow-up work, replications, or rebuttals): run
  `forager platform scholar cited-by 'REF_OR_URL'`; see "Google Scholar cited-by".

`forager platform <id> <op> --help` is the syntax authority; see the platform section of
[`cli.md`](cli.md) for output fields and exit codes.

## Content depth

Every item carries `depth`, the content it actually holds: `metadata` (bibliographic fields only,
`abstract: null`), `snippet` (a search excerpt, never the abstract), `abstract`, or `full_text`.

- **arXiv** `fetch` defaults to `--depth full_text`; `--depth abstract` skips the body.
- **SSRN** `fetch` defaults to `--depth metadata` and includes the abstract when a route has one.
  `--depth abstract` requires an abstract and fails with exit 5 when none is available. SSRN full
  text needs the `ssrn_browser` route: `--depth full_text` downloads the PDF in Chrome, converts
  it to Markdown through the Web Fetch chain, writes `ssrn-<id>.md`, and removes the PDF unless
  `--keep-pdf` keeps it next to the Markdown. Without the browser route it exits 2.
- **SSRN search** items from `ssrn_crossref` are `abstract` or `metadata`; items from
  `ssrn_browser` are `snippet` (`metadata` for a result card without an excerpt). A browser page never spans two SSRN result pages, so it can hold
  fewer items than `--limit`; follow `next_cursor` for more.
- **Xiaohongshu** search items are `metadata`: the card title, author, date, and counts, never the
  note body.
- **Scholar** search items are `snippet` (`metadata` for a result without an excerpt); `fetch` is
  `metadata` only and lists the paper's versions. Google Scholar holds no body; read it from the
  source, see "Read a Scholar paper".

## arXiv search queries

arXiv matches literal words, not meaning, and every query term must match. Use a few distinctive
keywords and quote multi-word concepts as phrases; scattered words match unrelated papers,
especially with `--sort submitted`. For an open-ended topic question, discover papers through
general search first and read them with `platform arxiv fetch`.

```console
forager platform arxiv search '"world model" robot manipulation' --category cs.RO --sort submitted
```

## SSRN advanced search

For title, author, affiliation, date, identifier, or ranking requirements, use typed SSRN search
flags; see [`cli.md`](cli.md#platform-ssrn-search). For example:

```console
forager platform ssrn search "dual momentum" --scope title --author Antonacci --has-abstract
```

The example uses Crossref because it requires an abstract field. Title queries do not require an exact phrase. Date filters
separate publication, Crossref registration, and metadata updates. ORCID and funder filters only
match deposited identifiers, so missing metadata can exclude relevant papers. If a configured
route cannot apply the criteria, preserve the request and report the support error. Continue a
result page with its cursor alone; do not repeat search criteria or fetch papers unless needed.

For native SSRN search, use an enabled browser route with `--scope title|all|full-text`,
`--mode fuzzy|boolean`, `--author`, `--date`, or `--sort posted|downloads|title` with `--order`.
Boolean supports AND, OR, NOT, and parentheses. Author text follows the site's matching rules;
try the surname when a full name has no matches, but report that changed condition.
`--scope full-text` still returns search cards only. The browser rejects affiliation, type,
abstract-presence, and arbitrary date-range filters; it does not approximate them locally.

```console
forager platform ssrn search "momentum AND portfolio" --scope title --mode boolean --date last-year --sort downloads
```

## Google Scholar search

Scholar runs on the user's own SerpApi key and its monthly search quota (250 on the free plan).
Every search page, every fetch, and every cited-by page costs one search, including a search with
no results and a fetch of a ref that does not exist. Repeating an identical request within one hour costs nothing.
`forager doctor --provider serpapi` costs nothing and reports each key's searches left and this
hour's usage; run it when the user asks how much Scholar quota remains.

- Search once with the default `--limit 20` and pick candidates from that page. Do not page with
  `--cursor` to collect more results; narrow the query or the years instead.
- Fetch a cluster only when you need its version links, for example to find a readable copy.

The query goes to Google Scholar unchanged. Quote phrases; `author:"Surname"` restricts the whole
query, including both sides of `OR`. These forms were checked against live Google Scholar:

```console
forager platform scholar search '"time series momentum" author:"Pedersen"'
forager platform scholar search 'trend OR reversal author:"Moskowitz"'
forager platform scholar search 'time series momentum' --year-from 2020
forager platform scholar search 'time series momentum' --review-only
```

`--year-from` and `--year-to` bound the publication year and can be used alone; `--review-only`
keeps review articles. Set them only when the request states or clearly implies them.

## Google Scholar cited-by

`cited-by` lists the papers Google Scholar counts as citing one paper, as search results with the
same fields. Use it to trace work forward from a known paper; arXiv and SSRN cannot. Pass the ref
or cluster URL exactly as forager or the user gave it.

- Narrow before paging: a well-cited paper has thousands of citing works, and each page costs one
  search. Use `--query` to match words within the citing papers, `--year-from`/`--year-to` for
  their publication years, or `--sort date` for the most recently indexed citations. Do not page
  with `--cursor` to collect more.
- `--sort date` cannot take years (exit 2): Google Scholar ignores the year range when it sorts by
  date. Its snippets start with Google Scholar's indexing age, such as `6 days ago - `; that is
  when Google Scholar indexed the citation, not the publication date.
- An empty list (exit 0) means either nobody cites the paper or Google Scholar does not know the
  ref; it cannot tell the two apart. Check the ref with `fetch` before reporting "no citations".
- A cited-by cursor works only with `cited-by`, and a search cursor only with `search`.

```console
forager platform scholar cited-by scholar:18208131694456651388 --query crash
forager platform scholar cited-by scholar:18208131694456651388 --year-from 2024
forager platform scholar cited-by scholar:18208131694456651388 --sort date
```

## SSRN browser route

`ssrn_browser` reads SSRN in the user's own Chrome through OpenCLI. It is off by default. Enable it
only when the user asks for it; it runs at most one browser command every 5 seconds.

- **Install** (verified with OpenCLI 1.8.6): OpenCLI with its Chrome Browser Bridge connected
  (`opencli doctor`), then copy the `opencli/ssrn` directory next to this skill's `SKILL.md` to
  `~/.opencli/clis/ssrn` (replace the whole directory to update). Run the same copy after updating
  forager, then `forager doctor` to check the adapter. A forager that expects a newer adapter
  contract fails its commands with an install hint until the copy is updated. The required contract
  is `forager-ssrn/3`; copy all files, including `search-state.js`.
- **Enable**: `forager config set platforms.ssrn.order '["ssrn_crossref", "ssrn_browser"]'`. With
  this order, a `--depth abstract` fetch that Crossref cannot serve falls through to the browser.
  Set `providers.ssrn_browser.command` when `opencli` is not on `PATH`.

## Xiaohongshu browser route

`xiaohongshu_browser` searches Xiaohongshu in the user's own Chrome through OpenCLI, using the
account the user is logged in with. It is off by default; enable it only when the user asks for
it. It runs at most one browser command every 10 seconds and never retries. Xiaohongshu's user
agreement forbids scraping and the site rate-limits accounts: tell the user that the searches run
under their account, suggest a secondary account, and keep the number of searches small.

- **Install** (verified with OpenCLI 1.8.6): OpenCLI with its Chrome Browser Bridge connected
  (`opencli doctor`), then copy the `opencli/forager-xhs` directory next to this skill's `SKILL.md`
  to `~/.opencli/clis/forager-xhs` (replace the whole directory to update), and run
  `forager doctor`. The required contract is `forager-xhs/1`.
- **Log in**: the user opens xiaohongshu.com in the Chrome that OpenCLI drives and logs in there.
  Never log in, solve a verification, or switch accounts for them.
- **Enable**: `forager config set platforms.xiaohongshu.order '["xiaohongshu_browser"]'`. Set
  `providers.xiaohongshu_browser.command` when `opencli` is not on `PATH`.
- **Search**: set `--sort`, `--note-type`, and `--publish-time` only when the request implies them.
  A search cannot continue in a later command (`next_cursor` is always `null`); when stderr says
  more results remain, rerun once with a larger `--limit` (at most 100) rather than paging.

## Read full text

A full-text fetch writes the body to a Markdown file and stdout carries only metadata and
`content_url`, `content_provider`, `content_path`, `content_len`.

1. Read the stdout metadata first.
2. List the headings with `grep -n '^#' CONTENT_PATH`, then read only the line ranges you need.
3. For several papers, hand each `content_path` to a subagent that returns only conclusions and
   citations.

Use `--depth abstract` when the abstract answers the request. Use `--format content` only when the
body must enter context or a pipe directly.

For SSRN, `content_url` is the canonical abstract page (the signed download address is never
reported), the download and the conversion share the command deadline, so retry a timed-out
full-text fetch with a larger `--timeout` (for example 300), and pass `--keep-pdf` when the
original PDF must be checked against the Markdown.

## Read a Scholar paper

Take a `link`, `versions[].link`, or `resources[].url` exactly as forager returned it:

- an arxiv.org URL: `forager platform arxiv fetch 'URL'`;
- an SSRN URL: `forager platform ssrn fetch 'URL'`;
- any other URL, or an arXiv or SSRN URL that the platform command rejects with exit 2:
  `forager fetch 'URL'`.

Never build an arXiv ref, SSRN ref, or URL from a Scholar title, byline, or snippet.

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
- **Scholar evidence**: cite the ref (`scholar:18208131694456651388`) and the title. A `snippet` is
  a search excerpt, not the abstract, and supports no claim about the paper's findings. `authors`
  and `source` copy Google Scholar's byline, whose names are often initials and whose author lists
  may be cut short with `…`; never present them as a complete author list. `cited_by` measures
  attention, not quality. The first entry of a fetch's `versions` is not necessarily the published
  version; choose the source from `versions` by its `link` and `source`. `version_count` is the
  count a search reports, not the number of versions a fetch lists (at most 20, often fewer); never
  report one as the other.

- **Xiaohongshu evidence**: cite the ref (`xiaohongshu:<note_id>`) with the title, and give the
  user the `access_url` to open the note; the canonical `url` does not open without the access
  token. A search item is a card, not the note: it supports claims about the title, author, date,
  and counts only. `published` is `null` for a relative time such as `3天前`; quote
  `published_text` instead. Counts are Xiaohongshu's display text (`1.2万`), not exact numbers.
  Results are personalized and change between runs; do not present them as a complete or stable
  ranking.

## Recover

- Exit 2 naming an unsupported option: retry once without that option and tell the user which
  condition you relaxed.
- A `--cursor` failure: drop the cursor and search again from the first page.
- Exit 3, or an `auth` error: go to Diagnose or configure in `SKILL.md`.
- Scholar `quota_exhausted` (exit 4): every configured SerpApi key has used up its searches. Tell
  the user the Scholar quota is exhausted and continue with `forager search` or the arXiv or SSRN
  platform. Do not retry Scholar.
- Scholar `rate_limited` (exit 4): SerpApi's hourly limit; tell the user and retry later, not in
  a loop.
- Scholar `auth` (exit 4): SerpApi rejected the key; ask the user to check
  `providers.serpapi.keys`.
- Scholar exit 3 naming `providers.serpapi.keys`: no SerpApi key is configured. Ask the user to
  run `forager config set providers.serpapi.keys -` and type `["THEIR_KEY"]` on stdin (or rerun
  `forager setup`), so the key stays out of shell history and out of this conversation.
- Exit 5 on full-text fetch (both HTML and PDF too thin): report it; use `--depth abstract` only
  when the abstract still answers the request, and say so.
- Exit 5 on an SSRN `--depth abstract` fetch: no route had the abstract. Fetch at the default
  `metadata` depth and state that the abstract is unavailable.
- Exit 5 on an SSRN `--depth full_text` fetch: the download failed its checks or the converted
  body was too thin. Retry once with a larger `--timeout`; when it still fails, read the kept PDF
  whose path the error message names, or fall back to `--depth abstract` and say so.
- "SSRN paper not found in Crossref": the paper may still exist on SSRN; say that Crossref has no
  record of it rather than that the paper does not exist.
- An `ssrn_browser` `auth` error saying SSRN security verification did not clear: ask the user to
  open SSRN in Chrome and pass the check by hand, then retry. Never try to pass it for them.
- An `ssrn_browser` error that names the forager OpenCLI adapter, or a doctor `message` with
  install steps: repeat the install step in "SSRN browser route".
- A `xiaohongshu_browser` `auth` error: the Chrome session is logged out, or Xiaohongshu wants a
  verification (461). Ask the user to open xiaohongshu.com in Chrome, log in or complete the check
  by hand, then retry once.
- A `xiaohongshu_browser` `parameter` error naming `300031`, `300017`, or a security restriction:
  Xiaohongshu is limiting the account. Stop using Xiaohongshu for this task, tell the user, and do
  not retry; it may clear after a long pause.
- A `xiaohongshu_browser` `timeout`: retry once with `--timeout 180`; when it times out again,
  report it.
- A `xiaohongshu_browser` error that names the forager OpenCLI adapter, or a doctor `message`
  with install steps: repeat the install step in "Xiaohongshu browser route".

Platform retrieval is complete when the requested items or content are read at the depth the answer
needs, or a terminal failure is reported with its recovery.
