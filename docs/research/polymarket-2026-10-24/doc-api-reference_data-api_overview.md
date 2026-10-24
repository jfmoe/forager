> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.com/llms.txt
> Use this file to discover all available pages before exploring further.

# Data API v2

> Read wallet portfolios, trade and activity feeds, market state, and ranked leaderboards through one consistent response contract.

The Data API answers what happened and who holds what: wallet portfolios and
PnL, trade and activity feeds, per-market state such as holders and open
interest, and ranked boards. Version 2 serves every read behind one contract, a
shared response envelope, cursor pagination, and a unified identifier
vocabulary, so a client written against one endpoint already knows how to
consume the rest.

All v2 routes are served from:

```
https://data-api.polymarket.com/v2
```

No API key or authentication is required.

## Make a First Request

Fetch a wallet's current positions:

```bash theme={null}
curl "https://data-api.polymarket.com/v2/positions?user=0x983eedfbd75803602e4a6e6ea9aab6dc6b9c6748&limit=1"
```

Every response wraps its payload in `data`; paginated routes add a `pagination`
object (row fields trimmed here for brevity):

```json theme={null}
{
  "data": [
    {
      "proxy_wallet": "0x983eedfbd75803602e4a6e6ea9aab6dc6b9c6748",
      "condition_id": "0xd9b06e2fd9ddb7ab61c9e3d5d8e074c555802478bbf75145804ff709a4246f79",
      "token_id": "31974447302330162086995746309500877260929998201718217388109724292047967921664",
      "outcome": "Yes",
      "title": "Will ŠK Slovan Bratislava win on 2026-08-19?",
      "status": "REDEEMABLE",
      "current_size": 86780.64,
      "avg_price": 0.5203,
      "entry_cost_usdc": 45159.4653,
      "current_value": 0.0,
      "realized_pnl": -1082.5533,
      "unrealized_pnl": -45159.4653
    }
  ],
  "pagination": {
    "limit": 1,
    "offset": 0,
    "has_more": true,
    "next_cursor": "eyJkYXRhIjp7InR5cGUiOiJwb3NpdGlvbnMi…"
  }
}
```

A documented miss is `data: null` or an empty list, never an error. Each
endpoint's reference page describes its full row shape and every filter.

## Paginate With Cursors

Pagination is cursor-only; there is no `offset` query parameter. To walk a
result set:

1. Send the first request with an optional `limit` (each endpoint documents its
   default and maximum).
2. Read `pagination.next_cursor` from the response and re-send the request with
   `cursor=<next_cursor>`.
3. Stop when `next_cursor` is `null`. `has_more` is exact, so an empty or short
   page does not mean the walk is over.

Cursors are opaque, signed, and typed per endpoint. Two rules keep a walk
consistent:

* On the feeds (`/v2/trades`, `/v2/activity`, `/v2/activity/combos`), re-send
  the same filters on every page. The cursor carries only its seek anchor, so
  changing a filter mid-walk silently re-anchors the feed.
* Where a cursor binds its query (positions, boards, combo positions),
  restating the same values is fine, but contradicting them returns a `400`.
  On `/v2/positions`, keep sending the same `user` or `condition` anchor and
  all filters, including `title`, any narrowing `condition`, and `start`/`end`.
  These filters are not carried by the cursor; omitting them can broaden the
  result set. The cursor binds the anchor, status, sort, and direction.
  On `/v2/positions/combos`, keep sending `user` alongside the cursor. On
  `/v2/holders`, keep sending `condition`. Other routes that bind the query can
  resume with the bare cursor.

The `limit` parameter only applies to the first page; once a `cursor` is
supplied, the cursor's own page size wins. The `offset` field in `pagination`
is display metadata for numbering rows across pages, not a request parameter.

## Shared Conventions

Every v2 endpoint follows the same vocabulary and encoding rules.

### Identifiers

| Identifier | Meaning |
| - | - |
| `condition` | The unified query key for on-chain condition ids (`0x`-prefixed, 64 hex chars). `condition_id` and `conditionId` are accepted aliases. Accepts up to 20 distinct comma-separated values where lists are allowed. |
| `event_id` | An event id, as served by the Gamma API's `/events` routes. |
| `market_id` | A market id, as served by the Gamma API's `/markets` routes. Carried in response rows next to the on-chain `condition_id`. |
| `token_id` | An outcome token id. The key for `/v2/prices-history`. |

Query parameters accept both `snake_case` and `camelCase` spellings.

### Units and Sentinels

* Bare `volume` and `size` values are outcome **shares**; fields suffixed
  `_usdc` are USD amounts; volumes prefixed `taker_` count one side of each
  trade.
* All amounts are JSON numbers.
* `outcome_index: 999` means the outcome could not be labeled.
* A missing or `null` numeric field means unavailable, never zero.

### Time Windows

Windowed routes take `start` and `end` as epoch seconds. Treatment of an
omitted or `0` bound differs by route (for example, `/v2/activity` floors an
omitted `start` to three years back, while `/v2/prices-history` rejects a `0`
bound with a `400`), so check the parameter documentation on each endpoint
page before relying on a default.

### Price History Resolution and Availability

`/v2/prices-history` serves each token from a graded store: a rolling
raw window (one-minute serving) plus five-minute, thirty-minute, three-hour,
and twelve-hour series. The coarse pair is **permanent**, so every token's
full history is always available at twelve-hour grain (and at three-hour
grain from July 2026 onward); the fine grains are windowed. The availability
**minimums** are: raw at least 7 days, five-minute at least 60 days,
thirty-minute at least 90 days. These are floors, not exact horizons: data
expires in multi-day chunks, so a grain usually reaches a few days further
back than its minimum. `bucket_seconds=60` over a window starting a month ago
is therefore an incompatible pair: both values are valid, and there is no
data at their intersection.

The two ways of asking behave differently on purpose:

* **Send `bucket_seconds`** and the request means exactly that grid: the
  response holds every stored observation snapped to it and nothing else.
  Over a range where the store has nothing at or below that grain the page is
  empty, and where the stored history is coarser than the grid the page is
  sparse. Both are correct answers, never a silent substitution at a
  different width.
* **Omit it** and the server measures, per request, which series actually
  cover this token and window, and serves the densest one: sized to the span,
  then coarsened only as far as coverage requires. Clients should not build
  their own availability tables; coverage is discovered from the data on
  every request, and it extends as the store grows.

Two consequences worth internalizing:

* On a pinned request, `resolution_seconds` echoes the requested grid, not
  the density of what filled it. Counting rows is the only density measure.
* Deep history predates the fine grains: before July 2026 the store's finest
  observations are three-hour or twelve-hour points, so a pinned fine bucket
  over old ranges returns those observations on your grid, sparsely.

For a multi-day window, use `bucket_seconds` of 300, 1800, 10800, or 43200,
or omit it. A 60-second width over several days is both slow and, past the
raw window, empty.

### Errors and Rate Limits

Request errors return a `400` with a message envelope:

```json theme={null}
{ "error": "required query param 'user' or 'condition' not provided" }
```

Heavy load answers with `429` and a `Retry-After` header; retry after the
given delay. The IP-based request limits for `/v2` routes are listed on the
[Rate Limits](https://docs.polymarket.com/api-reference/rate-limits.md) page.

## Endpoints

> **Wallet**
>
> | Endpoint | Returns |
> | - | - |
> | `GET /v2/positions` | Positions for a user or market, across the whole lifecycle (`OPEN`, `REDEEMABLE`, `REDEEMABLE_LOST`, `MERGEABLE`, `CLOSED`) |
> | `GET /v2/positions/combos` | Combo positions |
> | `GET /v2/value` | Portfolio value |
> | `GET /v2/approvals` | Wallet approvals |
> | `GET /v2/user-pnl` | [A user's PnL series](https://docs.polymarket.com/trading/wallet-activity.md#wallet-pnl-history) |
> | `GET /v2/user-stats` | [A user's profile stats](https://docs.polymarket.com/trading/wallet-activity.md#wallet-stats) |
> | `GET /v2/user-volume` | [A user's trading volume](https://docs.polymarket.com/trading/wallet-activity.md#wallet-trading-volume) |

> **Feeds**
>
> | Endpoint | Returns |
> | - | - |
> | `GET /v2/trades` | Trades for a wallet, market, event, or the global feed |
> | `GET /v2/activity` | Account activity |
> | `GET /v2/activity/combos` | Combo activity |

> **Markets**
>
> | Endpoint | Returns |
> | - | - |
> | `GET /v2/holders` | A market's top holders |
> | `GET /v2/oi` | Open interest |
> | `GET /v2/live-volume` | Live volume for an event |
> | `GET /v2/prices-history` | A token's price history |
> | `GET /v2/resolutions` | [Resolution state](https://docs.polymarket.com/market-data/public-analytics.md#market-resolution) |
> | `GET /v2/tokens` | [Token lookup](https://docs.polymarket.com/market-data/public-analytics.md#token-lookup): token ids or conditions to markets, outcomes and neg-risk structure |

> **Boards**
>
> | Endpoint | Returns |
> | - | - |
> | `GET /v2/leaderboard` | The trader leaderboard |
> | `GET /v2/biggest-winners` | [The biggest wins](https://docs.polymarket.com/market-data/public-analytics.md#biggest-winners) |
> | `GET /v2/builders/leaderboard` | The builders leaderboard |
> | `GET /v2/builders/volume` | Builder volume over time |

> **Service**
>
> | Endpoint | Returns |
> | - | - |
> | `GET /v2/status` | Data freshness |

The complete machine-readable contract is published at
[`https://data-api.polymarket.com/v2/openapi.json`](https://data-api.polymarket.com/v2/openapi.json),
with an interactive explorer at
[`https://data-api.polymarket.com/v2/docs`](https://data-api.polymarket.com/v2/docs).

## Next Steps

* Coming from the v1 Data API routes? See
  [Migrate from Data API v1 to v2](https://docs.polymarket.com/migrate/data-api-v1-to-v2.md).
* Browse the endpoint pages in the sidebar for full parameter and response
  documentation.


This documentation is built and hosted on [Mintlify](https://mintlify.com), a developer documentation platform.