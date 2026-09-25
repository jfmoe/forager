# Direct retrieval

For a known URL or PDF, run `forager fetch 'URL' --format json`. For site structure discovery, run
`forager map 'URL' --instructions "GOAL" --format json`. Single-quote every URL so the shell passes
`?` and `&` through unchanged.

For a long page, run `forager fetch 'URL' --format content --output FILE --receipt`, list its
headings with `grep -n '^#' FILE`, and read only the relevant line ranges.

`fetch` extracts readable page text. For a JSON API endpoint such as `api.github.com`, use `gh api`
or `curl` directly.

When a URL requires authentication and cannot be fetched directly, such as `x.com`, use an
available authenticated client to retrieve it.

Commands under `exa`, `context7`, and `anysearch` bypass capability routing. Use them when the user
requests that provider or the operation exists only as a direct command. Restrict
`exa search` to known domains with `--include-domains CSV` rather than `site:` in the query. With
`exa search --include-text`, each result's text is capped by `--text-max-characters` (default
3000); prefer `--include-highlights` when snippets suffice.

## Context7 documentation

Run `forager context7 library NAME QUERY` to obtain a `library_id`; when the user supplied a valid
`/owner/project[/version]`, use it directly. For later single-topic queries about the same library,
reuse the same `library_id` and change only the query. When the user requested a version, keep the
versioned ID.

A `library_id` is not a URL; do not pass it to `fetch`. When the returned content contains an
absolute URL, decide whether the claim risk warrants fetching it. When it contains no URL, report
the documentation and do not invent a source.

Complete direct retrieval when it returns the requested content or page set. Route an observed
authentication, configuration, or terminal provider failure to Diagnose or configure in
`SKILL.md`.
