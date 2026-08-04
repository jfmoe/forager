# Keep Context7 MCP docs as one readable payload

`context7 docs` will keep using the official Context7 MCP and expose one canonical documentation body. Successful JSON contains `provider`, `library_id`, `query`, and `content`, with `provider_attempts` added only for `--verbose`; it does not expose `code_snippets`, `info_snippets`, `results`, `total`, or a synthesized `sources` list. The official MCP returns formatted text rather than its REST API's structured snippet response, so retaining speculative raw fields duplicates content and makes an unsupported response shape part of the CLI contract.

`library_id` remains the reusable Context7 lookup identifier, not a fetchable URL. The forager skill must teach callers to resolve it once, preserve an explicit version, reuse it for later single-topic queries, and decide from URLs present in `content` whether an original page needs fetching. Forager will not parse `Source:` lines into structured sources.

The direct command can succeed with readable content alone. The automatic Documentation Search Capability has a stricter boundary: a Context7 response with no normalized source is not a consumable provider result and must continue to the next configured documentation provider rather than terminate the chain with an empty success.
