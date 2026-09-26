# Prefer Firecrawl for Web Fetch

The default Web Fetch order changes from Tavily → Firecrawl → Jina to Firecrawl → Tavily → Jina. This replaces only the order in ADR 0009. The request profiles, the thin-content gate, truncation, and the rule that there is no content-type-specific order are unchanged. Direct fetch, research evidence fetching, search-side Web Fetch, and PDFs still share one chain.

The evidence is two measured comparisons from 2026-09-26: `docs/research/2026-09-26-firecrawl-tavily-fetch-quality.md` (15 URLs across page types) and `docs/research/2026-09-26-arxiv-full-text-fetch-benchmark.md` (9 arXiv papers):

- The fallback chain stops at the first body that passes the thin-content gate, so an incomplete but long body from the first provider is the final result. Tavily produced such bodies: on the Python data-structures tutorial it kept 0 of 35 code blocks, and on arXiv HTML it dropped most figure captions. Firecrawl kept all 35 code blocks and all captions.
- On a Hacker News thread both providers kept all 71 comments, but Tavily's output was 2.63 times longer because of repeated content.
- Firecrawl extracted the body of all 8 arXiv papers that have official HTML, against 7 for Tavily and 3 for Jina. A scanned PDF and a one-page PDF succeeded only on Firecrawl.
- Tavily was not worse on plain articles and text-layer PDFs. The comparison supports Firecrawl as the better first provider, not a universal ranking.

## Consequences

- Firecrawl bills PDFs per page, and a Firecrawl attempt is billed even when the target fails. Research that reads many PDFs costs more than with Tavily first.
- A Firecrawl body that contains recognition errors but is not thin is accepted and does not fall through. For example, the default Mixtral PDF result contained `Mixral`.
- Firecrawl's default two-day cache stays enabled. Cache and privacy modes remain explicit, task-specific choices.
- Sites that Firecrawl refuses (HTTP 403 `support this site`) cost one extra attempt before Tavily runs.
- An existing configuration that sets `capabilities.web_fetch.order` keeps its order, because configuration is authoritative. `forager setup` wrote this key into every generated template, so users of those configurations must change the key to get the new order.
