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
- [Configuration and diagnostics](#configuration-and-diagnostics)
- [Exit codes](#exit-codes)

## Shared behavior

The public top-level commands are `search`, `research`, `fetch`, `map`, `exa`, `context7`,
`anysearch`, `platform`, `config`, `setup`, `doctor`, and `smoke`. Use `-h` or `--help` on any
command for parser-generated help. Available aliases are:

| Command | Alias |
| --- | --- |
| `search` | `s` |
| `research` | `rs` |
| `fetch` | `f` |
| `context7` | `c7` |
| `anysearch` | `as` |
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
| `--extra-sources N` | Set a target in `0..=20`. At `0`, Web Search uses 3 while Documentation Search and Vertical Search use 1; an explicit `1..=20` is the exact target. | `0` |
| `--fallback MODE` | Use `auto` or `off` for provider/model fallback. | `search.fallback`, normally `auto` |
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

### `anysearch search`

```console
forager anysearch search QUERY [--domain DOMAIN --sub-domain SUBDOMAIN]
                               [--sub-domain-params JSON] [--max-results N]
                               [--timeout SECONDS] [--format json|markdown]
                               [--output FILE [--receipt]] [--verbose]
```

| Argument or option | Meaning | Default |
| --- | --- | --- |
| `QUERY` | AnySearch discovery query. | Required |
| `--domain DOMAIN` | Select a parent vertical domain. Must be paired with `--sub-domain`. | Omitted |
| `--sub-domain SUBDOMAIN` | Select a subdomain without dotted shorthand. Must be paired with `--domain`. | Omitted |
| `--sub-domain-params JSON` | Pass a JSON object for the selected subdomain. It requires both domain options and cannot override `query`, `domain`, `sub_domain`, or `max_results`. | Empty object |
| `--max-results N` | Request between 1 and 100 results. | `5` |
| `--timeout SECONDS` | Override `providers.anysearch.timeout`. | Configured value |
| `--format FORMAT` | Use `json` or `markdown`. | `json` |
| `--output FILE` | Tee the rendered result to a file. | Omitted |
| `--receipt` | Print only a receipt on success; requires `--output`. | Off |
| `--verbose` | Include full provider attempts inline. | Off |

Use no domain options for general vertical discovery. Use separate undotted parent and subdomain
values for scoped discovery. The retired `security.cve` alias is invalid; use
`--domain security --sub-domain vuln`.

### `anysearch domains`

```console
forager anysearch domains DOMAIN [--timeout SECONDS] [--format json|markdown]
                                 [--output FILE [--receipt]] [--verbose]
```

| Argument or option | Meaning | Default |
| --- | --- | --- |
| `DOMAIN` | Undotted parent domain whose supported subdomains should be listed. | Required by the parser |
| `--timeout SECONDS` | Override `providers.anysearch.timeout`. | Configured value |
| `--format FORMAT` | Use `json` or `markdown`. | `json` |
| `--output FILE` | Tee the rendered result to a file. | Omitted |
| `--receipt` | Print only a receipt on success; requires `--output`. | Off |
| `--verbose` | Include full provider attempts inline. | Off |

## Platform commands

`forager platform <id> <op>` retrieves items of a built-in platform through the routes in
`platforms.<id>.order`; results never fall back to another platform. The platforms are listed in
[`platform-vocabulary.json`](platform-vocabulary.json). Platform commands write no result journal.
Their `--timeout` defaults to `120` and includes waits for the platform's request window, shared
across all local processes (arXiv: one request every 3 seconds; SSRN through Crossref: one request
per second; SSRN through the browser: one OpenCLI command every 5 seconds; Xiaohongshu: one OpenCLI
command every 10 seconds; Google Scholar through SerpApi has no local window).

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

When `platforms.ssrn.order` includes the opt-in `ssrn_browser` route, that route reads SSRN's own
result pages in Chrome through OpenCLI (see [`platforms.md`](platforms.md) to install and enable
it). Its items have `depth: snippet` (`metadata` for a result card without an excerpt), `snippet`, `posted` as the page shows it, and `published` as
an ISO date. A page never spans two 50-result SSRN pages, so it can hold fewer items than
`--limit`. An SSRN security check that did not clear is an `auth` failure; an unrecognized result
page is a `runtime` failure, never an empty result.

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
| `published` | `YYYY-MM-DD`, or `null` when the card shows a relative time such as `3天前`. |
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

## Configuration and diagnostics

### `config`

```console
forager config path
forager config list
forager config set KEY VALUE
printf '%s' "$TOML_VALUE" | forager config set KEY -
forager config unset KEY
```

| Command | Behavior |
| --- | --- |
| `config path` | Print the active configuration file path without loading or validating its schema. |
| `config list` | Print the effective JSON view, including each value's source and masked credentials. When parsing fails, report the path, bad key, and location. |
| `config set KEY VALUE` | Set one schema key from a TOML literal. Argument values may remain in shell history. Invalid paths, types, or enum values fail without writing. |
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
| `--provider PROVIDER` | Deep-probe one of `xai`, `openai_compatible`, `tavily`, `firecrawl`, `jina`, `context7`, `exa`, `anysearch`, `arxiv_api`, `ssrn_crossref`, `ssrn_browser`, `serpapi`, or `xiaohongshu_browser`. Without it, run the shallow all-provider report. | Omitted |
| `--timeout SECONDS` | Set the diagnostic deadline. | `30` |
| `--format FORMAT` | Use `json` or `markdown`. | `json` |

Use `doctor` for credentials, connectivity, provider responses, and effective configuration
inspection. In shallow mode, `ok` covers every configured provider: if any configured provider is
unreachable, the top-level result is false and the command uses exit code 4. The browser routes
`ssrn_browser` and `xiaohongshu_browser` are checked only when their platform's order lists them:
doctor runs their OpenCLI `contract` command, and a failed check carries a `message` with install
steps; `--provider ssrn_browser` or `--provider xiaohongshu_browser` also needs the route in the
order and runs one real search in Chrome. `--provider serpapi` runs no search and costs nothing: it asks
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
| `--timeout SECONDS` | Set the live run deadline. Requires `--live`. | `180` |
| `--outage-evidence CASE_ID=EVIDENCE_URL` | Attach repeatable provider-outage evidence to a live case. Requires `--live`. | Empty |

The offline form performs local configuration, registry, credential-presence, and journal
writability checks without contacting providers. `smoke` emits JSON. Hidden acceptance-harness
probe flags are internal and are not part of the public CLI.

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
and diagnostics or logs use stderr. After successful Clap parsing selects JSON, configuration,
stdin, and plan preflight failures use the parseable stdout error object described above. Clap
errors, panics, and non-JSON formats retain their existing channels.
