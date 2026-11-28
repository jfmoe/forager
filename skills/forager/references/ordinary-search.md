# Ordinary search

Declare the supplemental capabilities the task needs on every search; forager adds none you leave
out, and omitting `--capabilities` runs main search alone.

| Value | Declare when the answer needs |
| --- | --- |
| `docs_search` | Official documentation of a library, SDK, API or framework: references, configuration, integration steps |
| `web_search` | Recent or live information, a locale, current policy, news, market data, or extra sources to cross-check |
| `web_fetch` | The contents of a URL or PDF the request already gives |
| `none` | Nothing beyond main search's own answer |

Join several values with commas, such as `docs_search,web_search`. `web_fetch` covers only a URL
already in the request; fetch the URLs a search returns with `forager fetch` afterwards. Main
search already reads X; for X posts as items, use the X platform.

```console
forager search "QUERY" --capabilities CAPABILITIES --format json
```

`--extra-sources N` changes how many supplemental candidates are gathered; leave it out unless the
task needs more or fewer.

This step is done when the command has exited with a declaration chosen from the table.

## Read the result

- `answer` is main search's answer; `sources` are the sources it cites. Empty `sources` mark an
  uncited answer: treat each of its claims as unverified until fetched content supports it.
- `extra_sources` are further candidates, each with `provider`, `capability`, `title`, `url`,
  `summary`; a summary is the provider's text, not evidence. Fetch a candidate's `url` before you
  cite it.
- Tell the user about every `capability_gaps` entry and what it leaves uncovered.
- For a high-risk, time-sensitive or source-backed claim, fetch the key URLs and answer from their
  content.
- To keep a long result out of context, add `--output FILE --receipt` and read the file in parts.

## Recover

A `timeout` or `network` failure enters this chain; an `auth`, configuration or quota failure goes
to [`diagnostics.md`](diagnostics.md) instead.

1. Retry once with the same query and declaration, with `--timeout 300` after a timeout. Skip the
   retry when the user puts speed first.
2. Still failing: take candidates from the failure's `extra_sources`, kept without their
   summaries; when it has none, run `forager exa search QUERY --include-domains CSV` once,
   restricted to known authoritative domains.
3. Fetch at most two of the strongest candidates with `forager fetch 'URL' --format json`.

Run each step only when the previous one failed. Answer from fetched content alone and say it came
from this fallback chain and how many rounds it took; when the chain ends without evidence, report
the steps taken and the failure.

Ordinary search is complete when the search has exited successfully, every capability gap is
disclosed, and every claim that needs a source is backed by fetched content or marked unverified.
