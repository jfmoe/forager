# forager CLI reference

This reference documents the public CLI of the forager release this skill ships with. Load it under the conditions given
in `SKILL.md`: for exact command syntax, non-routine commands, or diagnosis and recovery details.
Routine `search` stays on its branch reference. Treat
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

The public top-level commands are `search`, `fetch`, `map`, `exa`, `platform`,
`browser`, `config`, `setup`, and `doctor`. Use `-h` or
`--help` on any command for parser-generated help. Available aliases are:

| Command | Alias |
| --- | --- |
| `search` | `s` |
| `fetch` | `f` |
| `config list` | `config ls` |

Subcommand prefixes are not inferred: an abbreviation that is not a listed alias is an unknown
command.

Network commands that expose these options share the following behavior:

| Option | Behavior |
| --- | --- |
| `--timeout SECONDS` | Set a positive hard deadline for the whole command, including retries and fallback. |
| `--format FORMAT` | Select stdout rendering. The default is `json`. |
| `--output FILE` | Write the same rendered result to `FILE` and still emit it to stdout. |
| `--receipt` | With `--output`, emit only a one-line `{"output_path","bytes","lines"}` receipt on success; failures still print the full failure payload. Use it to keep long results out of context and read the file in parts. |
| `--verbose` | Include full provider attempts inline. Without it, `search` retains full attempts in its journal; provider-direct commands do not create a result journal. |

After argument parsing selects `--format json`, `search` and `fetch` return a single parseable
error object on stdout for configuration or runtime preflight failures. Clap parser errors and
panics keep their existing channels, as do failures in non-JSON formats.
The object is `{error_kind, message, journal_ref, journal_status}`: `error_kind` is `config` or
`runtime` with exit `3` or `4`. When the failure comes before the
configuration has loaded, `journal_ref` is `null` with `journal_status: "unavailable"`. A `search`
whose main-search backends have no keys also fails this way with exit `3`, but it records the
failure in the journal and reports the journal pair as any search does. Every other
command, and these two in other formats, reports a preflight failure as one stderr line starting
`argument_error:`, `config_error:`, or `runtime_error:`.

A terminal failure prints a small failure payload on stdout: `error_kind`, a `message` of at most
500 characters, an `attempts` summary (`total`, counts `by_kind`, the `providers` tried, and
`by_kind_truncated`, `providers_truncated`, and `truncated` marking a cut list), and `journal_ref`
with `journal_status`. `journal_status` is `written`, `disabled`, or `failed` for `search`, and
`not_applicable` for commands that write no journal. A `search` failure also carries its non-empty
`capability_gaps`. A command that reaches its `--timeout` reports `timeout` and keeps the
attempts it completed.

When `--output` cannot write its file, the command exits `3` whatever its own result was; stdout
still carries the full result, and JSON output gains `output_status: "failed"` and an
`output_error` message. A failed journal write is only a stderr warning: it never changes the
result or the exit code.

`log.level=debug` enables an optional terminal projection with one bounded attempts summary;
`trace` adds safe fields for each attempt. This projection never changes stdout or replaces
`--verbose` and journal attempts.

`content` output is available only for `search`, `fetch`, and `platform <id> fetch`.
All other commands with `--format` accept only `json` and `markdown`.

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
| `--capabilities CSV\|none` | Authoritative supplemental capability set. Use canonical comma-separated IDs, or `none` for main search only. Omitting it runs main search only, like `none`: forager never adds a capability the caller did not declare. | Omitted (`none`) |
| `--model ID` | Override the model of the first main-search backend that runs; a fallback backend keeps its configured model. | Configured model |
| `--extra-sources N` | Set a target in `0..=20`. At `0`, Web Search uses 3 while Documentation Search uses 1; an explicit `1..=20` is the exact target. | `0` |
| `--fallback MODE` | Use `auto` or `off` for provider fallback. | `search.fallback`, normally `auto` |
| `--timeout SECONDS` | Set the whole-pipeline deadline. | `180` |
| `--format FORMAT` | Use `json`, `markdown`, or `content`. | `json` |
| `--output FILE` | Tee the rendered result to a file. | Omitted |
| `--receipt` | Print only a receipt on success; requires `--output`. | Off |
| `--verbose` | Include full provider attempts inline. | Off |

Default search JSON keeps `answer`, Primary Search Sources in `sources`, every non-primary Search
Candidate in `extra_sources`, capability gaps, and the journal reference. Each candidate has
required `provider`, `capability`, an HTTP(S) `url` that can enter Web Fetch, and
provider-specific `provider_data`, plus nullable `title` and `summary`. Provider-native summaries
are not verified evidence. Search-side Web Fetch successes use the actual fetch provider and a
300-character Normalized Fetch Content preview as their candidate summary. `--verbose` adds
provider attempts inline.
Content format emits only the main answer. Markdown labels the two source roles as `Primary
Sources` and `Extra Sources`. The `--extra-sources` target never selects a capability: with
`--capabilities none` no supplemental search runs. An Exa Documentation Search candidate has
`provider_data` with `id`, `highlights`, `published_date`, `author`, `image`, and `favicon`,
without the page text. A field the provider does not report is left out.

Main search runs the backends of `search.backends` in order: `xai` (the xAI Responses API) and
`openai` (an OpenAI Responses endpoint with its `web_search` tool, such as a CLIProxyAPI gateway in
front of a Codex account). The default order is `["xai", "openai"]`; a backend without
`providers.<backend>.keys` is skipped. With `--fallback auto`, `openai` answers when `xai` fails;
`["openai", "xai"]` makes it the first choice. `provider_attempts` show which backend failed and
which one answered. To use `openai`, set `providers.openai.url` and `providers.openai.keys`;
`providers.openai.model` defaults to `gpt-6.1-terra`, the Codex model name under CLIProxyAPI.

forager removes closed `<think>…</think>` blocks from the answer and moves a trailing source
heading block out of it into `sources`; inline `[[N]](URL)` citations stay in the answer and are
copied into `sources`, which are deduplicated by URL. An answer that is empty after this fails
with `runtime`. When main search fails, it alone decides the terminal
state and exit code, and the failure payload keeps the candidates already found: `extra_sources`
in order without their `summary`, as many as fit, with `extra_sources_truncated` saying whether
any were cut. Its attempt summary counts the supplemental attempts too. `--verbose` keeps every
candidate in full.

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

`--timeout` is both the command deadline and the timeout sent to Tavily; each request still waits
at most the smaller of the remaining deadline and `providers.tavily.timeout`.

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

## Platform commands

`forager platform <id> <op>` retrieves items of a built-in platform through the routes in
`platforms.<id>.order`; results never fall back to another platform. The platforms are listed in
[`platform-vocabulary.json`](platform-vocabulary.json). Platform commands write no result journal.
Their `--timeout` defaults to `120` (`600` for YouTube ask and transcribe) and includes waits for the platform's request window, shared
across all local processes (arXiv: one request every 3 seconds; SSRN through Crossref: one request
per second; Xiaohongshu: one browser
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
on the last page; a valid empty search returns `items: []` with exit 0. The arXiv metadata is
`updated`, `primary_category`, `categories`, `doi`, `journal_ref`, `comment`, and `pdf_url`, each
`null` when arXiv has none; `ref` carries the version, such as `arxiv:2401.01234v2`, and `title`
has its whitespace folded.

Exit codes: `2` before any request for an invalid cursor or option, a reversed date range, no query
and no filter, or an option no configured route supports; `3` for an empty `platforms.arxiv.order`
or one that lists another platform's route; `4` for `parameter` (arXiv answered with an error
entry, whose message is passed on), `timeout` (including too little time left to wait for the
request window), and `runtime` when the request window's shared state is unusable, before any
request.

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
`full_text` with no configured Web Fetch provider exits `3`. `export.arxiv.org` URLs are accepted
too. The file is named after the versioned ref, such as `arxiv-hep-th-9901001v3.md`. A failed
metadata read reads no body; arXiv's HTTP 429 is `rate_limited` (exit `4`); a file that cannot be
written is a `runtime` failure (exit `4`), never an inline fallback.

### `platform ssrn search`

```console
forager platform ssrn search QUERY [--limit N] [--cursor CURSOR]
                             [--timeout SECONDS] [--format json|markdown]
                             [--output FILE [--receipt]] [--verbose]
```

| Argument or option | Meaning | Default |
| --- | --- | --- |
| `QUERY` | Query expression; matching follows Crossref. Required unless `--cursor` is given. | Required |
| `--scope all\|title\|bibliographic` | Crossref default fields, title, or bibliographic fields. | `all` |
| `--author TEXT` | Author text query, not an identity selector. | Omitted |
| `--affiliation TEXT` | Author affiliation query. | Omitted |
| `--published-from DATE`, `--published-to DATE` | Inclusive publication date bounds, `YYYY-MM-DD`. Either bound can be omitted. | Omitted |
| `--created-from DATE`, `--created-to DATE` | Inclusive Crossref first-registration date bounds. | Omitted |
| `--updated-from DATE`, `--updated-to DATE` | Inclusive Crossref metadata deposit/update date bounds. | Omitted |
| `--has-abstract` | Require an upstream abstract field; does not fetch or guarantee usable abstract text. | Off |
| `--type TYPE` | Exact registered Crossref work type; see help for accepted values. | Omitted |
| `--orcid ID` | Exact contributor ORCID, as a bare ID with a valid check digit. | Omitted |
| `--funder DOI` | Exact Open Funder Registry DOI, `10.13039/<digits>`. | Omitted |
| `--sort METRIC` | relevance, published, created, updated, or citations. | `relevance` |
| `--order asc\|desc` | Ranking direction. | `desc` |
| `--limit N` | Results on this page, `1..=100`. | `10` |
| `--cursor CURSOR` | `next_cursor` of a previous page; it restores the whole request, so pass no query, search criteria, or `--limit` with it. | Omitted |

The `ssrn_crossref` route searches Crossref records under the SSRN DOI prefix; it needs no
credentials. Each item has `ref`, `url` (the SSRN abstract page), `depth`, `title`, `authors`,
`published`, and the SSRN fields `abstract`, `doi`, `crossref_type`, and `crossref_created`; a
field the Crossref record lacks is `null`. `depth` is `abstract` when the item has an abstract and `metadata` otherwise. Paging stops
at result 10000, where Crossref stops offset paging; `next_cursor` is then `null`. Records whose DOI
is not an SSRN DOI are dropped and reported on stderr. A valid empty search returns `items: []`
with exit 0.

Affiliation, ORCID, and funder searches depend on deposited metadata. Crossref publication,
registration, and update dates are distinct from SSRN's own posting and revision dates. Cursors
restore all criteria. `published` keeps the precision Crossref reports: `2012`, `2012-04`, or
`2012-04-19`.

Exit codes: `2` before any request for a blank query, an invalid calendar date or a range that ends
before it starts, an invalid cursor or one past Crossref's paging limit, or a cursor combined with
a query, criterion, or `--limit` (even one passed with its default value); `3` for an empty
`platforms.ssrn.order`; `4` for `rate_limited` (Crossref answered 429) and the other transport
failures.

### `platform ssrn fetch`

```console
forager platform ssrn fetch REF_OR_URL [--depth metadata|abstract]
                            [--timeout SECONDS] [--format json|markdown|content]
                            [--output FILE [--receipt]] [--verbose]
```

| Argument or option | Meaning | Default |
| --- | --- | --- |
| `REF_OR_URL` | `ssrn:<id>`, a `papers.ssrn.com/sol3/papers.cfm?abstract_id=<id>` URL, an `ssrn.com/abstract=<id>` URL, or the `10.2139/ssrn.<id>` DOI or its doi.org URL. | Required |
| `--depth DEPTH` | `metadata` returns the metadata and the abstract when available; `abstract` requires the abstract. forager does not read SSRN full text. | `metadata` |
| `--format FORMAT` | `json`, `markdown`, or `content`; `content` prints the abstract. | `json` |

SSRN download links, look-alike hosts, short links, and `--depth full_text` exit `2` before any
request. A DOI that Crossref does not know is a `parameter` failure (exit `4`) whose message says
the paper was not found in Crossref; `--depth abstract` without an abstract is a `quality` failure
(exit `5`). Hosts match in any letter case, and other query parameters and fragments of an
abstract-page URL are ignored. An empty `platforms.ssrn.order` exits `3`.

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
empty `platforms.scholar.order`; `4` for `auth` (SerpApi rejected the key; the request is sent once and not retried with another
key), `quota_exhausted`
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

Xiaohongshu is reached only through the opt-in `xiaohongshu_browser` route, which searches in a
headless browser that forager drives itself, on the Browser Profile the user logged into with
`forager browser login xiaohongshu`; see "Xiaohongshu browser route" in
[`platforms.md`](platforms.md) and [`browser`](#browser). With the default empty
`platforms.xiaohongshu.order` the command exits `3` before starting any browser and names the three
steps to enable it: `forager browser install`, `forager browser login xiaohongshu`, and adding the
route to the order.

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
Notes without a valid ID or access token are skipped and reported on stderr. A page that
listed notes but kept none of them is a `runtime` failure. A valid empty search
returns `items: []` with exit 0.

Exit codes: `2` before any request for a blank query or `--limit` outside `1..=100`; `3` before
starting any browser for an empty `platforms.xiaohongshu.order`, a Managed Browser that is not
installed (or has no package for this host), or a Browser Profile that was never logged in, each
message naming the command that fixes it; `4` for `auth` (the profile's session is logged out,
Xiaohongshu answered 461 or 471 and wants a verification, or it says the account requests too
often), `parameter` (a security restriction such as `300031` or `300017`), `timeout` (the results
did not arrive before the deadline), and `runtime` (the page or the responses did not match the
request, Xiaohongshu answered an API code forager cannot classify, the browser failed to start, or
the Browser Profile is in use by another forager process).

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

The note opens in the same headless browser and Browser Profile as search. The full text needs no
Web Fetch provider: the route reads it from the note page itself.

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
`rednote.com` link, or another depth; `3` as under search; `4` for `auth` and `timeout` as under
search, `parameter` for a note Xiaohongshu would not open
(`300031` or `300017`: the token may be stale, the note restricted or removed, or the account
rate-limited), and `runtime` (the page showed another note or an unexpected page, or the file
could not be written); `5` for `quality` (the note has no title, text, or images). The access token never appears in
attempt messages, in `--verbose` attempts, or on stderr.

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

The comments are read on the note page in the same headless browser and Browser Profile as fetch.
There is no cursor: a later command cannot continue where this one stopped.

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
`rednote.com` link, or `--limit` or `--replies` out of range; `3` as under search; `4` for
`auth`, `parameter`, and `timeout` as under fetch, and `runtime` (a comment response belonged to
another note, broke the page order, or answered a comment that was not expanded; the page logic
could not click "展开 N 条回复", with the reason; or the page was unexpected).

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
| `--handle HANDLE` | Only posts by this account, with or without `@`; repeat for up to 20. A handle is 1 to 15 letters, digits, or `_`; repeats are dropped in any letter case. | None |
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
`5` (`quality`) when xAI stops the answer at a content filter or the output limit; this holds for
fetch and ask too.

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

`QUERY` is the question; the filters are the same as under search. The question is sent
with this machine's current time in front of it. The model searches X only
and writes an answer with inline citations such as `[[1]](https://x.com/SpaceX/status/<id>)`; the
whole answer is relayed content.

JSON output is `{platform, provider, query, answer, sources}`. Each source is `{ref, url, title}`:
a cited post has its `x:<post_id>` ref and canonical URL, which `platform x fetch` accepts; a
profile or other page has `ref: null` and its own URL. `sources` may be empty; treat an answer
without sources as unverified. The answer passes the same normalization as main search, and
sources are deduplicated by URL. Ask writes no result journal.

Exit codes: `2` before any request for a blank question or invalid filters as under search; `3`
as under search; `4` for the xAI failures under search. An answer that is empty after
normalization is a `runtime` failure (exit `4`).

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
for another video; a caption track outside the configured origin; a caption track that needs a
PO Token), and the failures under search;
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
one that is not the requested JSON, has no language, has a segment with an invalid time, or has
no segment (no file is written); `4` (`runtime`) when the file cannot be
written.

### `platform polymarket search`

```console
forager platform polymarket search [QUERY] [--include-closed] [--limit N] [--cursor CURSOR]
                                   [--timeout SECONDS] [--format json|markdown]
                                   [--output FILE [--receipt]] [--verbose]
```

| Argument or option | Meaning | Default |
| --- | --- | --- |
| `QUERY` | Words to match, sent as written. Required without `--cursor`. | Omitted |
| `--include-closed` | Match closed events too. | Open events only |
| `--limit N` | Events on this page, `1..=20`. | `10` |
| `--cursor CURSOR` | `next_cursor` of a previous page; it restores the whole request, so pass no query, option, or `--limit` with it. | Omitted |

Polymarket runs through the `polymarket_api` route, which needs no key: the Gamma API
(`providers.polymarket_api.url`) searches and describes events and markets, the CLOB API
(`providers.polymarket_api.clob_url`) supplies live order books, and the Data API
(`providers.polymarket_api.data_url`) supplies price histories. JSON output is `{platform,
provider, items, next_cursor}`. Each item is an event: `ref` (`polymarket:<event_slug>`), `url`
(`https://polymarket.com/event/<slug>`), `depth` (`metadata`), `title`, `authors` (empty),
`published` (the start, UTC second), `event_id`, `status` (`open`, `closed`, or `resolved`),
`end_date`, `volume`, `volume_24h`, `liquidity`, `neg_risk`, `market_count`, `price_source`
(always `gamma`), and `top_markets`: its five most probable markets, each `{ref, label,
probability, basis, status}`, with markets without a probability last. Search prices are Gamma's
cache and may lag up to 5 minutes; fetch the event for live ones. Results Polymarket returns in an
unreadable shape are skipped with one stderr diagnostic. No cursor is issued past page 100. A
valid empty search returns `items: []` with exit 0.

Exit codes: `2` before any request for a blank query, `--limit` out of range, or a cursor past
page 100; `3` for an empty `platforms.polymarket.order`; `4` for `rate_limited`, `network`,
`timeout`, and `runtime` for an unexpected response or a page whose every result is unreadable.

### `platform polymarket fetch`

```console
forager platform polymarket fetch REF_OR_URL [--depth metadata|abstract] [--timeout SECONDS]
                                             [--format json|markdown|content]
                                             [--output FILE [--receipt]] [--verbose]
```

| Argument or option | Meaning | Default |
| --- | --- | --- |
| `REF_OR_URL` | `polymarket:<event_slug>`, `polymarket:market/<market_slug>`, or a polymarket.com `/event/<event>`, `/event/<event>/<market>` (the market), or `/market/<market>` URL, with or without a language prefix such as `/zh/`. | Required |
| `--depth DEPTH` | `metadata` returns the status and prices; `abstract` adds the resolution rules. | `metadata` |
| `--format FORMAT` | `json`, `markdown`, or `content`; `content` prints the resolution rules of an `abstract` fetch. | `json` |

JSON output is the item fields of search without `top_markets`, plus `prices_as_of` and
`markets` for an event, one entry per market: `ref`, `url`, `market_id`, `condition_id`,
`question`, `label`, `status` (`open`, `suspended`, `closed`, or `resolved`), `end_date`,
`probability` (the Yes price of a Yes/No market, otherwise `null`), `outcomes` (`[{name, price,
basis, best_bid, best_ask}]`), `spread`, `last_trade_price`, `one_day_change`,
`one_week_change`, `one_month_change`, `volume`, and `liquidity`. A market fetch returns those
market fields with `event_id`, `neg_risk`, `event: {ref, url, title}`, `price_source`, and
`prices_as_of`. An outcome's `basis` says where its `price` comes from: `midpoint` (the live
book), `midpoint_cached` (Gamma's cache), `resolution` (the payout of a resolved market),
`no_two_sided_book` or `not_trading` (no price). `price_source` is `clob` for live quotes, or
`gamma` when the CLOB API was unavailable (with one stderr diagnostic) or no market trades;
`last_trade_price` comes only from the CLOB API, so it is `null` without live quotes.
`prices_as_of` is when the prices were read. `abstract` adds `rules` to the event, and to each
market whose rules differ.

Exit codes: `2` before any request for a numeric ID, a polymarket.us link, another page or host,
or a depth other than the two; `3` for an empty `platforms.polymarket.order`; `4` for
`parameter` (`Polymarket event not found: polymarket:<slug>; slugs can change, search again`, or
the same for a market), the transport failures under search, and `runtime` for a response about
another slug or in an unexpected shape; `5` (`quality`) for `abstract` of an event or market
without rules.

### `platform polymarket history`

```console
forager platform polymarket history MARKET [--outcome NAME]
                                    [--interval 1d|1w|1m|max | --from YYYY-MM-DD --to YYYY-MM-DD]
                                    [--bucket 1m|5m|30m|1h|3h|12h|1d] [--timeout SECONDS]
                                    [--format json|markdown] [--output FILE [--receipt]] [--verbose]
```

| Argument or option | Meaning | Default |
| --- | --- | --- |
| `MARKET` | A market ref or URL, as fetch takes them. An event ref or URL works only when the event holds one market. | Required |
| `--outcome NAME` | The outcome, by name in any letter case. | The first outcome (Yes) |
| `--interval WINDOW` | A window ending now: `1d`, `1w`, `1m` (a month), or `max` (the market's whole life). Not with `--from`/`--to`. | `1m` |
| `--from DATE`, `--to DATE` | An explicit window of UTC days, both included, at most 15 days; pass both. | Omitted |
| `--bucket WIDTH` | The width of one point: `1m` (a minute), `5m`, `30m`, `1h`, `3h`, `12h`, or `1d`. `1m` and `max` take `30m` or wider, `1w` takes `5m` or wider. | `5m` for `1d`, `1h` for `1w`, `1d` for `1m` and `max`; `5m` for a range of up to 2 days, else `1h` |

JSON output is `{platform, provider, market: {ref, url, question, status}, outcome, interval,
bucket_seconds, points, settlement}`; an explicit range has `interval: null` with `from` and
`to`. `points` is `[{t, price}]`, oldest first, `t` a UTC second. A resolved market's payout is
`settlement: {t, price}` and never a point; `settlement` is `null` before resolution, or when the
window ends before the payout. A bucket finer than the Data API keeps for that window returns
`points: []` with exit 0. A series longer than five pages is cut, with a stderr diagnostic to
pass a larger `--bucket`.

Exit codes: `2` before any request for an unrecognized input, a window and a range together, only
one end of a range, a range over 15 days or ending before it starts, or a bucket finer than the
interval allows; `3` for an empty `platforms.polymarket.order`; `4` for `parameter` (an event with
several markets, with up to 10 of their refs and labels listed on stderr to pick from; an unknown
outcome, whose message lists the outcomes; a market not found; or a Data API refusal) and the
transport failures under search.

### `platform reddit search`

```console
forager platform reddit search QUERY [--subreddit NAME] [--sort relevance|hot|top|new|comments]
                               [--time hour|day|week|month|year|all] [--limit N]
                               [--timeout SECONDS] [--format json|markdown]
                               [--output FILE [--receipt]] [--verbose]
```

| Argument or option | Meaning | Default |
| --- | --- | --- |
| `QUERY` | Words to search for, in Reddit's own search syntax. | Required |
| `--subreddit NAME` | Only posts in this subreddit (1 to 21 letters, digits, or `_`), with or without `r/`. | All of Reddit |
| `--sort ORDER` | `relevance`, `hot`, `top`, `new`, or `comments` (most comments first). | `relevance` |
| `--time WINDOW` | `hour`, `day`, `week`, `month`, `year`, or `all`; only with `relevance`, `top`, or `comments`. | Omitted |
| `--limit N` | Results, `1..=25`; the first screen lists about seven posts, and the command scrolls for more. It cannot continue later. | `10` |

Reddit search runs through one opt-in route, `reddit_browser`. It opens Reddit's search page in
forager's headless browser, logged into the user's Reddit account (`forager browser install`,
then `forager browser login reddit`, then adding `reddit_browser` to `platforms.reddit.order`); it
runs at most one browser command every 10 seconds, scrolls the page while it lists fewer than
`--limit` posts (at most six scrolls), and issues no cursor (`next_cursor` is always `null`). When
it read fewer than `--limit` and Reddit has more, it says so on stderr. JSON output is `{platform, provider, items, next_cursor}`. Each item has
`ref` (`reddit:<id>`), `url` (`https://www.reddit.com/comments/<id>/`), `depth` (`snippet` when
Reddit showed an excerpt, otherwise `metadata`), `title`, `authors`, `published` (UTC second),
`subreddit`, `permalink`, `post_type` (`null` in search), `link_url`, `score`, `comments`,
`nsfw`, `spoiler`, `snippet`, and `snippet_source` (`post` or `comment`). A valid empty search
returns `items: []` with exit 0.

Exit codes: `2` before any request for a missing or blank query, an invalid subreddit name,
`--time` with `hot` or `new`, or `--limit` out of range; `3` for an empty
`platforms.reddit.order` (the default), with the steps and the risk of enabling the route, or when
the browser is missing or the Reddit Browser Profile was never logged in, with the command that
fixes it; `4` for `auth` (a verification page, HTTP 403 or 429, or a logged-out session, with
`forager browser login reddit` to fix it), `network`, `timeout`, and `runtime` for a page whose
format changed or that Reddit answered with a server error (HTTP 5xx; retry later).

### `platform reddit fetch`

```console
forager platform reddit fetch REF_OR_URL [--depth thread|metadata|full_text] [--content-dir DIR]
                                         [--timeout SECONDS] [--format json|markdown|content]
                                         [--output FILE [--receipt]] [--verbose]
```

| Argument or option | Meaning | Default |
| --- | --- | --- |
| `REF_OR_URL` | `reddit:<id>`, `reddit:t3_<id>`, a reddit.com post URL (`www`, `old`, `new`, `np`, or `m`), or a `redd.it/<id>` short link. | Required |
| `--depth DEPTH` | `thread` writes the post and the comments its page loads to a Markdown file; `metadata` returns the post with its own text; `full_text` reads the page a link post points to, or the post's own title and text. | `thread` |
| `--content-dir DIR` | Directory for the discussion or full-text Markdown file, named `reddit-<id>.md`. | New directory under the system temp directory |
| `--format FORMAT` | `json`, `markdown`, or `content`; `content` prints the discussion or the full text and writes no file. | `json` |

JSON output is the item fields as under search, with `post_type` (Reddit's own type, such as
`text`, `link`, `image`, `video`, `gallery`, `multi_media`, or `crosspost`), `spoiler`,
`snippet`, and `snippet_source` `null`, plus `text` (the post's own body as Markdown). At
`thread` depth it adds `content_url`, `content_provider` (the route), `content_path`,
`content_len`, `comments_delivered` (comments in the file), `has_more_comments`, and `order`:
`top`, `best`, `new`, `controversial`, `old`, or `qa`, the comment order Reddit applied
(forager asks for `top` and reports what the page says).
The file starts with the post's title, ref, permalink, subreddit, author, link, score, comment
count, time, and text, then the first batch of comments the page loaded with the post, with a line
naming their order, indented by depth, each with its author, score, time, and permalink. A deleted
comment keeps its place as `*[deleted or removed]*`. The route opens the post page once in the
logged-in browser and reads the comments it loaded with the post; it never scrolls for more or
expands replies. `full_text` needs a configured Web Fetch provider for a link post; a text post's
body comes from the route itself.

Exit codes: `2` before any request for a comment permalink (the message names the post's ref), a
share link (`/s/…`), another URL, or a depth other than the three; `3` for an empty
`platforms.reddit.order`, or `full_text` with no configured Web Fetch provider; `4` for
`parameter` (`Reddit post not found: reddit:<id>`, or a post the route cannot see because it is
removed, private, or restricted, or hidden by the account's adult-content setting) and the failures under search; `5` (`quality`) for `full_text` of a post
with neither text nor a link, such as an image or gallery post, or of a post type forager does
not know, such as `crosspost`.

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
| `config list` | Print the effective JSON view: each leaf as `{value, source}`, and each credential list as its masked values with `source`, `configured`, and `key_count`. When parsing fails, report the path, bad key, and location. |
| `config set KEY VALUE` | Set one schema key. Strings, numbers, and booleans are plain text, such as `https://api.exa.ai` or `45`; arrays are TOML literals, such as `'["KEY"]'`. A string wrapped in one pair of matching quotes fails with exit `2` instead of saving the quotes. Argument values may remain in shell history. Invalid paths, types, or enum values fail without writing. |
| `config set KEY -` | Read the complete value from stdin; use this form for secrets. |
| `config unset KEY` | Remove only the file-layer value. An environment override can remain effective. |

Use dotted schema paths such as `providers.exa.timeout`. `config set` and `config unset` edit the
document layer and remain available for repairing a schema-invalid but syntactically parseable
file. Edit the path printed by `config path` when the TOML syntax itself is damaged.

`config set KEY -` reads all of stdin, strips exactly one trailing LF or CRLF, and accepts empty
input as an empty string. `config unset` says so when an environment variable still sets the key.

The configuration file is `config.toml` in `$XDG_CONFIG_HOME/forager`, or `~/.config/forager`;
`FORAGER_CONFIG_DIR` replaces that directory. When the default directory cannot be resolved or
written, commands fail and ask for `FORAGER_CONFIG_DIR`. The directory is kept at mode `0700` and
the file at `0600`. Every key may be omitted to use its built-in default; `config list` shows the
effective value and source of each, and `setup --non-interactive` writes a template that comments
every key. An unknown file key exits 3 and names the key.

The environment overrides the file, which overrides the defaults. Each key's variable is
`FORAGER_` plus the dotted path in upper case with `__` between levels, such as
`FORAGER_PROVIDERS__XAI__MODEL`. A value is parsed as the key's type, so a string is taken
verbatim, and an array is a TOML literal such as `FORAGER_PROVIDERS__EXA__KEYS='["sk-a","sk-b"]'`.
An unknown `FORAGER_*` variable exits 3. Provider-native variables such as `EXA_API_KEY` are not
read.

- Credentials: every provider that takes credentials has a `keys` array, and several keys form a
  pool that forager rotates through. A provider that needs no credentials has no `keys` key;
  writing one is an unknown key (exit 3 in the file, exit 2 from `config set`). An empty `keys`
  leaves its provider unconfigured, except that `jina` then runs anonymously.
- `capabilities.<capability>.order` lists the providers that may serve `web_search`, `web_fetch`,
  or `docs_search`, in fallback order. The list is complete: a provider left out is never used,
  one without keys is skipped, and a provider that cannot serve the capability is a configuration
  error. `web_search` and `docs_search` may be empty, which leaves them unserved and reported in
  `capability_gaps`; an empty `web_fetch` order is a configuration error.
- `platforms.<id>.order` lists the platform's routes; empty disables the platform. Platform
  fetches that need a page body read it through `capabilities.web_fetch.order`.
- `journal`: `enabled` (default `true`), `dir` (default `$XDG_STATE_HOME/forager/journal`, else
  `~/.local/state/forager/journal`), and `retention_days` (default `30`; `0` keeps files forever).
  Each search run writes its own file, whose path is `journal_ref`; retention deletes
  only expired files that forager named.
- `retry`: `max_attempts` per request (at least 1), backoff `multiplier` (above 0), and `max_wait`
  seconds between attempts.
- `http.ssl_verify` turns TLS certificate verification off when `false`.
- `providers.jina.respond_with` sets Jina's `X-Respond-With` header, such as `readerlm-v2`.
- `log.level` is `error`, `warn`, `info` (default), `debug`, or `trace`. Only `debug` and `trace`
  add output. Terminal errors, capability gaps, and journal and `--output` warnings always reach
  stderr.
- Every `timeout` is a positive number of seconds.
- A leading `~/` in `journal.dir` or `browser.path` expands to `HOME`, and a relative path is
  relative to the configuration directory.

### `setup`

```console
forager setup [--non-interactive] [--lang zh|en]
```

| Option | Meaning | Default |
| --- | --- | --- |
| `--non-interactive` | Create a complete commented configuration template without prompting. Refuse to overwrite an existing target. | Off |
| `--lang LANG` | Use `zh` or `en` for the interactive setup prompts. | Prompt/default locale |

After setup, use `forager doctor` to check the resulting configuration.

Interactive setup has four steps: the language; the main model (`xai` URL, keys, and model); keys
for `exa`, `jina`, `tavily`, and `firecrawl`; and keys for the `serpapi` and
`youtube_api` routes. Enter keeps the current value, so a second
run updates the existing file. Setup asks only for credentials and model settings and sends no
request; set other keys with `config set`.

### `browser`

```console
forager browser install [--timeout SECONDS]
forager browser login PLATFORM [--timeout SECONDS]
```

| Command | Behavior |
| --- | --- |
| `browser install` | Download the Managed Browser this forager pins (fingerprint-chromium), check its SHA256, and install it into forager's cache. When the pinned version is already installed, download nothing. Prints `{version, executable, downloaded}`. `--timeout` defaults to `600`. |
| `browser login PLATFORM` | Open a browser window on the platform's login page and wait until the user has logged in there, then close it and mark the platform's Browser Profile as logged in. Prints `{platform, logged_in: true, profile}`. Only `xiaohongshu` and `reddit` have a login. `--timeout` is at least `10` and defaults to `600`. |

Browser routes need both: the Managed Browser, and for a route that needs a login, a Browser
Profile that has been logged in once. The profile keeps the session; forager never reads or exports
its cookies. Only macOS on Apple silicon has a Managed Browser package. A non-empty `browser.path`
(`FORAGER_BROWSER__PATH`) names a local browser executable that every browser command starts
instead; `browser install` still installs, and says on stderr that `browser.path` is in use.
Progress goes to stderr.

forager's cache is `$XDG_CACHE_HOME/forager`, or `~/.cache/forager`. `browser login` counts
against its route's request window like a platform command, so a platform command right after a
login can wait for that window.

Exit codes: `2` for a platform without a browser login; `3` when `browser login` finds no browser
(the message names `forager browser install` or `browser.path`); `4` with `{error_kind, message}`
on stdout for `network` (the download failed), `timeout` (the download or the login did not finish
in time), and `runtime` (the SHA256 did not match and the download was deleted, the package could
not be unpacked, this host has no package, the browser failed to start or its window closed before
the login, or the Browser Profile is in use by another forager process). A failed CDP handshake
with the started browser is also `runtime`.

### `doctor`

```console
forager doctor [--timeout SECONDS] [--format json|markdown]
forager doctor --offline [--format json|markdown]
forager doctor --provider PROVIDER [--timeout SECONDS] [--format json|markdown]
forager doctor --live [--tier TIER[,TIER...]] [--provider PROVIDER] [--platform PLATFORM]
               [--unattended] [--timeout SECONDS] [--format json|markdown]
forager doctor --live --list [--tier TIER[,TIER...]] [--provider PROVIDER]
               [--platform PLATFORM] [--unattended] [--format json|markdown]
```

| Option | Meaning | Default |
| --- | --- | --- |
| `--provider PROVIDER` | Check one of `xai`, `openai`, `tavily`, `firecrawl`, `jina`, `exa`, `arxiv_api`, `ssrn_crossref`, `serpapi`, `xiaohongshu_browser`, `reddit_browser`, `hn_api`, `polymarket_api`, `youtube_api`, `youtube_web`, or `gemini_api` without a billed request: its configuration, its prerequisites and, where the provider offers one, its free account check. Without it, run the shallow all-provider report. With `--live`, run only that provider's Live Checks. | Omitted |
| `--offline` | Run the shallow report's checks that send no request. Conflicts with `--provider`, `--live`, and `--timeout`. | Off |
| `--live` | Run Live Checks against the real services instead of the report. | Off |
| `--tier TIER[,TIER...]` | Run the Live Checks of these Live Tiers: `free`, `metered`, `attended`. Requires `--live`. | `free,metered`; all three with `--provider` |
| `--platform PLATFORM` | Run only the Live Checks of this platform's operations. Requires `--live`. | Omitted |
| `--list` | List the Live Checks and whether this selection runs them, without running any. Requires `--live`. | Off |
| `--unattended` | Mark the live run as one no person watches: refuse the `attended` tier and leave out expensive and manual-only checks. Requires `--live`. | Off |
| `--timeout SECONDS` | Set the diagnostic deadline; with `--live`, the time limit of the whole run. | `30`; `420` with `--live` |
| `--format FORMAT` | Use `json` or `markdown`. | `json` |

`doctor` has four modes. The default shallow report checks configuration, permissions, state
directories, the browser preflight check and the reachability of every configured provider.
`--offline` runs the same checks without any request. `--provider` checks one provider without a
billed request. `--live` runs Live Checks, which send real requests and are described below.

Use `doctor` for credentials, connectivity, and effective configuration inspection. In shallow
mode, `ok` covers every configured provider: if any configured provider is unreachable, the
top-level result is false and the command uses exit code 4. A browser route
(`xiaohongshu_browser`, `reddit_browser`) is checked only when its platform order lists it: doctor
checks, without starting a browser, that the browser can be found and the Browser Profile has been
logged in, and a failed check carries a `message` naming the command that fixes it. Without a key,
`jina` is configured and the shallow report notes that it runs anonymously.

`--provider PROVIDER` reports `mode: "provider"` with `provider`, `configured`, `key_count`,
`source`, `deadline_seconds`, and `checks`. It checks the provider's configuration and
prerequisites: a route that no default order lists needs a platform order that lists it, a browser
route needs the preflight check above to pass, and a provider that requires keys needs one. A
missing prerequisite fails with `config` and exit 3 before sending anything, and `message` names
the fix. It sends no billed request and opens no page, in any run: a provider without an account
check gets no request at all, and its `checks` is empty. `--provider serpapi` costs nothing: it
asks SerpApi's account endpoint about every configured key, recorded as the `account` check, and
adds a `keys` array, one entry per key with `key_index`, `ok`, `searches_left`,
`plan_searches_left`, `this_month_usage`, `this_hour_searches`, and `hourly_limit`, plus
`error_kind` for a failing key (`auth`, `quota_exhausted` with no searches left, `rate_limited` at
the hourly limit). Any failing key fails the check with exit 4, and `message` names it as
`providers.serpapi.keys[N]`. `--provider gemini_api` calls no model: it lists the models once with
every key, recorded as the `models` check, and its `keys` entries have only `key_index`, `ok`, and
`error_kind` for a failing key, with the failing key named as `providers.gemini_api.keys[N]`.
Neither account check proves that an operation's requests decode; `doctor --live` does. Do not use
doctor as the recovery path for configuration that cannot be loaded.

The shallow report also has a `config` block shaped like `config list`, `permission_warnings`
when the configuration directory or file is more open than `0700` or `0600` (on Windows, when it
grants access to anyone but its owner), and `state_directories`. `state_directories.state` is
forager's state directory, `$XDG_STATE_HOME/forager` or `~/.local/state/forager`, and
`state_directories.journal` is `journal.dir`, checked only while `journal.enabled` is true. Each
reports `writable`, and a `message` when forager cannot create the directory privately or write a
file in it. Warnings never change `ok`; an unwritable state directory makes it false with exit 4.
`ok` is true when nothing is configured. A platform route that no default order lists
reports `configured: false` until a platform order lists it. For a provider with a request window,
both the shallow check and the account check wait for that window; when it does not open in time,
the shallow report marks the provider unreachable and the account check fails with `timeout`, and
neither sends a request.

`--offline` sends no request. Its report has `mode: "offline"` and the same `config`,
`permission_warnings` and `state_directories`; its provider entries report configuration and key
presence without `reachable`, and an enabled browser route adds `preflight` with the same
preflight check, plus its `message` when the check fails. Its `ok` is true when the state
directories are writable, whatever is configured, including no main search; otherwise it exits 4.
Use it to check configuration, permissions and state directories without a network.

#### Live Checks

`doctor --live` runs Live Checks: each one runs a forager command against the real service, held
to the route it checks whatever the configured orders say, and judges the output against the
fields forager reads. Its requests are real: they can spend quota, and a check in the `attended`
tier opens pages of a logged-in site or puts the egress address at risk. Each check has an `id`
of the form `<provider>:<operation>` or `<provider>:<operation>:<variant>`, and one check of the
whole search pipeline, `pipeline:search`, belongs to no provider.

Each check belongs to one Live Tier, the highest of the routes it can reach:

| Tier | Meaning |
| --- | --- |
| `free` | Needs no credential and carries no account or egress risk. |
| `metered` | Needs a credential or spends quota, billed or not. |
| `attended` | Puts an account or the egress address at risk, such as a browser route on a logged-in site; runs only with a person present. |

Selection:

- Without `--tier`, a run selects `free` and `metered`, never `attended`, unless `--provider` names
  a provider: then it selects all three tiers of that provider.
- Selecting the `attended` tier, or naming a provider that has attended checks, is the user's
  consent to run them; there is no other consent option. `--platform` alone is not consent, so it
  runs none of the platform's attended checks.
- `--provider` and `--platform` narrow the selection. A check that reads another check's output,
  shown as `depends_on` in the listing, also selects that check.
- A check runs its route even when the platform's order leaves the route out. A check whose route
  lacks a prerequisite on this machine (the Managed Browser, a logged-in Browser Profile, or keys)
  does not run: it is `unconfigured`, and its `message` names the missing prerequisite and how to
  meet it.
- `--unattended` marks a run that no person watches, such as a scheduled job. Such a run refuses
  the `attended` tier, whether selected with `--tier` or reached by naming an attended provider,
  with exit `2` and an `argument_error:` line on stderr before any request, and it leaves out every
  expensive and manual-only check. A run whose environment sets `CI` to a non-empty value other
  than `false` behaves as if `--unattended` were given.

`--list` prints `{mode: "live_list", checks}` and runs nothing. Each entry has `id`, `provider`
(absent for the pipeline check), `operation`, `platform` (for a platform operation), `tier`,
`depends_on` (when it reads another check's output), `time_limit_seconds`, `expensive`,
`manual_only`, `hard_sample`, which says why the check's sample is hard for its route, and
`selected`, which says whether this selection would run it. An `expensive` check
spends a notable share of a quota in one run. A `manual_only` check uses a service in a way not
confirmed to comply with the provider's terms, so only a person starts it.

A run prints `{mode: "live", ok, unattended, tiers, summary, checks}`, plus `record_warning` when
the run record could not be written. `tiers` lists the tiers the selection used. `summary` counts
`passed`, `failed`, `unconfigured`, `not_started`, and `unselected`, the declared checks this
selection does not run. `checks` has one entry per selected check with `id`, `provider`,
`operation`, `platform`, `tier`, `status`, `attempts` (every command the check started), and, when
they apply, `cause`, `duration_ms` (how long its last run took), `rerun`, `error_kind`,
`http_status` (from the last provider attempt that reported one), `detail`, `message`,
`transition`, and `failing_since`. `status` is one of:

- `passed`: the command succeeded and its output met the check's contract.
- `failed`: the command failed, with the command's own `error_kind` and `message`, or its output
  broke the contract, with `error_kind: "contract"`.
- `unconfigured`: a prerequisite is missing, as above. It is neither a pass nor a failure.
- `not_started`: the check did not begin, and `cause` says why:
  - `deadline`: the run's `--timeout` ended first. This counts as a failure; rerun with a larger
    `--timeout`.
  - `site_stop`: an earlier check on the same site met a stop signal, so the run opened no more
    pages there. On Xiaohongshu the signals are an `auth`, `parameter` or `runtime` failure,
    which includes a verification page and an answer the route cannot classify, and two timeouts
    in a row; on Reddit, an `auth` failure and two timeouts in a row. Do not rerun: another run
    only triggers the block again. The user logs in with `forager browser login <platform>` and
    handles any check in the window.
  - `no_input`: the check it depends on did not pass or returned nothing it can read, such as a
    search that found nothing to fetch. Look at that check; a larger `--timeout` does not help.

A check's contract names every field its route decodes from the upstream response, with its type
and a lower bound (a non-blank string, a count or number of at least some value, a list of at
least some items), and never an exact value; a field that the upstream gives only some items must
appear in at least one. Some checks add a rule about their sample, such as a resolved market priced
by its payout. A failed check's `detail` says what broke: `rule`, and for a contract failure
`path` (the field as the contract names it), `at` (the item, such as `items[2].title`, when the
path steps into a list), and `observed`, which holds only the type of what was there with its
`length`, `count` or `fields`, or how many items had the field (`with_field` of `count`), never a
value of the output. The contract rules are `present`, `type`, `nonblank`, `at_least`,
`min_items`, `in_some_items`, `no_runtime_attempt`, `keyed_read`, `anonymous_read`,
`resolved_by_payout`, and `live_quotes`; a failure of the command itself is `command_succeeds`,
`time_limit`, `valid_json`, or `command_runs`. `message` says the same in words and, for a failed
command, quotes its error; only the report shows it.

Each check runs forager as a separate process with its own time limit, without that command's own
retries and without writing the Search Result Journal. A check on an HTTP route that fails with
`network` or `timeout` is run once more; any other failure, and any failure of a browser route's
check, is final. In an unattended run, every check that still failed, except with `auth` or
`parameter`, runs once more after a delay of one minute, shared by all of them; its result is the
second run's, with `rerun: true`, so a check that passes its rerun is `passed` and a failure counts
only when both runs failed. An `auth` or `parameter` failure is confirmed without a rerun, and a
run that is not unattended reruns nothing.

Every live run, attended or not, is recorded in `live_runs/` under forager's state directory
(`$XDG_STATE_HOME/forager` or `~/.local/state/forager`), one private file per run, and records older
than 90 days are removed. A record holds the run's identifier, time, forager version, tiers,
whether it was `scheduled` (true exactly when the run was unattended), the summary, and per check
its `status`, `cause`, `attempts`, `rerun`, `duration_ms`, `error_kind`, `http_status`, and
`detail`; it never holds a `message`, so no key, query, URL or content reaches it. A check that ran
(passed or failed) reports its `transition` against the most recent scheduled record in which it
ran: `newly_failed` when it failed after a pass or with no earlier run, `still_failing` with
`failing_since`, the time of the first scheduled failure in its unbroken run of failures, or
`recovered` when it passed after a failure. A check that passed as before has no `transition`. A
run that is not scheduled reports its transitions but never serves as a baseline, so a run by hand
changes no later transition.

A live run exits `0` when at least one check ran and every check that ran passed, so unconfigured
and unselected checks do not fail it; `ok` is true exactly then. It exits `4` when a check failed
(in an unattended run, failed its rerun too, or failed with `auth` or `parameter`) or the time
limit left a check unstarted, `3` when none of the selected checks ran or the
configuration cannot be loaded, and `2` when an unattended or CI run asks for the `attended` tier.
`--list` exits `0`, or `2` for that same refusal.

## Exit codes

| Code | Meaning |
| --- | --- |
| `0` | Success, including a valid empty result from a provider-direct command. |
| `2` | Argument or caller-input error, including an invalid `config set` path/value. |
| `3` | Configuration, stdin, or requested output-write error. |
| `4` | Transport/runtime terminal failure, including a hard timeout. |
| `5` | Content-quality or evidence terminal failure. |

Exit code `1` is intentionally unused. Panic status `101` is outside the CLI contract. After a
network command reaches a rendered terminal result, JSON output is emitted as clean JSON on stdout
and diagnostics or logs use stderr. A failed command renders its failure payload on stdout too, so
an empty stderr does not mean success; judge the outcome by the exit code. After successful Clap
parsing selects JSON, `search` and `fetch` report configuration and runtime preflight failures in
the parseable stdout error object described above. Clap errors, panics, and non-JSON formats retain
their existing channels.

Exit `2` covers only input that forager rejects before sending a request; when an upstream
service rejects a request, the resulting `parameter` failure exits `4`. Exit `3` also covers an
unknown file key or `FORAGER_*` variable, an empty `search.backends` or
`capabilities.web_fetch.order`, a platform order with no configured route for the command, and a
missing credential that the command needs. Exit `4` is the transport family (`auth`,
`rate_limited`, `quota_exhausted`, `parameter`, `network`, `timeout`, `runtime`), and exit `5` the
content family (`quality`, `evidence`). An empty result is never an error: a direct command with
no matches succeeds.
