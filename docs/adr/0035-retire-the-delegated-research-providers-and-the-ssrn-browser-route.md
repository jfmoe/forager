# Retire the delegated research providers and the SSRN browser route

## Decision

forager has no browser provider beyond the opt-in platform routes, and three that once existed
stay removed. Their old commands, options, configuration sections, environment variables, order
entries and cursors are rejected as unknown input by the existing checks, with no alias or
compatibility layer.

### 1. No delegated research

`gemini_browser` and `chatgpt_browser` drove Gemini and ChatGPT Deep Research pages in the user's
logged-in Chrome and delivered each report with its sources as a **Delegated Research Report**.
They are retired together with the `forager gemini` and `forager chatgpt` commands, the Delegated
Research Report concept, and every rule that existed only for providers that belong to no
platform: that registration kind, doctor's default skip and adapter status probe, the
foreground-window mode, and the single-attempt rule for writes that change account state.

### 2. SSRN only through Crossref

`ssrn_browser` read SSRN in the user's Chrome. It is retired, and SSRN search and fetch run only
through the anonymous Crossref route. With it went the search conditions only the site supports,
the item fields only it filled, SSRN full text, and the local-file full-text source that existed
only for SSRN PDFs, so a platform full text is a set of URLs or a Native body (ADR-0022).

## Why

- **Maintenance only by live testing.** The Deep Research adapters' page reading, completion
  conditions and click paths could be proven only on the live site, and no automated check
  against it was possible: one would either read the user's private sessions or spend Deep
  Research quota. Live tests also met Google's unusual-traffic page, response bodies the Browser
  Bridge dropped at random, and rejected navigations, each investigated within a budget of page
  opens.
- **Desktop takeover.** The providers drove the Chrome the user was working in, and starting Deep
  Research needed a foreground window.
- **An external toolchain.** They ran on OpenCLI, whose install and version drift broke routes
  (ADR-0036).
- **ChatGPT's terms.** OpenAI's terms of use forbid automatically or programmatically extracting
  data or Output, independent of robots.txt, and `chatgpt.com/robots.txt` disallows conversation
  pages. The route ran only under conditions the maintainer set: personal use, triggered by a named
  command, reading only the responses the page itself received, never using or building session
  credentials or anti-abuse fields, human pace, single-attempt writes. It was a higher risk than
  any other route, on a paid account. Google's terms, by contrast, forbid only automated access
  that breaks robots.txt.
- **SSRN's browser route was unreliable.** SSRN's Cloudflare challenge blocked it often. Reading
  the site's own anonymous search API from inside the browser avoided the results page's
  challenge, but it was a request of forager's own, which ADR-0036 now forbids. After the move to
  OpenCLI 2.x it had no live case and was not tested again.
- **Crossref costs little in default use.** In the 2026-09 comparisons, known-paper hits in the
  top 10 were 7 of 10 for Crossref and 8 of 10 for the browser route, with an MRR@20 of 0.575
  against 0.481. The two routes shared a median of only 6 of their top 20, and Crossref misses
  papers without a DOI; for recent papers with a DOI, Crossref registration trails SSRN posting
  by 0–1 days. The browser route was never in the default order (ADR-0020), so default SSRN
  search never used it.

## Rejected alternatives

- **Keeping the delegated research providers.** A report is another agent's synthesis: its
  sources are Gemini's or ChatGPT's own claim, and forager has read none of them. It could never
  be evidence, so the cost above bought only a third-party opinion.
- **Keeping `ssrn_browser` as an opt-in route.** It kept SSRN full text, exact Posted, Last
  revised and Date Written dates, the site-only search conditions and the candidates only the site
  finds, at the price of a route that was often blocked and could not be tested.

## Revisit when

- **Delegated research:** an access path appears that neither takes over the user's desktop nor
  depends on an external browser driver, there is a real need, and it stays within the service's
  terms: for Gemini that means respecting robots.txt, and for ChatGPT any programmatic extraction
  of Output breaches them, so only an official access path would do. A revived provider would deliver its report as a third-party view that is never
  evidence, and would run any write that changes account state as a single attempt,
  because a timed-out write may already have reached the service and a retry would spend quota
  twice.
- **SSRN:** SSRN full text, exact dates or site-only candidates are needed, and a browser route
  can read them within ADR-0036's boundaries.

## Supersedes

- ADR-0023, delegated research reports and providers run only by a named command: retired with
  the providers; what still informs a revisit is kept above.
- ADR-0024, the conditions for accepting OpenAI's terms risk: retired with `chatgpt_browser`; the
  risk is kept above as a reason and a revisit condition.
- ADR-0025, SSRN search requesting the site's search API in the browser: retired with
  `ssrn_browser`, and its allowance for a route's own request is now forbidden by ADR-0036.
