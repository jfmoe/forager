> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.com/llms.txt
> Use this file to discover all available pages before exploring further.

# Predictions Changelog

> Important changes to Polymarket prediction markets, including the CLOB, APIs, web application, and mobile applications.

**Oct 21, 2026**

PolyBolt: seven new forex pairs on price.equity

* **New symbols**: `usdchf`, `usdmxn`, `usdzar`, `usdbrl`, `usdnok`, `usdsek` and `usdtry` join `price.equity`, served by Pyth (the feeds the forex up-or-down markets resolve on). See [Supported Symbols](https://docs.polymarket.com/api-reference/live-data/overview.md#supported-symbols).
* `usdbrl` trades on weekdays only, so outside those hours its `market_status` reads `closed` and the snapshot carries the closing price.

**Oct 21, 2026**

PolyBolt: market status on equity prices

* **New field**: `price.equity` and `price.equity.twap` payloads carry `market_status` (`open`, `closed` or `unknown`) on every live update and once per snapshot batch, so a client can tell a closed market from a stalled price. The field is additive and absent when the producer states nothing; treat absent as not stated. See [Read the Market Status](https://docs.polymarket.com/api-reference/live-data/overview.md#read-the-market-status).
* **Status changes arrive as updates**: a change without a new price repeats the last price with the new status and `is_carried_forward: true`, so an update can share its `timestamp` with the one before it. Do not discard updates only because their `timestamp` repeats.
* **Closed snapshots keep the closing price**: while a series is `closed` or `unknown`, its snapshot carries the last price point even when it is older than two minutes.
* **Per provider**: the status describes the instrument each provider prices. Chainlink's around-the-clock equity products read `open` outside exchange hours, while a symbol pinned to `pyth` follows its exchange session.
* **SDKs**: the TypeScript and Python SDKs do not expose `market_status` yet; read it from the raw WebSocket payload. The machine-readable contract at `https://ws-live-v2.polymarket.com/asyncapi.json` carries the field.

**Oct 7, 2026**

Data API: /v2/tokens, plus the fixes shipped since September 28

* **New endpoint**: `GET /v2/tokens` resolves up to 50 ERC-1155 token ids per call (`token_id`, request order kept, unknown ids absent) or lists every token of up to 10 conditions (`condition`). A condition selector is any id the other routes carry: a ConditionalTokens condition id, a neg-risk market id (every market of the structure, in question order), a polymarket-v2 condition id in its 64- or 66-character form, or the on-chain event id of a polymarket-v2 neg-risk structure. Exactly one selector per request. See [Token Lookup](https://docs.polymarket.com/market-data/public-analytics.md#token-lookup).
* **One row per token**: `condition_id` in the 66-character form every route uses, `structural_condition_id` (the polymarket-v2 id in the same form, null on ConditionalTokens tokens), `module` (`v1_ctf` is the ConditionalTokens market; every other value is a polymarket-v2 module), outcome slot and label, the opposite token, on-chain settlement (`resolved`, `final_price`), market and event identity, and `question_index` for both neg-risk families. Not exposed in the TypeScript and Python SDKs yet; call the route over HTTP.
* **`/v2/trades`**: a request with `user` and several `condition` values now reads one bounded stream per market and merges them, instead of scanning the wallet's whole history. Same rows and order, faster pages.
* **`/v2/activity`**: rows for combo tokens carry their overall `Yes`/`No` outcome and outcome index on trades and redemptions. They were blank.
* **Settled tokens priced at settlement**: on `/v2/positions` with a `condition` selector and on `/v2/oi`, a resolved outcome is valued at its settlement payout (0 or 1) before the last order-book mid, so a stale book on a settled market no longer moves current values or open interest.
* **`/v2/positions`**: rows carry `first_entry_at`, the epoch seconds of the position's first acquisition (buy, split, mint, migration or transfer in), `0` when none is recorded; the same unit and sentinel as `last_event_at`, and an integer unlike the string `first_entry_at` on `/v2/positions/combos`. For a user's `CLOSED` positions, `sort_by=TOKENS`, `CURRENT_VALUE`, `PRICE` and `UNREALIZED_PNL` describe a holding that no longer exists and are served in `REALIZED_PNL` order; cursors minted for those four sorts before this change return `400`, so restart the walk from the first page.
* **CORS**: `/v2` answers `access-control-allow-origin: *` to every origin, as v1 always did.
* **`/v2/prices-history`**: a token with no data at or before the requested window answers empty immediately, and `as_of` now serves an asset whose history lives only in the finer tiers instead of returning nothing.
* **`/v2/resolutions`**: answers are cached for up to 5 seconds while any row is live, and for up to 10 minutes once every row in the answer is resolved, so a new proposal, dispute or expiry can take up to 5 seconds to appear. `log_index` on a row now identifies the oracle request's `RequestPrice` log, reconciled for the newest requests every 3 minutes.

**Oct 2, 2026**

PolyBolt: price.crypto defaults to Chainlink, with provider pins and two new symbols

* **Chainlink-first crypto prices**: `price.crypto` now serves Chainlink prices by default on every symbol, matching `price.equity`. Subscriptions that omit `filter.provider` see `source: "chainlink"` from their next connection onward, so re-read `source` after every reconnect and treat the value set as open.
* **Provider pins on `price.crypto`**: `filter.provider` (`chainlink` or `pyth`) now works on the crypto channel. Pyth remains available on `btcusd`, `ethusd`, `solusd`, `xrpusd`, `dogeusd` and `bnbusd`; the `subscribed` acknowledgement names the provider actually served, falling back to the symbol's default when the pin cannot be served. See [Pin a Price Provider](https://docs.polymarket.com/api-reference/live-data/overview.md#pin-a-price-provider).
* **New symbols**: `hypeusd` and `zecusd` join `price.crypto` (Chainlink only), aligning the spot catalog with `price.crypto.twap`'s eight symbols.
* The machine-readable contract at `https://ws-live-v2.polymarket.com/asyncapi.json` and the [migration guide](https://docs.polymarket.com/migrate/rtds-to-polybolt.md) carry all of the above.

**Sep 29, 2026**

PolyBolt: forex TWAPs, price provenance, and provider pins

* **New channel**: `price.equity.twap` streams 60-second Chainlink TWAP prices for `eurusd` and `usdjpy`, with the same envelope, snapshot and filter shape as `price.crypto.twap`. See [PolyBolt WebSocket](https://docs.polymarket.com/api-reference/live-data/overview.md).
* **`source` on every price payload**: `price.crypto`, `price.crypto.twap`, `price.equity` and `price.equity.twap` payloads now name their producing vendor (`pyth`, `massive` or `chainlink`) on every live update and once per snapshot batch. The field is additive; on the equity channels a symbol's vendor can change between connections, so re-read it after every reconnect.
* **Provider pins (rolling out)**: on the equity channels, `filter.provider` (`chainlink` or `pyth`) requests a specific vendor and the `subscribed` acknowledgement names the provider actually served, falling back to the symbol's default when the pin cannot be served. Where the selector is not yet enabled the field is accepted and ignored. See [Pin a Price Provider](https://docs.polymarket.com/api-reference/live-data/overview.md#pin-a-price-provider).
* **New symbols**: `zecusd` joins `price.crypto.twap`; `spcx`, `intc`, `mstr`, `crcl` and `mu` join `price.equity`. Equity defaults now come from Chainlink wherever Chainlink serves the symbol.
* The machine-readable contract at `https://ws-live-v2.polymarket.com/asyncapi.json` and the [Live Data Channel](https://docs.polymarket.com/api-reference/wss/polybolt.md) reference carry all of the above.

**Sep 28, 2026**

Data API: settlement estimates on /v2/resolutions

* **New optional fields**: rows from `GET /v2/resolutions` can now carry `expected_settlement_time` (RFC3339 UTC) and `settlement_time_basis`. Both are omitted when no estimate applies, so existing clients are unaffected.
* **What the basis means**: `managed_proposal_expiration` and `proposal_expiration` mark the end of a live UMA proposal's challenge window, `liveness` the end of a proposal window on Polymarket's v2 oracle, and `dvm_round_estimate` the earliest end of the UMA voting round a disputed market can settle in. These are estimates, not deadlines: a disputed vote can roll into a later round, and a proposal past its window that is still under review returns no estimate while `extended_review` is `true`. See [Market Resolution](https://docs.polymarket.com/market-data/public-analytics.md#market-resolution).
* **SDKs**: the TypeScript and Python `Resolution` types do not expose these fields yet; read them from the HTTP response in the meantime.

**Sep 15, 2026**

PolyBolt WebSocket: real-time prices on one socket

* **New socket**: `wss://ws-live-v2.polymarket.com/ws` streams licensed crypto, equity and Chainlink TWAP reference prices (`price.crypto`, `price.equity`, `price.crypto.twap`, CLOB API credentials required) with one envelope shape, a snapshot on every subscribe, a dense per-channel `seq`, and typed close codes. See [PolyBolt WebSocket](https://docs.polymarket.com/api-reference/live-data/overview.md) and the [Live Data Channel](https://docs.polymarket.com/api-reference/wss/polybolt.md) message reference.
* **RTDS is legacy** for reference prices: `crypto_prices`, `crypto_prices_chainlink` and `equity_prices` map to the new channels in [Migrating from RTDS](https://docs.polymarket.com/migrate/rtds-to-polybolt.md). The `comments` topic stays on RTDS for now.

**Sep 4, 2026**

Data API v2 release

* **New API**: Data API v2 is live at `https://data-api.polymarket.com/v2` for wallet activity, market data, and trader and builder analytics. Start with the [Data API overview](https://docs.polymarket.com/api-reference/data-api/overview.md) or [Migrating from v1](https://docs.polymarket.com/migrate/data-api-v1-to-v2.md).
* **Response contract**: Every endpoint wraps its payload in `data`. Paginated reads add `pagination` with opaque cursors (no offset parameter). Each endpoint takes snake\_case parameters with camelCase aliases, names on-chain ids `condition`/`condition_id`, encodes money and size as JSON numbers (bare fields are shares, `_usdc` fields are USD), and signals load with `429` plus `Retry-After`.
* **New capabilities over v1**: cumulative wallet PnL series (`/v2/user-pnl`), profile stats (`/v2/user-stats`), windowed wallet volume (`/v2/user-volume`), the biggest-winners board, builder leaderboard and volume, per-holder position economics (`/v2/holders?include_pnl=true`), and resolution lifecycle reads (`/v2/resolutions`).
* **Price history moves to the data host**: `GET /v2/prices-history` replaces the CLOB-hosted route, with three explicit window forms (`interval`, `start`/`end`, `as_of`), second-based `bucket_seconds`, and cursor pagination.
* **TypeScript SDK**: The SDK's Data API methods now use v2. See the [SDK changelog](https://docs.polymarket.com/changelog/sdks.md).
* **Rate limits**: per-family limits are published on [Rate Limits](https://docs.polymarket.com/api-reference/rate-limits.md).
* **v1 status**: `/v1` keeps serving unchanged but is frozen. New fields and endpoints land on v2 only. The v1 reference lives under **Data API v1 (Legacy)**.
* **Higher taker delay**: The taker delay on crypto markets is now `150ms`, up from `50ms`. The change took effect on **September 4 at 14:00 UTC (10:00 AM ET)**.

**Aug 17, 2026**

Crypto taker delay reduced to 50ms

* **Lower taker delay**: The taker delay on crypto markets is now `50ms`, down from `250ms`. The change took effect on **August 17 at 11:00 UTC (7:00 AM ET)**.

**Aug 14, 2026**

5-minute crypto markets moved to a 60-second Chainlink TWAP

* **Longer TWAP window**: All 5-minute crypto markets now resolve using a `60-second` Chainlink TWAP, replacing the `30-second` window introduced on August 7. The change took effect at **00:00 UTC**.
* **Updated documentation**: See [Chainlink TWAP Prices](https://docs.polymarket.com/market-data/realtime-data.md#twap-prices).

**Aug 10, 2026**

Data API: per-outcome redemption activity, position fee basis fields, and event artwork fallback

* **Redemptions are now reported per outcome**: on `GET /activity`, a `REDEEM` row carries the outcome it settles in `asset`, `outcome`, and `outcomeIndex`, with `size` as the tokens burned for that outcome and `usdcSize` as that outcome's payout. Redeeming both sides of a market returns **two rows** — one per outcome — where a single combined row appeared before, and the losing outcome's `usdcSize` is `0`. Sum `usdcSize` across a transaction's rows to get the total payout. See [Resolution](https://docs.polymarket.com/concepts/resolution.md).
* **New position basis fields**: `GET /positions` entries can include `grossInitialValue`, the remaining entry basis including attributed buy fees, and `entryFeesUsdc`, that fee component. `initialValue` and `avgPrice` keep their existing fee-exclusive meaning, so the fee-exclusive basis is `grossInitialValue - entryFeesUsdc`. Both fields are optional — treat an omitted field as unavailable, not as zero. See [Positions and Tokens](https://docs.polymarket.com/concepts/positions-tokens.md).
* **New `includeArchived` filter**: `GET /positions` accepts `includeArchived` to return positions in archived markets that are still active. Archived positions remain excluded by default, so existing clients are unaffected.
* **Event artwork fallback**: `icon` on `GET /activity` and `GET /trades` now falls back to the market image, then the parent event's icon and image, when a market has no icon of its own. Previously these rows returned an empty `icon`.

**Aug 7, 2026**

Chainlink TWAP resolution for crypto up/down markets

* **TWAP-based resolution**: Crypto up/down markets now resolve using Chainlink-computed time-weighted average prices instead of a single price snapshot.
* **Opening and settlement prices**: Both the price to beat and the final settlement price come from the applicable TWAP feed.
* **Averaging windows**: 5-minute markets use a `30-second` lookback; 15-minute and 4-hour markets use a `60-second` lookback.
* **Developer access**: See [Chainlink TWAP Prices](https://docs.polymarket.com/market-data/realtime-data.md#twap-prices) for the current streaming workflow.

**Jul 17, 2026**

Latency improvements and order response changes — Friday July 24, 04:00 UTC

* **Rollout Friday, July 24 at 04:00 UTC**: An async commit pipeline cuts matching latency. `POST /order` and `POST /orders` will no longer return `transactionHashes` on successful FAK/FOK matches — you get `tradeIDs` instead. `tradeIDs` behavior is unchanged; websocket fills are unaffected.
* **SDK users**: Bump to the latest CLOB client (TypeScript, Python, Rust) before rollout — it resolves hashes from `tradeIDs` for you.
* **Custom REST integrations**: If you depend on inline `transactionHashes`, poll trades by `tradeID` until each has a hash or returns `FAILED`. See [TypeScript](https://github.com/Polymarket/clob-client-v2/pull/89), [Python](https://github.com/Polymarket/py-clob-client-v2/pull/101), and [Rust](https://github.com/Polymarket/rs-clob-client-v2/pull/83).

**Jul 14, 2026**

Relayer: deprecating CLOB v1 Neg Risk Adapter

* **Old adapter retired**: Relayer calls to the CLOB v1 Neg Risk Adapter (`0xd91E80cF2E7be2e162c6513ceD06f1dD0dA35296`) are being deprecated. CLOB v2 integrations should use the current Neg Risk Adapter at `0xadA2005600Dec949baf300f4C6120000bDB6eAab` for all pUSD-collateral actions.
* **Grace period for redeems**: Relayer redeems targeting the old adapter remain available until **Friday, July 17, 2026 at 00:00 UTC**. After that, relayer calls to the old address are fully retired.
* **Updated documentation**: See [Contracts](https://docs.polymarket.com/resources/contracts.md).

**Jul 10, 2026**

Sports taker fee and maker rebate update

* **Sports fee coefficient**: The sports taker fee rate increases from `0.03` to `0.05` at midnight UTC.
* **Sports maker rebate**: The sports maker rebate decreases from 25% to 15% of collected taker fees.
* **Updated documentation**: [Fees](https://docs.polymarket.com/trading/fees.md) and [Maker Rebates Program](https://docs.polymarket.com/programs/maker-rebates.md).

**Jul 2, 2026**

World Cup markets decimalized to a 0.0025 (0.25¢) tick size

* **Finer tick size for World Cup markets**: All World Cup *to advance*, *moneyline*, *spreads*, and *totals* markets are now decimalized to a **0.0025 (0.25¢)** tick size. This lets you execute at smaller ticks and tighter spreads for the most competitive prices. Applies only to those World Cup markets — see [Place Orders](https://docs.polymarket.com/trading/place-orders.md).

**Jun 25, 2026**

Bridge API: optional X-Builder-Code header

* **New optional header**: `POST /deposit` and `POST /withdraw` now accept an `X-Builder-Code` header (bytes32 hex) for builder attribution. Without it, requests still succeed but return a `missing_builder_code` warning; a malformed code returns `400`. See [Deposit](https://docs.polymarket.com/trading/bridge/deposit.md) and [Withdraw](https://docs.polymarket.com/trading/bridge/withdraw.md).

**Jun 15, 2026**

CLOB DELETE /orders maximum batch size reduced to 1000

* **`DELETE /orders` limit**: The maximum number of order IDs per cancel request is now `1000`. Split larger cancellation batches across multiple requests.

**Jun 1, 2026**

Increased CLOB order rate limits

Raised burst and sustained rate limits for several CLOB trading endpoints.

* CLOB POST /order - 120000 every 10 minutes (200/s) - (SUSTAINED)
* CLOB DELETE /order - 120000 every 10 minutes (200/s) - (SUSTAINED)
* CLOB POST /orders - 2000 every 10s (200/s) - (BURST)
* CLOB DELETE /orders - 2000 every 10s (200/s) - (BURST)

See [Rate Limits](https://docs.polymarket.com/api-reference/rate-limits.md) for the full table.

**May 18, 2026**

Data API: builderCode added to /v1/builders/leaderboard and /v1/builders/volume

* **New field**: Both `GET /v1/builders/leaderboard` and `GET /v1/builders/volume` now include a `builderCode` string on each entry — the builder's onchain attribution code as attached to orders via `builderCode` (see [Migrating to CLOB V2](https://docs.polymarket.com/v2-migration)).
* **Additive change**: existing clients are unaffected. Legacy builders without a registered code return an empty string.

**May 14, 2026**

GET /markets/keyset maximum limit reduced to 100

* **`GET /markets/keyset` limit**: The maximum `limit` value is now `100`. Requests should use `after_cursor`/`next_cursor` to paginate through larger result sets.

**Apr 28, 2026**

CLOB V2 is live on production

Polymarket's CLOB V2 upgrade is live on `https://clob.polymarket.com`.

* **Production URL unchanged**: V2 now runs on the standard CLOB host. New integrations should use `https://clob.polymarket.com`.
* **No V1 compatibility**: Legacy V1 SDKs and V1-signed orders are no longer supported on production.
* **Open orders wiped**: Resting orders from before the cutover did not migrate and must be re-created with V2 signing.
* **Migration guide**: See [Migrating to CLOB V2](https://docs.polymarket.com/v2-migration) for the SDK, raw order signing, pUSD, and builder attribution changes.

**Apr 21, 2026**

Relayer API: POST /submit returns immediately without transactionHash

* **Faster `POST /submit` responses**: The Relayer's `POST /submit` endpoint now returns immediately with just `{ transactionID, state: "STATE_NEW" }`. The `transactionHash` field has been removed from the submit response to improve performance.
* **How to get the hash**: Poll [`GET /transaction`](https://docs.polymarket.com/api-reference/relayer/get-a-transaction-by-id) with the returned `transactionID` to retrieve the onchain `transactionHash` once the transaction has been broadcast.

**Apr 17, 2026**

CLOB V2: upgrades go live April 28 at \~11:00 UTC, with \~1 hour of downtime

Polymarket is shipping a coordinated upgrade: **new Exchange contracts, a rewritten CLOB backend, and a new collateral token (pUSD)**.

**Exchange upgrades go live April 28, 2026 at \~11:00 UTC with \~1 hour of downtime.** All integrations must migrate to the V2 SDK before the cutover — there will be no backward compatibility after go-live.

**Full walkthrough:** [Migrating to CLOB V2](https://docs.polymarket.com/v2-migration). Follow [Discord](https://discord.gg/polymarket), Telegram, and [status.polymarket.com](https://status.polymarket.com) for the exact start time.

**Historical pre-cutover note:** before go-live, integrations could test against `https://clob-v2.polymarket.com`. As of April 28, V2 runs on `https://clob.polymarket.com`.

**What's changing**

* New Exchange contracts (CTF Exchange V2 + Neg Risk CTF Exchange V2)
* **pUSD** replaces USDC.e as the collateral token (standard ERC-20 on Polygon, backed by USDC, backing enforced onchain)
* Order struct: `nonce`, `feeRateBps`, `taker` removed — `timestamp` (ms), `metadata`, `builder` added
* Fees are now set at match time — no more `feeRateBps` on orders
* Builder attribution is native via `builderCode` on orders (no more `builder-signing-sdk`)
* EIP-712 Exchange domain version bumps from `"1"` to `"2"` (ClobAuth stays at `"1"`)

**What you need to do**

* Install the V2 SDK — [`@polymarket/clob-client-v2`](https://www.npmjs.com/package/@polymarket/clob-client-v2) or [`py-clob-client-v2`](https://pypi.org/project/py-clob-client-v2/) — and remove the legacy `clob-client` / `py-clob-client` packages
* Update constructor from positional args to options object; rename `chainId` → `chain`
* Remove `feeRateBps`, `nonce`, and `taker` from your order creation code
* If you're a builder, copy your code from [Settings → Builder](https://polymarket.com/settings?tab=builder) and attach it to orders
* If you sign orders without the SDK, update the `verifyingContract` and the signed Order fields — see [For API users](https://docs.polymarket.com/v2-migration#for-api-users)
* Plan for all open orders to be wiped at cutover

**During the window:** Trading will be paused for \~1 hour on April 28 starting around 11:00 UTC. The SDK's hot-swap mechanism will auto-refresh the client when V2 goes live — no manual action needed if you're on the latest SDK.

**Apr 13, 2026**

Bridge API: added support link for bridging issues

* **Support contact**: Added a link to [our Bridge API provider's support](https://intercom.help/funxyz/en/articles/10732578-contact-us) (Fun.xyz) for failed, stuck, or compliance-held bridge transactions. See [Deposit Status](https://docs.polymarket.com/trading/bridge/status.md).

**Apr 10, 2026**

New keyset pagination endpoints for markets and events

* **New endpoints**: Added `GET /markets/keyset` and `GET /events/keyset` for cursor-based pagination, replacing offset-based `GET /markets` and `GET /events`.
* **How it works**: These use an opaque `after_cursor`/`next_cursor` token instead of `offset`, providing stable and efficient paging through large result sets. Same filters, same response shape per item — the only differences are the wrapper response (`{ "markets": [...], "next_cursor": "..." }`) and the rejection of `offset`.
* **Migration**: The existing `GET /markets` and `GET /events` endpoints remain available but will be deprecated in a future release. New integrations should use the keyset variants.

**Apr 9, 2026**

GET /markets: closed defaults to false

* **`closed` default change**: The `closed` query parameter on `GET /markets` now defaults to `false`. Closed markets are excluded from results unless you explicitly pass `closed=true`.

**April 8, 2026**

Increased API Rate Limits

Increased burst and sustained rate limits for several CLOB trading endpoints.

* CLOB POST /order - 5000 every 10s (500/s) - (BURST)
* CLOB POST /order - 48000 every 10 minutes (80/s)
* CLOB DELETE /order - 5000 every 10s (500/s) - (BURST)
* CLOB DELETE /order - 48000 every 10 minutes (80/s)
* CLOB POST /orders - 1500 every 10s (150/s) - (BURST)
* CLOB POST /orders - 21000 every 10 minutes (35/s)
* CLOB DELETE /cancel-market-orders - 1500 every 10s (150/s) - (BURST)
* CLOB DELETE /cancel-market-orders - 21000 every 10 minutes (35/s)

See [Rate Limits](https://docs.polymarket.com/api-reference/rate-limits.md) for the full table.

**Mar 31, 2026**

REST API Fee Fields Update

* **Fee calculation source**: Fees should now be calculated using the `feeSchedule` object within a market.

**Mar 30, 2026**

Fee Structure V2

* **New fee categories**: Fees now apply to Crypto, Sports, Finance, Politics, Economics, Culture, Weather, Tech, Mentions, and Other / General markets with updated rates per category. Geopolitical and world events markets remain fee-free.
* **Updated documentation**: [Trading Fees](https://docs.polymarket.com/trading/fees.md) and [Maker Rebates Program](https://docs.polymarket.com/programs/maker-rebates.md).

**Mar 17, 2026**

March Madness: \$2M+ in Liquidity Rewards

* **March Madness Liquidity Rewards**: Adding \$2M+ in liquidity rewards to both live and pregame markets.
* **How it works**: Liquidity rewards are payments for placing competitive bids. Rewards are paid out based on the size of your orders, how close they are to the midpoint, and how consistently they are quoted relative to other liquidity providers. Orders must be active on the book for a minimum of **3.5 seconds** to be eligible.

**Daily reward rates for markets (subject to change):**

**48 hours before GameStartTime:**

* \$7.5k for full game ML market
* \$500 for 5 other markets (most recently created full game spread, most recently created full game total, 1st half ML, most recently created 1st half spread, most recently created 1st half total)

**From game live to game completion (note: rewards are expressed in daily rates):**

* \$60k for full game ML
* \$4k for most recently created full game spread and most recently created full game total

**From game live to halftime (note: rewards are expressed in daily rates):**

* \$8k for 1st half ML, most recently created 1st half spread, most recently created 1st half total

* **Markets**: [Browse March Madness markets](https://polymarket.com/sports/cbb/games)

* **More details**: [Liquidity Rewards documentation](https://docs.polymarket.com/programs/liquidity-rewards.md)

**Mar 1, 2026**

Taker Fees & Maker Rebates: All Crypto Markets

* **Crypto market fees expansion**: Starting March 6, 2026, taker fees and maker rebates extend to all crypto markets including 1H, 4H, daily, and weekly. The same fee structure as existing crypto markets applies. Only new markets created after March 6 are affected.
* **Updated documentation**: [Trading Fees](https://docs.polymarket.com/trading/fees.md) and [Maker Rebates Program](https://docs.polymarket.com/programs/maker-rebates.md) updated to reflect all crypto market coverage.

**Feb 12, 2026**

5-Minute Crypto Markets

* **5-minute crypto markets**: Launched with taker fees enabled. Fees follow the same curve as 15-minute crypto markets, peaking at 1.56% at 50% probability.
* **Maker Rebates**: Liquidity providers earn daily USDC rebates funded by taker fees, same as 15-minute crypto markets.

**Feb 11, 2026**

Taker Fees & Maker Rebates: NCAAB and Serie A

* **Sports market fees**: Taker fees to be enabled on NCAAB (college basketball) and Serie A markets on February 18, 2026.
* **Per-market rebate calculation**: Rebates are now calculated per market, makers only compete with other makers in the same market.
* **Updated documentation**: [Maker Rebates Program](https://docs.polymarket.com/programs/maker-rebates.md) updated with sports fee tables and parameters.

**Jan 28, 2026**

Bridge API: Withdrawal Endpoint

* **Withdrawal Endpoint**: New `/withdraw` endpoint to bridge USDC.e from Polymarket to any supported chain and token.
* **Multi-chain withdrawals**: Withdraw to EVM chains (Ethereum, Arbitrum, Base, etc.), Solana, and Bitcoin.
* **Updated documentation**: Bridge API docs updated to reflect deposit and withdrawal functionality.

**Jan 16, 2026**

Docs Update: RTDS documentation

* RTDS docs updated to reflect RTDS supports **comments** and **crypto prices** only.
* Removed legacy CLOB references and `clob_auth` from RTDS docs.

**Jan 16, 2026**

Docs Update: Maker Rebates Program

* **Maker Rebates Program**: Updated funding schedule with distribution method (volume-weighted vs fee-curve weighted).
* **Fee-curve weighted rebates**: Documented fee-equivalent formula and rebate calculation.
* **FAQ**: Clarified how rebates are calculated during fee-curve weighted periods.

**Jan 6, 2026**

New API Features

* **Releases**: Daily Releases timing
* **HeartBeats API**: HeartBeats endpoint for monitoring connection status and canceling orders
* **Post Only Orders**: Orders that are rejected if they would immediately match against an existing order

**Jan 5, 2026**

Taker Fees & Maker Rebates

* **Taker Fees**: Enabled on 15-minute crypto markets. Fees vary by price and peak at 1.56% at 50% probability.
* **Maker Rebates**: Daily USDC rebates paid to liquidity providers, funded by taker fees.

**Sept 24, 2025**

Polymarket Real-Time Data Socket (RTDS) official release

* **Crypto Price Feeds**: Access real-time cryptocurrency prices from two sources (Binance & Chainlink)
* **Comment Streaming**: Real-time updates for comment events including new comments, replies, and reactions
* **Dynamic Subscriptions**: Add, remove, and modify subscriptions without reconnecting
* **TypeScript Client**: Official TypeScript client available at [real-time-data-client](https://github.com/Polymarket/real-time-data-client)
  For complete documentation, see [Market Data](https://docs.polymarket.com/market-data/overview.md).

**September 15, 2025**

WSS price\_change event update

* There has been a significant change to the structure of the price change message. This update will be applied at 11PM UTC September 15, 2025. We apologize for the short notice
  * Please see [Real-Time Data](https://docs.polymarket.com/market-data/realtime-data.md) for details.

**August 26, 2025**

Updated /trades and /activity endpoints

* Reduced maximum values for query parameters on Data-API /trades and /activity:
  * `limit`: 500
  * `offset`: 1,000

**August 21, 2025**

Batch Orders Increase

* The batch orders limit has been increased from 5 -> 15. Read more about the batch orders functionality [here](https://docs.polymarket.com/trading/place-orders.md#post-a-batch-of-orders).

**July 23, 2025**

Get Book(s) update

* We’re adding new fields to the `get-book` and `get-books` CLOB endpoints to include key market metadata that previously required separate queries.
  * `min_order_size`
    * type: string
    * description: Minimum price increment.
  * `neg_risk`
    * type: boolean
    * description: Boolean indicating whether the market is neg\_risk.
  * `tick_size`
    * type: string
    * description: Minimum price increment.

**June 3, 2025**

New Batch Orders Endpoint

* We’re excited to roll out a highly requested feature: **order batching**. With this new endpoint, users can now submit up to five trades in a single request. To help you get started, we’ve included sample code demonstrating how to use it. Please see [Place Orders](https://docs.polymarket.com/trading/place-orders.md) for more details.

**June 3, 2025**

Change to /data/trades

* We're adding a new `side` field to the `MakerOrder` portion of the trade object. This field will indicate whether the maker order is a `buy` or `sell`, helping to clarify trade events where the maker side was previously ambiguous. For more details, refer to the MakerOrder object on the [Manage Orders](https://docs.polymarket.com/trading/manage-orders.md) page.

**May 28, 2025**

Websocket Changes

* The 100 token subscription limit has been removed for the Markets channel. You can now subscribe to as many token IDs as needed for your use case.
* New Subscribe Field `initial_dump`
  * Optional field to indicate whether you want to receive the initial order book state when subscribing to a token or list of tokens.
  * `default: true`

**May 28, 2025**

New FAK Order Type

We’re excited to introduce a new order type soon to be available to all users: Fill and Kill (FAK). FAK orders behave similarly to the well-known Fill or Kill (FOK) orders, but with a key difference:

* FAK will fill as many shares as possible immediately at your specified price, and any remaining unfilled portion will be canceled.
* Unlike FOK, which requires the entire order to fill instantly or be canceled, FAK is more flexible and aims to capture partial fills if possible.

**May 15, 2025**

Increased API Rate Limits

All API users will enjoy increased rate limits for the CLOB endpoints.

* CLOB - /books (website) (300req - 10s / Throttle requests over the maximum configured rate)
* CLOB - /books (50 req - 10s / Throttle requests over the maximum configured rate)
* CLOB - /price (100req - 10s / Throttle requests over the maximum configured rate)
* CLOB markets/0x (50req / 10s - Throttle requests over the maximum configured rate)
* CLOB POST /order - 500 every 10s (50/s) - (BURST) - Throttle requests over the maximum configured rate
* CLOB POST /order - 3000 every 10 minutes (5/s) - Throttle requests over the maximum configured rate
* CLOB DELETE /order - 500 every 10s (50/s) - (BURST) - Throttle requests over the maximum configured rate
* DELETE /order - 3000 every 10 minutes (5/s) - Throttle requests over the maximum configured rate


This documentation is built and hosted on [Mintlify](https://mintlify.com), a developer documentation platform.