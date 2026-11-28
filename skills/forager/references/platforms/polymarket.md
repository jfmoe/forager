# Polymarket

Polymarket is a prediction market: traders buy outcomes of future events, and an outcome's price
is the probability the market implies. Use it when the task asks how likely something is, and
cite the price beside the commentary. It needs no key. Read [`shared.md`](shared.md) for what
every platform shares.

- **Markets on a topic**: `forager platform polymarket search 'QUERY'`; `--include-closed` adds
  closed events. Search prices come from a cache that can lag up to 5 minutes.
- **The live probability**: `forager platform polymarket fetch 'REF_OR_URL'` for an event and
  each of its markets; check `price_source: clob` and `prices_as_of` before you quote a current
  figure.
- **What an outcome means**: `fetch --depth abstract` adds the resolution rules; read them before
  you say what a market predicts.
- **A trend**: `forager platform polymarket history 'MARKET'` lists one outcome's prices over a
  window. Call it the price history, not a probability history: Polymarket does not say whether a
  point is a midpoint or a trade.
- **Read a price with its `basis`**:
  - `midpoint`: from the live order book; `midpoint_cached`: from the cache.
  - `no_two_sided_book`: one side has no orders, so there is no probability; never read it as 0 or
    50%.
  - `not_trading`: the market is closed or suspended; in a `neg_risk` event, a placeholder market
    for a candidate nobody trades yet is suspended.
  - `resolution`: the market resolved; `price` is the payout (1 for the winner), not a
    probability.
- **Weak prices**: a wide `spread` or a low `liquidity` makes the midpoint weak; quote `best_bid`
  and `best_ask` with it. A price is what traders pay now, not a calibrated forecast.
- **Terms**: Polymarket's terms forbid scraping the site, which forager does not do (it calls the
  public APIs), and forbid a market data distributor or capital market client from using the data
  without written consent; whether that applies depends on who the user is.

The step is done when each probability the answer cites has its basis and the time it was read.

## Refs

- An event: `polymarket:<event_slug>`, or `https://polymarket.com/event/<event>`.
- A market: `polymarket:market/<market_slug>`, `https://polymarket.com/event/<event>/<market>` or
  `https://polymarket.com/market/<market>`, with or without a language prefix such as `/zh/`.
- Numeric IDs and polymarket.us links are refused (exit 2); Polymarket US is a separate product.
- A slug can change or be reused by a new event. To store or compare a market over time, key it
  by `condition_id`, and an event by `event_id`.

## Result fields

- Search items are events: `ref`, `url`, `title`, `status` (`open`, `closed`, `resolved`),
  `end_date`, `volume`, `volume_24h`, `liquidity`, `neg_risk`, `market_count`, and
  `top_markets`, its five most probable markets.
- A fetch adds `prices_as_of` and `markets`: each with `question`, `label`, `status`,
  `probability` (the Yes price of a Yes/No market, else `null`), `outcomes` (`name`, `price`,
  `basis`, `best_bid`, `best_ask`), `spread`, `last_trade_price`, the 1-day, 1-week and 1-month
  changes, `volume` and `liquidity`.
- `price_source: gamma` on a fetch means live quotes were unavailable (stderr says so) or no
  market trades; `last_trade_price` is then `null`.
- The probabilities of a `neg_risk` event's markets need not sum to 1, and forager does not
  normalize them.
- Search issues no cursor past page 100; narrow the query instead.
- A history's `points` run oldest first. A resolved market's payout is `settlement`, never a
  point. An empty `points` means no data at that width for that window: fine widths expire (1-
  minute data after about a week), so pass a coarser `--bucket`.

## Failures

- Exit 4 `parameter` saying the event or market was not found: slugs change; search again.
- Exit 4 `parameter` on `history` of an event with several markets: stderr lists their refs; pass
  one market. An unknown `--outcome` names the outcomes that exist.
- Exit 5 on `--depth abstract`: the event or market has no rules.
- A history cut at five pages says so on stderr; pass a larger `--bucket`.
