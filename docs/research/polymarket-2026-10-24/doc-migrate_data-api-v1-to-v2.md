> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.com/llms.txt
> Use this file to discover all available pages before exploring further.

# Migrate from Data API v1 to v2

> Move an integration from the original Data API routes to their v2 counterparts: the route mapping, the contract changes, and what stays on v1.

Data API v2 replaces the original per-route contracts with one shared contract
across every read. You can migrate one call site at a time, but new
integrations should start on v2 directly.

Both versions are served at `https://data-api.polymarket.com`, with v2 routes
under the `/v2` prefix.

> [!WARNING]
>
> **Data API v1 is retired on October 24, 2026.** Move your integration to the
> v2 routes in the mapping below before then.

## What Changed

**Response envelope.** v1 routes return bare arrays or objects. Every v2
response wraps its payload in `data`, and paginated routes add a `pagination`
object. A documented miss is `data: null` or an empty list, never an error.

**Pagination.** v1 pages with `limit`/`offset`, and `offset` stops at 10,000
rows. v2 pages with an opaque cursor: follow `pagination.next_cursor` until it
is `null`, with no offset arithmetic to manage, and the feed routes stay
consistent while new rows arrive. See
[Paginate With Cursors](https://docs.polymarket.com/api-reference/data-api/overview.md#paginate-with-cursors).

**Field casing.** v1 responses are `camelCase` (`proxyWallet`, `conditionId`).
v2 responses are `snake_case` (`proxy_wallet`, `condition_id`). Request
parameters accept both spellings on v2.

**Market selection.** v1 selects markets with the `market` parameter. v2
unifies on `condition` (aliases `condition_id`, `conditionId`), taking at most
20 distinct comma-separated condition ids. `event_id` filtering carries over.

**Rejected v1 parameters.** v2 returns HTTP `400` for obsolete `market`,
`market_id`, `marketId`, and `offset` parameters rather than ignoring them.
Replace market filters with `condition`, `condition_id`, or `conditionId`.
Remove `offset` even when it is `0`: use `cursor` for paginated reads and omit
pagination parameters for nonpaginated reads.

**Position lifecycle.** v2 folds three v1 routes into one:
`GET /v2/positions` serves the whole lifecycle behind a `status` filter
(`OPEN`, `REDEEMABLE`, `REDEEMABLE_LOST`, `MERGEABLE`, `CLOSED`) with
`redeemable` and `mergeable` flags on every row, replacing the separate
`/closed-positions` and `/v1/market-positions` routes. `REDEEMABLE_LOST`
narrows to still-held losing positions, and `MERGEABLE` to live complementary
pairs a wallet can merge back to collateral.

## Route Mapping

| v1 route | v2 route | Notes |
| - | - | - |
| `GET /positions` | `GET /v2/positions` | `status` defaults to open positions. `market` becomes `condition` |
| `GET /closed-positions` | `GET /v2/positions?status=CLOSED` | Folded into the unified lifecycle |
| `GET /v1/market-positions` | `GET /v2/positions?condition=…` | Market-anchored listing through the same route |
| `GET /trades` | `GET /v2/trades` | |
| `GET /activity` | `GET /v2/activity` | |
| `GET /v1/activity/combos` | `GET /v2/activity/combos` | |
| `GET /v1/positions/combos` | `GET /v2/positions/combos` | |
| `GET /value` | `GET /v2/value` | |
| `GET /traded` | `GET /v2/user-stats` | `data.trades` is the distinct-market count. Unknown users return `data: null` |
| `GET /holders` | `GET /v2/holders` | Optional `include_pnl=true` adds entry and PnL economics per holder |
| `GET /oi` | `GET /v2/oi` | |
| `GET /live-volume` | `GET /v2/live-volume` | |
| `GET /v1/leaderboard` | `GET /v2/leaderboard` | |
| `GET /v1/builders/leaderboard` | `GET /v2/builders/leaderboard` | |
| `GET /v1/builders/volume` | `GET /v2/builders/volume` | |
| | `GET /v2/approvals` | |

Each v2 endpoint page documents its full parameter set and row shape; several
routes accept filters their v1 counterparts did not.

## New in v2

v2 adds these reads. User stats also covers the existing traded-market count:

| Route | Returns |
| - | - |
| `GET /v2/user-pnl` | A user's PnL series |
| `GET /v2/user-stats` | A user's profile stats |
| `GET /v2/user-volume` | A user's trading volume |
| `GET /v2/biggest-winners` | The biggest wins |
| `GET /v2/prices-history` | A token's price history |
| `GET /v2/resolutions` | Resolution state |
| `GET /v2/tokens` | Token lookup by token id or condition |
| `GET /v2/status` | Data freshness |

## Staying on v1

`GET /v1/accounting/snapshot` has no v2 counterpart and is not part of the
retirement above. Keep calling its existing route.

## SDK Migration to Data API V2

Data API v2 is supported starting with version `0.10.0` in both official SDKs:
the TypeScript package `@polymarket/client` and the Python package
`polymarket-client`. Review the
[TypeScript SDK changelog](https://docs.polymarket.com/changelog/sdks.md#typescript) or
[Python SDK changelog](https://docs.polymarket.com/changelog/sdks.md#python) for the breaking changes,
renamed methods, response fields, and pagination behavior to update when
migrating.


This documentation is built and hosted on [Mintlify](https://mintlify.com), a developer documentation platform.