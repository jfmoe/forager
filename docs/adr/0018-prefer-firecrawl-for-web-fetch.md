# Prefer Firecrawl for Web Fetch

The default Web Fetch order changes from Tavily → Firecrawl → Jina to Firecrawl → Tavily → Jina. This replaces only the order in ADR 0009. The request profiles, the thin-content gate, truncation, and the rule that there is no content-type-specific order are unchanged. Direct fetch, research evidence fetching, search-side Web Fetch, and PDFs still share one chain.

The evidence is two measured comparisons from 2026-09-26: `docs/research/2026-09-26-firecrawl-tavily-fetch-quality.md` (15 URLs across page types) and `docs/research/2026-09-26-arxiv-full-text-fetch-benchmark.md` (9 arXiv papers):

- The fallback chain stops at the first body that passes the thin-content gate, so an incomplete but long body from the first provider is the final result. Tavily produced such bodies: on the Python data-structures tutorial it kept 0 of 35 code blocks, and on arXiv HTML it dropped most figure captions. Firecrawl kept all 35 code blocks and all captions.
- On a Hacker News thread both providers kept all 71 comments, but Tavily's output was 2.63 times longer because of repeated content.
- Firecrawl extracted the body of all 8 arXiv papers that have official HTML, against 7 for Tavily and 3 for Jina. A scanned PDF and a one-page PDF succeeded only on Firecrawl.
- Tavily was not worse on plain articles and text-layer PDFs. The comparison supports Firecrawl as the better first provider, not a universal ranking.

## PDF parser with page markers

Every Firecrawl Web Fetch request also sends `parsers: [{"type": "pdf", "mode": "auto", "pageMarkers": true}]`, and the default `providers.firecrawl.timeout` changes from 30 to 60 seconds.

The evidence is `docs/research/2026-09-26-firecrawl-pdf-parser-options.md`, which compared requests with and without the parser on 5 PDFs and 2 HTML pages:

- On 3 of the 4 PDFs with a text layer, the output improved and none became worse. On Mixtral, the body grew from 4,330 to 8,486 words, 10 `Mixral` errors disappeared, and Table 2 matched the source in all 98 cells. On Llama 3, a wrong table header (`Genma`) became correct. The Bitcoin paper and a scanned page did not change.
- `creditsUsed` was the same with and without the parser: 1 per PDF page.
- The two HTML pages returned byte-identical Markdown. Firecrawl applies `parsers` only to files, so forager does not need to detect PDFs before the request.
- The improvement probably comes from a different server-side parsing path that any PDF structure option selects, not from the page markers themselves. The markers add about 4 tokens each.

The parser makes PDFs slower: 6 to 13 seconds for 9 to 28 pages, and 32 seconds for the 92-page Llama 3 paper. The request already allows Firecrawl 60 seconds, but forager stopped waiting at 30 seconds. Firecrawl can finish and bill a job whose result forager then discards, so the local timeout now matches the request.

## Consequences

- Firecrawl bills PDFs per page, and a Firecrawl attempt is billed even when the target fails. Research that reads many PDFs costs more than with Tavily first.
- A Firecrawl body that contains recognition errors but is not thin is accepted and does not fall through. For example, the default Mixtral PDF result contained `Mixral`.
- Firecrawl's default two-day cache stays enabled. Cache and privacy modes remain explicit, task-specific choices.
- Sites that Firecrawl refuses (HTTP 403 `support this site`) cost one extra attempt before Tavily runs.
- PDF fetches through Firecrawl take several times longer. A Firecrawl attempt can use 60 seconds of the 180-second fetch deadline, which leaves 30 seconds each for Tavily and Jina.
- `providers.firecrawl.timeout` is shared by all Firecrawl capabilities, so a failing Firecrawl Web Search request can also wait up to 60 seconds.
- An existing configuration that sets `capabilities.web_fetch.order` or `providers.firecrawl.timeout` keeps its value, because configuration is authoritative. `forager setup` wrote both keys into every generated template, so users of those configurations must change the keys to get the new defaults.
