# Ordinary search

Read [`capability-vocabulary.json`](capability-vocabulary.json), then declare the complete
supplemental capability set. Preserve the vocabulary order and use `none` when main search alone is
sufficient.

Declare `web_fetch` only when the request already supplies or identifies a concrete HTTP(S) URL or
PDF for this search command. Intending to fetch URLs discovered after search does not add
`web_fetch` to the initial declaration; run those later fetches explicitly.

```console
forager search "QUERY" --capabilities CAPABILITIES --format json
```

Set `--extra-sources` only to control supplemental breadth. Its public range is `0..=20`: `0` uses
the selected branch's local default, where Web Search uses 3 and Documentation Search and Vertical
Search use 1. An explicit value from `1..=20` is the exact target.

## Consume the result

- `answer` is the main-search answer.
- `sources` contains only Primary Search Sources attributed to the main answer.
- `extra_sources` contains every non-primary Search Candidate, including Vertical Discovery
  results. Each candidate has stable `provider`, `capability`, `title`, `url`, `summary`, and
  `provider_data` fields; `title`, `url`, and `summary` can be null, while `provider_data` carries
  provider-specific selection data. A provider-native summary is not verified evidence.
- Fetch a candidate with an HTTP(S) `url` before using it as evidence. A Context7 Documentation
  Candidate instead carries a typed library locator in `provider_data`; consume it through
  Documentation Search or the Research Evidence Pipeline, never by fabricating a URL or passing it
  to Web Fetch.
- Disclose every `capability_gaps` entry and its effect on coverage.

For high-risk, time-sensitive, or source-backed claims, fetch the key URLs and limit the answer to
their content. Use `--output` when a long or multi-source result must persist beyond stdout.

## Bounded recovery

Only a terminal `timeout` or `network` error enters this recovery chain. An authentication,
configuration, or quota error goes directly to Diagnose or configure in `SKILL.md`.

1. Retry at most once with the same query and capability declaration. Use `--timeout 300` for a
   timeout; retain the normal timeout for a network error. Skip this retry when the user explicitly
   prioritizes speed.
2. If search remains unavailable, run `forager exa search` at most once. Add
   `--include-domains CSV` when authoritative domains are known.
3. Fetch at most two URLs from the strongest results with `forager fetch URL --format json`.

Each step runs only after the preceding step produces its continue signal. An answer from this
chain cites only fetched content and discloses `source_mode: fallback` plus the actual rounds used.
When the chain stops without evidence, report the steps completed and the observed failure.

Complete ordinary search when the command has reached terminal success, every capability gap is
disclosed, and every claim requiring source evidence is supported by fetched content or marked
unverified.
