# Direct retrieval

For a known URL or PDF, run `forager fetch 'URL' --format json`. For site structure discovery, run
`forager map 'URL' --instructions "GOAL" --format json`. Single-quote every URL so the shell passes
`?` and `&` through unchanged.

For a long page, run `forager fetch 'URL' --format content --output FILE --receipt`, list its
headings with `grep -n '^#' FILE`, and read only the relevant line ranges.

`fetch` extracts readable page text. For a JSON API endpoint such as `api.github.com`, use `gh api`
or `curl` directly.

For an x.com or twitter.com post, use `forager platform x fetch 'URL'`; for a
news.ycombinator.com item, use `forager platform hn fetch 'URL'`, which delivers the whole
discussion; for a YouTube video, use `forager platform youtube fetch 'URL' --depth abstract`
when a YouTube Data API key is configured, or without `--depth` for its captions when the user
has enabled the `youtube_web` route; see [`platforms.md`](platforms.md). When another URL requires authentication and cannot be fetched
directly, use an available authenticated client to retrieve it.

Commands under `exa` bypass capability routing. Use them when the user
requests that provider or the operation exists only as a direct command. Restrict
`exa search` to known domains with `--include-domains CSV` rather than `site:` in the query. With
`exa search --include-text`, each result's text is capped by `--text-max-characters` (default
3000); prefer `--include-highlights` when snippets suffice.

Complete direct retrieval when it returns the requested content or page set. Route an observed
authentication, configuration, or terminal provider failure to Diagnose or configure in
`SKILL.md`.
