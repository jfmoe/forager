# Price Polymarket outcomes only from two-sided order books

## Decision

The `polymarket` platform has one anonymous route, `polymarket_api`, on by default. Inside the
route it combines three public read APIs of Polymarket: Gamma for discovery and metadata, the CLOB
for live bids, asks and last trades, and the Data API for price history. It provides search, fetch
and the Platform Operation Price History.

- **A probability is only the midpoint of a two-sided book.** The Market-Implied Probability of an
  outcome is the midpoint of its best bid and best ask while both sides of the order book have
  orders.
  - When either side is empty or the market has stopped trading, the probability is null and the
    reason is stated.
  - An unclosed neg-risk placeholder market that is not active counts as suspended, and the CLOB
    is not asked about it. The active flag is read only for unclosed markets, because a settled
    market stays active.
  - A settled market gives its payout, labelled as settlement and never called a probability.
  - forager also delivers bid, ask, spread and last trade, and leaves judging a wide-spread market
    to the caller instead of choosing between midpoint and last trade for it.
- **One route reaches three endpoints**, under ADR-0027's multi-endpoint rule, because fetch has
  to combine Gamma's structure with the CLOB's live quotes and the chain never merges routes.
  - The CLOB only enhances: when it fails, fetch prices from Gamma's cached quotes by the same rule
    and says so.
  - Search prices only from Gamma, keeping it to one request.
  - Price History exists only in the Data API, so its failure fails the command.
- **Events first, markets reachable.** A Platform Ref has two kinds matching the site's URL levels:
  an event (the default) and a market. Search returns one item per event, keeping a group of
  mutually exclusive markets together.
- **Refs use slugs; on-chain identity is output.** A numeric ID as a ref would need a network call
  to resolve a pasted web link, which the platform contract rejects before any request. The
  event, market and condition IDs come with every fetch, and programmatic consumers key markets by
  condition ID and events by event ID.
- **On by default.** The route calls only public read APIs that Polymarket documents as needing no
  key, reads no site pages, and paces far below the official limits. The terms' two restrictions
  are written into the product skill's platform reference for users to judge by their own status.
- **Price series and settlement stay apart.** Price History holds only market prices; a settled
  market's payout is given separately. Polymarket does not say whether a history point is a trade
  or a midpoint, so it is not called a probability history.

## Why

From documentation checks and anonymous tests on 2026-10-24:

- None of the three APIs needs credentials to read; the geoblock restricts only order placement.
- Gamma's price can be stale: its outcome prices equal Gamma's own bid/ask midpoint, and responses
  are cached by a CDN for up to 300 seconds.
- An empty book gives a false number: the CLOB midpoint and Gamma both report 0.5, a placeholder
  rather than any trader's judgement.
- The official display rule is unreliable. The documentation says the site shows the last trade
  when the spread exceeds $0.10, but the one page sample still showed the midpoint at a spread of
  0.627.
- Neg-risk groups contain placeholder markets that never trade: unclosed and inactive yet still
  accepting orders, with no cached price and no CLOB book. 74 of the 128 markets in
  `presidential-election-winner-2028` were like that.
- The CLOB's two last-trade prices disagree on neg-risk markets: the book's last trade is the
  complement of the dedicated last-trade endpoint's.
- Only slugs have web URLs; numeric IDs have no page. No documentation says whether a slug can
  change, and an existing implementation warns that a recreated event may reuse one.
- The terms target scraping and institutional use: they forbid robots or crawlers on the site, and
  forbid capital-market clients and market-data distributors from using the data through the API
  without written consent.

## Rejected alternatives

- **Gamma's outcome prices directly.** One request would do, but the value can be five minutes
  stale and an empty book's 0.5 would pass as a probability.
- **Copying the site's display rule** (last trade above a 0.10 spread). The rule contradicts the
  sample, and the last trade may be days old, passing an old price off as the current probability.
- **Markets first, or events only.** The first breaks up groups of exclusive markets, so a
  13-outcome event would fill the search limit; the second loses the precision of a pasted market
  link, and history could match a market only by label.
- **Opt-in, as `youtube_web` is.** It would hand the terms judgement to the user explicitly, at the
  price of one more configuration step on first use. `youtube_web` reads undocumented endpoints
  against the terms; this route calls APIs published for developers.
- **Query expansion and local relevance filtering.** Gamma's search relevance is poor, but judging
  relevance and retrying with other words belong to the caller; heuristics in platform search
  would make results unpredictable.
- **Gamma's reduced search response.** It is smaller, but it leaves out each market's condition ID
  and the token IDs of its outcomes. CLOB quotes and Price History are requested by those token
  IDs, and search reads markets through the same decoder as fetch, which rejects an open market
  that lacks a token for every outcome.
- **A sort option for Polymarket search.** The upstream sort parameter was tested and has no
  effect.

## Revisit when

- Polymarket changes its rate limits, which its terms allow at any time, or restricts keyless
  reads.
- Callers are misled by null probabilities on illiquid markets or by neg-risk group probabilities
  that do not sum to 1. forager does not normalise them; callers read the basis and the bid and
  ask.
- Slugs prove unstable: a stale slug makes fetch report the item missing, so callers have to search
  again and compare event IDs when referring to one market over time.
- Polymarket US, a separate product with a different identity space, needs to be covered; it is
  outside this platform.
