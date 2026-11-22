# Let a route deliver platform-native full text without Web Fetch

## Decision

A platform fetch has two kinds of full-text source: a set of URLs that the shared Web Fetch chain
reads in order, or a **Native** body that the route read and checked itself in the same attempt
that produced the item's metadata. A Native body skips the Web Fetch chain and its thin-content
gate and goes straight to the same delivery as any full text: written to a local file, with only
its path on stdout (ADR-0015).

- **Declared in the registration.** Each route declares whether its fetch reads the full text
  itself. The plan for a fetch uses that declaration to decide whether a Web Fetch provider may be
  needed, and only then does the CLI fail a full-text request up front when no Web Fetch provider
  is configured.
- **Attribution.** The content provider is whoever produced the Markdown: the Web Fetch provider
  for a URL source, the route for a Native one. The content URL of a Native body is the item's
  canonical identity URL, with no promise that it opens anonymously.
- **Web Fetch stays platform-free.** The Web Fetch capability holds no platform knowledge; a route
  that can read and check its own body reuses the Native source by declaring it.

## Why

- Some bodies cannot be read by a third-party fetcher at all. A Xiaohongshu note opens only in the
  user's own logged-in session with its access token; on 2026-10-08 an anonymous request with the
  token returned 200 with no note in the page.
- The route already holds a structured body it has checked against the requested item, such as
  the note's title, text, tags and images or a discussion tree. Sending the same page to Web Fetch
  would cost another visit and, on sites with risk control, another chance of a block, for a worse
  result.
- These bodies need no format conversion, so the Web Fetch providers would add nothing.

## Rejected alternatives

- **Fetching the canonical URL through Web Fetch.** It cannot see a logged-in session, and where
  it can read the page it duplicates a visit for worse output.
- **Platform knowledge inside Web Fetch.** It would couple the shared capability to every
  platform's page shape.

## Revisit when

- A Native body turns out to need conversion that the Web Fetch providers do better than the
  route.
- A platform's full text can be read both natively and by Web Fetch with materially different
  quality, which would call for a per-route choice instead of a fixed declaration.
