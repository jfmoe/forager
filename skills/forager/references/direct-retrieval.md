# Direct retrieval

Single-quote every URL so the shell passes `?` and `&` through unchanged.

- **A known page or PDF**: `forager fetch 'URL' --format json`.
- **A long page**: `forager fetch 'URL' --format content --output FILE --receipt`, list its
  headings with `grep -n '^#' FILE`, and read only the line ranges you need.
- **A site's structure**: `forager map 'URL' --instructions "GOAL" --format json`.
- **A URL of a platform in the entry's table**: use that platform's fetch instead, which returns
  the platform's ref and fields; for example an x.com post goes to `forager platform x fetch`, a
  news.ycombinator.com item to `forager platform hn fetch`, a YouTube video to `forager platform
  youtube fetch`. Read the platform's file first. Any other site, a WeChat article included, stays
  with `forager fetch`.
- **A JSON API endpoint** such as `api.github.com`: `fetch` extracts readable page text, so call
  the API with `gh api` or `curl`.
- **A page that needs the user's login**: forager cannot read it; use an authenticated client
  the user has, or ask the user for the content.

Commands under `exa` call Exa directly, without capability routing or fallback; use them when the
user asks for Exa or the operation exists only there. `forager exa similar 'URL'` finds pages like
a given one. Restrict `exa search` to known domains with `--include-domains CSV` rather than
`site:` in the query. With `--include-text`, each result's text is capped by
`--text-max-characters`; prefer `--include-highlights` when snippets suffice.

Direct retrieval is complete when it returns the requested content or page set. Take an
authentication, configuration or quota failure to [`diagnostics.md`](diagnostics.md), and a
result that looks wrong to [`feedback.md`](feedback.md).
