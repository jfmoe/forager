# forager CLI reference

This reference documents the public CLI for `forager >=0.5.4`. Load it under the conditions given
in `SKILL.md`: for exact command syntax, non-routine commands, or diagnosis and recovery details.
Routine `search` and `research` stay on their branch references. Treat
`forager <command> --help` as the final authority for argument parsing.

## Contents

- [Shared behavior](#shared-behavior)
- [Smart pipelines](#smart-pipelines)
- [Direct operations](#direct-operations)
- [Provider-direct commands](#provider-direct-commands)
- [Platform commands](#platform-commands)
- [Gemini Deep Research](#gemini-deep-research)
- [Configuration and diagnostics](#configuration-and-diagnostics)
- [Exit codes](#exit-codes)

## Shared behavior

The public top-level commands are `search`, `research`, `fetch`, `map`, `exa`, `context7`,
`platform`, `gemini`, `config`, `setup`, `doctor`, and `smoke`. Use `-h` or
`--help` on any command for parser-generated help. Available aliases are:

| Command | Alias |
| --- | --- |
| `search` | `s` |
| `research` | `rs` |
| `fetch` | `f` |
| `context7` | `c7` |
| `config list` | `config ls` |

Network commands that expose these options share the following behavior:

| Option | Behavior |
| --- | --- |
| `--timeout SECONDS` | Set a positive hard deadline for the whole command, including retries and fallback. |
| `--format FORMAT` | Select stdout rendering. The default is `json`. |
| `--output FILE` | Write the same rendered result to `FILE` and still emit it to stdout. |
| `--receipt` | With `--output`, emit only a one-line `{"output_path","bytes","lines"}` receipt on success; failures still print the full failure payload. Use it to keep long results out of context and read the file in parts. |
| `--verbose` | Include full provider attempts inline. Without it, `search` and `research` retain full attempts in their journal; provider-direct commands do not create a result journal. |

After argument parsing selects `--format json`, `search`, `fetch`, and `research` return a single
parseable error object on stdout for configuration, stdin, or plan preflight failures. Clap parser
errors and panics keep their existing channels, as do failures in non-JSON formats.

`log.level=debug` enables an optional terminal projection with one bounded attempts summary;
`trace` adds safe fields for each attempt. This projection never changes stdout or replaces
`--verbose` and journal attempts.

`content` output is available only for `search`, `research`, `fetch`, `context7 docs`, and
`platform <id> fetch`.
All other commands with `--format` accept only `json` and `markdown`. `smoke` emits JSON and does
not expose `--format`.

## Smart pipelines

### `search`

```console
forager search QUERY [--capabilities CSV|none] [--model ID] [--extra-sources N]
                     [--fallback auto|off] [--timeout SECONDS]
                     [--format json|markdown|content] [--output FILE [--receipt]] [--verbose]
```

| Argument or option | Meaning | Default |
| --- | --- | --- |
| `QUERY` | Search request passed to the main-search pipeline. | Required |
| `--capabilities CSV\|none` | Authoritative supplemental capability set. Use canonical comma-separated IDs, or `none` for main search only. Omit it to use classifier routing, with the configured default Web route when no classifier is available. | Omitted |
| `--model ID` | Override the configured main-search model for this invocation. | Configured model |
| `--extra-sources N` | Set a target in `0..=20`. At `0`, Web Search uses 3 while Documentation Search uses 1; an explicit `1..=20` is the exact target. | `0` |
| `--fallback MODE` | Use `auto` or `off` for provider fallback. | `search.fallback`, normally `auto` |
| `--timeout SECONDS` | Set the whole-pipeline deadline. | `180` |
| `--format FORMAT` | Use `json`, `markdown`, or `content`. | `json` |
| `--output FILE` | Tee the rendered result to a file. | Omitted |
| `--receipt` | Print only a receipt on success; requires `--output`. | Off |
| `--verbose` | Include full provider attempts inline. | Off |

Default search JSON keeps `answer`, Primary Search Sources in `sources`, every non-primary Search
Candidate in `extra_sources`, capability gaps, and the journal reference. Each candidate has
required `provider`, `capability`, and provider-specific `provider_data`, plus nullable `title`,
`url`, and `summary`. Provider-native summaries are not verified evidence. Candidates with an
HTTP(S) URL can enter Web Fetch; a Context7 candidate carries a typed library locator in
`provider_data` for Documentation Search or the Research Evidence Pipeline and has no fabricated
URL. Search-side Web Fetch successes use the actual fetch provider and a 300-character Normalized
Fetch Content preview as their candidate summary. `--verbose` adds provider attempts inline.
Content format emits only the main answer. Markdown labels the two source roles as `Primary
Sources` and `Extra Sources`.

### `research`

```console
forager research QUERY [--plan FILE|-] [--budget quick|standard|deep]
                       [--evidence-dir DIR] [--fallback auto|off]
                       [--timeout SECONDS] [--format json|markdown|content]
                       [--output FILE [--receipt]] [--verbose]
```

| Argument or option | Meaning | Default |
| --- | --- | --- |
| `QUERY` | Research question passed to the research pipeline. | Required |
| `--plan FILE\|-` | Load a strict Schema v1 plan from `FILE`; use `-` to read it from stdin. Omit it to have the configured classifier generate the plan. | Classifier-generated |
| `--budget BUDGET` | Select `quick`, `standard`, or `deep` execution breadth. | `standard` |
| `--evidence-dir DIR` | Store the plan, fetched evidence, and summary under `DIR`. | A unique temporary directory |
| `--fallback MODE` | Use `auto` or `off` for provider fallback. | `auto` |
| `--timeout SECONDS` | Set the whole research deadline. | `600` |
| `--format FORMAT` | Use `json`, `markdown`, or `content`. | `json` |
| `--output FILE` | Tee the rendered result to a file. | Omitted |
| `--receipt` | Print only a receipt on success; requires `--output`. | Off |
| `--verbose` | Include full provider attempts inline. | Off |

Pipe stdin all the way into the command when using `--plan -`:

```console
printf '%s' "$PLAN_JSON" | forager research "QUERY" --plan - --budget standard --format json
```

Use `research-plan.json` as the Schema v1 shape. A caller-provided plan is authoritative and skips
plan generation. An omitted plan requires a configured classifier. Invalid JSON, unsupported
versions, unknown or missing fields, invalid capabilities, empty decomposition, and empty or
duplicate subquestion IDs exit with code 2.

Default JSON is a Research Evidence Index. Each item in `evidence_items` contains evidence identity
and metadata plus a readable `evidence_items[].path`; fetched body content lives at that path rather
than in stdout. The top level contains `evidence_dir`, `plan_path`, `unconsumed_candidates` as a
count and path, `gap_check`, `capability_gaps`, `synthesis_policy: "fetch_before_claim"`, and the
`journal_ref`/`journal_status` pair. Markdown and content render the same index and unresolved gaps.
On success, the Evidence Index locates these artifacts. On terminal failure, a non-null
`summary_path` points to the readable Research Recovery Manifest containing completed evidence,
gaps, and artifact paths. If it is null, use the other reported paths and gaps before diagnosis.
`--verbose` adds provider attempts inline.

## Direct operations

### `fetch`

```console
forager fetch URL [--timeout SECONDS] [--format json|markdown|content]
                  [--output FILE [--receipt]] [--verbose]
```

| Argument or option | Meaning | Default |
| --- | --- | --- |
| `URL` | Known page or document URL to retrieve through the configured `web_fetch` chain. | Required |
| `--timeout SECONDS` | Set the deadline for the complete fallback chain. | `180` |
| `--format FORMAT` | Use `json`, `markdown`, or `content`. | `json` |
| `--output FILE` | Tee the rendered result to a file. | Omitted |
| `--receipt` | Print only a receipt on success; requires `--output`. | Off |
| `--verbose` | Include full provider attempts inline. | Off |

The default `web_fetch` order is Firecrawl, Jina, then Tavily. Jina needs no key: without one, or
after its keys fail on auth, quota, or rate limit, it sends one request without a key, and that
attempt has `anonymous: true`.

### `map`

```console
forager map URL [--instructions TEXT] [--max-depth N] [--max-breadth N]
                [--limit N] [--timeout SECONDS] [--format json|markdown]
                [--output FILE [--receipt]] [--verbose]
```

| Argument or option | Meaning | Default |
| --- | --- | --- |
| `URL` | Site root or page from which mapping starts. | Required |
| `--instructions TEXT` | Tell the mapper which pages or structure to prioritize. | Empty |
| `--max-depth N` | Set traversal depth in `1..=5`. | `1` |
| `--max-breadth N` | Set per-level breadth in `1..=500`. | `20` |
| `--limit N` | Set a positive total result limit. | `50` |
| `--timeout SECONDS` | Set the whole-command deadline in `10..=150`. | `150` |
| `--format FORMAT` | Use `json` or `markdown`. | `json` |
| `--output FILE` | Tee the rendered result to a file. | Omitted |
| `--receipt` | Print only a receipt on success; requires `--output`. | Off |
| `--verbose` | Include full provider attempts inline. | Off |

## Provider-direct commands

Provider-direct commands bypass capability routing and automatic cross-provider fallback. If their
optional `--timeout` is omitted, they use the corresponding `providers.<name>.timeout`
configuration value, which defaults to 30 seconds.

### `exa search`

```console
forager exa search QUERY [--num-results N] [--search-type neural|keyword|auto]
                         [--include-text [--text-max-characters N]]
                         [--include-highlights] [--start-published-date DATE]
                         [--include-domains CSV] [--exclude-domains CSV]
                         [--category NAME] [--timeout SECONDS]
                         [--format json|markdown] [--output FILE [--receipt]] [--verbose]
```

| Argument or option | Meaning | Default |
| --- | --- | --- |
| `QUERY` | Exa search query. | Required |
| `--num-results N` | Request between 1 and 100 results. | `5` |
| `--search-type TYPE` | Use `neural`, `keyword`, or `auto` search. | `auto` |
| `--include-text` | Include page text for each result. | Off |
| `--text-max-characters N` | Cap each result's page text at `N` characters. Requires `--include-text`. | `3000` |
| `--include-highlights` | Include result highlights. | Off |
| `--start-published-date DATE` | Restrict results to the given lower publication-date bound. | Omitted |
| `--include-domains CSV` | Include only the comma-separated domains. | Empty |
| `--exclude-domains CSV` | Exclude the comma-separated domains. | Empty |
| `--category NAME` | Restrict results to an Exa category. | Omitted |
| `--timeout SECONDS` | Override `providers.exa.timeout`. | Configured value |
| `--format FORMAT` | Use `json` or `markdown`. | `json` |
| `--output FILE` | Tee the rendered result to a file. | Omitted |
| `--receipt` | Print only a receipt on success; requires `--output`. | Off |
| `--verbose` | Include full provider attempts inline. | Off |

### `exa similar`

```console
forager exa similar URL [--num-results N] [--timeout SECONDS]
                        [--format json|markdown] [--output FILE [--receipt]] [--verbose]
```

| Argument or option | Meaning | Default |
| --- | --- | --- |
| `URL` | URL for which Exa should find similar pages. | Required |
| `--num-results N` | Request between 1 and 100 results. | `5` |
| `--timeout SECONDS` | Override `providers.exa.timeout`. | Configured value |
| `--format FORMAT` | Use `json` or `markdown`. | `json` |
| `--output FILE` | Tee the rendered result to a file. | Omitted |
| `--receipt` | Print only a receipt on success; requires `--output`. | Off |
| `--verbose` | Include full provider attempts inline. | Off |

### `context7 library`

```console
forager context7 library NAME [QUERY] [--timeout SECONDS]
                                     [--format json|markdown]
                                     [--output FILE [--receipt]] [--verbose]
```

| Argument or option | Meaning | Default |
| --- | --- | --- |
| `NAME` | Library or package name to resolve. | Required |
| `QUERY` | Optional context used to rank matching libraries. | Empty |
| `--timeout SECONDS` | Override `providers.context7.timeout`. | Configured value |
| `--format FORMAT` | Use `json` or `markdown`. | `json` |
| `--output FILE` | Tee the rendered result to a file. | Omitted |
| `--receipt` | Print only a receipt on success; requires `--output`. | Off |
| `--verbose` | Include full provider attempts inline. | Off |

### `context7 docs`

```console
forager context7 docs LIBRARY_ID QUERY [--timeout SECONDS]
                                       [--format json|markdown|content]
                                       [--output FILE [--receipt]] [--verbose]
```

| Argument or option | Meaning | Default |
| --- | --- | --- |
| `LIBRARY_ID` | Context7-compatible library ID, normally obtained from `context7 library`. | Required |
| `QUERY` | Documentation question or topic. | Required |
| `--timeout SECONDS` | Override `providers.context7.timeout`. | Configured value |
| `--format FORMAT` | Use `json`, `markdown`, or `content`. | `json` |
| `--output FILE` | Tee the rendered result to a file. | Omitted |
| `--receipt` | Print only a receipt on success; requires `--output`. | Off |
| `--verbose` | Include full provider attempts inline. | Off |

## Platform commands

`forager platform <id> <op>` retrieves items of a built-in platform through the routes in
`platforms.<id>.order`; results never fall back to another platform. The platforms are listed in
[`platform-vocabulary.json`](platform-vocabulary.json). Platform commands write no result journal.
Their `--timeout` defaults to `120` (`600` for YouTube ask and transcribe) and includes waits for the platform's request window, shared
across all local processes (arXiv: one request every 3 seconds; SSRN through Crossref: one request
per second; SSRN through the browser: one OpenCLI command every 5 seconds; Xiaohongshu: one OpenCLI
command every 10 seconds; Hacker News: one request to either of its two endpoints every half
second; YouTube through the Data API: one request per second; YouTube through `youtube_web`: one request every 4 seconds; Google Scholar through SerpApi, X through xAI, and YouTube ask and transcribe through Gemini have no local window).

### `platform arxiv search`

```console
forager platform arxiv search [QUERY] [--category CODE]... [--author NAME] [--title TEXT]
                              [--submitted-from YYYY-MM-DD] [--submitted-to YYYY-MM-DD]
                              [--sort relevance|submitted|updated] [--limit N] [--cursor CURSOR]
                              [--timeout SECONDS] [--format json|markdown]
                              [--output FILE [--receipt]] [--verbose]
```

| Argument or option | Meaning | Default |
| --- | --- | --- |
| `QUERY` | Keywords and double-quoted phrases that must all match; arXiv query syntax in them is literal text. An unmatched double quote exits 2. | Omitted |
| `--category CODE` | arXiv category such as `q-fin.PM`; repeat to match any of several. | None |
| `--author NAME` / `--title TEXT` | Author-name or title phrase. | None |
| `--submitted-from` / `--submitted-to` | Inclusive UTC submission-date range; either end may be omitted. | None |
| `--sort ORDER` | `relevance`, `submitted`, or `updated`; always newest or best first. | `relevance` |
| `--limit N` | Results on this page, `1..=100`. | `10` |
| `--cursor CURSOR` | `next_cursor` of a previous page; it restores the whole request, so pass no query, option, or `--limit` with it. | Omitted |

Give a query or at least one of `--category`, `--author`, `--title`. JSON output is
`{platform, provider, items, next_cursor}`; each item has `ref`, `url`, `depth: "abstract"`,
`title`, `authors`, `published`, the full `abstract`, and arXiv metadata. `next_cursor` is `null`
on the last page; a valid empty search returns `items: []` with exit 0.

### `platform arxiv fetch`

```console
forager platform arxiv fetch REF_OR_URL [--depth full_text|abstract] [--content-dir DIR]
                             [--timeout SECONDS] [--format json|markdown|content]
                             [--output FILE [--receipt]] [--verbose]
```

| Argument or option | Meaning | Default |
| --- | --- | --- |
| `REF_OR_URL` | `arxiv:<id>[v<n>]`, or an arxiv.org abs, pdf, or html URL. Without a version, arXiv's current version is fetched and reported. | Required |
| `--depth DEPTH` | `full_text` reads the body; `abstract` returns only metadata and the abstract and needs no Web Fetch provider. | `full_text` |
| `--content-dir DIR` | Directory for the full-text Markdown file. | New directory under the system temp directory |
| `--format FORMAT` | `json`, `markdown`, or `content`; `content` prints the body (or the abstract) and writes no file. | `json` |

Full text comes from the version's official HTML, or its PDF when the version has no HTML or the
HTML read fails, through the configured `web_fetch` chain. JSON output carries the metadata and
`content_url`, `content_provider`, `content_path`, and `content_len`; the body itself stays in the
file. Short links and unrecognized inputs exit `2` before any request; a missing paper is a
`parameter` failure (exit `4`); both HTML and PDF too thin is a `quality` failure (exit `5`);
`full_text` with no configured Web Fetch provider exits `3`.

### `platform ssrn search`

```console
forager platform ssrn search QUERY [--limit N] [--cursor CURSOR]
                             [--timeout SECONDS] [--format json|markdown]
                             [--output FILE [--receipt]] [--verbose]
```

| Argument or option | Meaning | Default |
| --- | --- | --- |
| `QUERY` | Query expression; matching follows the selected route and mode. Required unless `--cursor` is given. | Required |
| `--scope all\|title\|bibliographic\|full-text` | Route default fields, title, Crossref bibliographic fields, or browser full-text fields. | `all` |
| `--author TEXT` | Author text query; browser uses the native Author(s) field, not an identity selector. | Omitted |
| `--affiliation TEXT` | Crossref affiliation query. | Omitted |
| `--mode fuzzy\|boolean` | Browser only. Boolean supports AND, OR, NOT, and parentheses. | Native Fuzzy when omitted |
| `--date PRESET` | Browser only: all-time, last-week, last-month, last-3-months, last-6-months, last-year, last-2-years, last-3-years. | Native all-time when omitted |
| `--published-from DATE`, `--published-to DATE` | Inclusive publication date bounds, `YYYY-MM-DD`. Either bound can be omitted. | Omitted |
| `--created-from DATE`, `--created-to DATE` | Inclusive Crossref first-registration date bounds. | Omitted |
| `--updated-from DATE`, `--updated-to DATE` | Inclusive Crossref metadata deposit/update date bounds. | Omitted |
| `--has-abstract` | Require an upstream abstract field; does not fetch or guarantee usable abstract text. | Off |
| `--type TYPE` | Exact registered Crossref work type; see help for accepted values. | Omitted |
| `--orcid ID` | Exact contributor ORCID, as a bare ID with a valid check digit. | Omitted |
| `--funder DOI` | Exact Open Funder Registry DOI, `10.13039/<digits>`. | Omitted |
| `--sort METRIC` | Crossref: relevance, published, created, updated, citations. Browser: relevance, posted, downloads, title. | `relevance` |
| `--order asc\|desc` | Ranking direction. Browser relevance only accepts desc; title asc/desc means A–Z/Z–A. | `desc` |
| `--limit N` | Results on this page, `1..=100`. | `10` |
| `--cursor CURSOR` | `next_cursor` of a previous page; it restores the whole request, so pass no query, search criteria, or `--limit` with it. | Omitted |

The default `ssrn_crossref` route searches Crossref records under the SSRN DOI prefix; it needs no
credentials. Each item has `ref`, `url` (the SSRN abstract page), `depth`, `title`, `authors`,
`published`, and the SSRN fields `abstract`, `snippet`, `doi`, `crossref_type`,
`crossref_created`, `posted`, `last_revised`, and `date_written`; a field the route did not read is
`null`. `depth` is `abstract` when the item has an abstract and `metadata` otherwise. Paging stops
at result 10000, where Crossref stops offset paging; `next_cursor` is then `null`. Records whose DOI
is not an SSRN DOI are dropped and reported on stderr. A valid empty search returns `items: []`
with exit 0.

When `platforms.ssrn.order` includes the opt-in `ssrn_browser` route, that route sends SSRN's own
search request in Chrome through OpenCLI (see [`platforms.md`](platforms.md) to install and enable
it). Its items have `depth: snippet` (`metadata` for a result without an excerpt), `snippet`,
`posted` as the UTC date in SSRN's page format (`07 Sep 2019`), `published` as the same date in ISO
form, and `last_revised` when SSRN reports a revision. A page never spans two 50-result SSRN pages,
so it can hold fewer items than `--limit`; SSRN lists at most 10,000 results, and the last cursor
stops there. SSRN does not keep the order of results stable between requests, so deduplicate items
by `ref` across pages. An SSRN security check that blocked the request is an `auth` failure: ask the
user to open SSRN in Chrome and pass the check, then retry once; never try to pass it yourself. An
unrecognized answer is a `runtime` failure, never an empty result.

Both routes support title and author queries with their own matching rules. Browser-only options
require an enabled browser route; explicit mode/date values retain native semantics even at their
defaults. Crossref-only filters and date ranges are rejected by the browser. Routes that cannot
apply every criterion are skipped; when none support the request, the command exits 2 before IO.
Affiliation, ORCID, and funder searches depend on deposited metadata. Crossref publication,
registration, and update dates are distinct from SSRN Posted dates. Cursors restore all criteria.
Full-text search changes the matching scope only; it does not fetch papers or download PDFs.

### `platform ssrn fetch`

```console
forager platform ssrn fetch REF_OR_URL [--depth metadata|abstract|full_text]
                            [--content-dir DIR] [--keep-pdf]
                            [--timeout SECONDS] [--format json|markdown|content]
                            [--output FILE [--receipt]] [--verbose]
```

| Argument or option | Meaning | Default |
| --- | --- | --- |
| `REF_OR_URL` | `ssrn:<id>`, a `papers.ssrn.com/sol3/papers.cfm?abstract_id=<id>` URL, an `ssrn.com/abstract=<id>` URL, or the `10.2139/ssrn.<id>` DOI or its doi.org URL. | Required |
| `--depth DEPTH` | `metadata` returns the metadata and the abstract when available; `abstract` requires the abstract; `full_text` downloads the paper and delivers it as Markdown (needs the `ssrn_browser` route). | `metadata` |
| `--content-dir DIR` | Directory for the full-text Markdown file. | New directory under the system temp directory |
| `--keep-pdf` | Move the downloaded PDF next to the Markdown and report `pdf_path` and `pdf_bytes`. | Off |
| `--format FORMAT` | `json`, `markdown`, or `content`; `content` prints the abstract or the full text and writes no file. | `json` |

SSRN download links, look-alike hosts, and short links exit `2` before any request, and so does
`--depth full_text` when no enabled route can download. A DOI that Crossref does not know is a
`parameter` failure (exit `4`) whose message says the paper was not found in Crossref; `--depth
abstract` without an abstract is a `quality` failure (exit `5`). With the order
`["ssrn_crossref", "ssrn_browser"]`, a `--depth abstract`
fetch whose Crossref record has no abstract falls through to the browser, which reads the abstract page and also fills
`posted`, `last_revised`, and `date_written`. The browser reports a paper that SSRN shows as under
review or removed as a `parameter` failure.

A `full_text` fetch makes the browser route download the PDF, verifies it, and converts it
through the configured `web_fetch` chain. JSON output carries the metadata and `content_url` (the
canonical abstract page, never the expiring signed download address), `content_provider`,
`content_path` (`ssrn-<id>.md`), and `content_len`; the PDF is removed after a successful
delivery. A failed download check (incomplete download, missing file, not a PDF, wrong page) is a
`quality` failure (exit `5`); when the conversion itself fails, the PDF is kept and the error
message names its path. `full_text` with no configured Web Fetch provider exits `3`. Give the
download and the conversion room: pass a larger `--timeout` than the default 120 seconds when a
full-text fetch times out.

### `platform scholar search`

```console
forager platform scholar search QUERY [--limit N] [--year-from YYYY] [--year-to YYYY]
                                [--review-only] [--cursor CURSOR]
                                [--timeout SECONDS] [--format json|markdown]
                                [--output FILE [--receipt]] [--verbose]
```

| Argument or option | Meaning | Default |
| --- | --- | --- |
| `QUERY` | Google Scholar query, sent unchanged; Google Scholar's own operators apply. Required unless `--cursor` is given. | Required |
| `--limit N` | Results on this page, `1..=20`; every page costs one SerpApi search whatever its size. | `20` |
| `--year-from` / `--year-to` | Inclusive publication-year range, `1000..=9999`; either end may be omitted. | None |
| `--review-only` | Only review articles. | Off |
| `--cursor CURSOR` | `next_cursor` of a previous page; it restores the whole request, so pass no query, option, or `--limit` with it. Every page costs one search. | Omitted |

Google Scholar is reached only through the `serpapi` route, which needs a SerpApi key in
`providers.serpapi.keys`; without one the command exits `3` before any request and names that
key. Every page costs one search of the key's quota, including an empty page; an identical
request repeated within one hour is served from SerpApi's cache and costs none.

JSON output is `{platform, provider, items, next_cursor}`. Each item has:

| Field | Meaning |
| --- | --- |
| `ref`, `url` | `scholar:<cluster_id>` and the cluster's Google Scholar page. |
| `depth` | `snippet`, or `metadata` when the result has no excerpt. |
| `title`, `authors`, `published` | Title, the byline's author names (often initials, possibly cut short), and the year. |
| `snippet` | Google Scholar's excerpt; never the abstract. |
| `link` | The page the result title links to, or `null`. |
| `source` | The byline exactly as Google Scholar shows it. |
| `cited_by`, `version_count` | Google Scholar's citation and version counts, or `null`. |
| `resources` | PDF or HTML copies, each `{title, file_format, url}`. |
| `result_type` | Google Scholar's result type such as `Pdf` or `Html`, or `null`. |

Results without a verifiable cluster ID are skipped and reported on stderr; a page whose every
result was skipped is a `runtime` failure. A valid empty search returns `items: []` with exit 0.
`next_cursor` is `null` on the last page and before any page that would pass result 1000, the
most Google Scholar serves.

Exit codes: `2` before any request for a blank query, `--limit` outside `1..=20`, a year out of
range or `--year-from` after `--year-to`, a cursor combined with a query, option, or `--limit`, a
tampered cursor, or a cursor whose route left the order or lost its keys; `3` for no key or an
empty `platforms.scholar.order`; `4` for `auth` (SerpApi rejected the key), `quota_exhausted`
(every key has used its monthly searches), `rate_limited` (the hourly limit), `network`, `timeout`,
and `runtime` failures.

### `platform scholar fetch`

```console
forager platform scholar fetch REF_OR_URL [--depth metadata]
                               [--timeout SECONDS] [--format json|markdown]
                               [--output FILE [--receipt]] [--verbose]
```

| Argument or option | Meaning | Default |
| --- | --- | --- |
| `REF_OR_URL` | `scholar:<cluster_id>`, or a `scholar.google.com/scholar?cluster=<id>` URL. | Required |
| `--depth DEPTH` | Only `metadata`; any other depth exits `2` before any request. | `metadata` |

Fetch costs one search, including a fetch of a cluster that does not exist. JSON output has
`platform`, `provider`, `ref` and `url` (the requested cluster), `depth: "metadata"`, `title`,
`authors`, and `published` taken from the first version, and `versions`: the versions Google
Scholar groups under the cluster, at most 20, in its order, each `{title, link, source,
resources}`. The first version is not marked canonical and is often not the published version.
The number of versions differs from a search item's `version_count`. When the cluster has more
versions, stderr says so and names the cluster's Google Scholar page.

Exit codes: `2` before any request for citation lists, author profiles, other Scholar URLs, a
repeated or overflowing `cluster`, or a depth other than `metadata`; `3` for no key or an empty
order; `4` for a cluster that Google Scholar does not know (`parameter`, message
`Google Scholar has no cluster scholar:<id>`) and for the failures listed under search.

### `platform scholar cited-by`

```console
forager platform scholar cited-by REF_OR_URL [--query TEXT] [--limit N]
                                  [--year-from YYYY] [--year-to YYYY]
                                  [--sort relevance|date] [--cursor CURSOR]
                                  [--timeout SECONDS] [--format json|markdown]
                                  [--output FILE [--receipt]] [--verbose]
```

| Argument or option | Meaning | Default |
| --- | --- | --- |
| `REF_OR_URL` | The cited paper: `scholar:<cluster_id>`, or a `scholar.google.com/scholar?cluster=<id>` URL. Required unless `--cursor` is given. | Required |
| `--query TEXT` | Only citing papers that match this Google Scholar query; its operators apply. | None |
| `--limit N` | Results on this page, `1..=20`; every page costs one SerpApi search whatever its size. | `20` |
| `--year-from` / `--year-to` | Inclusive publication-year range of the citing papers, `1000..=9999`. | None |
| `--sort relevance\|date` | `date` lists the most recently indexed citing papers first; it cannot take years. | `relevance` |
| `--cursor CURSOR` | `next_cursor` of a previous cited-by page; pass no ref, query, option, or `--limit` with it. | Omitted |

JSON output and items are the same as `platform scholar search`, as are paging, the key, and the
cost of each page. With `--sort date`, each `snippet` starts with Google Scholar's indexing age,
such as `6 days ago - `. A paper nobody cites and a ref Google Scholar does not know both return
`items: []` with exit 0, and both cost one search.

Exit codes: `2` before any request for an unrecognized ref or URL, a blank `--query`, `--limit`
outside `1..=20`, a year out of range or `--year-from` after `--year-to`, `--sort date` with a year,
a search cursor, a tampered cursor, or a cursor combined with a ref, query, option, or `--limit`;
`3` and `4` as under search.

### `platform xiaohongshu search`

```console
forager platform xiaohongshu search QUERY [--limit N]
                                    [--sort comprehensive|latest|most-liked|most-commented|most-collected]
                                    [--note-type all|image|video] [--publish-time any|day|week|half-year]
                                    [--timeout SECONDS] [--format json|markdown]
                                    [--output FILE [--receipt]] [--verbose]
```

| Argument or option | Meaning | Default |
| --- | --- | --- |
| `QUERY` | Search words, typed into Xiaohongshu unchanged; no phrase or operator syntax is promised. | Required |
| `--limit N` | Results, `1..=100`; one command reads up to five pages of 20. | `20` |
| `--sort ORDER` | `comprehensive`, `latest`, `most-liked`, `most-commented`, or `most-collected`. | `comprehensive` |
| `--note-type TYPE` | `all`, `image`, or `video`. | `all` |
| `--publish-time PERIOD` | `any`, `day`, `week`, or `half-year`. | `any` |

Xiaohongshu is reached only through the opt-in `xiaohongshu_browser` route, which searches in the
user's own logged-in Chrome through OpenCLI; see "Xiaohongshu browser route" in
[`platforms.md`](platforms.md). With the default empty `platforms.xiaohongshu.order` the command
exits `3` before starting any browser and names the steps to enable it. A search takes about 30 to
60 seconds.

JSON output is `{platform, provider, items, next_cursor}`. Each item has:

| Field | Meaning |
| --- | --- |
| `ref`, `url` | `xiaohongshu:<note_id>` and the note's canonical page, which does not open without an access token. |
| `depth` | Always `metadata`: a result card has no body text. |
| `title`, `authors` | The card title (may be `""`) and the author's nickname. |
| `published` | `YYYY-MM-DD`. A relative time such as `3天前` or `昨天 21:38` is counted back from this machine's clock, in its local timezone; `null` for any other form. |
| `published_text` | The card's time text exactly as shown. |
| `note_type`, `author_id` | `image` or `video`, and the author's user ID. |
| `likes`, `collects`, `comments`, `shares` | Counts as Xiaohongshu shows them, possibly abbreviated such as `1.2万`. |
| `access_url` | The note URL with its access token; the link that opens the note. |

`next_cursor` is always `null`: Xiaohongshu ranks differently on every visit, so a search cannot
continue in a later command. When more results remain, stderr says so; raise `--limit` instead.
Notes without a valid ID or access token are skipped and reported on stderr. A valid empty search
returns `items: []` with exit 0.

Exit codes: `2` before any request for a blank query or `--limit` outside `1..=100`; `3` for an
empty `platforms.xiaohongshu.order`; `4` for `auth` (the Chrome session is logged out, or
Xiaohongshu answered 461 and wants a verification), `parameter` (a security restriction such as
`300031` or `300017`), `timeout` (the results did not arrive before the deadline), and `runtime`
(the page or the responses did not match the request, or the adapter is missing or outdated).

### `platform xiaohongshu fetch`

```console
forager platform xiaohongshu fetch ACCESS_URL [--depth metadata|full_text]
                                   [--content-dir DIR] [--timeout SECONDS]
                                   [--format json|markdown|content]
                                   [--output FILE [--receipt]] [--verbose]
```

| Argument or option | Meaning | Default |
| --- | --- | --- |
| `ACCESS_URL` | A search item's `access_url`, or a full xiaohongshu.com note URL with its `xsec_token` copied from the browser. A ref or a URL without the token exits `2`. | Required |
| `--depth DEPTH` | `full_text` writes the note as a Markdown file; `metadata` returns only the note fields. | `full_text` |
| `--content-dir DIR` | Directory for the Markdown file. | A new directory under the system temporary directory |
| `--format content` | Print the note text to stdout and write no file. | `json` |

The note opens in the user's own logged-in Chrome through `xiaohongshu_browser`, like search; a
fetch takes about 20 seconds. The full text needs no Web Fetch provider: the route reads it from
the note page itself.

JSON output has `platform`, `provider`, `ref`, `url` (the canonical page, without the token),
`depth`, `title` (may be `""`), `authors` (the nickname), and `published` (exact Beijing time,
such as `2025-10-08T13:06:40+08:00`), plus:

| Field | Meaning |
| --- | --- |
| `updated` | When the note was last edited, in the same format. |
| `note_type`, `author_id` | `image` or `video`, and the author's user ID. |
| `likes`, `collects`, `comments`, `shares` | Counts as Xiaohongshu shows them. |
| `tags` | The note's topic names. |
| `images` | `[{url, width, height}]`; the URLs are Xiaohongshu CDN images. |
| `video` | `{duration_seconds, width, height}` for a video note, else `null`; the video itself is never downloaded or linked. |
| `ip_location` | The region Xiaohongshu shows for the author, or `null`. |
| `access_url` | The note URL with its access token, rebuilt from the ref and token. |

At `full_text`, the output adds `content_url` (the canonical page), `content_provider`
(`xiaohongshu_browser`), `content_path` (`xiaohongshu-<note_id>.md`), and `content_len`. The file
holds the title as a heading, the text exactly as shown (topics stay as `#话题[话题]#`), a `标签：`
line, one `![](url)` line per image, and, for a video note, a line giving its length.

Exit codes: `2` before any request for a missing or malformed token, a short link, a
`rednote.com` link, or another depth; `3` for an empty `platforms.xiaohongshu.order`; `4` for
`auth` and `timeout` as under search, `parameter` for a note Xiaohongshu would not open
(`300031` or `300017`: the token may be stale, the note restricted or removed, or the account
rate-limited), and `runtime` (the page showed another note or an unexpected page, or the file
could not be written); `5` for `quality` (the note has no title, text, or images).

### `platform xiaohongshu comments`

```console
forager platform xiaohongshu comments ACCESS_URL [--limit N] [--replies N]
                                      [--timeout SECONDS] [--format json|markdown]
                                      [--output FILE [--receipt]] [--verbose]
```

| Argument or option | Meaning | Default |
| --- | --- | --- |
| `ACCESS_URL` | As for fetch: a search item's `access_url`, or a note URL with its `xsec_token`. A ref or a URL without the token exits `2`. | Required |
| `--limit N` | Top-level comments, `1..=50`; one command reads up to five pages of 10. | `20` |
| `--replies N` | Expand the replies of the first `N` returned comments that have more replies than they show, `0..=10`; reads the first page (about 5 replies) of each. | `0` |

The comments are read on the note page in the user's own logged-in Chrome through
`xiaohongshu_browser`, like fetch; a read takes about 20 to 30 seconds. There is no cursor: a
later command cannot continue where this one stopped.

JSON output is `{platform, provider, note, comments, has_more}`. `note` is the note ref;
`has_more` is `true` when the note has top-level comments this result did not return. Each
comment has:

| Field | Meaning |
| --- | --- |
| `id` | The comment ID. |
| `author`, `author_id` | The commenter's nickname and user ID. |
| `text` | The comment text exactly as stored; emoji codes such as `[doge]` stay as written. |
| `likes`, `reply_count` | Counts as Xiaohongshu shows them. |
| `published` | Exact Beijing time, such as `2026-10-06T18:30:18+08:00`. |
| `ip_location` | The region Xiaohongshu shows for the commenter, or `null`. |
| `replies` | The one reply a comment shows by itself, plus the first page of an expanded comment's replies. Each reply has the same fields without `reply_count` and `replies`, plus `reply_to`, the ID of the comment or reply it answers. |
| `replies_has_more` | Whether the comment has replies this result did not return. |

A note without comments returns `comments: []` with exit 0. The output never contains the
access token.

Exit codes: `2` before any request for a missing or malformed token, a short link, a
`rednote.com` link, or `--limit` or `--replies` out of range; `3` for an empty
`platforms.xiaohongshu.order`; `4` for `auth`, `parameter`, and `timeout` as under fetch, and
`runtime` (a comment response belonged to another note, broke the page order, or answered a
comment that was not expanded; the adapter could not click "展开 N 条回复", with the reason; or the
page was unexpected).

### `platform x search`

```console
forager platform x search QUERY [--handle HANDLE]... [--exclude-handle HANDLE]...
                                [--from YYYY-MM-DD] [--to YYYY-MM-DD] [--limit N]
                                [--timeout SECONDS] [--format json|markdown]
                                [--output FILE [--receipt]] [--verbose]
```

| Argument or option | Meaning | Default |
| --- | --- | --- |
| `QUERY` | What to find on X, in natural language or X search syntax; the model chooses how to search. | Required |
| `--handle HANDLE` | Only posts by this account, with or without `@`; repeat for up to 20. | None |
| `--exclude-handle HANDLE` | No posts by this account; repeat for up to 20. Cannot be combined with `--handle`. | None |
| `--from` / `--to` | Inclusive range of UTC publication days. | None |
| `--limit N` | Maximum posts, `1..=20`. | `10` |

X runs through the `xai` route: the xAI Responses API with only the `x_search` tool, using
`providers.xai.url`, `.keys`, and `.model` of main search; `providers.xai.tools` does not apply.
xAI enforces the handle and date filters on its server; the model chooses the search order. A
search takes about 10 to 30 seconds and reads as many posts as the model decides.

JSON output is `{platform, provider, items, next_cursor}`; `next_cursor` is always `null`, because
X search cannot continue a page: rewrite the query or raise `--limit` for more. Each item has
`ref` (`x:<post_id>`), `url` (`https://x.com/i/status/<post_id>`), `depth: snippet`, `title:
null`, `authors` (the handle), `published` (UTC second from the post ID, `null` for posts from
before November 2010), `author_handle`, and `text`. The text and handle are relayed by the model
that read the post and may differ from the original. Posts the response does not cite as read,
and posts dated outside `--from`/`--to`, are dropped with one stderr diagnostic. An empty list
(exit 0) means the model found no matching post.

Exit codes: `2` before any request for a blank query, `--handle` with `--exclude-handle`, more than
20 handles, a malformed handle, an invalid date, `--from` after `--to`, or `--limit` out of range;
`3` for no `providers.xai.keys` or an empty `platforms.x.order`; `4` for xAI failures as in main
search (`auth`, `rate_limited` and `quota_exhausted` after every key, `parameter`, `network`,
`timeout`) and `runtime` when the posts are not the requested JSON or none could be verified.

### `platform x fetch`

```console
forager platform x fetch REF_OR_URL [--depth thread|metadata] [--timeout SECONDS]
                                    [--format json|markdown|content]
                                    [--output FILE [--receipt]] [--verbose]
```

| Argument or option | Meaning | Default |
| --- | --- | --- |
| `REF_OR_URL` | `x:<post_id>`, or an x.com or twitter.com post URL (`/<handle>/status/<id>`, `/i/status/<id>`, `/i/web/status/<id>`, with or without `/photo/N`, a query, or a fragment). | Required |
| `--depth thread\|metadata` | `thread` adds the conversation; `metadata` returns the post alone. | `thread` |

JSON output is the post's item fields (as under search, with `depth` set to the requested depth)
plus, at `thread` depth, `thread`: every post in the model's conversation order, each `{ref, url,
author_handle, published, text, role}` with `role` one of `parent`, `root`, `reply`, `quoted`. The
root must be the requested post and cited as read; replies cannot be checked against the
response. `--format content` prints the post text, or the thread as Markdown. Nothing is written
to a file.

Exit codes: `2` before any request for a `t.co` short link (open it in a browser and pass the
x.com URL), another URL, or a depth other than `thread` and `metadata`; `3` as under search; `4`
for a post the model cannot see (`parameter`, `X post x:<id> was not found or is not visible`), a
root that is not the requested post or not cited (`runtime`), and the xAI failures under search.

### `platform x ask`

```console
forager platform x ask QUERY [--handle HANDLE]... [--exclude-handle HANDLE]...
                             [--from YYYY-MM-DD] [--to YYYY-MM-DD] [--timeout SECONDS]
                             [--format json|markdown] [--output FILE [--receipt]] [--verbose]
```

`QUERY` is the question; the filters are the same as under search. The model searches X only
and writes an answer with inline citations such as `[[1]](https://x.com/SpaceX/status/<id>)`; the
whole answer is relayed content.

JSON output is `{platform, provider, query, answer, sources}`. Each source is `{ref, url, title}`:
a cited post has its `x:<post_id>` ref and canonical URL, which `platform x fetch` accepts; a
profile or other page has `ref: null` and its own URL. `sources` may be empty; treat an answer
without sources as unverified. Ask writes no result journal.

Exit codes: `2` before any request for a blank question or invalid filters as under search; `3`
as under search; `4` for the xAI failures under search.

### `platform hn search`

```console
forager platform hn search [QUERY] [--sort relevance|date] [--type TYPE]... [--author USER]
                           [--story ID] [--from YYYY-MM-DD] [--to YYYY-MM-DD]
                           [--min-points N] [--min-comments N] [--limit N] [--cursor CURSOR]
                           [--timeout SECONDS] [--format json|markdown]
                           [--output FILE [--receipt]] [--verbose]
```

| Argument or option | Meaning | Default |
| --- | --- | --- |
| `QUERY` | Words to match; `"double quotes"` match a phrase and `-word` excludes a word. Required without `--cursor`. | Omitted |
| `--sort ORDER` | `relevance`, or `date` for newest first. | `relevance` |
| `--type TYPE` | `story`, `comment`, `ask_hn`, `show_hn`, `launch_hn`, `job`, or `poll`; repeat to match any of several. | Any |
| `--author USER` | Only items by this user (2 to 15 letters, digits, `_`, or `-`). | None |
| `--story ID` | Only items in the discussion of this story ID, such as `8863`. | None |
| `--from` / `--to` | Inclusive range of UTC publication days; either end may be omitted. | None |
| `--min-points N` / `--min-comments N` | Minimum points or comment count. Comments have no points. | None |
| `--limit N` | Results on this page, `1..=100`. | `20` |
| `--cursor CURSOR` | `next_cursor` of a previous page; it restores the whole request, so pass no query, option, or `--limit` with it. | Omitted |

Hacker News runs through the `hn_api` route, which needs no key: Algolia's HN Search API
(`providers.hn_api.url`) searches and the official Firebase API (`providers.hn_api.firebase_url`)
supplies ranks and live counts. JSON output is `{platform, provider, items, next_cursor}`. Each
item has `ref` (`hn:<id>`), `url` (`https://news.ycombinator.com/item?id=<id>`), `depth`
(`snippet` for an item with its own text, otherwise `metadata`), `title` (`null` for a comment),
`authors`, `published` (UTC second), `item_type` (`story`, `comment`, `poll`, `pollopt`, `job`),
`link_url`, `points`, `comments`, `text` (the item's own body as Markdown, never its comments or
linked page), and, for a comment, `story_ref`, `story_title`, and `parent_ref`. Search counts may
lag the site by a few minutes. A search reaches at most its first 1000 results: no cursor is
issued past them. A valid empty search returns `items: []` with exit 0.

Exit codes: `2` before any request for a blank query, an invalid user name, story ID, or date,
`--from` after `--to`, `--min-points` with `--type comment` alone, `--limit` out of range, or a
cursor past the first 1000 results; `3` for an empty `platforms.hn.order`; `4` for `parameter`
(the search service rejected the request), `rate_limited` (Algolia allows 10,000 requests per IP
per hour), `network`, `timeout`, and `runtime` for an unexpected response.

### `platform hn fetch`

```console
forager platform hn fetch REF_OR_URL [--depth thread|metadata|full_text] [--content-dir DIR]
                                     [--timeout SECONDS] [--format json|markdown|content]
                                     [--output FILE [--receipt]] [--verbose]
```

| Argument or option | Meaning | Default |
| --- | --- | --- |
| `REF_OR_URL` | `hn:<id>`, or a news.ycombinator.com item URL; other query parameters such as `p=2` are ignored. | Required |
| `--depth DEPTH` | `thread` writes the item and its whole discussion to a Markdown file; `metadata` returns the item alone with live counts; `full_text` reads the page a story or job links to, or the item's own title and text when it links nowhere. | `thread` |
| `--content-dir DIR` | Directory for the discussion or full-text Markdown file, named `hn-<id>.md`. | New directory under the system temp directory |
| `--format FORMAT` | `json`, `markdown`, or `content`; `content` prints the discussion or the full text and writes no file. | `json` |

At `thread` depth JSON output is the item fields (as under search, with live `points` and
`comments`) plus `content_url`, `content_provider` (`hn_api`), `content_path`, `content_len`,
`comments_delivered` (comments in the file), and `order`: `ranked_top_level` when top-level
comments follow the rank HN shows, or `chronological` when the rank was unavailable. Replies are
always in time order. The file starts with the item's title, link, author, points, comments,
time, ref, and text, then every comment indented by depth with its author, time, and `hn:<id>`.
Fetching a comment delivers the discussion below it, with its story. Deleted and dead comments
are not in the discussion, and comments posted in the last few minutes may be missing. A failed
Firebase request keeps the discussion in time order with one stderr diagnostic.

`full_text` JSON output carries the metadata and `content_url`, `content_provider`,
`content_path`, and `content_len`; it needs a configured Web Fetch provider, which reads the
linked page, while a text-only post's body comes from `hn_api` itself.

Exit codes: `2` before any request for a user, site, or other page URL, or a depth other than the
three; `3` for an empty `platforms.hn.order`, or `full_text` with no configured Web Fetch
provider; `4` for `parameter` (`HN item not found: hn:<id>`, `HN item hn:<id> is deleted or
dead`, or `full_text` of a comment, which names `--depth thread`), the transport failures under
search, and a `quality` failure (exit `5`) for a post without title or text.

### `platform hn stories`

```console
forager platform hn stories [top|new|best|ask|show|job] [--limit N] [--timeout SECONDS]
                            [--format json|markdown] [--output FILE [--receipt]] [--verbose]
```

| Argument or option | Meaning | Default |
| --- | --- | --- |
| `LIST` | `top` (the front page), `new`, `best`, `ask`, `show`, or `job`. | `top` |
| `--limit N` | Maximum items, `1..=100`. | `30` |

JSON output is `{platform, provider, list, items}`: the list's current items in rank order, each
with the item fields of search plus `rank`, its 1-based position in the list. No cursor exists:
the list changes constantly. Points and comment counts come from the search index and may lag by
a few minutes. Deleted, dead, and missing items are skipped with one stderr diagnostic, so ranks
may have gaps. An empty list returns `items: []` with exit 0.

Exit codes: `2` before any request for an unknown list or `--limit` out of range; `3` for an empty
`platforms.hn.order`; `4` when the list itself cannot be read, and for the transport failures
under search.

### `platform youtube search`

```console
forager platform youtube search [QUERY] [--sort relevance|date|views|rating]
                                [--from YYYY-MM-DD] [--to YYYY-MM-DD]
                                [--duration short|medium|long] [--channel CHANNEL]
                                [--has-captions] [--limit N] [--cursor CURSOR]
                                [--timeout SECONDS] [--format json|markdown]
                                [--output FILE [--receipt]] [--verbose]
```

| Argument or option | Meaning | Default |
| --- | --- | --- |
| `QUERY` | Words to match as YouTube reads them: `-word` excludes a word and `a\|b` matches either. Required without `--cursor`. | Omitted |
| `--sort ORDER` | `relevance`, `date` (newest first), `views`, or `rating`. | `relevance` |
| `--from` / `--to` | Inclusive range of UTC publication days; either end may be omitted. | None |
| `--duration CLASS` | `short` (under 4 minutes), `medium` (4 to 20), or `long` (over 20). | Any |
| `--channel CHANNEL` | Only videos of this channel: a `youtube:channel/<UC…>` ref or a `/channel/<UC…>` URL. | None |
| `--has-captions` | Only videos with captions, in any language. | Off |
| `--limit N` | Results on this page, `1..=50`. | `20` |
| `--cursor CURSOR` | `next_cursor` of a previous page; it restores the whole request, so pass no query, option, or `--limit` with it. | Omitted |

YouTube runs through the `youtube_api` route: the official Data API v3
(`providers.youtube_api.url`) with exactly one key in `providers.youtube_api.keys`. A search costs
one of the key's default 100 searches a day, plus one unit of its separate 10,000-unit budget.
JSON output is `{platform, provider, items, next_cursor}`. Each item has `ref`
(`youtube:<video_id>`), `url` (`https://www.youtube.com/watch?v=<video_id>`), `depth` (`snippet`
when the description excerpt is not empty, otherwise `metadata`), `title`, `authors` (the
channel name), `published` (UTC second), `channel_ref` (`youtube:channel/<UC…>`), `snippet` (the
description excerpt), `duration_seconds`, `live_status` (`none`, `upcoming`, `live`, or
`completed`), `views`, `likes`, `comments`, `has_captions`, and `language` and `caption_tracks`
(always `null` in search). Duration, counts, and `has_captions` come from a second request; when
it fails they are `null` with one stderr diagnostic and the exit code is unchanged. A count the
channel hides is `null`. A valid empty search returns `items: []` with exit 0.

Exit codes: `2` before any request for a blank query, an invalid date, `--from` after `--to`, a
`--channel` that is not a channel, or `--limit` out of range; `3` for an empty
`platforms.youtube.order`, or no key or more than one key in `providers.youtube_api.keys`; `4` for
`auth` (the key is invalid), `quota_exhausted` (the day's quota is used up), `parameter` (the API
rejected the request), `network`, `timeout`, and `runtime` for an unexpected response.

### `platform youtube fetch`

```console
forager platform youtube fetch REF_OR_URL [--depth metadata|abstract|full_text] [--lang LANG]...
                                          [--content-dir DIR] [--timeout SECONDS]
                                          [--format json|markdown|content]
                                          [--output FILE [--receipt]] [--verbose]
```

| Argument or option | Meaning | Default |
| --- | --- | --- |
| `REF_OR_URL` | `youtube:<video_id>`, or a watch, `youtu.be`, `shorts`, `live`, or `embed` video URL; `t`, `list`, `si`, and other viewing parameters are ignored. | Required |
| `--depth DEPTH` | `metadata` returns the video's fields; `abstract` adds the full description and its chapters; `full_text` renders one caption track as a transcript file and needs the `youtube_web` route in `platforms.youtube.order`. | `full_text` |
| `--lang LANG` | `full_text` only: a caption language such as `en` or `pt-BR`, repeatable in priority order. Every publisher track in these languages comes before any automatic (ASR) one; a bare tag matches any variant, a tag with a subtag matches exactly; machine translations never count. | The video's original language |
| `--content-dir DIR` | Directory for the transcript file. | New directory under the system temp directory |
| `--format FORMAT` | `json`, `markdown`, or `content`; `content` prints the description (`abstract`) or the transcript (`full_text`) and writes no file. | `json` |

`metadata` and `abstract` come from `youtube_api` (from `youtube_web` when the order has no
configured `youtube_api`). JSON output is the item fields of search with `snippet: null` and
`language` (through `youtube_api`, the audio or description language; through `youtube_web`, the
language of its only automatic caption track, else `null`); `youtube_web` adds `caption_tracks`
(`[{language, source}]`, `source` `manual` or `asr`, never machine translations) and reports
`likes` and `comments` as `null`. `abstract` adds `description` and `chapters`
(`[{start_seconds, title}]`, empty when the description does not mark at least three chapters
starting at `0:00`). `full_text` writes `youtube-<id>.md`: a header (title, ref, channel, date,
duration, caption language and source, and a note that the text is captions only), the
description, then the transcript under chapter headings in paragraphs that each open with a
`[mm:ss](<url>&t=<seconds>s)` link. Its JSON adds `content_url`, `content_provider`
(`youtube_web`), `content_path`, `content_len`, and `transcript`
(`{language, source, cues, paragraphs}`), and leaves out `description` and `chapters`. A fetch
through `youtube_api` costs one unit of the 10,000-unit budget, not a search; one through
`youtube_web` costs nothing but sends two requests (three at `full_text`), 4 seconds apart.

Exit codes: `2` before any request for an unrecognized input, a playlist or channel ref or URL
(list its videos with `videos` instead), an `@handle`, `/c/`, or `/user/` page, `--lang` without `full_text` or with an invalid tag, or
`full_text` while the order has no `youtube_web` (the message gives the command that enables it
and its risk); `3` as under search; `4` for `parameter` (`YouTube video not found or not public`
through `youtube_api`, `not found or not playable` through `youtube_web`), `auth` (YouTube asks
to sign in or to confirm the caller is not a bot: the egress IP is temporarily restricted),
`rate_limited`, `runtime` (an empty caption track, which may mean a soft IP block; a player answer
for another video; a caption track outside the configured origin), and the failures under search;
`5` (`quality`) for `abstract` of a video without a description, a `full_text` with no qualifying
caption track (the message lists the available languages and points to `transcribe`; fetch never
falls back to it), or a live stream.

### `platform youtube comments`

```console
forager platform youtube comments VIDEO [--limit N] [--sort relevance|time]
                                        [--timeout SECONDS] [--format json|markdown]
                                        [--output FILE [--receipt]] [--verbose]
```

| Argument or option | Meaning | Default |
| --- | --- | --- |
| `VIDEO` | A video ref or URL, as for fetch. A playlist, channel, or `@handle` exits `2`. | Required |
| `--limit N` | Top-level comments, `1..=100`, read in one request. | `20` |
| `--sort ORDER` | `relevance`, or `time` (newest first, though a pinned comment may still lead). | `relevance` |

Comments run through `youtube_api` and cost one unit of the 10,000-unit budget. There is no
cursor: one command reads the first page, and replies beyond those a comment carries are out of
reach. JSON output is `{platform, provider, video, comments, has_more}`; `has_more` is `true`
when the video has top-level comments this page did not return. Each comment has `id`,
`author` (a display name such as `@handle`), `author_channel_ref` (`youtube:channel/<UC…>`),
`text` (plain text), `likes`, `published` (UTC second), `reply_count` (all its replies),
`replies` (the few that come with it, each with the same fields up to `published`), and
`replies_complete` (whether `replies` holds all of them).

Exit codes: `2` before any request for an input that is not a video or `--limit` out of range;
`3` as under search; `4` for `parameter` (`comments are disabled for youtube:<id>`: disabled
comments are a failure, never an empty list; or `YouTube video not found or not public:
youtube:<id>`), the key and quota
failures under search, and `runtime` for an unexpected response. A video without comments
returns `comments: []` with exit 0.

### `platform youtube videos`

```console
forager platform youtube videos COLLECTION [--limit N] [--cursor CURSOR]
                                           [--timeout SECONDS] [--format json|markdown]
                                           [--output FILE [--receipt]] [--verbose]
```

| Argument or option | Meaning | Default |
| --- | --- | --- |
| `COLLECTION` | A `youtube:playlist/<id>` or `youtube:channel/<UC…>` ref, or a playlist URL, a watch URL with `list=<id>`, a `/channel/<UC…>` URL, or an `/@<handle>` URL (a channel tab such as `/videos` is fine). Required without `--cursor`. | Required |
| `--limit N` | Videos on this page, `1..=50`. | `20` |
| `--cursor CURSOR` | `next_cursor` of a previous page; it restores the collection and the limit, so pass neither with it. | Omitted |

Videos run through `youtube_api`: a page costs one unit, and the first page of a channel or handle
one more to look up the channel's uploads; its cursor remembers them. A playlist lists in its own order; a channel lists its
uploads newest first. JSON output is `{platform, provider, collection, items, next_cursor}`:
`collection` is `{ref, url}` of the playlist or channel read (a handle comes back as its channel
ref). Each item has the fields of a search item plus `position`, its 0-based place in the list;
`authors` and `channel_ref` name the channel that published the video, `published` is when the
video was published (not when it joined the list), `snippet` is the first 160 characters of the
description, and `duration_seconds`, `live_status`, the counts, `language`, and `has_captions`
are `null` (fetch a video for them). Private and deleted videos are skipped with one stderr
diagnostic naming their positions. An empty list returns `items: []` with exit 0.

Exit codes: `2` before any request for a video, a `/c/` or `/user/` page, a malformed handle, an
unrecognized input, `--limit` out of range, or a cursor that cannot be decoded or belongs to
another command; `3` as under search; `4` for `parameter` (the channel, handle, or playlist does
not exist or is not public), the key and quota failures under search, and `runtime` for an
unexpected response.

### `platform youtube ask`

```console
forager platform youtube ask VIDEO QUESTION [--timeout SECONDS] [--format json|markdown]
                                            [--output FILE [--receipt]] [--verbose]
```

| Argument or option | Meaning | Default |
| --- | --- | --- |
| `VIDEO` | A video ref or URL, as for fetch. A playlist, channel, or `@handle` exits `2`. | Required |
| `QUESTION` | The question about the video. | Required |
| `--timeout SECONDS` | Deadline for the whole command; a model takes minutes to watch a long video. | `600` |

A model watches the video's pictures and listens to its sound, then answers; it is told to answer
from the video alone and to mark every moment it relies on as `mm:ss`. Ask runs through the
`gemini_api` route with the user's own Gemini key (`providers.gemini_api.keys`), and first reads
the video's metadata through the fetch routes (one Data API unit through `youtube_api`); a video
that cannot be read, or one streaming live, fails before any model request. JSON output is
`{platform, provider, video, question, answer, model, usage}`: `video` is `{ref, url, title}`,
`model` the model version that answered, and `usage` the tokens spent
(`{prompt_tokens, output_tokens, thoughts_tokens, cached_tokens, total_tokens}`, `null` where the
service reports none). The whole answer is relayed content: the model's words, not the video's.
Ask writes no result journal.

Exit codes: `2` before any request for an input that is not a video or a blank question; `3`
before any request when `platforms.youtube.order` has no configured `gemini_api` (no key in
`providers.gemini_api.keys`) or no configured route that reads the video; `4` for the fetch
failures of the metadata read (`parameter` for a video that does not exist or is not public), and
from Gemini `parameter` (HTTP 400), `auth` (401 or 403), `rate_limited` or `quota_exhausted` (429,
after every key of the pool), and `runtime` (an empty answer or an unexpected response); `5`
(`quality`) for a live stream, an answer cut off at the output limit, or one a safety filter
blocked.

### `platform youtube transcribe`

```console
forager platform youtube transcribe VIDEO [--content-dir DIR] [--timeout SECONDS]
                                          [--format json|markdown|content]
                                          [--output FILE [--receipt]] [--verbose]
```

| Argument or option | Meaning | Default |
| --- | --- | --- |
| `VIDEO` | A video ref or URL, as for fetch. A playlist, channel, or `@handle` exits `2`. | Required |
| `--content-dir DIR` | Directory for the transcript file. | New directory under the system temp directory |
| `--timeout SECONDS` | As under ask. | `600` |
| `--format FORMAT` | `json`, `markdown`, or `content`; `content` prints the transcript and writes no file. | `json` |

A model watches the video and writes a timestamped transcript of its speech, through
`gemini_api` like ask; run it only when the video has no caption track to fetch. It first reads
the video with its description (one Data API unit through `youtube_api`), so the transcript file
`youtube-<id>-transcribed.md` has the same layout as a `full_text` fetch: the description, then
paragraphs split by its chapters, each opening with a `[mm:ss](<url>&t=<seconds>s)` link. Its
header says a model generated it and that it is not guaranteed to be verbatim. Segments that end
more than 5 seconds past the video's length are dropped with one stderr diagnostic. JSON output is
that of a `full_text` fetch, with `provider` and `content_provider` `gemini_api`,
`transcript.source` `model`, and `model` and `usage` as under ask.

Exit codes: as under ask; `5` (`quality`) also for a transcript cut off at the output limit or
one that is not the requested JSON (no file is written); `4` (`runtime`) when the file cannot be
written.

## Gemini Deep Research

Run these commands only when the user explicitly asks for Gemini Deep Research. They drive
Gemini in the user's own logged-in Chrome through OpenCLI and the `forager-gemini` adapter: install
OpenCLI 2.x (`npm install -g @jackwener/opencli@2`) and its Browser Bridge extension 1.0.25 or
later (`opencli doctor`), then copy the `opencli/forager-gemini` directory next to this skill's
`SKILL.md` to `~/.opencli/clis/forager-gemini` (replace the whole directory to update), and
check with `forager doctor --provider gemini_browser`. No platform order or other setting enables
them; `providers.gemini_browser.command` and `.timeout` (default `opencli` and 240 seconds per
OpenCLI command) configure them. The required contract is `forager-gemini/3`. When a command
reports an older contract such as `forager-gemini/2`, the installed adapter is the older one:
replace the whole `~/.opencli/clis/forager-gemini` directory with the one from this skill and
check again with `forager doctor --provider gemini_browser`.

A Gemini report is a Delegated Research Report, not research evidence: relay it as Gemini's
conclusions with attribution, and fetch a cited source with `forager fetch` before stating one of
its claims as fact.

### `gemini research start`

```console
forager gemini research start QUERY [--timeout SECONDS] [--format json|markdown]
```

| Argument or option | Meaning | Default |
| --- | --- | --- |
| `QUERY` | The research question, sent to Gemini as written. An empty question exits `2` before Chrome opens. | Required |
| `--timeout SECONDS` | Whole-command deadline. | `240` |

The command brings a Chrome window to the front, opens a new Gemini chat, selects Deep Research,
sends the question once, clicks "Start research" on the plan Gemini proposes, and opens the
conversation again to check that the research is running. It creates a
conversation and spends the account's Deep Research quota, so it runs exactly one attempt whatever
the `retry` configuration; never run it again after a failure without the user's go-ahead. JSON
output has `route` (`gemini_browser`), `conversation_id`, `conversation_url`, and `plan`
(`{title, steps: [{index, label, description}], eta_text}`, Gemini's own research plan). Poll the
conversation with `gemini research result`.

Exit codes: `2` for an empty question; `4` for `auth` (Gemini asks the browser to sign in),
`quota_exhausted` (the Deep Research quota is used up), `timeout`, and `runtime` (the account's
tools menu offers no Deep Research, or shows it disabled, which may mean the quota is used up;
Gemini replied with text instead of a plan, quoted; the plan could not be confirmed, so the user
must click "Start research" on the conversation page; the research had not started after the
click, so read it with `gemini research result` before anything else; the Gemini response
structure changed; or the adapter is missing or outdated). A failure payload adds
`conversation_url` once forager knows the conversation. A failure without one whose message says
forager does not know whether Gemini received the question means the user should look in the
Gemini history before anything is started again.

### `gemini research result`

```console
forager gemini research result CONVERSATION [--report-dir DIR] [--timeout SECONDS]
                               [--format json|markdown|content]
                               [--output FILE [--receipt]]
```

| Argument or option | Meaning | Default |
| --- | --- | --- |
| `CONVERSATION` | A `https://gemini.google.com/app/<id>` URL or its hexadecimal `<id>`. Anything else exits `2` before Chrome opens. | Required |
| `--report-dir DIR` | Directory for the report and sources files. | A new directory under the system temporary directory |
| `--timeout SECONDS` | Whole-command deadline. | `120` |
| `--format content` | Print a completed report and its sources to stdout and write no file; any other status prints the JSON below. | `json` |

The command opens the conversation in a background Chrome window and reads where its newest
research turn stands; it changes nothing in the conversation and takes about 15 to 30 seconds.
JSON output has `route` (`gemini_browser`), `conversation_id`, `conversation_url`, and `status`:

| `status` | Further fields |
| --- | --- |
| `awaiting_confirmation` | `plan`: `{title, steps: [{index, label, description}], eta_text}`. Gemini waits for the user to click "Start research" on the conversation page. |
| `running` | `progress`: `{sources_visited, thoughts, latest_thought}`, as far as the page shows them. |
| `completed` | `title`, `report_path`, `sources_path`, `content_len` (characters), and `source_count`. |

A completed report is written as `gemini-<id>.md`, Gemini's Markdown body unchanged followed by a
`## Sources` list that resolves every `[cite: N]`, and `gemini-<id>.sources.json`,
`[{id, title, url}]` ordered by `id`.

Exit codes: `2` for an unrecognized conversation; `4` for `auth` (Gemini asks the browser to sign
in), `parameter` (the conversation is unavailable to this account or holds no Deep Research),
`timeout`, and `runtime` (the page showed another conversation; the Gemini response structure
changed, with its location; the adapter is missing or outdated, with install steps; or the report
could not be written). A failure payload adds `conversation_url`.

## ChatGPT Deep Research

Run these commands only when the user explicitly asks for ChatGPT Deep Research. They drive
ChatGPT in the user's own logged-in Chrome through OpenCLI and the `forager-chatgpt` adapter:
install OpenCLI 2.x (`npm install -g @jackwener/opencli@2`) and its Browser Bridge extension
1.0.25 or later (`opencli doctor`), then copy the `opencli/forager-chatgpt` directory next to
this skill's `SKILL.md` to `~/.opencli/clis/forager-chatgpt` (replace the whole directory to
update), and check with `forager doctor --provider chatgpt_browser`. The required contract is
`forager-chatgpt/2`. When a command or the doctor check reports contract `forager-chatgpt/1`, the
installed adapter is the older one: replace the whole `~/.opencli/clis/forager-chatgpt` directory
with the one from this skill and run `forager doctor --provider chatgpt_browser` again. No
platform order or other setting enables them; `providers.chatgpt_browser.command` and
`.timeout` (default `opencli` and 240 seconds per OpenCLI command) configure them.

A ChatGPT report is a Delegated Research Report, not research evidence: relay it as ChatGPT's
conclusions with attribution, and fetch a cited source with `forager fetch` before stating one of
its claims as fact.

### `chatgpt research start`

```console
forager chatgpt research start QUERY [--timeout SECONDS] [--format json|markdown]
```

| Argument or option | Meaning | Default |
| --- | --- | --- |
| `QUERY` | The research question, sent to ChatGPT as written. An empty question exits `2` before Chrome opens. | Required |
| `--timeout SECONDS` | Whole-command deadline. | `240` |

The command brings a Chrome window to the front, switches the ChatGPT composer to chat mode (and
leaves it there), types the question, selects Deep Research in the "Add files and more" menu,
sends once, keeps the page open for about 90 seconds after the plan shows so that the plan's
countdown starts the research (forager never clicks "Start"), and reopens the conversation to
check that the research runs; a run usually takes two to three minutes. It creates a conversation and spends the account's Deep Research quota, so it runs
exactly one attempt whatever the `retry` configuration; never run it again after a failure without
the user's go-ahead. JSON output has `route` (`chatgpt_browser`), `conversation_id`,
`conversation_url`, and `plan` (`{title, steps: [{index, text}]}`, ChatGPT's own research plan).
Poll the conversation with `chatgpt research result`.

Exit codes: `2` for an empty question; `4` for `auth` (ChatGPT asks the browser to sign in),
`quota_exhausted` (the usage receipt or a new usage notice says the Deep Research limit is
reached), `timeout`, and `runtime` (the chat-mode menu offers no Deep Research; the adapter
stopped before sending, with its reason (such as `obscured_target` for a button it could not
click, or `click_missed` for a click the page did not take), last step, and whether the composer
draft was cleared; ChatGPT replied with text instead of a plan, quoted, so the user answers on the page; the plan has
not started, and its countdown may still start it, so read the result before anything else; the
ChatGPT response structure changed; or the adapter is missing or outdated). A message that says
nothing was sent means no conversation exists. A failure payload adds `conversation_url` once
forager knows the conversation. A failure without one whose message says forager does not know
whether ChatGPT received the question means the user should look in the ChatGPT history before
anything is started again.

### `chatgpt research result`

```console
forager chatgpt research result CONVERSATION [--report-dir DIR] [--timeout SECONDS]
                                [--format json|markdown|content]
                                [--output FILE [--receipt]]
```

| Argument or option | Meaning | Default |
| --- | --- | --- |
| `CONVERSATION` | A `https://chatgpt.com/c/<uuid>` or `https://chatgpt.com/g/<project>/c/<uuid>` URL, or the `<uuid>`. Anything else exits `2` before Chrome opens. | Required |
| `--report-dir DIR` | Directory for the report and sources files. | A new directory under the system temporary directory |
| `--timeout SECONDS` | Whole-command deadline. | `120` |
| `--format content` | Print a completed report and its sources to stdout and write no file; any other status prints the JSON below. | `json` |

The command opens the conversation in a background Chrome window and reads where its newest Deep
Research stands from the conversation response the page receives; it changes nothing in the
conversation, and also reads a research the user started on the web page. JSON output has
`route` (`chatgpt_browser`), `conversation_id`, `conversation_url`, and `status`:

| `status` | Further fields |
| --- | --- |
| `awaiting_confirmation` | `plan`: `{title, steps: [{index, text}]}`. ChatGPT waits for the user to click "Start" on the conversation page, or for its countdown to run out. |
| `running` | `plan_title`, and `progress`: `{steps_completed, steps_total}`. |
| `completed` | `title`, `report_path`, `sources_path`, `content_len` (characters, without the sources list), and `source_count`. |

A completed report is written as `chatgpt-<uuid>.md`, ChatGPT's Markdown body with every citation
marker replaced by `[N]` or `[N, M]`, followed by a `## Sources` list, and
`chatgpt-<uuid>.sources.json`, `[{id, title, url}]` ordered by `id`. Source URLs lose the
`utm_source=chatgpt.com` parameter; a source without a title has `"title": null` and is listed
as `- [N] <URL>`. The report may be written in another language variant than the prompt.

Exit codes: `2` for an unrecognized conversation; `4` for `auth` (ChatGPT asks the browser to
sign in), `parameter` (the conversation is unavailable to this account or holds no Deep Research),
`quota_exhausted` (the usage receipt says the Deep Research limit is reached), `timeout`, and
`runtime` (the page or the response showed another conversation, and nothing was written; the
ChatGPT response structure changed, with its location or status; the adapter is missing or
outdated, with install steps; or the report could not be written). A failure payload adds
`conversation_url`.

## Configuration and diagnostics

### `config`

```console
forager config path
forager config list
forager config set KEY VALUE
printf '%s' "$VALUE" | forager config set KEY -
forager config unset KEY
```

| Command | Behavior |
| --- | --- |
| `config path` | Print the active configuration file path without loading or validating its schema. |
| `config list` | Print the effective JSON view, including each value's source and masked credentials. When parsing fails, report the path, bad key, and location. |
| `config set KEY VALUE` | Set one schema key. Strings, numbers, and booleans are plain text, such as `https://api.exa.ai` or `45`; arrays are TOML literals, such as `'["KEY"]'`. A string wrapped in one pair of matching quotes fails with exit `2` instead of saving the quotes. Argument values may remain in shell history. Invalid paths, types, or enum values fail without writing. |
| `config set KEY -` | Read the complete value from stdin; use this form for secrets. |
| `config unset KEY` | Remove only the file-layer value. An environment override can remain effective. |

Use dotted schema paths such as `providers.exa.timeout`. `config set` and `config unset` edit the
document layer and remain available for repairing a schema-invalid but syntactically parseable
file. Edit the path printed by `config path` when the TOML syntax itself is damaged.

### `setup`

```console
forager setup [--non-interactive] [--lang zh|en]
```

| Option | Meaning | Default |
| --- | --- | --- |
| `--non-interactive` | Create a complete commented configuration template without prompting. Refuse to overwrite an existing target. | Off |
| `--lang LANG` | Use `zh` or `en` for the interactive setup prompts. | Prompt/default locale |

After setup, use `forager doctor` to check the resulting configuration.

### `doctor`

```console
forager doctor [--provider PROVIDER] [--timeout SECONDS] [--format json|markdown]
```

| Option | Meaning | Default |
| --- | --- | --- |
| `--provider PROVIDER` | Deep-probe one of `xai`, `tavily`, `firecrawl`, `jina`, `context7`, `exa`, `arxiv_api`, `ssrn_crossref`, `ssrn_browser`, `serpapi`, `xiaohongshu_browser`, `hn_api`, or `gemini_browser`. Without it, run the shallow all-provider report. | Omitted |
| `--timeout SECONDS` | Set the diagnostic deadline. | `30` |
| `--format FORMAT` | Use `json` or `markdown`. | `json` |

Use `doctor` for credentials, connectivity, provider responses, and effective configuration
inspection. In shallow mode, `ok` covers every configured provider: if any configured provider is
unreachable, the top-level result is false and the command uses exit code 4. The browser routes
`ssrn_browser` and `xiaohongshu_browser` are checked only when their platform's order lists them:
doctor runs their OpenCLI `contract` command, and a failed check carries a `message` with install
steps; `--provider ssrn_browser` or `--provider xiaohongshu_browser` also needs the route in the
order and runs one real search in Chrome. The shallow report never checks `gemini_browser`;
`--provider gemini_browser` opens the Gemini app in a background Chrome window and reports whether
the adapter is installed and current and the browser is signed in, without reading or starting
any research. Without a key, `jina` is configured and the shallow report notes that it runs
anonymously; `--provider jina` then probes without a key, and with keys it reports a failing key
even when the anonymous request succeeds. `--provider xai` adds an `x_search` check, one X search, while
`platforms.x.order` lists `xai`. `--provider hn_api` runs one search and adds a `firebase` check,
one request to the Firebase API. `--provider serpapi` runs no search and costs nothing: it asks
SerpApi's account endpoint about every configured key and adds a `keys` array, one entry per key
with `key_index`, `ok`, `searches_left`, `plan_searches_left`, `this_month_usage`,
`this_hour_searches`, and `hourly_limit`, plus `error_kind` for a failing key (`auth`,
`quota_exhausted` with no searches left, `rate_limited` at the hourly limit). Any failing key fails
the probe with exit 4, and `message` names it as `providers.serpapi.keys[N]`. It does not prove
that Scholar searches decode; `smoke --live` does. Do not use doctor as
the recovery path for configuration that cannot be loaded.

### `smoke`

```console
forager smoke
forager smoke --live [--timeout SECONDS]
                   [--outage-evidence CASE_ID=EVIDENCE_URL ...]
forager smoke --live --list
```

| Option | Meaning | Default |
| --- | --- | --- |
| `--live` | Run the live acceptance registry instead of offline checks. | Off |
| `--list` | Print the live case registry without running it. Requires `--live`. | Off |
| `--timeout SECONDS` | Set the live run deadline. Requires `--live`. | `420` |
| `--outage-evidence CASE_ID=EVIDENCE_URL` | Attach repeatable provider-outage evidence to a live case. Requires `--live`. | Empty |

The offline form performs local configuration, registry, credential-presence, and journal
writability checks without contacting providers. `smoke` emits JSON. Hidden acceptance-harness
probe flags are internal and are not part of the public CLI.

All live cases share the one `--timeout` deadline. A case runs up to three times, but only once
when it fails on Auth or an attempt-level Parameter error. A case that the deadline left no time to
start is `skipped` with a message beginning `not started:`; raise `--timeout` and rerun rather than
treating it as a provider failure.

## Exit codes

| Code | Meaning |
| --- | --- |
| `0` | Success, including a valid empty result from a provider-direct command. |
| `2` | Argument or caller-input error, including an invalid plan or invalid `config set` path/value. |
| `3` | Configuration, stdin, or requested output-write error. |
| `4` | Transport/runtime terminal failure, including a hard timeout. |
| `5` | Content-quality or evidence terminal failure. |

Exit code `1` is intentionally unused. Panic status `101` is outside the CLI contract. After a
network command reaches a rendered terminal result, JSON output is emitted as clean JSON on stdout
and diagnostics or logs use stderr. A failed command renders its failure payload on stdout too, so
an empty stderr does not mean success; judge the outcome by the exit code. After successful Clap parsing selects JSON, configuration,
stdin, and plan preflight failures use the parseable stdout error object described above. Clap
errors, panics, and non-JSON formats retain their existing channels.
