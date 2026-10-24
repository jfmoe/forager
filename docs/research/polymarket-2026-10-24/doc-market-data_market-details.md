> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.com/llms.txt
> Use this file to discover all available pages before exploring further.

# Market Details

> Understand the structure and state of a Polymarket market.

The market object represents a prediction market. Alongside its question and
outcomes, it describes the market's current state and provides the information
an integration needs to read prices, follow activity, or trade.

The examples below assume you already have a market object. To find or fetch
one, see [Discover Markets](https://docs.polymarket.com/market-data/discover-markets.md).

**TypeScript**

Let's say you fetched a market with `fetchMarket()` on a `PublicClient` or
`SecureClient`:

```ts theme={null}
const market = await client.fetchMarket({
  slug: "will-the-us-confirm-that-aliens-exist-before-2027-789-924-249",
});
// market: Market
```

**Python**

Let's say you fetched a market with `get_market()` on an
`AsyncPublicClient` or `AsyncSecureClient`:

```python theme={null}
market = await client.get_market(
    slug="will-the-us-confirm-that-aliens-exist-before-2027-789-924-249",
)
# market: Market
```

**API**

Let's say you fetched a market from the Gamma API:

```bash theme={null}
curl "https://gamma-api.polymarket.com/markets/slug/will-the-us-confirm-that-aliens-exist-before-2027-789-924-249"
```

## Market Identifiers

A market has three identifiers, each used in a different context:

| Identifier | Use it for |
| - | - |
| Market ID | Fetching a known market by its Gamma ID. |
| Market slug | Fetching a known market or linking to its human-readable page. |
| Condition ID | Public analytics, positions, and some trading or lifecycle workflows. |

**TypeScript**

Read the identifiers from the market object:

```ts theme={null}
const marketId = market.id;
const slug = market.slug;
const conditionId = market.conditionId;
```

**Python**

Read the identifiers from the market object:

```python theme={null}
market_id = market.id
slug = market.slug
condition_id = market.condition_id
```

**API**

Read the identifiers from the Gamma response:

```json theme={null}
{
  "id": "703257",
  "slug": "will-the-us-confirm-that-aliens-exist-before-2027-789-924-249",
  "conditionId": "0x747dc809fb79e1b05be09c42d6179459a58de2ef3e40f02484a4e1260f741f75"
}
```

## Market Outcomes

Each market has YES and NO outcomes. Its position system determines which
outcome ID to use for order book reads and trading.

**TypeScript**

Read both outcomes from `market.outcomes`:

```ts theme={null}
const yes = market.outcomes.yes;
const no = market.outcomes.no;
```

Each outcome can carry a CTF token ID, a Polymarket Protocol V2 Position
ID, or both:

```ts theme={null}
type MarketOutcome = {
  label: string;
  tokenId: TokenId | null;
  positionId: PositionId | null;
  price: DecimalString | null;
};

type MarketOutcomes = {
  yes: MarketOutcome;
  no: MarketOutcome;
};
```

Select the trading identifier by market version:

```ts theme={null}
const yesAssetId = market.version === "v2" ? yes.positionId : yes.tokenId;
```

**Python**

Read both outcomes from `market.outcomes`:

```python theme={null}
yes = market.outcomes.yes
no = market.outcomes.no
```

Each outcome can carry a CTF token ID, a Polymarket Protocol V2 Position
ID, or both:

```python theme={null}
class MarketOutcome:
    label: str
    token_id: TokenId | None
    position_id: PositionId | None
    price: Decimal | None

class MarketOutcomes:
    yes: MarketOutcome
    no: MarketOutcome
```

Select the trading identifier by market version:

```python theme={null}
yes_asset_id = yes.position_id if market.version == "v2" else yes.token_id
if yes_asset_id is None:
    raise ValueError("Market outcome ID not found.")
```

**API**

Gamma returns the market outcomes and their identifiers:

```json theme={null}
{
  "version": "v2",
  "outcomes": "[\"Yes\", \"No\"]",
  "outcomePrices": "[\"0.085\", \"0.915\"]",
  "clobTokenIds": null,
  "positionIds": [
    "651150819117105875331414918119047680898421632356043490229292782433651916800",
    "651150819117105875331414918119047680898421632356043490229292782433651916801"
  ]
}
```

Outcome labels and prices are JSON-encoded strings. Parse them into arrays;
entries at the same index describe the same outcome.

```js theme={null}
const labels = JSON.parse(market.outcomes);
const prices = JSON.parse(market.outcomePrices);
```

Use `market.version` to select the identifiers: `positionIds` is already an
array for Polymarket Protocol V2 (`v2`); parse `clobTokenIds` for CTF (`v1`).
Both fields can be populated, so their presence does not select the protocol.

```js Polymarket V2 theme={null}
const assetIds = market.positionIds;
```

```js CTF theme={null}
const assetIds = JSON.parse(market.clobTokenIds);
```

Combine labels, prices, and identifiers by index. For these binary markets,
index `0` is YES and index `1` is NO.

```js theme={null}
const outcomes = labels.map((label, index) => ({
  label,
  price: prices[index],
  assetId: assetIds[index],
}));
```

These examples assume outcome identifiers are available. If the selected
field is absent, the IDs are not yet available. Treat versions other than
`v1` and `v2` as unsupported.

## Market Status

Market status tells you whether a market is currently available for trading.
Check it before relying on live prices or submitting an order, since a market
may exist before its order book opens and remain discoverable after it closes.
It also indicates whether the market belongs to a [negative-risk
group](https://docs.polymarket.com/concepts/negative-risk.md), where only one of several mutually exclusive
markets can resolve YES.

**TypeScript**

Read market status from `market.state`:

```ts theme={null}
const state = market.state;
// state: MarketState
```

`MarketState` groups the market's operational state and timing:

```ts MarketState Type theme={null}
type MarketState = {
  active?: boolean | null;
  closed?: boolean | null;
  archived?: boolean | null;
  acceptingOrders?: boolean | null;
  enableOrderBook?: boolean | null;
  negRisk?: boolean | null;
  startDate?: IsoDateTimeString | null;
  endDate?: IsoDateTimeString | null;
  closedTime?: IsoDateTimeString | null;
};
```

```json MarketState Example theme={null}
{
  "active": true,
  "closed": false,
  "archived": false,
  "acceptingOrders": true,
  "enableOrderBook": true,
  "negRisk": false,
  "startDate": "2024-03-15T00:00:00Z",
  "endDate": "2027-01-01T00:00:00Z",
  "closedTime": null
}
```

Use its status fields to check whether the market is ready for trading:

```ts theme={null}
const isTradeReady = state.active && !state.closed && state.acceptingOrders;
```

**Python**

Read market status from `market.state`:

```python theme={null}
state = market.state
# state: MarketState
```

`MarketState` groups the market's operational state and timing:

```python MarketState Type theme={null}
class MarketState:
    active: bool | None
    closed: bool | None
    archived: bool | None
    accepting_orders: bool | None
    enable_order_book: bool | None
    neg_risk: bool | None
    start_date: datetime | None
    end_date: datetime | None
    closed_time: datetime | None
```

```json MarketState Example theme={null}
{
  "active": true,
  "closed": false,
  "archived": false,
  "accepting_orders": true,
  "enable_order_book": true,
  "neg_risk": false,
  "start_date": "2024-03-15T00:00:00Z",
  "end_date": "2027-01-01T00:00:00Z",
  "closed_time": null
}
```

Use its status fields to check whether the market is ready for trading:

```python theme={null}
is_trade_ready = (
    state.active
    and not state.closed
    and state.accepting_orders
)
```

**API**

Read the corresponding fields from the Gamma response:

```json theme={null}
{
  "active": true,
  "closed": false,
  "acceptingOrders": true,
  "negRisk": false,
  "restricted": false,
  "archived": false
}
```

Where:

| Field | Type | Description |
| - | - | - |
| `active` | boolean | Market is deployed and not archived. |
| `closed` | boolean | Market has resolved or been closed, so no further trading is possible. |
| `acceptingOrders` | boolean | Order book is open for new limit and market orders. |
| `negRisk` | boolean | Market belongs to a [negative-risk group](https://docs.polymarket.com/concepts/negative-risk.md). |
| `restricted` | boolean | Market is geo-restricted for some jurisdictions. |
| `archived` | boolean | Market is archived and read-only: no trading, no resolution updates. |

### Identify Augmented Negative Risk

Negative-risk membership is a market-level property, but augmented negative
risk is configured on the event. After confirming that the market's `negRisk`
value is `true`, fetch its event and check that augmented negative risk is
enabled:

**TypeScript**

Fetch the event referenced by the market, then check its trading
configuration:

```ts theme={null}
const event = await client.fetchEvent({
  id: market.events[0].id,
});

const isAugmentedNegativeRisk =
  event.trading.enableNegRisk === true &&
  event.trading.negRiskAugmented === true;
```

**Python**

Fetch the event referenced by the market, then check its trading
configuration:

```python theme={null}
event = await client.get_event(id=market.events[0].id)

is_augmented_negative_risk = (
    event.trading.enable_neg_risk is True
    and event.trading.neg_risk_augmented is True
)
```

**API**

Read the event ID from the market's `events` array, then fetch that event:

```bash theme={null}
curl "https://gamma-api.polymarket.com/events/$EVENT_ID"
```

An event uses augmented negative risk when both fields are `true`:

```json theme={null}
{
  "enableNegRisk": true,
  "negRiskAugmented": true
}
```

See [Augmented Negative Risk](https://docs.polymarket.com/concepts/negative-risk.md#augmented-negative-risk)
for how named outcomes, placeholders, and Other behave.

## Trading Constraints

Every market enforces a minimum price increment, also known as the **tick size**,
and a minimum order size. The minimum price increment defines the grid of
prices you can submit; an order price must be a multiple of the active value.
Orders smaller than the minimum order size are rejected.

| Minimum price increment | Step size | Example prices |
| - | - | - |
| `0.1` | 10¢ | `0.1`, `0.5`, `0.9` |
| `0.01` | 1¢ | `0.01`, `0.50`, `0.99` |
| `0.005` | 0.5¢ | `0.005`, `0.500`, `0.995` |
| `0.0025` | 0.25¢ | `0.0025`, `0.5000`, `0.9975` |
| `0.001` | 0.1¢ | `0.001`, `0.500`, `0.999` |
| `0.0001` | 0.01¢ | `0.0001`, `0.5000`, `0.9999` |

> [!NOTE]
>
> The `0.0025` (0.25¢) increment applies only to World Cup *to advance*,
> *moneyline*, *spreads*, and *totals* markets. Always read the active value
> from the market rather than assuming a fixed increment.

Read the active constraints for a market in your integration:

**TypeScript**

Read the market's trading constraints:

```ts theme={null}
const minimumTickSize = market.trading.minimumTickSize;
const minimumOrderSize = market.trading.minimumOrderSize;

// minimumTickSize: TickSizeValue | null
// minimumOrderSize: DecimalString | null
//   minimum USDC notional; orders smaller than this are rejected
```

The SDK groups these constraints in `MarketTrading`:

```ts MarketTrading Type theme={null}
type MarketTrading = {
  minimumTickSize?: TickSizeValue | null;
  minimumOrderSize?: DecimalString | null;
};
```

```json MarketTrading Example theme={null}
{
  "minimumTickSize": 0.01,
  "minimumOrderSize": "5"
}
```

**Python**

Read the market's trading constraints:

```python theme={null}
minimum_tick_size = market.trading.minimum_tick_size
minimum_order_size = market.trading.minimum_order_size

# minimum_tick_size: Decimal | None
# minimum_order_size: Decimal | None
#   minimum USDC notional per order
```

The SDK groups these constraints in `MarketTrading`:

```python MarketTrading Type theme={null}
class MarketTrading:
    minimum_tick_size: Decimal | None
    minimum_order_size: Decimal | None
```

```json MarketTrading Example theme={null}
{
  "minimum_tick_size": 0.01,
  "minimum_order_size": "5"
}
```

**API**

Read the corresponding fields from the Gamma response:

```json theme={null}
{
  "orderPriceMinTickSize": 0.01,
  "orderMinSize": 5
}
```

Where:

| Field | Type | Description |
| - | - | - |
| `orderPriceMinTickSize` | number | Minimum price increment for limit orders. Prices must be multiples of this value. |
| `orderMinSize` | number | Minimum order size in USDC. The CLOB rejects orders below this threshold. |

## Trading Fees

Some markets charge trading fees. The fee schedule determines how fees vary
with price, which side pays them, and what share is returned to makers.

**TypeScript**

Read the market's fee configuration:

```ts theme={null}
const feesEnabled = market.trading.feesEnabled;
const feeSchedule = market.trading.feeSchedule;

// feesEnabled: boolean | null
// feeSchedule: {
//   rate: DecimalString;  base rate used in the fee calculation
//   exponent: number;     exponent applied to the price component
//   takerOnly: boolean;   fees charged to taker side only
//   rebateRate: DecimalString; maker rebate as fraction of taker fee
// } | null
```

The SDK exposes this configuration through `MarketTrading` and
`FeeSchedule`:

```ts MarketTrading Type theme={null}
type FeeSchedule = {
  rate: DecimalString;
  exponent: number;
  takerOnly: boolean;
  rebateRate: DecimalString;
};

type MarketTrading = {
  feesEnabled?: boolean | null;
  feeSchedule?: FeeSchedule | null;
};
```

```json MarketTrading Example theme={null}
{
  "feesEnabled": true,
  "feeSchedule": {
    "rate": "0.04",
    "exponent": 1,
    "takerOnly": true,
    "rebateRate": "0.25"
  }
}
```

**Python**

Read the market's fee configuration:

```python theme={null}
fees_enabled = market.trading.fees_enabled
fee_schedule = market.trading.fee_schedule

# fees_enabled: bool | None
# fee_schedule.rate: Decimal            base rate used in the fee calculation
# fee_schedule.exponent: int | float    exponent applied to the price component
# fee_schedule.taker_only: bool         taker-only flag
# fee_schedule.rebate_rate: Decimal     maker rebate fraction
```

The SDK exposes this configuration through `MarketTrading` and
`FeeSchedule`:

```python MarketTrading Type theme={null}
class FeeSchedule:
    rate: Decimal
    exponent: int | float
    taker_only: bool
    rebate_rate: Decimal

class MarketTrading:
    fees_enabled: bool | None
    fee_schedule: FeeSchedule | None
```

```json MarketTrading Example theme={null}
{
  "fees_enabled": true,
  "fee_schedule": {
    "rate": 0.04,
    "exponent": 1,
    "taker_only": true,
    "rebate_rate": 0.25
  }
}
```

**API**

Read the corresponding fields from the Gamma response:

```json theme={null}
{
  "feesEnabled": true,
  "feeSchedule": {
    "rate": 0.04,
    "exponent": 1,
    "takerOnly": true,
    "rebateRate": 0.25
  }
}
```

Where:

| Field | Type | Description |
| - | - | - |
| `feesEnabled` | boolean | Whether fees are active for this market. |
| `feeSchedule.rate` | number | Base rate used in the fee calculation. |
| `feeSchedule.exponent` | number | Exponent applied to the price component of the fee curve. |
| `feeSchedule.takerOnly` | boolean | When `true`, fees are charged to the taker side only, and makers pay no fee. |
| `feeSchedule.rebateRate` | number | Fraction of taker fees rebated back to the resting maker. Example: `0.25` = 25% rebate. |

## Liquidity Reward Settings

Liquidity reward settings determine which resting orders can qualify for
incentives and how much funding is available. An order must meet the market's
minimum qualifying size and remain within its maximum qualifying spread. See
[Liquidity Rewards](https://docs.polymarket.com/programs/liquidity-rewards.md) for the scoring methodology.

**TypeScript**

Read liquidity reward settings from `market.rewards`:

```ts theme={null}
const minimumSize = market.rewards.rewardsMinSize;
const maximumSpread = market.rewards.rewardsMaxSpread;
const allocations = market.rewards.clobRewards;

// minimumSize: DecimalString | null
// maximumSpread: number | null
// allocations: ClobRewards[] | null
```

The SDK exposes the market configuration through `MarketRewards` and each
dated allocation through `ClobRewards`:

```ts ClobRewards Type theme={null}
type MarketRewards = {
  clobRewards?: ClobRewards[] | null;
  rewardsMinSize?: DecimalString | null;
  rewardsMaxSpread?: number | null;
  holdingRewardsEnabled?: boolean | null;
};

type ClobRewards = {
  id: ClobRewardId;
  conditionId: ConditionId;
  assetAddress: string;
  rewardsAmount: DecimalString;
  rewardsDailyRate: DecimalString;
  startDate: IsoCalendarDateString;
  endDate: IsoCalendarDateString | null;
};
```

```json ClobRewards Example theme={null}
{
  "rewardsMinSize": "100",
  "rewardsMaxSpread": 3,
  "clobRewards": [
    {
      "id": "<reward_id>",
      "conditionId": "<condition_id>",
      "assetAddress": "<asset_address>",
      "rewardsAmount": "10000",
      "rewardsDailyRate": "100",
      "startDate": "2026-07-01",
      "endDate": "2026-07-31"
    }
  ]
}
```

**Python**

Read liquidity reward settings from `market.rewards`:

```python theme={null}
minimum_size = market.rewards.rewards_min_size
maximum_spread = market.rewards.rewards_max_spread
allocations = market.rewards.clob_rewards

# minimum_size: Decimal | None
# maximum_spread: float | None
# allocations: tuple[ClobReward, ...] | None
```

The SDK exposes the market configuration through `MarketRewards` and each
dated allocation through `ClobReward`:

```python ClobReward Type theme={null}
class MarketRewards:
    clob_rewards: tuple[ClobReward, ...] | None
    rewards_min_size: Decimal | None
    rewards_max_spread: float | None
    holding_rewards_enabled: bool | None

class ClobReward:
    id: ClobRewardId
    condition_id: ConditionId
    asset_address: str
    rewards_amount: Decimal
    rewards_daily_rate: Decimal
    start_date: date
    end_date: date | None
```

```json ClobReward Example theme={null}
{
  "rewards_min_size": 100,
  "rewards_max_spread": 3,
  "clob_rewards": [
    {
      "id": "<reward_id>",
      "condition_id": "<condition_id>",
      "asset_address": "<asset_address>",
      "rewards_amount": 10000,
      "rewards_daily_rate": 100,
      "start_date": "2026-07-01",
      "end_date": "2026-07-31"
    }
  ]
}
```

**API**

Read the corresponding fields from the Gamma response:

```json theme={null}
{
  "rewardsMinSize": 100,
  "rewardsMaxSpread": 3,
  "clobRewards": [
    {
      "id": "<reward_id>",
      "conditionId": "<condition_id>",
      "assetAddress": "<asset_address>",
      "rewardsAmount": 10000,
      "rewardsDailyRate": 100,
      "startDate": "2026-07-01",
      "endDate": "2026-07-31"
    }
  ]
}
```

Where:

| Field | Type | Description |
| - | - | - |
| `rewardsMinSize` | number | Minimum order size, in shares, that can qualify for rewards. |
| `rewardsMaxSpread` | number | Maximum qualifying distance from the midpoint, in cents. |
| `clobRewards` | array | Dated reward allocations configured for the market. |
| `clobRewards[].rewardsAmount` | number | Total reward amount configured for the allocation. |
| `clobRewards[].rewardsDailyRate` | number | Reward amount available per day during the allocation. |
| `clobRewards[].startDate` | string | Date when the allocation begins. |
| `clobRewards[].endDate` | string | Date when the allocation ends, or `null` when it has no end date. |

## Market Timing

Market timing places a market within its expected schedule. Markets include
start and end dates, while sports markets may also include the scheduled start
time of the game.

**TypeScript**

Read timing data from the market's state, trading configuration, and sports
metadata:

```ts theme={null}
const startDate = market.state.startDate;
const endDate = market.state.endDate;
const secondsDelay = market.trading.secondsDelay;
const gameStartTime = market.sports.gameStartTime;

// startDate: IsoDateTimeString | null
// endDate: IsoDateTimeString | null
// secondsDelay: number | null
// gameStartTime: IsoDateTimeString | null
```

The SDK groups these values by their role:

```ts MarketSportsMetadata Type theme={null}
type MarketState = {
  startDate?: IsoDateTimeString | null;
  endDate?: IsoDateTimeString | null;
};

type MarketTrading = {
  secondsDelay?: number | null;
};

type MarketSportsMetadata = {
  gameStartTime?: IsoDateTimeString | null;
};
```

```json MarketSportsMetadata Example theme={null}
{
  "state": {
    "startDate": "2024-03-15T00:00:00Z",
    "endDate": "2027-01-01T00:00:00Z"
  },
  "trading": {
    "secondsDelay": 0
  },
  "sports": {
    "gameStartTime": null
  }
}
```

**Python**

Read timing data from the market's state, trading configuration, and sports
metadata:

```python theme={null}
start_date = market.state.start_date
end_date = market.state.end_date
seconds_delay = market.trading.seconds_delay
game_start_time = market.sports.game_start_time

# start_date: datetime | None
# end_date: datetime | None
# seconds_delay: int | None
# game_start_time: datetime | None
```

The SDK groups these values by their role:

```python MarketSportsMetadata Type theme={null}
class MarketState:
    start_date: datetime | None
    end_date: datetime | None

class MarketTrading:
    seconds_delay: int | None

class MarketSportsMetadata:
    game_start_time: datetime | None
```

```json MarketSportsMetadata Example theme={null}
{
  "state": {
    "start_date": "2024-03-15T00:00:00Z",
    "end_date": "2027-01-01T00:00:00Z"
  },
  "trading": {
    "seconds_delay": 0
  },
  "sports": {
    "game_start_time": null
  }
}
```

**API**

Read the corresponding fields from the Gamma response:

```json theme={null}
{
  "startDateIso": "2024-03-15T00:00:00Z",
  "endDateIso": "2027-01-01T00:00:00Z",
  "gameStartTime": null,
  "secondsDelay": 0
}
```

Where:

| Field | Type | Description |
| - | - | - |
| `startDateIso` | string | Market start date in ISO 8601 format. |
| `endDateIso` | string | Market end date in ISO 8601 format. |
| `gameStartTime` | string | Scheduled start time of the underlying game for a sports market. |
| `secondsDelay` | integer | Delay, in seconds, before a newly placed marketable order can match. |

See [Place Orders](https://docs.polymarket.com/trading/place-orders.md#place-a-market-order) for the
implications of trading on a market with a configured delay.

## Liquidity and Activity

Liquidity, volume, and recent price activity describe how actively a market
trades. Use them to compare markets or monitor changes over time.

**TypeScript**

Read the market's liquidity, volume, and price summary:

```ts theme={null}
const liquidity = market.metrics.liquidity;
const volume24hr = market.metrics.volume24hr;
const lastTradePrice = market.prices.lastTradePrice;
const bestBid = market.prices.bestBid;
const bestAsk = market.prices.bestAsk;
const spread = market.prices.spread;
const oneDayPriceChange = market.prices.oneDayPriceChange;

// liquidity: DecimalString | null
// volume24hr: DecimalString | null
// lastTradePrice: DecimalString | null
// bestBid: DecimalString | null
// bestAsk: DecimalString | null
// spread: DecimalString | null
// oneDayPriceChange: DecimalString | null
```

The SDK groups liquidity and volume in `MarketMetrics`, and price activity in
`MarketPrices`:

```ts MarketPrices Type theme={null}
type MarketMetrics = {
  liquidity?: DecimalString | null;
  volume24hr?: DecimalString | null;
};

type MarketPrices = {
  bestBid?: DecimalString | null;
  bestAsk?: DecimalString | null;
  lastTradePrice?: DecimalString | null;
  spread?: DecimalString | null;
  oneDayPriceChange?: DecimalString | null;
};
```

```json MarketPrices Example theme={null}
{
  "metrics": {
    "liquidity": "150248.32",
    "volume24hr": "5842.3"
  },
  "prices": {
    "lastTradePrice": "0.085",
    "bestBid": "0.08",
    "bestAsk": "0.09",
    "spread": "0.01",
    "oneDayPriceChange": "-0.003"
  }
}
```

**Python**

Read the market's liquidity, volume, and price summary:

```python theme={null}
liquidity = market.metrics.liquidity
volume_24hr = market.metrics.volume_24hr
last_trade_price = market.prices.last_trade_price
best_bid = market.prices.best_bid
best_ask = market.prices.best_ask
spread = market.prices.spread
one_day_price_change = market.prices.one_day_price_change
```

The SDK groups liquidity and volume in `MarketMetrics`, and price activity in
`MarketPrices`:

```python MarketPrices Type theme={null}
class MarketMetrics:
    liquidity: Decimal | None
    volume_24hr: Decimal | None

class MarketPrices:
    best_bid: Decimal | None
    best_ask: Decimal | None
    last_trade_price: Decimal | None
    spread: Decimal | None
    one_day_price_change: Decimal | None
```

```json MarketPrices Example theme={null}
{
  "metrics": {
    "liquidity": "150248.32",
    "volume_24hr": "5842.3"
  },
  "prices": {
    "last_trade_price": "0.085",
    "best_bid": "0.08",
    "best_ask": "0.09",
    "spread": "0.01",
    "one_day_price_change": "-0.003"
  }
}
```

**API**

Read the corresponding fields from the Gamma response:

```json theme={null}
{
  "liquidity": "150248.32",
  "volume24hr": 5842.3,
  "lastTradePrice": 0.085,
  "bestBid": 0.08,
  "bestAsk": 0.09,
  "spread": 0.01,
  "oneDayPriceChange": -0.003
}
```

Where:

| Field | Type | Description |
| - | - | - |
| `liquidity` | string | Reported market liquidity. |
| `volume24hr` | number | Trading volume over the past 24 hours. |
| `lastTradePrice` | number | Most recent traded price reported for the market. |
| `bestBid` | number | Highest resting bid reported for the market. |
| `bestAsk` | number | Lowest resting ask reported for the market. |
| `spread` | number | Difference between the best ask and best bid. |
| `oneDayPriceChange` | number | Change in the reported market price over the past 24 hours. |


This documentation is built and hosted on [Mintlify](https://mintlify.com), a developer documentation platform.