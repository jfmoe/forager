> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.com/llms.txt
> Use this file to discover all available pages before exploring further.

# Discover Markets

> Learn how to find and filter events and markets and explore related metadata.

Find the events and markets your integration needs, from a specific Polymarket link to a broader view of what is active. This data is public and does not require authentication.

## Events

An event groups one or more markets under a shared question set. A single-market event asks one yes/no question; a multi-market event splits a broader question into individual outcomes, such as one market per candidate in an election.

### Fetch an Event

Fetch an event when you already know its identifier.

**TypeScript**

Call `fetchEvent()` on a `PublicClient` or `SecureClient` to fetch an event by ID.

```ts theme={null}
const event = await client.fetchEvent({ id: "90177" });

// event: Event
```

You can also fetch an event by its Polymarket URL or slug:

```ts URL theme={null}
const event = await client.fetchEvent({
  url: "https://polymarket.com/event/will-the-us-confirm-that-aliens-exist-before-2027",
});
```

```ts Slug theme={null}
const event = await client.fetchEvent({
  slug: "will-the-us-confirm-that-aliens-exist-before-2027",
});
```

> **Output: Event**
>
> ```ts Event Type theme={null}
> type Market = {
>   id: string;
>   version?: ProtocolVersion | null;
>   slug?: string | null;
>   question?: string | null;
>   conditionId: ConditionId | null;
>   outcomes: {
>     yes: { tokenId: TokenId | null; positionId: PositionId | null };
>     no: { tokenId: TokenId | null; positionId: PositionId | null };
>   };
> };
>
> type Event = {
>   id: string;
>   slug?: string | null;
>   title?: string | null;
>   markets: Market[];
> };
> ```
>
> ```json Event Example (Polymarket V2) theme={null}
> {
>   "id": "<event_id>",
>   "slug": "<event_slug>",
>   "title": "<event_title>",
>   "markets": [
>     {
>       "id": "1",
>       "version": "v2",
>       "slug": "<market_slug>",
>       "question": "<market_question>",
>       "conditionId": "0x017089ce3ba22aaa0a4cba8250b8c8e1eb0000000000000000000000000000",
>       "outcomes": {
>         "yes": {
>           "tokenId": null,
>           "positionId": "651150819117105875331414918119047680898421632356043490229292782433651916800"
>         },
>         "no": {
>           "tokenId": null,
>           "positionId": "651150819117105875331414918119047680898421632356043490229292782433651916801"
>         }
>       }
>     }
>   ]
> }
> ```
>
> ```json Event Example (CTF) theme={null}
> {
>   "id": "90177",
>   "slug": "will-the-us-confirm-that-aliens-exist-before-2027",
>   "title": "Will the US confirm that aliens exist by...?",
>   "markets": [
>     {
>       "id": "703257",
>       "version": "v1",
>       "slug": "will-the-us-confirm-that-aliens-exist-before-2027-789-924-249",
>       "question": "Will the US confirm that aliens exist before 2027?",
>       "conditionId": "0x747dc809fb79e1b05be09c42d6179459a58de2ef3e40f02484a4e1260f741f75",
>       "outcomes": {
>         "yes": {
>           "tokenId": "107505882767731489358349912513945399560393482969656700824895970500493757150417",
>           "positionId": null
>         },
>         "no": {
>           "tokenId": "7305630249804085635496399869905769372294302716159034447326228509068694952392",
>           "positionId": null
>         }
>       }
>     },
>     "..."
>   ]
> }
> ```

**Python**

Call `get_event()` on an `AsyncPublicClient` or `AsyncSecureClient` to fetch an event by ID.
The synchronous `PublicClient` and `SecureClient` provide the same method.

```python theme={null}
event = await client.get_event(id="90177")

# event: Event
```

You can also fetch an event by its Polymarket URL or slug:

```python URL theme={null}
event = await client.get_event(
    url="https://polymarket.com/event/will-the-us-confirm-that-aliens-exist-before-2027",
)
```

```python Slug theme={null}
event = await client.get_event(
    slug="will-the-us-confirm-that-aliens-exist-before-2027",
)
```

> **Output: Event**
>
> ```python Event Type theme={null}
> class MarketOutcome:
>     token_id: TokenId | None
>     position_id: PositionId | None
>
> class MarketOutcomes:
>     yes: MarketOutcome
>     no: MarketOutcome
>
> class Market:
>     id: str
>     version: ProtocolVersion | None
>     slug: str | None
>     question: str | None
>     condition_id: ConditionId | None
>     outcomes: MarketOutcomes
>
> class Event:
>     id: str
>     slug: str | None
>     title: str | None
>     markets: tuple[Market, ...]
> ```
>
> ```json Event Example (Polymarket V2) theme={null}
> {
>   "id": "<event_id>",
>   "slug": "<event_slug>",
>   "title": "<event_title>",
>   "markets": [
>     {
>       "id": "1",
>       "version": "v2",
>       "slug": "<market_slug>",
>       "question": "<market_question>",
>       "condition_id": "0x017089ce3ba22aaa0a4cba8250b8c8e1eb0000000000000000000000000000",
>       "outcomes": {
>         "yes": {
>           "token_id": null,
>           "position_id": "651150819117105875331414918119047680898421632356043490229292782433651916800"
>         },
>         "no": {
>           "token_id": null,
>           "position_id": "651150819117105875331414918119047680898421632356043490229292782433651916801"
>         }
>       }
>     }
>   ]
> }
> ```
>
> ```json Event Example (CTF) theme={null}
> {
>   "id": "90177",
>   "slug": "will-the-us-confirm-that-aliens-exist-before-2027",
>   "title": "Will the US confirm that aliens exist by...?",
>   "markets": [
>     {
>       "id": "703257",
>       "version": "v1",
>       "slug": "will-the-us-confirm-that-aliens-exist-before-2027-789-924-249",
>       "question": "Will the US confirm that aliens exist before 2027?",
>       "condition_id": "0x747dc809fb79e1b05be09c42d6179459a58de2ef3e40f02484a4e1260f741f75",
>       "outcomes": {
>         "yes": {
>           "token_id": "107505882767731489358349912513945399560393482969656700824895970500493757150417",
>           "position_id": null
>         },
>         "no": {
>           "token_id": "7305630249804085635496399869905769372294302716159034447326228509068694952392",
>           "position_id": null
>         }
>       }
>     },
>     "..."
>   ]
> }
> ```

**API**

Fetch an event by ID:

```bash theme={null}
curl "https://gamma-api.polymarket.com/events/90177"
```

You can also fetch an event by slug:

```bash theme={null}
curl "https://gamma-api.polymarket.com/events/slug/will-the-us-confirm-that-aliens-exist-before-2027"
```

> **Output: Event**
>
> ```ts Event Type theme={null}
> type Market = {
>   id: string;
>   version: string | null;
>   slug: string;
>   question: string | null;
>   conditionId: string | null;
>   clobTokenIds: string | null;
>   positionIds: string[] | null;
> };
>
> type Event = {
>   id: string;
>   slug: string | null;
>   title: string | null;
>   markets: Market[];
> };
> ```
>
> ```json Event Example (Polymarket V2) theme={null}
> {
>   "id": "<event_id>",
>   "slug": "<event_slug>",
>   "title": "<event_title>",
>   "markets": [
>     {
>       "id": "1",
>       "version": "v2",
>       "slug": "<market_slug>",
>       "question": "<market_question>",
>       "conditionId": "0x017089ce3ba22aaa0a4cba8250b8c8e1eb0000000000000000000000000000",
>       "clobTokenIds": null,
>       "positionIds": [
>         "651150819117105875331414918119047680898421632356043490229292782433651916800",
>         "651150819117105875331414918119047680898421632356043490229292782433651916801"
>       ]
>     }
>   ]
> }
> ```
>
> ```json Event Example (CTF) theme={null}
> {
>   "id": "90177",
>   "slug": "will-the-us-confirm-that-aliens-exist-before-2027",
>   "title": "Will the US confirm that aliens exist by...?",
>   "markets": [
>     {
>       "id": "703257",
>       "version": "v1",
>       "slug": "will-the-us-confirm-that-aliens-exist-before-2027-789-924-249",
>       "question": "Will the US confirm that aliens exist before 2027?",
>       "conditionId": "0x747dc809fb79e1b05be09c42d6179459a58de2ef3e40f02484a4e1260f741f75",
>       "clobTokenIds": "[\"107505882767731489358349912513945399560393482969656700824895970500493757150417\",\"7305630249804085635496399869905769372294302716159034447326228509068694952392\"]",
>       "positionIds": null
>     },
>     "..."
>   ]
> }
> ```

### List Events

List events when you need a browsable or filterable feed.

**TypeScript**

Call `listEvents()` on a `PublicClient` or `SecureClient` to page through events.

```ts theme={null}
const pages = client.listEvents({ closed: false, pageSize: 20 });

for await (const page of pages) {
  // page.items: Event[]
}
```

> **Output: Event\[]**
>
> ```ts Page Type theme={null}
> type Market = {
>   id: string;
>   slug?: string | null;
>   question?: string | null;
>   conditionId: string | null;
> };
>
> type Event = {
>   id: string;
>   slug?: string | null;
>   title?: string | null;
>   markets: Market[];
> };
>
> type Page = {
>   items: Event[];
>   hasMore: boolean;
>   nextCursor?: string;
> };
> ```
>
> ```json Page Example theme={null}
> {
>   "items": [
>     {
>       "id": "16183",
>       "slug": "kraken-ipo-in-2025",
>       "title": "Kraken IPO by ___ ?",
>       "markets": [
>         {
>           "id": "516950",
>           "slug": "kraken-ipo-in-2025",
>           "question": "Kraken IPO in 2025?"
>         },
>         "..."
>       ]
>     },
>     {
>       "id": "16263",
>       "slug": "macron-out-in-2025",
>       "title": "Macron out by...?",
>       "markets": ["..."]
>     },
>     "..."
>   ],
>   "hasMore": true,
>   "nextCursor": "9YTr9qyfU9571U9_leL7LNSWUI6EXd7rpAqdh-EbPr17InYiOjEsImsiOiJldmVudHMiLCJvaCI6IjRmNTNjZGExOGMyYmFhMGMwMzU0YmI1ZjlhM2VjYmU1ZWQxMmFiNGQ4ZTExYmE4NzNjMmYxMTE2MTIwMmI5NDUiLCJrZXlzIjpbeyJ0Ijoic3RyaW5nIiwidiI6IjI1ODE1In1dfQ"
> }
> ```

Filter by tag by passing one or more numeric tag IDs:

```ts theme={null}
const pages = client.listEvents({
  tagIds: [745], // numeric ID for the "nba" tag; see Tags below to look these up
  closed: false,
});

for await (const page of pages) {
  // page.items: Event[]
}
```

**Python**

Call `list_events()` on an `AsyncPublicClient` or `AsyncSecureClient` to page through events.
The synchronous `PublicClient` and `SecureClient` provide the same method.

```python theme={null}
pages = client.list_events(closed=False, page_size=20)

async for page in pages:
    ...  # page.items: tuple[Event, ...]
```

> **Output: Event\[]**
>
> ```python Page Type theme={null}
> class Market:
>     id: str
>     slug: str | None
>     question: str | None
>     condition_id: str | None
>
> class Event:
>     id: str
>     slug: str | None
>     title: str | None
>     markets: tuple[Market, ...]
>
> class Page:
>     items: tuple[Event, ...]
>     has_more: bool
>     next_cursor: str | None
> ```
>
> ```json Page Example theme={null}
> {
>   "items": [
>     {
>       "id": "16183",
>       "slug": "kraken-ipo-in-2025",
>       "title": "Kraken IPO by ___ ?"
>     },
>     {
>       "id": "16263",
>       "slug": "macron-out-in-2025",
>       "title": "Macron out by...?"
>     },
>     "..."
>   ],
>   "has_more": true,
>   "next_cursor": "eyJmIjoiYWQyZjRkNmY4ZDZkIiwiayI6IjlZVHI5cXlmVTk1NzFVOV9sZUw3TE5TV1VJNkVYZDdycEFxZGgtRWJQcjE3SW5ZaU9qRXNJbXNpT2lKbGRtVnVkSE1pTENKdmFDSTZJalJtTlROalpHRXhPR015WW1GaE1HTXdNelUwWW1JMVpqbGhNMlZqWW1VMVpXUXhNbUZpTkdRNFpURXhZbUU0TnpOak1tWXhNVEUyTVRJd01tSTVORFVpTENKclpYbHpJanBiZXlKMElqb2ljM1J5YVc1bklpd2lkaUk2SWpJMU9ERTFJbjFkZlEiLCJwIjoiL2V2ZW50cy9rZXlzZXQiLCJzdmMiOiJnYW1tYSIsInYiOjF9"
> }
> ```

Filter by tag by passing a numeric tag ID:

```python theme={null}
pages = client.list_events(
    tag_ids=[745],  # numeric ID for the "nba" tag; see Tags below to look these up
    closed=False,
)

async for page in pages:
    ...  # page.items: tuple[Event, ...]
```

**API**

List active events:

```bash theme={null}
curl "https://gamma-api.polymarket.com/events/keyset?closed=false&limit=20"
```

> **Output: Events Page**
>
> ```json Example theme={null}
> {
>   "events": [
>     {
>       "id": "16183",
>       "slug": "kraken-ipo-in-2025",
>       "title": "Kraken IPO by ___ ?"
>     },
>     "..."
>   ],
>   "next_cursor": "9YTr9qyfU9571U9_leL7LNSWUI6EXd7rpAqdh-EbPr17InYiOjEsImsiOiJldmVudHMiLCJvaCI6IjRmNTNjZGExOGMyYmFhMGMwMzU0YmI1ZjlhM2VjYmU1ZWQxMmFiNGQ4ZTExYmE4NzNjMmYxMTE2MTIwMmI5NDUiLCJrZXlzIjpbeyJ0Ijoic3RyaW5nIiwidiI6IjI1ODE1In1dfQ"
> }
> ```

Pass `next_cursor` as `after_cursor` to fetch the next page:

```bash theme={null}
curl "https://gamma-api.polymarket.com/events/keyset?closed=false&limit=20&after_cursor=<next_cursor>"
```

Filter by tag with `tag_id`:

```bash theme={null}
curl "https://gamma-api.polymarket.com/events/keyset?tag_id=745&closed=false&limit=20"
```

On `GET /events` and `GET /events/keyset`, omitting `closed` returns open and closed events. `closed=true` selects closed events and `closed=false` selects open events. `closed=any` (case-insensitive) also includes both states, with no identifier filter required.

### Filter Events by Protocol

Select events for the position system your integration handles.

**TypeScript**

Call `listEvents()` on an existing `PublicClient` or `SecureClient`.
If needed, [create a Public Client](https://docs.polymarket.com/getting-started/typescript.md#create-a-public-client) first.

```ts theme={null}
import { ProtocolVersion } from "@polymarket/client";

const pages = client.listEvents({
  version: ProtocolVersion.V2,
  closed: false,
  pageSize: 20,
});

for await (const page of pages) {
  // page.items: Event[]
}
```

Use `ProtocolVersion.V2` for Polymarket Protocol V2 events or
`ProtocolVersion.V1` for CTF events. To include both, pass
`version: [ProtocolVersion.V1, ProtocolVersion.V2]`. Arrays accept one or
two values. Omission uses the current server default of CTF events (v1).
The paginator keeps the
filter on subsequent pages. When resuming with `cursor`, pass the same
`version`.

**Python**

Call `list_events()` on an existing `AsyncPublicClient` or
`AsyncSecureClient`. If needed,
[create a Public Client](https://docs.polymarket.com/getting-started/python.md#create-a-public-client) first.
Sync clients expose the same option.

```python theme={null}
pages = client.list_events(version="v2", closed=False, page_size=20)

async for page in pages:
    # page.items: tuple[Event, ...]
    pass
```

Use `version="v2"` for Polymarket Protocol V2 events or `version="v1"`
for CTF events. To include both, pass `version=["v1", "v2"]`. The option
accepts a string, a `ProtocolVersion` enum value, or a sequence of one or
two values. Omission or `None` uses the current server default of CTF
events (v1). The paginator keeps the filter on subsequent pages. To resume,
create `pages` with the same `version` and other filters, then continue
with the paginator returned by `pages.from_cursor(saved_cursor)`.

**API**

List Polymarket Protocol V2 events:

```bash theme={null}
curl "https://gamma-api.polymarket.com/events/keyset?version=v2&closed=false&limit=20"
```

Illustrative response (fields trimmed):

> **Output: Events Page**
>
> ```json theme={null}
> {
>   "events": [
>     {
>       "id": "12345",
>       "slug": "example-v2-event",
>       "title": "Example Protocol V2 Event"
>     }
>   ],
>   "next_cursor": "<next_cursor>"
> }
> ```

Use `version=v1` for CTF events. Repeat the parameter as
`version=v1&version=v2` to include both. Each value must be `v1` or `v2`,
with at most two values. Omission uses the current server default of CTF
events (v1).
Continue with `next_cursor` as `after_cursor`, keeping the same `version`
and other filters:

```bash theme={null}
curl "https://gamma-api.polymarket.com/events/keyset?version=v2&closed=false&limit=20&after_cursor=<next_cursor>"
```

## Markets

A market is a single yes/no question with two outcome IDs, one for YES
and one for NO.

### Fetch a Market

Fetch a market when you already know its identifier.

**TypeScript**

Call `fetchMarket()` on a `PublicClient` or `SecureClient` to fetch a market by ID.

```ts theme={null}
const market = await client.fetchMarket({ id: "703257" });

// market: Market
```

You can also fetch a market by its Polymarket URL or slug:

```ts URL theme={null}
const market = await client.fetchMarket({
  url: "https://polymarket.com/market/will-the-us-confirm-that-aliens-exist-before-2027-789-924-249",
});
```

```ts Slug theme={null}
const market = await client.fetchMarket({
  slug: "will-the-us-confirm-that-aliens-exist-before-2027-789-924-249",
});
```

> **Output: Market**
>
> ```ts Market Type theme={null}
> type Market = {
>   id: string;
>   version?: ProtocolVersion | null;
>   slug?: string | null;
>   question?: string | null;
>   conditionId: ConditionId | null;
>   outcomes: {
>     yes: { tokenId: TokenId | null; positionId: PositionId | null };
>     no: { tokenId: TokenId | null; positionId: PositionId | null };
>   };
> };
> ```
>
> ```json Market Example theme={null}
> {
>   "id": "703257",
>   "version": "v1",
>   "slug": "will-the-us-confirm-that-aliens-exist-before-2027-789-924-249",
>   "question": "Will the US confirm that aliens exist before 2027?",
>   "conditionId": "0x747dc809fb79e1b05be09c42d6179459a58de2ef3e40f02484a4e1260f741f75",
>   "outcomes": {
>     "yes": {
>       "tokenId": "107505882767731489358349912513945399560393482969656700824895970500493757150417",
>       "positionId": null
>     },
>     "no": {
>       "tokenId": "7305630249804085635496399869905769372294302716159034447326228509068694952392",
>       "positionId": null
>     }
>   }
> }
> ```

**Python**

Call `get_market()` on an `AsyncPublicClient` or `AsyncSecureClient` to fetch a market by ID.
The synchronous `PublicClient` and `SecureClient` provide the same method.

```python theme={null}
market = await client.get_market(id="703257")

# market: Market
```

You can also fetch a market by its Polymarket URL or slug:

```python URL theme={null}
market = await client.get_market(
    url="https://polymarket.com/market/will-the-us-confirm-that-aliens-exist-before-2027-789-924-249",
)
```

```python Slug theme={null}
market = await client.get_market(
    slug="will-the-us-confirm-that-aliens-exist-before-2027-789-924-249",
)
```

> **Output: Market**
>
> ```python Market Type theme={null}
> class MarketOutcome:
>     token_id: TokenId | None
>     position_id: PositionId | None
>
> class MarketOutcomes:
>     yes: MarketOutcome
>     no: MarketOutcome
>
> class Market:
>     id: str
>     version: ProtocolVersion | None
>     slug: str | None
>     question: str | None
>     condition_id: ConditionId | None
>     outcomes: MarketOutcomes
> ```
>
> ```json Market Example theme={null}
> {
>   "id": "703257",
>   "version": "v1",
>   "slug": "will-the-us-confirm-that-aliens-exist-before-2027-789-924-249",
>   "question": "Will the US confirm that aliens exist before 2027?",
>   "condition_id": "0x747dc809fb79e1b05be09c42d6179459a58de2ef3e40f02484a4e1260f741f75",
>   "outcomes": {
>     "yes": {
>       "token_id": "107505882767731489358349912513945399560393482969656700824895970500493757150417",
>       "position_id": null
>     },
>     "no": {
>       "token_id": "7305630249804085635496399869905769372294302716159034447326228509068694952392",
>       "position_id": null
>     }
>   }
> }
> ```

**API**

Fetch a market by ID:

```bash theme={null}
curl "https://gamma-api.polymarket.com/markets/703257"
```

You can also fetch a market by slug:

```bash theme={null}
curl "https://gamma-api.polymarket.com/markets/slug/will-the-us-confirm-that-aliens-exist-before-2027-789-924-249"
```

> **Output: Market**
>
> ```json Example theme={null}
> {
>   "id": "703257",
>   "version": "v1",
>   "slug": "will-the-us-confirm-that-aliens-exist-before-2027-789-924-249",
>   "question": "Will the US confirm that aliens exist before 2027?",
>   "conditionId": "0x747dc809fb79e1b05be09c42d6179459a58de2ef3e40f02484a4e1260f741f75",
>   "clobTokenIds": "[\"107505882767731489358349912513945399560393482969656700824895970500493757150417\",\"7305630249804085635496399869905769372294302716159034447326228509068694952392\"]",
>   "positionIds": null
> }
> ```

### List Markets

List markets when you need a filterable feed, such as browsing by tag, sport, or liquidity.

**TypeScript**

Call `listMarkets()` on a `PublicClient` or `SecureClient` to page through markets.

```ts theme={null}
const pages = client.listMarkets({
  tagId: 745, // numeric ID for the "nba" tag; see Tags below to look these up
  closed: false,
  pageSize: 20,
});

for await (const page of pages) {
  // page.items: Market[]
}
```

> **Output: Market\[]**
>
> ```json Example theme={null}
> [
>   {
>     "id": "741099",
>     "slug": "will-lebron-james-retire-before-next-nba-season",
>     "question": "Will LeBron James retire before next NBA season?",
>     "conditionId": "0x73057b771600660ac6e659c5b831587fd3bdd378e63f359731aa3e1538577fb0"
>   },
>   {
>     "id": "1747460",
>     "slug": "will-giannis-antetokounmpo-play-for-the-atlanta-hawks-in-2026-27",
>     "question": "Will Giannis Antetokounmpo play for the Atlanta Hawks in 2026-27?",
>     "conditionId": "0xff2715084bb2cf20d9b88b74ef809c3317b1e99f6c006faee9b83bc85aa0fc99"
>   },
>   "..."
> ]
> ```

**Python**

Call `list_markets()` on an `AsyncPublicClient` or `AsyncSecureClient` to page through markets.
The synchronous `PublicClient` and `SecureClient` provide the same method.

```python theme={null}
pages = client.list_markets(
    tag_id=745,  # numeric ID for the "nba" tag; see Tags below to look these up
    closed=False,
    page_size=20,
)

async for page in pages:
    ...  # page.items: tuple[Market, ...]
```

> **Output: Market\[]**
>
> ```json Example theme={null}
> [
>   {
>     "id": "741099",
>     "slug": "will-lebron-james-retire-before-next-nba-season",
>     "question": "Will LeBron James retire before next NBA season?",
>     "condition_id": "0x73057b771600660ac6e659c5b831587fd3bdd378e63f359731aa3e1538577fb0"
>   },
>   {
>     "id": "1747460",
>     "slug": "will-giannis-antetokounmpo-play-for-the-atlanta-hawks-in-2026-27",
>     "question": "Will Giannis Antetokounmpo play for the Atlanta Hawks in 2026-27?",
>     "condition_id": "0xff2715084bb2cf20d9b88b74ef809c3317b1e99f6c006faee9b83bc85aa0fc99"
>   },
>   "..."
> ]
> ```

**API**

List active markets by tag:

```bash theme={null}
curl "https://gamma-api.polymarket.com/markets/keyset?tag_id=745&closed=false&limit=20"
```

> **Output: Markets Page**
>
> ```json Example theme={null}
> {
>   "markets": [
>     {
>       "id": "540817",
>       "slug": "new-rhianna-album-before-gta-vi-926",
>       "question": "New Rihanna Album before GTA VI?",
>       "conditionId": "0x1fad72fae204143ff1c3035e99e7c0f65ea8d5cd9bd1070987bd1a3316f772be"
>     },
>     "..."
>   ],
>   "next_cursor": "8ejWEAtEdA8gG05m-tRMeLtBDl1Wbw1fUbXVfm0eYSZ7InYiOjEsImsiOiJtYXJrZXRzIiwib2giOiI0ZjUzY2RhMThjMmJhYTBjMDM1NGJiNWY5YTNlY2JlNWVkMTJhYjRkOGUxMWJhODczYzJmMTExNjEyMDJiOTQ1Iiwia2V5cyI6W3sidCI6InN0cmluZyIsInYiOiI1NTg5NDAifV19"
> }
> ```

Pass `next_cursor` as `after_cursor` to fetch the next page:

```bash theme={null}
curl "https://gamma-api.polymarket.com/markets/keyset?tag_id=745&closed=false&limit=20&after_cursor=<next_cursor>"
```

To look up markets without knowing whether they are open or closed, pass an identifier and `closed=any`:

```bash theme={null}
curl "https://gamma-api.polymarket.com/markets/keyset?id=540817&closed=any&limit=20"
```

On `GET /markets` and `GET /markets/keyset`, omitted `closed` and `closed=false` return open markets; `closed=true` selects closed markets. `closed=any` (case-insensitive) removes that filter and requires at least one of `id`, `slug`, `clob_token_ids`, `position_ids`, `condition_ids`, `request_ids`, `onchain_event_ids`, or `question_ids`. Each identifier filter accepts at most 100 values. Other filters still apply.

`closed=any` without an identifier, including a request with only `tag_id`, returns HTTP 422. An invalid `closed` value also returns HTTP 422.

After you choose a market, continue to [Market Details](https://docs.polymarket.com/market-data/market-details.md)
to select its outcome IDs and trading fields.

### Find Markets by Oracle Request

Use a known oracle request or on-chain event to find related Polymarket Protocol
V2 markets, or filter by their resolution state when building a settlement view.

**TypeScript**

Call `listMarkets()` on a `PublicClient` or `SecureClient`. Start with the
resolution state you want to follow.

```ts theme={null}
import { MarketResolutionStatus, ProtocolVersion } from "@polymarket/client";

const pages = client.listMarkets({
  version: ProtocolVersion.V2,
  resolutionStatus: MarketResolutionStatus.Active,
});

for await (const page of pages) {
  for (const market of page.items) {
    console.log(
      market.resolution.requestId,
      market.resolution.onchainEventId,
      market.resolution.resolutionStatus,
    );
  }
}
```

To query both protocols, omit the resolution-state filter and pass
`version: [ProtocolVersion.V1, ProtocolVersion.V2]`.
`version` accepts one value or an array of one or two values. Omission uses the
current server default of CTF markets (v1). Keep the same selection when resuming
a cursor.

`requestId` is the bytes32 oracle key. `onchainEventId` is the parent bytes32
on-chain event ID with its bottom three bytes zero. Both, and `resolutionStatus`,
can be `null` or absent when metadata is unavailable.

To narrow a subsequent read, pass `requestIds: [requestId]` or
`onchainEventIds: [onchainEventId]` when you have a returned non-null ID. Each
filter accepts at most 100 values and is retained across pages.

**Python**

Call `list_markets()` on an `AsyncPublicClient` or `AsyncSecureClient`.
The synchronous clients provide the same filters.

```python theme={null}
pages = client.list_markets(version="v2", resolution_status="active")

async for page in pages:
    for market in page.items:
        print(
            market.resolution.request_id,
            market.resolution.onchain_event_id,
            market.resolution.resolution_status,
        )
```

To query both protocols, omit the resolution-state filter and pass
`version=["v1", "v2"]`. `version` accepts one
string or enum value, or a sequence of one or two values. Omission or `None` uses
the current server default of CTF markets (v1). Keep the same selection when
resuming with `pages.from_cursor(saved_cursor)`.

`request_id` is the bytes32 oracle key. `onchain_event_id` is the parent bytes32
on-chain event ID with its bottom three bytes zero. Both, and
`resolution_status`, are `None` when metadata is unavailable.

To narrow a subsequent read, pass `request_ids=[request_id]` or
`onchain_event_ids=[onchain_event_id]` when you have a returned non-`None` ID.
Each filter accepts at most 100 values and is retained across pages.

**API**

Filter the market collection by protocol version and resolution state.

```bash theme={null}
curl -G "https://gamma-api.polymarket.com/markets/keyset" \
  --data-urlencode "version=v2" \
  --data-urlencode "resolution_status=active"
```

Illustrative response (fields trimmed):

> **Output: Markets Page**
>
> ```json theme={null}
> {
>   "markets": [
>     {
>       "id": "12345",
>       "slug": "example-v2-market",
>       "question": "Will Team A win?",
>       "version": "v2",
>       "requestId": "0xcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd00",
>       "onchainEventId": "0xababababababababababababababababababababababababababababab000000",
>       "resolutionStatus": "active"
>     }
>   ],
>   "next_cursor": "<next_cursor>"
> }
> ```

Omit `resolution_status` and repeat `version` as `version=v1&version=v2` to
query both protocols. Each
value must be `v1` or `v2`, with at most two values. Omission uses the current
server default of CTF markets (v1). Keep the same selection when continuing with
`after_cursor`.

Response markets can include `requestId`, `onchainEventId`, and
`resolutionStatus`. `requestId` is the bytes32 oracle key. `onchainEventId` is
the parent bytes32 on-chain event ID with its bottom three bytes zero.

Filter by a known ID with `request_ids=<request_id>` or
`onchain_event_ids=<onchain_event_id>`. Repeat the query parameter for multiple
IDs, up to 100 per filter, and preserve these filters when continuing with
`after_cursor`.

The resolution states are `inactive` (drafted, not deployed on chain), `active`
(deployed, unresolved), and `resolved`. This state belongs to Polymarket
Protocol V2 markets. CTF markets retain their UMA resolution status.
An oracle request identifies one binary or incremental neg-risk market by its
condition ID, or an atomic neg-risk event by its on-chain event ID. Markets in
one on-chain event share the parent event ID.

## Series

A series groups a recurring set of events under one theme. For example, a weekly Fed rate-decision series has one event per meeting, and a season-long league series has one event per matchup.

### Fetch a Series

Fetch a series by ID.

**TypeScript**

Call `fetchSeries()` on a `PublicClient` or `SecureClient` to fetch a series by ID.

```ts theme={null}
const series = await client.fetchSeries({ id: "1" });

// series: Series
```

> **Output: Series**
>
> ```json Example theme={null}
> {
>   "id": "1",
>   "slug": "nfl",
>   "title": "NFL",
>   "recurrence": "weekly",
>   "closed": false
> }
> ```

**Python**

Call `get_series()` on an `AsyncPublicClient` or `AsyncSecureClient` to fetch a series by ID.
The synchronous `PublicClient` and `SecureClient` provide the same method.

```python theme={null}
series = await client.get_series("1")

# series: Series
```

> **Output: Series**
>
> ```json Example theme={null}
> {
>   "id": "1",
>   "slug": "nfl",
>   "title": "NFL",
>   "recurrence": "weekly",
>   "closed": false
> }
> ```

**API**

Fetch a series by ID:

```bash theme={null}
curl "https://gamma-api.polymarket.com/series/1"
```

> **Output: Series**
>
> ```json Example theme={null}
> {
>   "id": "1",
>   "slug": "nfl",
>   "title": "NFL",
>   "recurrence": "weekly",
>   "closed": false
> }
> ```

### List Series

**TypeScript**

Call `listSeries()` on a `PublicClient` or `SecureClient` to page through series.

```ts theme={null}
const pages = client.listSeries({
  recurrence: "weekly",
  closed: false,
  pageSize: 20,
});

for await (const page of pages) {
  // page.items: Series[]
}
```

> **Output: Series\[]**
>
> ```json Example theme={null}
> [
>   { "id": "1", "slug": "nfl", "title": "NFL" },
>   { "id": "2", "slug": "nba", "title": "NBA" },
>   { "id": "3", "slug": "mlb", "title": "MLB" },
>   "..."
> ]
> ```

**Python**

Call `list_series()` on an `AsyncPublicClient` or `AsyncSecureClient` to page through series.
The synchronous `PublicClient` and `SecureClient` provide the same method.

```python theme={null}
pages = client.list_series(
    recurrence="weekly",
    closed=False,
    page_size=20,
)

async for page in pages:
    ...  # page.items: tuple[Series, ...]
```

> **Output: Series\[]**
>
> ```json Example theme={null}
> [
>   { "id": "1", "slug": "nfl", "title": "NFL" },
>   { "id": "2", "slug": "nba", "title": "NBA" },
>   { "id": "3", "slug": "mlb", "title": "MLB" },
>   "..."
> ]
> ```

**API**

List active weekly series:

```bash theme={null}
curl "https://gamma-api.polymarket.com/series?recurrence=weekly&closed=false&limit=20"
```

> **Output: Series\[]**
>
> ```json Example theme={null}
> [
>   { "id": "1", "slug": "nfl", "title": "NFL" },
>   { "id": "2", "slug": "nba", "title": "NBA" },
>   { "id": "3", "slug": "mlb", "title": "MLB" },
>   "..."
> ]
> ```

## Sports

Sports metadata maps a sport to its Polymarket tags and market types. Use it to browse sports markets by league, look up the valid market types for filtering, find events associated with a game, or resolve team rosters.

### List Sports

**TypeScript**

Call `listSports()` on a `PublicClient` or `SecureClient` to list supported sports.

```ts theme={null}
const sports = await client.listSports();

// sports: SportsMetadata[]
```

> **Output: SportsMetadata\[]**
>
> ```json Example theme={null}
> [
>   { "id": 1, "sport": "ncaab", "tags": "1,100149,100639" },
>   { "id": 2, "sport": "epl", "tags": "1,82,306,100639,100350" },
>   { "id": 3, "sport": "lal", "tags": "1,780,100639,100350" }
> ]
> ```

**Python**

Call `get_sports()` on an `AsyncPublicClient` or `AsyncSecureClient` to list supported sports.
The synchronous `PublicClient` and `SecureClient` provide the same method.

```python theme={null}
sports = await client.get_sports()

# sports: tuple[SportsMetadata, ...]
```

> **Output: SportsMetadata\[]**
>
> ```json Example theme={null}
> [
>   { "id": 1, "sport": "ncaab", "tags": "1,100149,100639" },
>   { "id": 2, "sport": "epl", "tags": "1,82,306,100639,100350" },
>   { "id": 3, "sport": "lal", "tags": "1,780,100639,100350" }
> ]
> ```

**API**

List supported sports:

```bash theme={null}
curl "https://gamma-api.polymarket.com/sports"
```

> **Output: SportsMetadata\[]**
>
> ```json Example theme={null}
> [
>   { "sport": "ncaab", "tags": "1,100149,100639" },
>   { "sport": "epl", "tags": "1,82,306,100639,100350" },
>   { "sport": "lal", "tags": "1,780,100639,100350" }
> ]
> ```

### Sports Market Types

Every sports market carries a market type that tells you which line it prices: a moneyline (which team wins), a spread (win by how much), or a total (over/under a combined score). Fetch the valid values, then pass one or more of them when listing markets to filter.

**TypeScript**

Call `fetchSportsMarketTypes()` on a `PublicClient` or `SecureClient` to list the
supported market types. When present, a market reports its type in
`sports.sportsMarketType`.

```ts theme={null}
const { marketTypes } = await client.fetchSportsMarketTypes();

// marketTypes: string[]
```

> **Output: string\[]**
>
> ```json Example theme={null}
> {
>   "marketTypes": ["moneyline", "spreads", "totals"]
> }
> ```

Filter markets by type:

```ts theme={null}
const pages = client.listMarkets({
  sportsMarketTypes: ["spreads"],
  closed: false,
  pageSize: 20,
});

for await (const page of pages) {
  // page.items: Market[]
}
```

**Python**

Call `get_sports_market_types()` on an `AsyncPublicClient` or
`AsyncSecureClient` to list the supported market types. The synchronous
`PublicClient` and `SecureClient` provide the same method. Each market
reports its type in `sports.sports_market_type` when present.

```python theme={null}
market_types = await client.get_sports_market_types()

# market_types.market_types: tuple[str, ...] | None
```

> **Output: string\[]**
>
> ```json Example theme={null}
> {
>   "market_types": ["moneyline", "spreads", "totals"]
> }
> ```

Filter markets by type:

```python theme={null}
pages = client.list_markets(
    sports_market_types=["spreads"],
    closed=False,
    page_size=20,
)

async for page in pages:
    ...  # page.items: tuple[Market, ...]
```

**API**

List supported sports market types. When present, a market reports its type in
`sportsMarketType`:

```bash theme={null}
curl "https://gamma-api.polymarket.com/sports/market-types"
```

> **Output: string\[]**
>
> ```json Example theme={null}
> {
>   "marketTypes": ["moneyline", "spreads", "totals"]
> }
> ```

Filter markets by type:

```bash theme={null}
curl "https://gamma-api.polymarket.com/markets/keyset?sports_market_types=spreads&closed=false&limit=20"
```

### List Events for a Game

A sports game's markets can be split across a main event and companion events. Depending on the game, companion events may cover player props, half-time and second-half results, exact score, or additional lines grouped under a more-markets event. Their slugs extend the main event's slug with a suffix such as `-player-props`, `-halftime-result`, or `-more-markets`.

Append a known suffix when you need one companion event. To discover current companion events, use the listing workflow instead of guessing each suffix.

**TypeScript**

Call `fetchEvent()` on a `PublicClient` or `SecureClient`. To fetch only the
more-markets event, append `-more-markets` to the main event's slug.

```ts theme={null}
const mainSlug = "mls-chi-vwh-2026-07-16";

const moreMarkets = await client.fetchEvent({
  slug: `${mainSlug}-more-markets`,
});
```

To list the game's current events, fetch the main event and read `sports.gameId`, then
pass it to `listEvents()`. Not every event has a game ID, so check it before filtering.

```ts theme={null}
const mainSlug = "mls-chi-vwh-2026-07-16";
const game = await client.fetchEvent({ slug: mainSlug });
const gameId = game.sports.gameId;

if (gameId == null) {
  throw new Error("Event does not have a game ID");
}

const pages = client.listEvents({ gameIds: [gameId], pageSize: 20 });

for await (const page of pages) {
  // page.items: Event[]
}
```

> **Output: Event Page**
>
> ```json Example theme={null}
> {
>   "items": [
>     {
>       "id": "662915",
>       "slug": "mls-chi-vwh-2026-07-16",
>       "title": "Chicago Fire FC vs. Vancouver Whitecaps FC"
>     },
>     {
>       "id": "662970",
>       "slug": "mls-chi-vwh-2026-07-16-halftime-result",
>       "title": "Chicago Fire FC vs. Vancouver Whitecaps FC - Halftime Result"
>     },
>     {
>       "id": "662972",
>       "slug": "mls-chi-vwh-2026-07-16-second-half-result",
>       "title": "Chicago Fire FC vs. Vancouver Whitecaps FC - Second Half Result"
>     },
>     {
>       "id": "662974",
>       "slug": "mls-chi-vwh-2026-07-16-exact-score",
>       "title": "Chicago Fire FC vs. Vancouver Whitecaps FC - Exact Score"
>     },
>     {
>       "id": "662976",
>       "slug": "mls-chi-vwh-2026-07-16-first-to-score",
>       "title": "Chicago Fire FC vs. Vancouver Whitecaps FC - First Team to Score"
>     },
>     {
>       "id": "663131",
>       "slug": "mls-chi-vwh-2026-07-16-more-markets",
>       "title": "Chicago Fire FC vs. Vancouver Whitecaps FC - More Markets"
>     }
>   ],
>   "hasMore": false
> }
> ```

When present, `market.sports.sportsMarketType` identifies lines in the more-markets
event such as `first_half_totals`, `both_teams_to_score`, and `soccer_team_totals`.

**Python**

Call `get_event()` on an `AsyncPublicClient` or `AsyncSecureClient`. To fetch only the
more-markets event, append `-more-markets` to the main event's slug. The synchronous
`PublicClient` and `SecureClient` provide the same method.

```python theme={null}
main_slug = "mls-chi-vwh-2026-07-16"

more_markets = await client.get_event(slug=f"{main_slug}-more-markets")
```

To list the game's current events, fetch the main event and read `sports.game_id`, then
pass it to `list_events()`. Not every event has a game ID, so check it before filtering.

```python theme={null}
main_slug = "mls-chi-vwh-2026-07-16"
game = await client.get_event(slug=main_slug)
game_id = game.sports.game_id

if game_id is None:
    raise ValueError("Event does not have a game ID")

pages = client.list_events(game_ids=[game_id], page_size=20)

async for page in pages:
    ...  # page.items: tuple[Event, ...]
```

> **Output: Event Page**
>
> ```json Example theme={null}
> {
>   "items": [
>     {
>       "id": "662915",
>       "slug": "mls-chi-vwh-2026-07-16",
>       "title": "Chicago Fire FC vs. Vancouver Whitecaps FC"
>     },
>     {
>       "id": "662970",
>       "slug": "mls-chi-vwh-2026-07-16-halftime-result",
>       "title": "Chicago Fire FC vs. Vancouver Whitecaps FC - Halftime Result"
>     },
>     {
>       "id": "662972",
>       "slug": "mls-chi-vwh-2026-07-16-second-half-result",
>       "title": "Chicago Fire FC vs. Vancouver Whitecaps FC - Second Half Result"
>     },
>     {
>       "id": "662974",
>       "slug": "mls-chi-vwh-2026-07-16-exact-score",
>       "title": "Chicago Fire FC vs. Vancouver Whitecaps FC - Exact Score"
>     },
>     {
>       "id": "662976",
>       "slug": "mls-chi-vwh-2026-07-16-first-to-score",
>       "title": "Chicago Fire FC vs. Vancouver Whitecaps FC - First Team to Score"
>     },
>     {
>       "id": "663131",
>       "slug": "mls-chi-vwh-2026-07-16-more-markets",
>       "title": "Chicago Fire FC vs. Vancouver Whitecaps FC - More Markets"
>     }
>   ],
>   "has_more": false
> }
> ```

When present, `market.sports.sports_market_type` identifies lines in the more-markets
event such as `first_half_totals`, `both_teams_to_score`, and `soccer_team_totals`.

**API**

To fetch only the more-markets event, append `-more-markets` to the main event's slug:

```bash theme={null}
curl "https://gamma-api.polymarket.com/events/slug/mls-chi-vwh-2026-07-16-more-markets"
```

To list the game's current events, fetch the main event and read its `gameId`:

```bash theme={null}
curl "https://gamma-api.polymarket.com/events/slug/mls-chi-vwh-2026-07-16"
```

Pass the ID as `game_id`:

```bash theme={null}
curl "https://gamma-api.polymarket.com/events/keyset?game_id=90104306&limit=20"
```

> **Output: Events Page**
>
> ```json Example theme={null}
> {
>   "events": [
>     {
>       "id": "662915",
>       "slug": "mls-chi-vwh-2026-07-16",
>       "title": "Chicago Fire FC vs. Vancouver Whitecaps FC",
>       "gameId": 90104306
>     },
>     {
>       "id": "662970",
>       "slug": "mls-chi-vwh-2026-07-16-halftime-result",
>       "title": "Chicago Fire FC vs. Vancouver Whitecaps FC - Halftime Result",
>       "gameId": 90104306
>     },
>     {
>       "id": "662972",
>       "slug": "mls-chi-vwh-2026-07-16-second-half-result",
>       "title": "Chicago Fire FC vs. Vancouver Whitecaps FC - Second Half Result",
>       "gameId": 90104306
>     },
>     {
>       "id": "662974",
>       "slug": "mls-chi-vwh-2026-07-16-exact-score",
>       "title": "Chicago Fire FC vs. Vancouver Whitecaps FC - Exact Score",
>       "gameId": 90104306
>     },
>     {
>       "id": "662976",
>       "slug": "mls-chi-vwh-2026-07-16-first-to-score",
>       "title": "Chicago Fire FC vs. Vancouver Whitecaps FC - First Team to Score",
>       "gameId": 90104306
>     },
>     {
>       "id": "663131",
>       "slug": "mls-chi-vwh-2026-07-16-more-markets",
>       "title": "Chicago Fire FC vs. Vancouver Whitecaps FC - More Markets",
>       "gameId": 90104306
>     }
>   ]
> }
> ```

If the response includes `next_cursor`, pass it as `after_cursor` in the next request.
When present, `market.sportsMarketType` identifies lines in the more-markets event such
as `first_half_totals`, `both_teams_to_score`, and `soccer_team_totals`.

### Identify Home and Away Teams

Sports event responses identify which team is designated home and which is
designated away. Use this value when aligning Polymarket data with an external
sports feed.

**TypeScript**

Use `PublicClient.fetchEvent()` or `SecureClient.fetchEvent()` to identify the teams.

```ts theme={null}
import { TeamOrdering } from "@polymarket/client";

const event = await client.fetchEvent({
  slug: "fl1-psg-ren-2026-08-23",
});
const homeTeam = event.sports.teams.find(
  (team) => team.ordering === TeamOrdering.Home,
);
const awayTeam = event.sports.teams.find(
  (team) => team.ordering === TeamOrdering.Away,
);
```

> **Output: Team**
>
> ```json theme={null}
> {
>   "id": 114315,
>   "name": "Paris Saint-Germain FC",
>   "league": "fl1",
>   "abbreviation": "psg",
>   "ordering": "home"
> }
> ```

Each `Team.ordering` value is `TeamOrdering.Home`, `TeamOrdering.Away`, or `null`.

**Python**

Use `AsyncPublicClient.get_event()` or `AsyncSecureClient.get_event()` to identify the teams.

```python theme={null}
from polymarket import TeamOrdering


event = await client.get_event(slug="fl1-psg-ren-2026-08-23")
home_team = next(
    (team for team in event.sports.teams if team.ordering is TeamOrdering.HOME),
    None,
)
away_team = next(
    (team for team in event.sports.teams if team.ordering is TeamOrdering.AWAY),
    None,
)
```

> **Output: Team**
>
> ```json theme={null}
> {
>   "id": 114315,
>   "name": "Paris Saint-Germain FC",
>   "league": "fl1",
>   "abbreviation": "psg",
>   "ordering": "home"
> }
> ```

Each `Team.ordering` value is `TeamOrdering.HOME`, `TeamOrdering.AWAY`, or `None`.

**API**

Fetch the event by slug to identify its home and away teams.

```bash theme={null}
curl "https://gamma-api.polymarket.com/events/slug/fl1-psg-ren-2026-08-23"
```

> **Output: Event**
>
> ```jsonc theme={null}
> {
>   "slug": "fl1-psg-ren-2026-08-23",
>   "title": "Paris Saint-Germain FC vs. Stade Rennais FC 1901",
>   // …
>   "teams": [
>     {
>       "id": 114315,
>       "name": "Paris Saint-Germain FC",
>       // …
>       "ordering": "home",
>     },
>     {
>       "id": 114317,
>       "name": "Stade Rennais FC 1901",
>       // …
>       "ordering": "away",
>     },
>   ],
> }
> ```

Read `ordering` from each entry in the event's top-level `teams` field. Its
value is `"home"`, `"away"`, or `null`.

> [!WARNING]
>
> Home and away assignments can change after markets are created, especially
> when games are rescheduled or moved. If you plan to submit quotes in Combos,
> check [Common Footguns](https://docs.polymarket.com/trading/combos/market-makers.md#common-footguns).

### List Teams

**TypeScript**

Call `listTeams()` on a `PublicClient` or `SecureClient` to page through teams.

```ts theme={null}
const pages = client.listTeams({ league: ["nba"], pageSize: 20 });

for await (const page of pages) {
  // page.items: Team[]
}
```

> **Output: Team\[]**
>
> ```json Example theme={null}
> [
>   {
>     "id": 114168,
>     "name": "Candace's Rising Stars",
>     "league": "nba",
>     "abbreviation": "crs"
>   },
>   {
>     "id": 114166,
>     "name": "Chuck's Global Stars",
>     "league": "nba",
>     "abbreviation": "cgs"
>   },
>   "..."
> ]
> ```

**Python**

Call `list_teams()` on an `AsyncPublicClient` or `AsyncSecureClient` to page
through teams. The synchronous `PublicClient` and `SecureClient` provide the
same method.

```python theme={null}
pages = client.list_teams(league="nba", page_size=20)

async for page in pages:
    ...  # page.items: tuple[Team, ...]
```

> **Output: Team\[]**
>
> ```json Example theme={null}
> [
>   {
>     "id": 114168,
>     "name": "Candace's Rising Stars",
>     "league": "nba",
>     "abbreviation": "crs"
>   },
>   {
>     "id": 114166,
>     "name": "Chuck's Global Stars",
>     "league": "nba",
>     "abbreviation": "cgs"
>   },
>   "..."
> ]
> ```

**API**

List teams in a league:

```bash theme={null}
curl "https://gamma-api.polymarket.com/teams?league=nba&limit=20"
```

> **Output: Team\[]**
>
> ```json Example theme={null}
> [
>   {
>     "id": 114168,
>     "name": "Candace's Rising Stars",
>     "league": "nba",
>     "abbreviation": "crs"
>   },
>   "..."
> ]
> ```

## Search

Search returns matching events, tags, and profiles for a free-text query in one call. Use it for a search bar or a "jump to market" input.

**TypeScript**

Call `search()` on a `PublicClient` or `SecureClient` to search Polymarket.

```ts theme={null}
const pages = client.search({ q: "aliens", pageSize: 10 });

for await (const page of pages) {
  // page.items.events: Event[]
  // page.items.tags: SearchTag[]
  // page.items.profiles: Profile[]
}
```

> **Output: SearchResults**
>
> ```ts SearchResults Type theme={null}
> type Event = {
>   id: string;
>   slug?: string | null;
>   title?: string | null;
> };
>
> type SearchTag = {
>   id: string;
>   label?: string | null;
>   slug?: string | null;
>   eventCount?: number | null;
> };
>
> type Profile = {
>   pseudonym?: string | null;
>   wallet?: string | null;
> };
>
> type SearchResults = {
>   events: Event[];
>   tags: SearchTag[];
>   profiles: Profile[];
> };
> ```
>
> ```json SearchResults Example theme={null}
> {
>   "events": [
>     {
>       "id": "90177",
>       "slug": "will-the-us-confirm-that-aliens-exist-before-2027",
>       "title": "Will the US confirm that aliens exist by...?"
>     },
>     "..."
>   ],
>   "tags": [],
>   "profiles": []
> }
> ```

**Python**

Call `search()` on an `AsyncPublicClient` or `AsyncSecureClient` to search
Polymarket. The synchronous `PublicClient` and `SecureClient` provide the
same method.

```python theme={null}
pages = client.search(q="aliens", page_size=10)

async for page in pages:
    for search_results in page.items:
        # search_results.events: tuple[Event, ...]
        # search_results.tags: tuple[SearchTag, ...]
        # search_results.profiles: tuple[Profile, ...]
        ...
```

> **Output: SearchResults**
>
> ```python SearchResults Type theme={null}
> class Event:
>     id: str
>     slug: str | None
>     title: str | None
>
> class SearchTag:
>     id: str
>     label: str | None
>     slug: str | None
>     event_count: int | None
>
> class Profile:
>     pseudonym: str | None
>     wallet: str | None
>
> class SearchResults:
>     events: tuple[Event, ...]
>     tags: tuple[SearchTag, ...]
>     profiles: tuple[Profile, ...]
> ```
>
> ```json SearchResults Example theme={null}
> {
>   "events": [
>     {
>       "id": "90177",
>       "slug": "will-the-us-confirm-that-aliens-exist-before-2027",
>       "title": "Will the US confirm that aliens exist by...?"
>     },
>     "..."
>   ],
>   "tags": [],
>   "profiles": []
> }
> ```

**API**

Search Polymarket:

```bash theme={null}
curl "https://gamma-api.polymarket.com/public-search?q=aliens"
```

> **Output: Search**
>
> ```json Example theme={null}
> {
>   "events": [
>     {
>       "id": "90177",
>       "slug": "will-the-us-confirm-that-aliens-exist-before-2027",
>       "title": "Will the US confirm that aliens exist by...?"
>     },
>     "..."
>   ],
>   "tags": [],
>   "profiles": [],
>   "pagination": {
>     "hasMore": true,
>     "totalResults": 28
>   }
> }
> ```

## Tags

Tags group events and markets by categories, leagues, people, and themes. Each
tag can link to multiple ranked related tags, forming a directed graph rather
than a strict tree. Use these relationships to expand discovery from one topic
to adjacent topics.

### List Tags

Browse the available tags, for example to build a category filter.

**TypeScript**

Call `listTags()` on a `PublicClient` or `SecureClient` to page through tags.

```ts theme={null}
const pages = client.listTags({ pageSize: 20 });

for await (const page of pages) {
  // page.items: Tag[]
}
```

> **Output: Tag\[]**
>
> ```json Example theme={null}
> [
>   {
>     "id": "101867",
>     "slug": "product-marekt-fit",
>     "label": "product marekt fit"
>   },
>   { "id": "1512", "slug": "caitlin-clark", "label": "caitlin clark" },
>   "..."
> ]
> ```

**Python**

Call `list_tags()` on an `AsyncPublicClient` or `AsyncSecureClient` to page
through tags. The synchronous `PublicClient` and `SecureClient` provide the
same method.

```python theme={null}
pages = client.list_tags(page_size=20)

async for page in pages:
    ...  # page.items: tuple[Tag, ...]
```

> **Output: Tag\[]**
>
> ```json Example theme={null}
> [
>   {
>     "id": "101867",
>     "slug": "product-marekt-fit",
>     "label": "product marekt fit"
>   },
>   { "id": "1512", "slug": "caitlin-clark", "label": "caitlin clark" },
>   "..."
> ]
> ```

**API**

List tags:

```bash theme={null}
curl "https://gamma-api.polymarket.com/tags?limit=20"
```

> **Output: Tag\[]**
>
> ```json Example theme={null}
> [
>   {
>     "id": "101867",
>     "slug": "product-marekt-fit",
>     "label": "product marekt fit"
>   },
>   { "id": "1512", "slug": "caitlin-clark", "label": "caitlin clark" },
>   "..."
> ]
> ```

### Fetch a Tag

Fetch a tag by slug to resolve its numeric ID. You'll need that ID for the `tagId`/`tagIds` filters used earlier in Events and Markets.

**TypeScript**

Call `fetchTag()` on a `PublicClient` or `SecureClient` to fetch a tag by slug.

```ts theme={null}
const nba = await client.fetchTag({ slug: "nba" });

// nba: Tag
```

> **Output: Tag**
>
> ```json Example theme={null}
> { "id": "745", "slug": "nba", "label": "NBA" }
> ```

**Python**

Call `get_tag()` on an `AsyncPublicClient` or `AsyncSecureClient` to fetch a
tag by slug. The synchronous `PublicClient` and `SecureClient` provide the
same method.

```python theme={null}
nba = await client.get_tag(slug="nba")

# nba: Tag
```

> **Output: Tag**
>
> ```json Example theme={null}
> { "id": "745", "slug": "nba", "label": "NBA" }
> ```

**API**

Fetch a tag by slug:

```bash theme={null}
curl "https://gamma-api.polymarket.com/tags/slug/nba"
```

> **Output: Tag**
>
> ```json Example theme={null}
> { "id": "745", "slug": "nba", "label": "NBA" }
> ```

### Fetch Tag Relationships

Get the relationship records that link a tag to related tags, useful for building "you might also like" surfaces. Each record is a lightweight pointer (a rank and the two numeric tag IDs), not a full tag object.

**TypeScript**

Call `fetchRelatedTags()` on a `PublicClient` or `SecureClient` to fetch a tag's
relationship records.

```ts theme={null}
const relatedTags = await client.fetchRelatedTags({ slug: "nba" });

// relatedTags: RelatedTag[]
```

> **Output: RelatedTag\[]**
>
> ```json Example theme={null}
> [{ "id": "58212", "tagId": 745, "relatedTagId": 1512, "rank": 1 }]
> ```

**Python**

Call `get_related_tags()` on an `AsyncPublicClient` or `AsyncSecureClient`
to fetch a tag's relationship records. The synchronous `PublicClient` and
`SecureClient` provide the same method.

```python theme={null}
related_tags = await client.get_related_tags(slug="nba")

# related_tags: tuple[RelatedTag, ...]
```

> **Output: RelatedTag\[]**
>
> ```json Example theme={null}
> [{ "id": "58212", "tag_id": 745, "related_tag_id": 1512, "rank": 1 }]
> ```

**API**

Fetch a tag's relationship records:

```bash theme={null}
curl "https://gamma-api.polymarket.com/tags/slug/nba/related-tags"
```

> **Output: RelatedTag\[]**
>
> ```json Example theme={null}
> [{ "id": "58212", "tagID": 745, "relatedTagID": 1512, "rank": 1 }]
> ```

### Fetch Related Tags

Get the full `Tag` objects for a tag's related tags in one call, skipping the extra round trip to fetch each related tag by ID.

**TypeScript**

Call `fetchRelatedTagResources()` on a `PublicClient` or `SecureClient` to fetch the
related tag objects.

```ts theme={null}
const relatedTagObjects = await client.fetchRelatedTagResources({
  slug: "nba",
  status: "active",
});

// relatedTagObjects: Tag[]
```

> **Output: Tag\[]**
>
> ```json Example theme={null}
> [{ "id": "1512", "slug": "caitlin-clark", "label": "caitlin clark" }]
> ```

**Python**

Call `get_related_tag_resources()` on an `AsyncPublicClient` or
`AsyncSecureClient` to fetch the related tag objects. The synchronous
`PublicClient` and `SecureClient` provide the same method.

```python theme={null}
related_tag_objects = await client.get_related_tag_resources(
    slug="nba",
    status="active",
)

# related_tag_objects: tuple[Tag, ...]
```

> **Output: Tag\[]**
>
> ```json Example theme={null}
> [{ "id": "1512", "slug": "caitlin-clark", "label": "caitlin clark" }]
> ```

**API**

Fetch the related tag objects:

```bash theme={null}
curl "https://gamma-api.polymarket.com/tags/slug/nba/related-tags/tags"
```

> **Output: Tag\[]**
>
> ```json Example theme={null}
> [{ "id": "1512", "slug": "caitlin-clark", "label": "caitlin clark" }]
> ```


This documentation is built and hosted on [Mintlify](https://mintlify.com), a developer documentation platform.