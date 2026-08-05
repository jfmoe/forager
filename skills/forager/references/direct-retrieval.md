# Direct retrieval

For a known URL or PDF, run `forager fetch URL --format json`. For site structure discovery, run
`forager map URL --instructions "GOAL" --format json`.

When a URL requires authentication and cannot be fetched directly, such as `x.com`, use an
available authenticated client to retrieve it.

Commands under `exa`, `context7`, and `anysearch` bypass capability routing. Use them when the user
requests that provider or the operation exists only as a direct command.

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
