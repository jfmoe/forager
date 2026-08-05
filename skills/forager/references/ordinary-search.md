# Ordinary search

Read [`capability-vocabulary.json`](capability-vocabulary.json), then declare the complete
supplemental capability set. Preserve the vocabulary order and use `none` when main search alone is
sufficient.

```console
forager search "QUERY" --capabilities CAPABILITIES --format json
```

Set `--extra-sources` only to control supplemental breadth. A selected supplemental capability has
an effective minimum of one result, so `0` and `1` currently match; use `1` to `3` for normal
coverage and increase it only for an explicitly broad request.

## Consume the result

- `answer` is the main-search answer.
- `sources` contains only Primary Search Sources attributed to the main answer.
- `extra_sources` contains only Supplemental Search Candidates. Use each candidate's provider,
  optional title, and provider-native summary to select a small number of URLs for claim-level
  evidence. Provider-native summaries remain complete; only a preview derived from a full
  search-side Web Fetch uses the 300-character rule. A candidate becomes evidence after its content
  is fetched.
- `vertical_results` contains complete Vertical Discovery results. Its records stay out of
  `sources` and `extra_sources`, including records with URLs.
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
