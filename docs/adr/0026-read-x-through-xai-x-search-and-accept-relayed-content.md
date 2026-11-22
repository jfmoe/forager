# Read X through xAI x_search and accept relayed content

## Decision

The `x` platform has one route, `xai`, which reuses the main-search xAI provider's endpoint,
credentials and model and reads X through the Responses API's `x_search` tool. It provides search,
fetch and the Platform Operation Ask.

- **Relayed Content is accepted as a platform item, but identity and time never depend on the
  model.**
  - Search asks for a post list through structured output. With structured output the response's
    annotations list every source the model read, and an item whose post ID is not among them is
    dropped.
  - The publication time is derived from the post ID (a snowflake), never taken from the model.
  - Fetch reads the thread. The root post must match the request, and the response must either
    record an `x_thread_fetch` call for the requested ID or carry that ID in its annotations. xAI
    returns the parameters of server-side tool calls, so the call record does not depend on the
    model's retelling. Fetch requires a tool call, because without that the model often skipped
    the tool and returned an empty list. Other posts in the thread cannot be checked, and the
    platform reference says so.
  - Body text and author handles are not guaranteed verbatim, so search items are at snippet
    depth.
- **Only server-enforced parameters are exposed:** account allow and deny lists and a date range.
  The model picks the ordering itself and the server does not enforce one, so forager offers none.
- **Answers are a separate Platform Operation, Ask**, not a field or switch on search. Ask uses no
  structured output, keeps inline `[[N]](url)` citations with main search's source normalisation,
  and maps the cited posts to Platform Refs that can be fetched next.
- **The route is named after the vendor and reuses its configuration**, as SerpApi does: one
  credential pool serves both main search and the platform. Platform requests send only
  `x_search`, whatever tools main search is configured with.

## Why

- `x_search` bundles keyword search, semantic search, user search and thread reading, and the
  model decides which to call and how often. The xAI API returns only the tool-call parameters,
  not the posts the tools read (xAI's Tool Usage Details, 2026-10-17); forager gets only the
  model's text and its `url_citation` annotations. X is the first platform whose items the model
  relays instead of forager reading them directly.
- Tested on 2026-10-17 through a gateway that serves the Responses API on a SuperGrok
  subscription: every post the model returned was among the annotations. The times derived from
  three same-day post IDs matched the model's to the second.
- Tested on 2026-10-18: thread reading annotates only the top post of the thread, and when the
  requested post is a reply the annotation is its parent, so a check by annotation alone would
  judge every reply unread. The recorded tool call closes that gap.
- With structured output the answer loses its per-sentence citations (every annotation position
  was 0), which is why answers are a separate operation.

## Rejected alternatives

- **Only an answer command.** It yields no post identity to fetch next and cannot list an
  account's posts.
- **A browser route first.** It would read verbatim text and engagement counts, but needs a
  logged-in X account and carries risk control and ban risk. It can be added later as a second
  route of the same platform without overturning this decision.
- **An X search capability in main search.** Main search already enables `x_search` by default; a
  new capability would duplicate it.
- **A sort option and a limit on tool turns for X search.** The model picks top or latest posts
  itself and the service does not enforce an order; a turn limit was tested and does not bound the
  number of tool calls.

## Revisit when

- Cost or volume matters. How many posts a request reads is the model's choice, and limiting turns
  did not cap the number of tool calls: one search made 16 tool calls and read 52 posts. Through
  xAI's official API, billing follows posts and users read.
- A caller needs verbatim text: today the platform reference tells callers to open the post's URL
  for an exact quote.
- xAI starts returning the posts its tools read, which would let items stop being relayed.
