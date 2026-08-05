# Ordinary search

Read [`capability-vocabulary.json`](capability-vocabulary.json), then declare the complete
supplemental capability set. Preserve the vocabulary order and use `none` when main search alone is
sufficient.

```console
forager search "QUERY" --capabilities CAPABILITIES --format json
```

Use the Direct retrieval branch in `SKILL.md` when a known URL or PDF supplies the required
content. In ordinary search, `web_fetch` currently contributes URL validation rather than content
to `answer`.

Set `--extra-sources` only to control supplemental breadth. A selected supplemental capability has
an effective minimum of one result, so `0` and `1` currently match; use `1` to `3` for normal
coverage and increase it only for an explicitly broad request.

## Results

- `answer` is the main-search answer.
- `sources` contains only Primary Search Sources attributed to the main answer.
- `extra_sources` contains only Supplemental Search Candidates. Use each candidate's provider,
  optional title, and provider-native summary to choose a small number of URLs to fetch as
  claim-level evidence. A candidate is not evidence before its content is fetched.
- `vertical_results` without URLs are structured discovery results, not sources or evidence.
- Disclose every `capability_gaps` entry and its effect on coverage.

For high-risk, time-sensitive, or source-backed claims, fetch the key URLs and limit the answer to
what their content supports. Use `--output` only when a long or multi-source result must persist
beyond stdout.

## Recovery

After a terminal `timeout` or `network` error, make at most one additional attempt when the user's
time budget allows:

```console
forager search "QUERY" --capabilities CAPABILITIES --timeout 300 --format json
```

If it still fails, run `forager exa search "QUERY" --num-results 5 --format json`, adding
`--include-domains CSV` when authoritative domains are known. Fetch the strongest one or two
results with `forager fetch URL --format json`.

Answer only from fetched content, preserve its URLs, and disclose `source_mode: fallback`. Report
the observed Exa or fetch failure when this path cannot produce evidence.

Complete ordinary search when the command has reached terminal success, every capability gap is
disclosed, and every claim requiring source evidence is supported by fetched content or marked
unverified.
