> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.com/llms.txt
> Use this file to discover all available pages before exploring further.

# Real-Time Data

> Keep market data current with Polymarket's real-time feeds.

Use Polymarket's real-time feeds to react as market conditions and related data
change. They keep the public market data your application depends on current
without repeated polling. For authenticated order and trade updates, see
[Real-Time Order Updates](https://docs.polymarket.com/trading/realtime-order-updates.md).

## Market Stream

Use the market stream to keep your application in sync with changes to a
market's order book and trading state.

**TypeScript**

The unified SDK uses `assetId` for an outcome's trading identifier: a token
ID for a CTF market or a position ID for a Polymarket Protocol V2 market.
Use the identifier exposed for that outcome.

Given a `PublicClient` or `SecureClient`, subscribe to the `market` topic with one or
more outcome IDs:

```ts theme={null}
const assetId = "<asset_id>";

const stream = await client.subscribe([
  {
    topic: "market",
    assetIds: [assetId],
  },
]);

for await (const event of stream) {
  switch (event.type) {
    case "book":
      // event: MarketBookEvent
      break;
    case "price_change":
      // event: MarketPriceChangeEvent
      break;
    case "last_trade_price":
      // event: MarketLastTradePriceEvent
      break;
    case "tick_size_change":
      // event: MarketTickSizeChangeEvent
      break;
  }
}
```

> **Standard Market Events**
>
> #### Order Book
>
> ```ts MarketBookEvent Type theme={null}
> type OrderBookLevel = {
>   price: DecimalString;
>   size: DecimalString;
> };
>
> type MarketBookEvent = {
>   topic: "market";
>   type: "book";
>   payload: {
>     conditionId: ConditionId;
>     assetId: ClobAssetId;
>     bids: OrderBookLevel[];
>     asks: OrderBookLevel[];
>     hash?: string | null;
>     timestamp?: string | null;
>     minOrderSize?: DecimalString | null;
>     tickSize?: DecimalString | null;
>     negRisk?: boolean | null;
>     lastTradePrice?: DecimalString | null;
>   };
> };
> ```
>
> ```json MarketBookEvent Example theme={null}
> {
>   "topic": "market",
>   "type": "book",
>   "payload": {
>     "conditionId": "0x747dc809fb79e1b05be09c42d6179459a58de2ef3e40f02484a4e1260f741f75",
>     "assetId": "107505882767731489358349912513945399560393482969656700824895970500493757150417",
>     "bids": [{ "price": "0.08", "size": "33343.4" }],
>     "asks": [{ "price": "0.09", "size": "163939.58" }],
>     "hash": "0xabc123…",
>     "timestamp": "1782753357257"
>   }
> }
> ```
>
> #### Price Change
>
> ```ts MarketPriceChangeEvent Type theme={null}
> type PriceChange = {
>   assetId: ClobAssetId;
>   price: DecimalString;
>   size: DecimalString;
>   side: OrderSide;
>   hash?: string | null;
>   bestBid?: DecimalString | null;
>   bestAsk?: DecimalString | null;
> };
>
> type MarketPriceChangeEvent = {
>   topic: "market";
>   type: "price_change";
>   payload: {
>     conditionId: ConditionId;
>     priceChanges: PriceChange[];
>     timestamp?: string | null;
>   };
> };
> ```
>
> ```json MarketPriceChangeEvent Example theme={null}
> {
>   "topic": "market",
>   "type": "price_change",
>   "payload": {
>     "conditionId": "0x747dc809fb79e1b05be09c42d6179459a58de2ef3e40f02484a4e1260f741f75",
>     "priceChanges": [
>       {
>         "assetId": "107505882767731489358349912513945399560393482969656700824895970500493757150417",
>         "price": "0.08",
>         "size": "33343.4",
>         "side": "BUY",
>         "hash": "56621a121a47ed9333273e21c83b660cff37ae50",
>         "bestBid": "0.08",
>         "bestAsk": "0.09"
>       }
>     ],
>     "timestamp": "1782753357257"
>   }
> }
> ```
>
> #### Last Trade Price
>
> ```ts MarketLastTradePriceEvent Type theme={null}
> type MarketLastTradePriceEvent = {
>   topic: "market";
>   type: "last_trade_price";
>   payload: {
>     conditionId: ConditionId;
>     assetId: ClobAssetId;
>     price: DecimalString;
>     size?: DecimalString | null;
>     feeRateBps?: DecimalString | null;
>     side: OrderSide;
>     timestamp?: string | null;
>     transactionHash?: string | null;
>   };
> };
> ```
>
> ```json MarketLastTradePriceEvent Example theme={null}
> {
>   "topic": "market",
>   "type": "last_trade_price",
>   "payload": {
>     "conditionId": "0x747dc809fb79e1b05be09c42d6179459a58de2ef3e40f02484a4e1260f741f75",
>     "assetId": "107505882767731489358349912513945399560393482969656700824895970500493757150417",
>     "price": "0.08",
>     "size": "219.217767",
>     "feeRateBps": "0",
>     "side": "SELL",
>     "timestamp": "1782753357257",
>     "transactionHash": "0xeeefff…"
>   }
> }
> ```
>
> #### Tick Size Change
>
> ```ts MarketTickSizeChangeEvent Type theme={null}
> type MarketTickSizeChangeEvent = {
>   topic: "market";
>   type: "tick_size_change";
>   payload: {
>     conditionId: ConditionId;
>     assetId: ClobAssetId;
>     oldTickSize?: DecimalString | null;
>     newTickSize: DecimalString;
>     timestamp?: string | null;
>   };
> };
> ```
>
> ```json MarketTickSizeChangeEvent Example theme={null}
> {
>   "topic": "market",
>   "type": "tick_size_change",
>   "payload": {
>     "conditionId": "0x747dc809fb79e1b05be09c42d6179459a58de2ef3e40f02484a4e1260f741f75",
>     "assetId": "107505882767731489358349912513945399560393482969656700824895970500493757150417",
>     "oldTickSize": "0.01",
>     "newTickSize": "0.001",
>     "timestamp": "1782753357257"
>   }
> }
> ```

Enable `customFeatureEnabled` to include top-of-book and market lifecycle
updates:

```ts theme={null}
const stream = await client.subscribe([
  {
    topic: "market",
    assetIds: [assetId],
    customFeatureEnabled: true,
  },
]);

for await (const event of stream) {
  switch (event.type) {
    case "book":
      // event: MarketBookEvent
      break;
    case "price_change":
      // event: MarketPriceChangeEvent
      break;
    case "last_trade_price":
      // event: MarketLastTradePriceEvent
      break;
    case "tick_size_change":
      // event: MarketTickSizeChangeEvent
      break;
    case "best_bid_ask":
      // event: MarketBestBidAskEvent
      break;
    case "new_market":
      // event: NewMarketEvent
      break;
    case "market_resolved":
      // event: MarketResolvedEvent
      break;
  }
}
```

> **Additional Market Events**
>
> #### Best Bid and Ask
>
> ```ts MarketBestBidAskEvent Type theme={null}
> type MarketBestBidAskEvent = {
>   topic: "market";
>   type: "best_bid_ask";
>   payload: {
>     conditionId: ConditionId;
>     assetId: ClobAssetId;
>     bestBid?: DecimalString | null;
>     bestAsk?: DecimalString | null;
>     spread?: DecimalString | null;
>     timestamp?: string | null;
>   };
> };
> ```
>
> ```json MarketBestBidAskEvent Example theme={null}
> {
>   "topic": "market",
>   "type": "best_bid_ask",
>   "payload": {
>     "conditionId": "0x747dc809fb79e1b05be09c42d6179459a58de2ef3e40f02484a4e1260f741f75",
>     "assetId": "107505882767731489358349912513945399560393482969656700824895970500493757150417",
>     "bestBid": "0.08",
>     "bestAsk": "0.09",
>     "spread": "0.01",
>     "timestamp": "1782753357257"
>   }
> }
> ```
>
> #### New Market
>
> ```ts NewMarketEvent Type theme={null}
> type NewMarketEvent = {
>   topic: "market";
>   type: "new_market";
>   payload: {
>     id: string;
>     question?: string | null;
>     market: string;
>     slug?: string | null;
>     description?: string | null;
>     assetIds?: ClobAssetId[] | null;
>     outcomes?: string[] | null;
>     eventMessage?: {
>       id: string;
>       ticker?: string | null;
>       slug?: string | null;
>       title?: string | null;
>       description?: string | null;
>     } | null;
>     timestamp?: string | null;
>     tags?: string[] | null;
>     conditionId?: ConditionId | null;
>     active?: boolean | null;
>     clobTokenIds?: string[] | null;
>     sportsMarketType?: string | null;
>     line?: DecimalString | null;
>     gameStartTime?: IsoDateTimeString | null;
>     orderPriceMinTickSize?: DecimalString | null;
>     groupItemTitle?: string | null;
>     takerBaseFee?: DecimalString | null;
>     feesEnabled?: boolean | null;
>     feeSchedule?: unknown;
>   };
> };
> ```
>
> ```json NewMarketEvent Example theme={null}
> {
>   "topic": "market",
>   "type": "new_market",
>   "payload": {
>     "id": "123456",
>     "market": "0x747dc809fb79e1b05be09c42d6179459a58de2ef3e40f02484a4e1260f741f75",
>     "question": "Will the US confirm that aliens exist before 2027?",
>     "slug": "will-the-us-confirm-that-aliens-exist-before-2027",
>     "assetIds": [
>       "107505882767731489358349912513945399560393482969656700824895970500493757150417",
>       "7305630249804085635496399869905769372294302716159034447326228509068694952392"
>     ],
>     "outcomes": ["Yes", "No"],
>     "active": true,
>     "timestamp": "1782753357257"
>   }
> }
> ```
>
> #### Market Resolved
>
> ```ts MarketResolvedEvent Type theme={null}
> type MarketResolvedEvent = {
>   topic: "market";
>   type: "market_resolved";
>   payload: {
>     id: string;
>     conditionId: ConditionId;
>     assetIds?: ClobAssetId[] | null;
>     winningAssetId?: ClobAssetId | null;
>     winningOutcome?: string | null;
>     eventMessage?: {
>       id: string;
>       ticker?: string | null;
>       slug?: string | null;
>       title?: string | null;
>       description?: string | null;
>     } | null;
>     timestamp?: string | null;
>     tags?: string[] | null;
>   };
> };
> ```
>
> ```json MarketResolvedEvent Example theme={null}
> {
>   "topic": "market",
>   "type": "market_resolved",
>   "payload": {
>     "id": "123456",
>     "conditionId": "0x747dc809fb79e1b05be09c42d6179459a58de2ef3e40f02484a4e1260f741f75",
>     "assetIds": [
>       "107505882767731489358349912513945399560393482969656700824895970500493757150417",
>       "7305630249804085635496399869905769372294302716159034447326228509068694952392"
>     ],
>     "winningAssetId": "107505882767731489358349912513945399560393482969656700824895970500493757150417",
>     "winningOutcome": "Yes",
>     "timestamp": "1782753357257"
>   }
> }
> ```

**Python**

The unified SDK uses `asset_id` for an outcome's trading identifier: a token
ID for a CTF market or a position ID for a Polymarket Protocol V2 market.
Use the identifier exposed for that outcome.

Given an `AsyncPublicClient` or `AsyncSecureClient`, subscribe to the market
stream with a `MarketSpec` containing one or more outcome IDs:

```python theme={null}
from polymarket.streams import MarketSpec

asset_id = "<asset_id>"

async with await client.subscribe(
    MarketSpec(asset_ids=[asset_id]),
) as stream:
    async for event in stream:
        if event.type == "book":
            ...  # event: MarketBookEvent
        elif event.type == "price_change":
            ...  # event: MarketPriceChangeEvent
        elif event.type == "last_trade_price":
            ...  # event: MarketLastTradePriceEvent
        elif event.type == "tick_size_change":
            ...  # event: MarketTickSizeChangeEvent
```

> **Standard Market Events**
>
> #### Order Book
>
> ```python MarketBookEvent Type theme={null}
> class OrderBookLevel:
>     price: Decimal
>     size: Decimal
>
> class MarketBookPayload:
>     condition_id: ConditionId
>     asset_id: ClobAssetId
>     bids: tuple[OrderBookLevel, ...]
>     asks: tuple[OrderBookLevel, ...]
>     hash: str | None
>     timestamp: datetime | None
>     min_order_size: Decimal | None
>     tick_size: Decimal | None
>     neg_risk: bool | None
>     last_trade_price: Decimal | None
>
> class MarketBookEvent:
>     topic: Literal["market"]
>     type: Literal["book"]
>     payload: MarketBookPayload
> ```
>
> ```json MarketBookEvent Example theme={null}
> {
>   "topic": "market",
>   "type": "book",
>   "payload": {
>     "condition_id": "0x747dc809fb79e1b05be09c42d6179459a58de2ef3e40f02484a4e1260f741f75",
>     "asset_id": "107505882767731489358349912513945399560393482969656700824895970500493757150417",
>     "bids": [{ "price": "0.08", "size": "33343.4" }],
>     "asks": [{ "price": "0.09", "size": "163939.58" }],
>     "hash": "0xabc123…",
>     "timestamp": "2026-06-29T17:15:57.257000Z"
>   }
> }
> ```
>
> #### Price Change
>
> ```python MarketPriceChangeEvent Type theme={null}
> class PriceChange:
>     asset_id: ClobAssetId
>     price: Decimal
>     size: Decimal
>     side: Literal["BUY", "SELL"]
>     hash: str | None
>     best_bid: Decimal | None
>     best_ask: Decimal | None
>
> class MarketPriceChangePayload:
>     condition_id: ConditionId
>     price_changes: tuple[PriceChange, ...]
>     timestamp: datetime | None
>
> class MarketPriceChangeEvent:
>     topic: Literal["market"]
>     type: Literal["price_change"]
>     payload: MarketPriceChangePayload
> ```
>
> ```json MarketPriceChangeEvent Example theme={null}
> {
>   "topic": "market",
>   "type": "price_change",
>   "payload": {
>     "condition_id": "0x747dc809fb79e1b05be09c42d6179459a58de2ef3e40f02484a4e1260f741f75",
>     "price_changes": [
>       {
>         "asset_id": "107505882767731489358349912513945399560393482969656700824895970500493757150417",
>         "price": "0.08",
>         "size": "33343.4",
>         "side": "BUY",
>         "hash": "56621a121a47ed9333273e21c83b660cff37ae50",
>         "best_bid": "0.08",
>         "best_ask": "0.09"
>       }
>     ],
>     "timestamp": "2026-06-29T17:15:57.257000Z"
>   }
> }
> ```
>
> #### Last Trade Price
>
> ```python MarketLastTradePriceEvent Type theme={null}
> class MarketLastTradePricePayload:
>     condition_id: ConditionId
>     asset_id: ClobAssetId
>     price: Decimal
>     size: Decimal | None
>     side: Literal["BUY", "SELL"]
>     fee_rate_bps: Decimal | None
>     transaction_hash: str | None
>     timestamp: datetime | None
>
> class MarketLastTradePriceEvent:
>     topic: Literal["market"]
>     type: Literal["last_trade_price"]
>     payload: MarketLastTradePricePayload
> ```
>
> ```json MarketLastTradePriceEvent Example theme={null}
> {
>   "topic": "market",
>   "type": "last_trade_price",
>   "payload": {
>     "condition_id": "0x747dc809fb79e1b05be09c42d6179459a58de2ef3e40f02484a4e1260f741f75",
>     "asset_id": "107505882767731489358349912513945399560393482969656700824895970500493757150417",
>     "price": "0.08",
>     "size": "219.217767",
>     "side": "SELL",
>     "fee_rate_bps": "0",
>     "transaction_hash": "0xeeefff…",
>     "timestamp": "2026-06-29T17:15:57.257000Z"
>   }
> }
> ```
>
> #### Tick Size Change
>
> ```python MarketTickSizeChangeEvent Type theme={null}
> class MarketTickSizeChangePayload:
>     condition_id: ConditionId
>     asset_id: ClobAssetId
>     old_tick_size: Decimal | None
>     new_tick_size: Decimal
>     timestamp: datetime | None
>
> class MarketTickSizeChangeEvent:
>     topic: Literal["market"]
>     type: Literal["tick_size_change"]
>     payload: MarketTickSizeChangePayload
> ```
>
> ```json MarketTickSizeChangeEvent Example theme={null}
> {
>   "topic": "market",
>   "type": "tick_size_change",
>   "payload": {
>     "condition_id": "0x747dc809fb79e1b05be09c42d6179459a58de2ef3e40f02484a4e1260f741f75",
>     "asset_id": "107505882767731489358349912513945399560393482969656700824895970500493757150417",
>     "old_tick_size": "0.01",
>     "new_tick_size": "0.001",
>     "timestamp": "2026-06-29T17:15:57.257000Z"
>   }
> }
> ```

Enable `custom_feature_enabled` to include top-of-book and market lifecycle
updates:

```python theme={null}
async with await client.subscribe(
    MarketSpec(asset_ids=[asset_id], custom_feature_enabled=True),
) as stream:
    async for event in stream:
        if event.type == "book":
            ...  # event: MarketBookEvent
        elif event.type == "price_change":
            ...  # event: MarketPriceChangeEvent
        elif event.type == "last_trade_price":
            ...  # event: MarketLastTradePriceEvent
        elif event.type == "tick_size_change":
            ...  # event: MarketTickSizeChangeEvent
        elif event.type == "best_bid_ask":
            ...  # event: MarketBestBidAskEvent
        elif event.type == "new_market":
            ...  # event: NewMarketEvent
        elif event.type == "market_resolved":
            ...  # event: MarketResolvedEvent
```

> **Additional Market Events**
>
> #### Best Bid and Ask
>
> ```python MarketBestBidAskEvent Type theme={null}
> class MarketBestBidAskPayload:
>     condition_id: ConditionId
>     asset_id: ClobAssetId
>     best_bid: Decimal | None
>     best_ask: Decimal | None
>     spread: Decimal | None
>     timestamp: datetime | None
>
> class MarketBestBidAskEvent:
>     topic: Literal["market"]
>     type: Literal["best_bid_ask"]
>     payload: MarketBestBidAskPayload
> ```
>
> ```json MarketBestBidAskEvent Example theme={null}
> {
>   "topic": "market",
>   "type": "best_bid_ask",
>   "payload": {
>     "condition_id": "0x747dc809fb79e1b05be09c42d6179459a58de2ef3e40f02484a4e1260f741f75",
>     "asset_id": "107505882767731489358349912513945399560393482969656700824895970500493757150417",
>     "best_bid": "0.08",
>     "best_ask": "0.09",
>     "spread": "0.01",
>     "timestamp": "2026-06-29T17:15:57.257000Z"
>   }
> }
> ```
>
> #### New Market
>
> ```python NewMarketEvent Type theme={null}
> class MarketEventMessage:
>     id: str
>     ticker: str | None
>     slug: str | None
>     title: str | None
>     description: str | None
>
> class NewMarketPayload:
>     id: str
>     market: str
>     question: str | None
>     slug: str | None
>     description: str | None
>     asset_ids: tuple[ClobAssetId, ...] | None
>     outcomes: tuple[str, ...] | None
>     event_message: MarketEventMessage | None
>     timestamp: datetime | None
>     tags: tuple[str, ...] | None
>     condition_id: ConditionId | None
>     active: bool | None
>     clob_token_ids: tuple[str, ...] | None
>     sports_market_type: str | None
>     line: Decimal | None
>     game_start_time: datetime | None
>     order_price_min_tick_size: Decimal | None
>     group_item_title: str | None
>     taker_base_fee: Decimal | None
>     fees_enabled: bool | None
>     fee_schedule: object | None
>
> class NewMarketEvent:
>     topic: Literal["market"]
>     type: Literal["new_market"]
>     payload: NewMarketPayload
> ```
>
> ```json NewMarketEvent Example theme={null}
> {
>   "topic": "market",
>   "type": "new_market",
>   "payload": {
>     "id": "123456",
>     "market": "0x747dc809fb79e1b05be09c42d6179459a58de2ef3e40f02484a4e1260f741f75",
>     "question": "Will the US confirm that aliens exist before 2027?",
>     "slug": "will-the-us-confirm-that-aliens-exist-before-2027",
>     "asset_ids": [
>       "107505882767731489358349912513945399560393482969656700824895970500493757150417",
>       "7305630249804085635496399869905769372294302716159034447326228509068694952392"
>     ],
>     "outcomes": ["Yes", "No"],
>     "active": true,
>     "timestamp": "2026-06-29T17:15:57.257000Z"
>   }
> }
> ```
>
> #### Market Resolved
>
> ```python MarketResolvedEvent Type theme={null}
> class MarketResolvedPayload:
>     id: str
>     condition_id: ConditionId
>     asset_ids: tuple[ClobAssetId, ...] | None
>     winning_asset_id: ClobAssetId | None
>     winning_outcome: str | None
>     event_message: MarketEventMessage | None
>     timestamp: datetime | None
>     tags: tuple[str, ...] | None
>
> class MarketResolvedEvent:
>     topic: Literal["market"]
>     type: Literal["market_resolved"]
>     payload: MarketResolvedPayload
> ```
>
> ```json MarketResolvedEvent Example theme={null}
> {
>   "topic": "market",
>   "type": "market_resolved",
>   "payload": {
>     "id": "123456",
>     "condition_id": "0x747dc809fb79e1b05be09c42d6179459a58de2ef3e40f02484a4e1260f741f75",
>     "asset_ids": [
>       "107505882767731489358349912513945399560393482969656700824895970500493757150417",
>       "7305630249804085635496399869905769372294302716159034447326228509068694952392"
>     ],
>     "winning_asset_id": "107505882767731489358349912513945399560393482969656700824895970500493757150417",
>     "winning_outcome": "Yes",
>     "timestamp": "2026-06-29T17:15:57.257000Z"
>   }
> }
> ```

**API**

Connect to the market WebSocket:

```text theme={null}
wss://ws-subscriptions-clob.polymarket.com/ws/market
```

> [!NOTE]
>
> The market WebSocket uses an application-level heartbeat. Send the text frame
> `PING` every 10 seconds; the server replies with `PONG`.

Once connected, send a `market` subscription frame with one or more outcome
IDs:

```json theme={null}
{
  "assets_ids": ["<asset_id>"],
  "type": "market"
}
```

> **Standard Market Events**
>
> #### Order Book
>
> ```json theme={null}
> {
>   "event_type": "book",
>   "market": "0x747dc809fb79e1b05be09c42d6179459a58de2ef3e40f02484a4e1260f741f75",
>   "asset_id": "107505882767731489358349912513945399560393482969656700824895970500493757150417",
>   "timestamp": "1782753357257",
>   "hash": "0xabc123…",
>   "bids": [
>     { "price": "0.08", "size": "33343.4" },
>     { "price": "0.09", "size": "163939.58" }
>   ],
>   "asks": [
>     { "price": "0.99", "size": "218442.27" },
>     { "price": "0.98", "size": "13229.55" }
>   ]
> }
> ```
>
> #### Price Change
>
> ```json theme={null}
> {
>   "event_type": "price_change",
>   "market": "0x747dc809fb79e1b05be09c42d6179459a58de2ef3e40f02484a4e1260f741f75",
>   "price_changes": [
>     {
>       "asset_id": "107505882767731489358349912513945399560393482969656700824895970500493757150417",
>       "price": "0.08",
>       "size": "33343.4",
>       "side": "BUY",
>       "hash": "56621a121a47ed9333273e21c83b660cff37ae50",
>       "best_bid": "0.08",
>       "best_ask": "0.09"
>     }
>   ],
>   "timestamp": "1782753357257"
> }
> ```
>
> #### Last Trade Price
>
> ```json theme={null}
> {
>   "event_type": "last_trade_price",
>   "market": "0x747dc809fb79e1b05be09c42d6179459a58de2ef3e40f02484a4e1260f741f75",
>   "asset_id": "107505882767731489358349912513945399560393482969656700824895970500493757150417",
>   "price": "0.08",
>   "size": "219.217767",
>   "fee_rate_bps": "0",
>   "side": "SELL",
>   "timestamp": "1782753357257",
>   "transaction_hash": "0xeeefff…"
> }
> ```
>
> #### Tick Size Change
>
> ```json theme={null}
> {
>   "event_type": "tick_size_change",
>   "market": "0x747dc809fb79e1b05be09c42d6179459a58de2ef3e40f02484a4e1260f741f75",
>   "asset_id": "107505882767731489358349912513945399560393482969656700824895970500493757150417",
>   "old_tick_size": "0.01",
>   "new_tick_size": "0.001",
>   "timestamp": "1782753357257"
> }
> ```

Enable `custom_feature_enabled` to include top-of-book and market lifecycle
updates:

```json theme={null}
{
  "assets_ids": ["<asset_id>"],
  "type": "market",
  "custom_feature_enabled": true
}
```

> **Additional Market Events**
>
> #### Best Bid and Ask
>
> ```json theme={null}
> {
>   "event_type": "best_bid_ask",
>   "market": "0x747dc809fb79e1b05be09c42d6179459a58de2ef3e40f02484a4e1260f741f75",
>   "asset_id": "107505882767731489358349912513945399560393482969656700824895970500493757150417",
>   "best_bid": "0.08",
>   "best_ask": "0.09",
>   "spread": "0.01",
>   "timestamp": "1782753357257"
> }
> ```
>
> #### New Market
>
> ```json theme={null}
> {
>   "event_type": "new_market",
>   "id": "123456",
>   "question": "Will the US confirm that aliens exist before 2027?",
>   "market": "0x747dc809fb79e1b05be09c42d6179459a58de2ef3e40f02484a4e1260f741f75",
>   "slug": "will-the-us-confirm-that-aliens-exist-before-2027",
>   "assets_ids": [
>     "107505882767731489358349912513945399560393482969656700824895970500493757150417",
>     "7305630249804085635496399869905769372294302716159034447326228509068694952392"
>   ],
>   "outcomes": ["Yes", "No"],
>   "timestamp": "1782753357257"
> }
> ```
>
> #### Market Resolved
>
> ```json theme={null}
> {
>   "event_type": "market_resolved",
>   "id": "123456",
>   "market": "0x747dc809fb79e1b05be09c42d6179459a58de2ef3e40f02484a4e1260f741f75",
>   "assets_ids": [
>     "107505882767731489358349912513945399560393482969656700824895970500493757150417",
>     "7305630249804085635496399869905769372294302716159034447326228509068694952392"
>   ],
>   "winning_asset_id": "107505882767731489358349912513945399560393482969656700824895970500493757150417",
>   "winning_outcome": "Yes",
>   "timestamp": "1782753357257"
> }
> ```

You can add or remove outcome IDs without opening a new connection:

```json Subscribe theme={null}
{
  "assets_ids": ["<new_asset_id>"],
  "operation": "subscribe"
}
```

```json Unsubscribe theme={null}
{
  "assets_ids": ["<old_asset_id>"],
  "operation": "unsubscribe"
}
```

These frames update only the outcome set for the current market-stream
connection.

## Sports Stream

Use the sports stream to keep live game information current alongside sports
markets. Updates arrive when a game goes live, its score or period changes, or
it ends. NFL and CFB updates can also reflect possession changes.

> [!WARNING]
>
> Sports data is provided for informational purposes only. It may be delayed,
> contain errors, or omit recent events. Polymarket does not provide trading or
> investment advice, and this data should not be used as the basis for a trading
> decision.

**TypeScript**

Given a `PublicClient` or `SecureClient`, subscribe to the `sports` topic to receive
every game update:

```ts theme={null}
const stream = await client.subscribe([{ topic: "sports" }]);

for await (const event of stream) {
  // event: SportsEvent
}
```

> **Sports Event**
>
> ```ts SportsEvent Type theme={null}
> type SportsEvent = {
>   topic: "sports";
>   type: "sport_result";
>   payload: {
>     gameId: number;
>     sportradarGameId?: string | null;
>     slug?: string | null;
>     leagueAbbreviation: string;
>     homeTeam?: string | null;
>     awayTeam?: string | null;
>     status: string;
>     live: boolean;
>     ended: boolean;
>     score: string;
>     period?: string | null;
>     elapsed?: string | null;
>     finishedAt?: IsoDateTimeString | null;
>     turn?: string | null;
>   };
> };
> ```
>
> ```json SportsEvent Example theme={null}
> {
>   "topic": "sports",
>   "type": "sport_result",
>   "payload": {
>     "gameId": 5127839,
>     "leagueAbbreviation": "NBA",
>     "homeTeam": "Los Angeles Lakers",
>     "awayTeam": "Boston Celtics",
>     "status": "InProgress",
>     "live": true,
>     "ended": false,
>     "score": "98-94",
>     "period": "Q4",
>     "elapsed": "05:12"
>   }
> }
> ```
>
> `score` is a combined `"<home>-<away>"` string, not separate home and away
> fields.

**Python**

Given an `AsyncPublicClient` or `AsyncSecureClient`, subscribe with a
`SportsSpec` to receive every game update:

```python theme={null}
from polymarket.streams import SportsSpec

async with await client.subscribe(SportsSpec()) as stream:
    async for event in stream:
        ...  # event: SportsEvent
```

> **Sports Event**
>
> ```python SportsEvent Type theme={null}
> class SportsGameResult:
>     game_id: int
>     sportradar_game_id: str | None
>     slug: str | None
>     league_abbreviation: str
>     home_team: str | None
>     away_team: str | None
>     status: str
>     live: bool
>     ended: bool
>     score: str
>     period: str | None
>     elapsed: str | None
>     finished_at: datetime | None
>     turn: str | None
>
> class SportsResultEvent:
>     topic: Literal["sports"]
>     type: Literal["sport_result"]
>     payload: SportsGameResult
>
> SportsEvent = SportsResultEvent
> ```
>
> ```json SportsEvent Example theme={null}
> {
>   "topic": "sports",
>   "type": "sport_result",
>   "payload": {
>     "game_id": 5127839,
>     "league_abbreviation": "NBA",
>     "home_team": "Los Angeles Lakers",
>     "away_team": "Boston Celtics",
>     "status": "InProgress",
>     "live": true,
>     "ended": false,
>     "score": "98-94",
>     "period": "Q4",
>     "elapsed": "05:12"
>   }
> }
> ```
>
> `score` is a combined `"<home>-<away>"` string, not separate home and away
> fields.

**API**

Connect to the sports WebSocket:

```text theme={null}
wss://sports-api.polymarket.com/ws
```

> [!NOTE]
>
> The sports WebSocket uses an application-level heartbeat. The server sends the
> text frame `ping` every 5 seconds; reply with `pong` within 10 seconds or the
> server closes the connection.

Once connected, the server starts streaming every game update. No
subscription frame is required.

> **Sports Event**
>
> ```json theme={null}
> {
>   "gameId": 5127839,
>   "leagueAbbreviation": "NBA",
>   "homeTeam": "Los Angeles Lakers",
>   "awayTeam": "Boston Celtics",
>   "status": "InProgress",
>   "live": true,
>   "ended": false,
>   "score": "98-94",
>   "period": "Q4",
>   "elapsed": "05:12"
> }
> ```
>
> Sports messages have no envelope or event-type field. Each message is the
> game update object itself, and `score` is a combined `"<home>-<away>"`
> string.

### Period Values

The meaning and format of a period depend on the sport:

| Values | Meaning |
| - | - |
| `1H`, `2H` | First or second half |
| `1Q`, `2Q`, `3Q`, `4Q` | Quarter |
| `HT` | Halftime |
| `FT` | Full time in regulation |
| `FT OT` | Full time after overtime |
| `FT NR` | Full time with no result |
| `End 1`, `End 2`, … | End of an MLB inning |
| `1/3`, `2/3`, `3/3` | Map number in a best-of-three series |
| `1/5`, `2/5`, … | Map number in a best-of-five series |

### Game Status Values

Status values vary by sport and are case-sensitive:

| Sport | Values |
| - | - |
| NFL | `Scheduled`, `InProgress`, `Final`, `F/OT`, `Suspended`, `Postponed`, `Delayed`, `Canceled`, `Forfeit`, `NotNecessary` |
| NHL | `Scheduled`, `InProgress`, `Final`, `F/OT`, `F/SO`, `Suspended`, `Postponed`, `Delayed`, `Canceled`, `Forfeit`, `NotNecessary` |
| MLB | `Scheduled`, `InProgress`, `Final`, `Suspended`, `Delayed`, `Postponed`, `Canceled`, `Forfeit`, `NotNecessary` |
| NBA and CBB | `Scheduled`, `InProgress`, `Final`, `F/OT`, `Suspended`, `Postponed`, `Delayed`, `Canceled`, `Forfeit`, `NotNecessary` |
| CFB | `Scheduled`, `InProgress`, `Final`, `F/OT`, `Suspended`, `Postponed`, `Delayed`, `Canceled`, `Forfeit` |
| Soccer | `Scheduled`, `InProgress`, `Break`, `Suspended`, `PenaltyShootout`, `Final`, `Awarded`, `Postponed`, `Canceled` |
| Esports | `not_started`, `running`, `finished`, `postponed`, `canceled` |
| Tennis | `scheduled`, `inprogress`, `suspended`, `finished`, `postponed`, `cancelled` |

## Reference Prices

Stream crypto, equity, and time-weighted average reference prices through Polymarket. Authenticate, select the symbols you need, and process historical prices and live updates.

> [!NOTE]
>
> Reference price subscriptions require authentication.
> Existing RTDS integrations should follow the [migration
> guide](https://docs.polymarket.com/migrate/rtds-to-polybolt.md).

Use lowercase symbols. See the [PolyBolt overview](https://docs.polymarket.com/api-reference/live-data/overview.md#supported-symbols) for crypto and TWAP lists, and the equity catalog below for equity symbols.

### Historical Snapshot

Each new subscription provides a snapshot of the preceding two minutes of prices and live updates. Seed local state from the snapshot, then apply the latest price. Applications that only need live prices can ignore the snapshot.

### Crypto Prices

Stream crypto reference prices in USD to keep prices current alongside related markets.

**TypeScript**

Call `client.subscribe` with an authenticated `SecureClient` from `@polymarket/client` 0.11.0 or later. See [Wallet Integrations](https://docs.polymarket.com/getting-started/typescript.md#wallet-integrations) to create the client.

```ts theme={null}
const stream = await client.subscribe([
  { topic: "prices.crypto", symbols: ["btcusd", "ethusd"] },
]);

try {
  for await (const event of stream) {
    // event: CryptoPriceEvent
    if (event.type === "subscribe") {
      const history = event.payload.data;
      // Seed local state from the snapshot.
    } else {
      const price = event.payload.value;
      const observedAt = event.payload.timestamp;
      // Apply the latest price.
    }
  }
} finally {
  await stream.close();
}
```

> **Output: CryptoPriceEvent**
>
> **events: CryptoPriceEvent\[]**
>
> ```ts CryptoPriceEvent Union theme={null}
> type CryptoPriceEvent =
>   | {
>       topic: "prices.crypto";
>       type: "subscribe";
>       timestamp: EpochMilliseconds;
>       seq?: number;
>       dropped?: number;
>       payload: {
>         symbol: string;
>         data: { timestamp: EpochMilliseconds; value: DecimalString }[];
>       };
>     }
>   | {
>       topic: "prices.crypto";
>       type: "update";
>       timestamp: EpochMilliseconds;
>       seq?: number;
>       dropped?: number;
>       payload: {
>         symbol: string;
>         timestamp: EpochMilliseconds;
>         value: DecimalString;
>         receivedAt: EpochMilliseconds | undefined;
>         isCarriedForward: boolean | undefined;
>       };
>     };
> ```
>
> ```json CryptoPriceEvent Example theme={null}
> [
>   {
>     "topic": "prices.crypto",
>     "type": "subscribe",
>     "timestamp": 1788886176000,
>     "seq": 1,
>     "payload": {
>       "symbol": "btcusd",
>       "data": [
>         {
>           "timestamp": 1788886057000,
>           "value": "78803.76173179"
>         },
>         {
>           "timestamp": 1788886058000,
>           "value": "78801.86287741"
>         },
>         {
>           "timestamp": 1788886059000,
>           "value": "78798.28703224"
>         }
>       ]
>     }
>   },
>   {
>     "topic": "prices.crypto",
>     "type": "update",
>     "timestamp": 1788886177000,
>     "seq": 5,
>     "payload": {
>       "symbol": "btcusd",
>       "timestamp": 1788886177000,
>       "value": "78794.15450441"
>     }
>   }
> ]
> ```

For live prices only, skip events whose `type` is `"subscribe"`.

**Python**

Call `client.subscribe` with an authenticated `AsyncSecureClient` from `polymarket-client` 0.11.0 or later. Complete [Secure Client setup](https://docs.polymarket.com/getting-started/python.md#secure-client) first, and run this example inside an async function.

```python theme={null}
from polymarket.streams import CryptoPriceSpec

async with await client.subscribe(
    CryptoPriceSpec(symbols=["btcusd", "ethusd"])
) as stream:
    async for event in stream:
        # event: CryptoPriceEvent
        if event.type == "subscribe":
            history = event.payload.data
            # Seed local state from the snapshot.
        else:
            price = event.payload.value
            observed_at = event.payload.timestamp
            # Apply the latest price.
```

> **Output: CryptoPriceEvent**
>
> **event: CryptoPriceEvent**
>
> ```python CryptoPriceEvent Union theme={null}
> class RealtimePricePoint:
>     timestamp: datetime
>     value: Decimal
>
> class RealtimePriceSnapshot:
>     symbol: str
>     data: tuple[RealtimePricePoint, ...]
>
> class RealtimePriceUpdate:
>     symbol: str
>     timestamp: datetime
>     value: Decimal
>     received_at: datetime | None
>     is_carried_forward: bool | None
>
> class CryptoPriceSnapshotEvent:
>     topic: Literal["prices.crypto"]
>     type: Literal["subscribe"]
>     timestamp: datetime
>     seq: int | None
>     dropped: int | None
>     payload: RealtimePriceSnapshot
>
> class CryptoPriceUpdateEvent:
>     topic: Literal["prices.crypto"]
>     type: Literal["update"]
>     timestamp: datetime
>     seq: int | None
>     dropped: int | None
>     payload: RealtimePriceUpdate
>
> CryptoPriceEvent = CryptoPriceSnapshotEvent | CryptoPriceUpdateEvent
> ```
>
> ```json CryptoPriceEvent Example theme={null}
> {
>   "timestamp": "2026-09-08T16:49:37Z",
>   "seq": 5,
>   "topic": "prices.crypto",
>   "type": "update",
>   "payload": {
>     "timestamp": "2026-09-08T16:49:37Z",
>     "value": "78794.15450441",
>     "symbol": "btcusd"
>   }
> }
> ```

Values are `Decimal`, and timestamps are UTC `datetime` objects.

For live prices only, skip events whose `type` is `"subscribe"`.

**API**

Connect to PolyBolt and authenticate before subscribing to crypto prices.

**1. Connect**

Open a WebSocket connection to:

```text theme={null}
wss://ws-live-v2.polymarket.com/ws
```

**2. Authenticate**

Send your [CLOB API credentials](https://docs.polymarket.com/getting-started/api.md#authentication) on the open connection:

```json theme={null}
{
  "op": "auth",
  "rid": "a1",
  "auth": {
    "apiKey": "<api key>",
    "secret": "<api secret>",
    "passphrase": "<api passphrase>"
  }
}
```

**3. Wait for Authentication**

Wait for the server to confirm authentication before sending a subscription:

```json theme={null}
{
  "op": "authed",
  "rid": "a1"
}
```

**4. Subscribe to Crypto Prices**

Send this frame on the authenticated connection to subscribe to BTC/USD:

```json theme={null}
{
  "op": "subscribe",
  "rid": "s1",
  "subscriptions": [
    {
      "channel": "price.crypto",
      "filter": {
        "symbol": "btcusd"
      }
    }
  ]
}
```

### Equity Prices

Stream reference prices for stocks, ETFs, forex pairs, precious metals, and commodities.

**TypeScript**

Call `client.subscribe` to subscribe to the `prices.equity` topic and your chosen symbols with an authenticated `SecureClient`.

```ts theme={null}
const stream = await client.subscribe([
  { topic: "prices.equity", symbol: "aapl" },
]);

try {
  for await (const event of stream) {
    // event: EquityPriceEvent
    if (event.type === "subscribe") {
      const history = event.payload.data;
      // Seed local state from the snapshot.
    } else {
      const price = event.payload.value;
      const observedAt = event.payload.timestamp;
      // Apply the latest price.
    }
  }
} finally {
  await stream.close();
}
```

> **Output: EquityPriceEvent**
>
> **events: EquityPriceEvent\[]**
>
> ```ts EquityPriceEvent Union theme={null}
> import type { EquityMarketStatus } from "@polymarket/client";
>
> type EquityPriceEvent =
>   | {
>       topic: "prices.equity";
>       type: "subscribe";
>       timestamp: EpochMilliseconds;
>       seq?: number;
>       dropped?: number;
>       payload: {
>         symbol: string;
>         marketStatus?: EquityMarketStatus;
>         data: { timestamp: EpochMilliseconds; value: DecimalString }[];
>       };
>     }
>   | {
>       topic: "prices.equity";
>       type: "update";
>       timestamp: EpochMilliseconds;
>       seq?: number;
>       dropped?: number;
>       payload: {
>         symbol: string;
>         marketStatus?: EquityMarketStatus;
>         timestamp: EpochMilliseconds;
>         value: DecimalString;
>         receivedAt: EpochMilliseconds | undefined;
>         isCarriedForward: boolean | undefined;
>       };
>     };
> ```
>
> ```json EquityPriceEvent Example theme={null}
> [
>   {
>     "topic": "prices.equity",
>     "type": "subscribe",
>     "timestamp": 1788886176400,
>     "seq": 1,
>     "payload": {
>       "symbol": "aapl",
>       "marketStatus": "open",
>       "data": [
>         {
>           "timestamp": 1788886056800,
>           "value": "316.11"
>         },
>         {
>           "timestamp": 1788886057000,
>           "value": "316.11"
>         },
>         {
>           "timestamp": 1788886057200,
>           "value": "316.11001"
>         }
>       ]
>     }
>   },
>   {
>     "topic": "prices.equity",
>     "type": "update",
>     "timestamp": 1788886176600,
>     "seq": 2,
>     "payload": {
>       "symbol": "aapl",
>       "marketStatus": "open",
>       "timestamp": 1788886176600,
>       "value": "316.1",
>       "receivedAt": 1788886176600
>     }
>   }
> ]
> ```

For live prices only, skip events whose `type` is `"subscribe"`.

**Python**

Call `client.subscribe` to subscribe to the `prices.equity` topic and your chosen symbols with an authenticated `AsyncSecureClient`.

```python theme={null}
from polymarket.streams import EquityPriceSpec

async with await client.subscribe(
    EquityPriceSpec(symbol="aapl")
) as stream:
    async for event in stream:
        # event: EquityPriceEvent
        if event.type == "subscribe":
            history = event.payload.data
            # Seed local state from the snapshot.
        else:
            price = event.payload.value
            observed_at = event.payload.timestamp
            # Apply the latest price.
```

> **Output: EquityPriceEvent**
>
> **event: EquityPriceEvent**
>
> ```python EquityPriceEvent Union theme={null}
> from polymarket.streams import EquityMarketStatus
>
> class RealtimePricePoint:
>     timestamp: datetime
>     value: Decimal
>
> class RealtimeEquityPriceSnapshot:
>     symbol: str
>     market_status: EquityMarketStatus | None = None
>     data: tuple[RealtimePricePoint, ...]
>
> class RealtimeEquityPriceUpdate:
>     symbol: str
>     market_status: EquityMarketStatus | None = None
>     timestamp: datetime
>     value: Decimal
>     received_at: datetime | None
>     is_carried_forward: bool | None
>
> class EquityPriceSnapshotEvent:
>     topic: Literal["prices.equity"]
>     type: Literal["subscribe"]
>     timestamp: datetime
>     seq: int | None
>     dropped: int | None
>     payload: RealtimeEquityPriceSnapshot
>
> class EquityPriceUpdateEvent:
>     topic: Literal["prices.equity"]
>     type: Literal["update"]
>     timestamp: datetime
>     seq: int | None
>     dropped: int | None
>     payload: RealtimeEquityPriceUpdate
>
> EquityPriceEvent = EquityPriceSnapshotEvent | EquityPriceUpdateEvent
> ```
>
> ```json EquityPriceEvent Example theme={null}
> {
>   "timestamp": "2026-09-08T16:49:36.600000Z",
>   "seq": 2,
>   "topic": "prices.equity",
>   "type": "update",
>   "payload": {
>     "timestamp": "2026-09-08T16:49:36.600000Z",
>     "value": "316.1",
>     "symbol": "aapl",
>     "market_status": "open",
>     "received_at": "2026-09-08T16:49:36.600000Z"
>   }
> }
> ```

Values are `Decimal`, and timestamps are UTC `datetime` objects.

For live updates only, set `types=["update"]`. Omit `types` or pass an empty list to receive history and updates. Updates may include `received_at` and `is_carried_forward`.

**API**

Reuse the authenticated connection from the [Crypto Prices](#crypto-prices) API example and send this frame to subscribe to AAPL:

```json theme={null}
{
  "op": "subscribe",
  "rid": "s2",
  "subscriptions": [
    {
      "channel": "price.equity",
      "filter": {
        "symbol": "aapl"
      }
    }
  ]
}
```

#### Supported Equity Symbols

| Asset class | Supported symbols |
| - | - |
| Stocks | `aapl`, `tsla`, `msft`, `googl`, `amzn`, `meta`, `nvda`, `nflx`, `pltr`, `open`, `rklb`, `abnb`, `coin`, `hood` |
| ETFs | `qqq`, `spy`, `ewy`, `vxx` |
| Forex | `eurusd`, `gbpusd`, `usdcad`, `usdjpy`, `usdkrw`, `usdchf`, `usdmxn`, `usdzar`, `usdbrl`, `usdnok`, `usdsek`, `usdtry` |
| Precious metals | `xauusd`, `xagusd` |
| Commodities | `wti`, `cc`, `ngd` |

#### Read the Market Status

Use market status to distinguish a closed market from a price that has stopped
updating. Status describes the instrument priced by the provider, so two
providers can report different states for the same symbol. For example, a
Chainlink series can remain open over a weekend while a Pyth series follows the
underlying exchange's session hours.

| Status | Meaning |
| - | - |
| `open` | The provider reports that its instrument's market is trading. |
| `closed` | The provider reports that its instrument's market is closed. |
| `unknown` | The provider cannot determine the market's state. |
| Not stated | No status was supplied. Do not assume the market is open. |

**TypeScript**

In SDK 0.14.0 and later, read `payload.marketStatus` on both `subscribe` snapshots
and `update` events. Import `EquityMarketStatus` from `@polymarket/client` and use
an existing authenticated `SecureClient`:

```ts theme={null}
import { EquityMarketStatus } from "@polymarket/client";

const stream = await client.subscribe([
  { topic: "prices.equity", symbol: "aapl" },
]);

try {
  for await (const event of stream) {
    const status = event.payload.marketStatus;
    if (status === EquityMarketStatus.Closed) {
      console.log(event.payload.symbol, "Market closed");
    } else if (status === EquityMarketStatus.Unknown || status === undefined) {
      console.log(event.payload.symbol, "Market state unavailable");
    }
  }
} finally {
  await stream.close();
}
```

`EquityMarketStatus.Open`, `.Closed`, and `.Unknown` correspond to the three stated
values. `undefined` means not stated. The same field is available on
`prices.equity.twap`. Subscribe with `{ topic: "prices.equity.twap", symbol: "eurusd" }`.
In a snapshot, status is on `payload`, alongside `data`, rather than on each
historical price point.

**Python**

In SDK 0.14.0 and later, read `payload.market_status` on both `subscribe` snapshots
and `update` events. Import `EquityMarketStatus` from `polymarket.streams` and use
an existing authenticated `AsyncSecureClient`:

```python theme={null}
from polymarket.streams import EquityMarketStatus, EquityPriceSpec

async with await client.subscribe(EquityPriceSpec(symbol="aapl")) as stream:
    async for event in stream:
        status = event.payload.market_status
        if status == EquityMarketStatus.CLOSED:
            print(event.payload.symbol, "Market closed")
        elif status is None or status == EquityMarketStatus.UNKNOWN:
            print(event.payload.symbol, "Market state unavailable")
```

`EquityMarketStatus.OPEN`, `.CLOSED`, and `.UNKNOWN` correspond to the three stated
values. `None` means not stated. For equity TWAP, import `EquityTwapPriceSpec` from
`polymarket.streams` and pass `EquityTwapPriceSpec(symbol="eurusd")`. Its payload
has the same `market_status` field. In a snapshot, read status from `payload`,
rather than from the historical points in `payload.data`.

**API**

Read `payload.market_status` on `price.equity` and `price.equity.twap`. A missing
field means not stated. On an authenticated PolyBolt WebSocket connection,
send this subscription frame:

```json Subscribe theme={null}
{
  "op": "subscribe",
  "rid": "s2",
  "subscriptions": [
    {
      "channel": "price.equity",
      "filter": { "symbol": "aapl", "provider": "pyth" }
    }
  ]
}
```

These examples show a received status-only spot update and a snapshot with
status at batch level:

```json Status-Only Update theme={null}
{
  "v": 1,
  "channel": "price.equity",
  "seq": 41,
  "ts": 1792526399800,
  "payload": {
    "symbol": "aapl",
    "value": 262.71,
    "full_accuracy_value": "262.71",
    "timestamp": 1792526399800,
    "received_at": 1792526401200,
    "is_carried_forward": true,
    "source": "pyth",
    "market_status": "closed"
  }
}
```

```json Snapshot theme={null}
{
  "v": 1,
  "channel": "price.equity",
  "seq": 1,
  "ts": 1792526399800,
  "snapshot": true,
  "payload": {
    "symbol": "aapl",
    "source": "pyth",
    "market_status": "closed",
    "data": [
      {
        "timestamp": 1792526399800,
        "value": 262.71,
        "full_accuracy_value": "262.71"
      }
    ]
  }
}
```

See the [PolyBolt overview](https://docs.polymarket.com/api-reference/live-data/overview.md#read-the-market-status)
for the wire contract and provider behavior.

A spot status change can repeat the last price and its timestamp. The update is
marked as carried forward (`isCarriedForward` in TypeScript,
`is_carried_forward` in Python and the API). **Do not discard an update solely
because its price timestamp repeats**: doing so can hide a market close or reopen.
Equity TWAP status changes arrive with a new price.

While status is closed or unknown, a snapshot can retain a closing price older
than the usual two-minute history window. Re-read status and source after every
reconnect. SDK cached spot snapshots retain the last stated status when a later
update omits it. A status change on an older price does not roll newer history
back. An unstated live update still has `undefined` or `None` for its own status.

During market hours, prices can update up to five times per second for each feed.

### TWAP Prices

A time-weighted average price (TWAP) represents an asset's price across a lookback window. Stream Chainlink-computed crypto TWAPs with a fixed 60-second window through Polymarket.

**TypeScript**

Call `client.subscribe` to subscribe to the `prices.crypto.twap` topic and your chosen symbols with an authenticated `SecureClient`.

```ts theme={null}
const stream = await client.subscribe([
  { topic: "prices.crypto.twap", symbols: ["btcusd"] },
]);

try {
  for await (const event of stream) {
    // event: CryptoTwapPriceEvent
    if (event.type === "subscribe") {
      const history = event.payload.data;
      // Seed local state from the snapshot.
    } else {
      const price = event.payload.value;
      const observedAt = event.payload.timestamp;
      // Apply the latest price.
    }
  }
} finally {
  await stream.close();
}
```

> **Output: CryptoTwapPriceEvent**
>
> **events: CryptoTwapPriceEvent\[]**
>
> ```ts CryptoTwapPriceEvent Union theme={null}
> type CryptoTwapPriceEvent =
>   | {
>       topic: "prices.crypto.twap";
>       type: "subscribe";
>       timestamp: EpochMilliseconds;
>       seq?: number;
>       dropped?: number;
>       payload: {
>         symbol: string;
>         data: { timestamp: EpochMilliseconds; value: DecimalString }[];
>         windowSeconds: 60;
>       };
>     }
>   | {
>       topic: "prices.crypto.twap";
>       type: "update";
>       timestamp: EpochMilliseconds;
>       seq?: number;
>       dropped?: number;
>       payload: {
>         symbol: string;
>         timestamp: EpochMilliseconds;
>         value: DecimalString;
>         windowSeconds: 60;
>       };
>     };
> ```
>
> ```json CryptoTwapPriceEvent Example theme={null}
> [
>   {
>     "topic": "prices.crypto.twap",
>     "type": "subscribe",
>     "timestamp": 1788886175000,
>     "seq": 2,
>     "payload": {
>       "symbol": "btcusd",
>       "data": [
>         {
>           "timestamp": 1788886057000,
>           "value": "78788.525642908795142144"
>         },
>         {
>           "timestamp": 1788886058000,
>           "value": "78788.786579938813673472"
>         },
>         {
>           "timestamp": 1788886059000,
>           "value": "78789.052518750893375488"
>         }
>       ],
>       "windowSeconds": 60
>     }
>   },
>   {
>     "topic": "prices.crypto.twap",
>     "type": "update",
>     "timestamp": 1788886177000,
>     "seq": 3,
>     "payload": {
>       "symbol": "btcusd",
>       "timestamp": 1788886177000,
>       "value": "78803.715261094101516288",
>       "windowSeconds": 60
>     }
>   }
> ]
> ```

For live prices only, skip events whose `type` is `"subscribe"`. `payload.value` is already an exact decimal string. No E18 conversion is needed.

Use a canonical symbol such as `btcusd`. The returned `windowSeconds: 60` describes the lookback period, not the update interval. Use `payload.timestamp` to check price freshness.

**Python**

Call `client.subscribe` to subscribe to the `prices.crypto.twap` topic and your chosen symbols with an authenticated `AsyncSecureClient`.

```python theme={null}
from polymarket.streams import CryptoTwapPriceSpec

async with await client.subscribe(
    CryptoTwapPriceSpec(symbols=["btcusd"])
) as stream:
    async for event in stream:
        # event: CryptoTwapPriceEvent
        if event.type == "subscribe":
            history = event.payload.data
            # Seed local state from the snapshot.
        else:
            price = event.payload.value
            observed_at = event.payload.timestamp
            # Apply the latest price.
```

> **Output: CryptoTwapPriceEvent**
>
> **event: CryptoTwapPriceEvent**
>
> ```python CryptoTwapPriceEvent Union theme={null}
> class RealtimePricePoint:
>     timestamp: datetime
>     value: Decimal
>
> class RealtimeTwapSnapshot:
>     symbol: str
>     data: tuple[RealtimePricePoint, ...]
>     window_seconds: Literal[60]
>
> class RealtimeTwapUpdate:
>     symbol: str
>     timestamp: datetime
>     value: Decimal
>     window_seconds: Literal[60]
>
> class CryptoTwapPriceSnapshotEvent:
>     topic: Literal["prices.crypto.twap"]
>     type: Literal["subscribe"]
>     timestamp: datetime
>     seq: int | None
>     dropped: int | None
>     payload: RealtimeTwapSnapshot
>
> class CryptoTwapPriceUpdateEvent:
>     topic: Literal["prices.crypto.twap"]
>     type: Literal["update"]
>     timestamp: datetime
>     seq: int | None
>     dropped: int | None
>     payload: RealtimeTwapUpdate
>
> CryptoTwapPriceEvent = CryptoTwapPriceSnapshotEvent | CryptoTwapPriceUpdateEvent
> ```
>
> ```json CryptoTwapPriceEvent Example theme={null}
> {
>   "timestamp": "2026-09-08T16:49:37Z",
>   "seq": 3,
>   "topic": "prices.crypto.twap",
>   "type": "update",
>   "payload": {
>     "timestamp": "2026-09-08T16:49:37Z",
>     "value": "78803.715261094101516288",
>     "symbol": "btcusd",
>     "window_seconds": 60
>   }
> }
> ```

Values are `Decimal`, and timestamps are UTC `datetime` objects.

`payload.window_seconds` is always `60` and describes the lookback window. Use `payload.value` directly without E18 scaling, and use `payload.timestamp` to check freshness.

**API**

Reuse the authenticated connection from the [Crypto Prices](#crypto-prices) API example and send this frame to subscribe to the 60-second BTC/USD TWAP:

```json theme={null}
{
  "op": "subscribe",
  "rid": "s3",
  "subscriptions": [
    {
      "channel": "price.crypto.twap",
      "filter": {
        "symbol": "btcusd",
        "window_seconds": 60
      }
    }
  ]
}
```

## Comments

> [!WARNING]
>
> Live comment and reaction streaming is being retired without a replacement
> streaming channel. Check below for the corresponding fetch method.

**TypeScript**

Create a public client and call `client.listComments()` to list comments for an event. This method does not provide live comment or reaction events.

```ts theme={null}
import {
  CommentParentEntityType,
  createPublicClient,
} from "@polymarket/client";

const client = createPublicClient();
const pages = client.listComments({
  parentEntityId: "18396",
  parentEntityType: CommentParentEntityType.Event,
  pageSize: 20,
  order: "id",
  ascending: true,
});
const page = await pages.firstPage();

console.log(page.items);
```

`page.items` contains `Comment` objects. Check `page.hasMore` for more results and continue with `pages.from(page.nextCursor)`.

> **Output: Comment**
>
> One comment from `page.items`, serialized as JSON with selected fields shown.
>
> ```json theme={null}
> {
>   "id": "1020073",
>   "body": "@truce @yayyo",
>   "parentEntityType": "Event",
>   "parentEntityID": "18396",
>   "createdAt": "2025-02-12T00:48:05.40822Z",
>   "reactionCount": 2
> }
> ```

**Python**

Create a public client and call `client.list_comments()` to list comments for an event. This method does not provide live comment or reaction events.

```python theme={null}
from polymarket import AsyncPublicClient

async with AsyncPublicClient() as client:
    pages = client.list_comments(
        parent_entity_id="18396",
        parent_entity_type="Event",
        page_size=20,
        order="id",
        ascending=True,
    )
    page = await pages.first_page()

    print(page.items)
```

`page.items` contains `Comment` objects. Check `page.has_more` for more results and continue with `pages.from_cursor(page.next_cursor)`.

> **Output: Comment**
>
> One comment from `page.items`, serialized as JSON with selected fields shown.
>
> ```json theme={null}
> {
>   "id": "1020073",
>   "body": "@truce @yayyo",
>   "parent_entity_type": "Event",
>   "parent_entity_id": "18396",
>   "created_at": "2025-02-12T00:48:05.408220Z",
>   "reaction_count": 2
> }
> ```

**API**

Use `GET /comments` on the Gamma API to list comments for an event. This request does not provide live comment or reaction events.

```bash theme={null}
curl --get "https://gamma-api.polymarket.com/comments" \
  --data-urlencode "parent_entity_id=18396" \
  --data-urlencode "parent_entity_type=Event" \
  --data-urlencode "limit=20" \
  --data-urlencode "offset=0" \
  --data-urlencode "order=id" \
  --data-urlencode "ascending=true"
```

The response is an array of comments.

> **Output: Comment\[]**
>
> First comment in the response, with selected fields shown.
>
> ```json theme={null}
> [
>   {
>     "id": "1020073",
>     "body": "@truce @yayyo",
>     "parentEntityType": "Event",
>     "parentEntityID": 18396,
>     "createdAt": "2025-02-12T00:48:05.40822Z",
>     "reactionCount": 2
>   }
> ]
> ```


This documentation is built and hosted on [Mintlify](https://mintlify.com), a developer documentation platform.