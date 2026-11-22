# Fetch through one provider-native chain: Firecrawl, then Jina, then Tavily

## Decision

**Content contract.** Web Fetch returns Normalized Fetch Content: the body Markdown decoded from
the successful provider's response, without its transport envelope or diagnostics. The provider's
own extraction chooses the page body. forager applies no local semantic denoiser and offers no raw
or clean variants. A body that is empty or too thin is one quality failure that falls through to
the next provider.

**One chain.** Direct fetch, research evidence fetching, search-side Web Fetch and platform full
text share one chain, by default Firecrawl, then Jina, then Tavily. There is no order by content
type or domain. A configuration that sets its own order keeps it when the default changes, because
configuration is authoritative.

**Request profiles.** Each provider attempt asks for complete Markdown once:

- Firecrawl gets one scrape that relies on its own wait for rendering, with no extra waits,
  browser actions or second rendering probe. Every request also asks for Firecrawl's PDF parser
  with page markers, which Firecrawl applies only to files, so forager does not detect PDFs first.
  forager waits for Firecrawl as long as the request allows Firecrawl to work.
- Jina is read through its structured JSON response, without link removal or generic selectors.
- Tavily is asked for its complete basic Markdown, without query-directed chunks.

**Anonymous Jina.** Jina's credentials are optional. With no key Jina runs anonymously; with keys,
when the credential pool ends in an authentication, quota or rate-limit failure, Jina tries once
more anonymously (the Anonymous Fallback). Failures unrelated to credentials do not trigger it.
Because Jina always counts as configured, the default chain can fetch with no Web Fetch key at all.

## Why

- A shared output shape cannot make three different extraction algorithms agree on where the body
  ends, and a local denoiser would be a fourth algorithm deciding what the caller never sees.
- The chain stops at the first body that passes the thin-content gate, so an incomplete but long
  body from an earlier provider is the final result. The order is therefore set by what each
  provider silently drops. forager delivers source text, and losing content silently does more
  harm than extra page clutter.
- Firecrawl first (comparisons from 2026-09-26: 15 URLs across page types and 9 arXiv papers):
  on the Python data-structures tutorial Tavily kept 0 of 35 code blocks and Firecrawl kept all
  35; on arXiv HTML Tavily dropped most figure captions. Firecrawl extracted all 8 papers with
  official HTML, against 7 for Tavily and 3 for Jina, and was the only one to read a scanned PDF
  and a one-page PDF. On a Hacker News thread both kept all 71 comments, but Tavily's output was
  much longer from repetition. Tavily was not worse on plain articles and text-layer PDFs:
  the evidence supports Firecrawl as the first provider, not a universal ranking.
- The PDF parser (5 PDFs and 2 HTML pages, with and without it): output improved on 3 of the 4
  PDFs with a text layer and got worse on none; on one paper the body nearly doubled and its
  recognition errors disappeared. Credits were unchanged and HTML output was byte-identical. It
  makes long PDFs take tens of seconds, and Firecrawl bills a job forager stopped waiting for, so
  the local timeout matches the request.
- Jina before Tavily (2026-10-18, two X articles): Firecrawl's cloud refused x.com outright. Jina
  kept everything Tavily kept plus whole tables, correctly wrapped code blocks and sentences Tavily
  lost; Tavily collapsed code blocks into one repeated line, emptied table cells and returned
  different versions of the same URL. An independent web-extraction benchmark also scored Jina's
  key-sentence recall above Tavily's, losing points only on clutter.
- Jina Reader serves calls without a key, at a lower official rate per IP than with a key, and an
  anonymous read was confirmed on 2026-10-18. Skipping a Jina without a key as unconfigured, or
  failing Jina when its keys are exhausted or failing, would refuse pages an anonymous call reads.

## Rejected alternatives

- **A local denoiser, or raw and clean output modes.** No safe common content boundary exists
  across providers.
- **Provider order by content type or domain.** A second routing layer. Tavily matched Firecrawl
  on plain articles and text-layer PDFs but did not beat it, so per-type rules would add routing
  without a measured gain.
- **Repeated Firecrawl scrapes or render waits for delayed pages.** Some delayed-rendering pages
  fall through earlier, but repeated scrapes would multiply billing and blur the ownership of
  quality, retry, rotation and deadline accounting.
- **Cache or privacy modes, site selectors, browser actions, query-directed chunks or structured
  extraction as defaults.** Each is a task- or site-specific choice that a caller can make
  explicitly; as a default it would change every fetch to suit a few. Firecrawl's own default
  cache stays on.
- **Tavily first, or Tavily second.** It produced the long-but-incomplete bodies that the gate
  cannot catch.
- **Requiring a key for Jina.** It turned a provider that can always read into one that is skipped
  or fails on credential trouble.
- **Anonymous retries for non-credential failures, retries of an anonymous rate limit, or local
  pacing of anonymous calls.** Network, timeout, parameter and thin-body failures do not depend on
  the key, and an anonymous rate limit is left to the next provider in the chain.

## Revisit when

- Jina is seen delivering a wrong body that still passes the thin-content gate.
- Jina's results after a Firecrawl failure prove generally worse than Tavily's.
- One of these accepted costs becomes significant:
  - Firecrawl bills PDFs per page, and bills failed targets too.
  - Firecrawl's timeout is shared by all its capabilities, so a failing Firecrawl web search can
    wait as long as a fetch.
  - A Firecrawl body with recognition errors that is not thin is accepted.
  - Sites Firecrawl refuses cost one attempt.
  - arXiv full text that Firecrawl fails on may spend a Jina attempt, which reads arXiv HTML
    poorly, before reaching Tavily.
  - When Firecrawl is unavailable, Jina becomes the de facto first provider and returns more
    clutter, billed by token.
  - Jina cannot be reached directly from some networks (for example mainland China), which costs
    a failed attempt and a budget slot each time.
  - Anonymous calls share a per-IP limit and can be blocked per domain for a while, so a working
    key stays more reliable.

## Supersedes

- ADR-0009, the provider-first Web Fetch content contract. It is the content contract above; only
  its order was replaced.
- ADR-0018, which put Firecrawl first and added its PDF parser. Its order was replaced in part by
  Jina moving ahead of Tavily; its evidence and parser decision are kept above.
