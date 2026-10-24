> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.com/llms.txt
> Use this file to discover all available pages before exploring further.

# Markets & Events

> Understanding the fundamental building blocks of Polymarket

Every prediction on Polymarket is organized into **events** and **markets**. An event is the question or topic shown to users, while each market is a resolvable binary condition with Yes and No outcomes. See [Positions & Tokens](https://docs.polymarket.com/concepts/positions-tokens.md) to learn how outcome balances are represented.

![](https://mintcdn.com/polymarket-292d1b1b/FOMte3ewbG-LVy3k/images/core-concepts/event-market.png?fit=max\&auto=format\&n=FOMte3ewbG-LVy3k\&q=85\&s=4c62bd08a405868307cdd6799b368ca5)

## Markets

A **market** is the fundamental tradable unit on Polymarket. Each market represents a resolvable condition with Yes and No outcomes.

![](https://mintcdn.com/polymarket-292d1b1b/FOMte3ewbG-LVy3k/images/core-concepts/event.png?fit=max\&auto=format\&n=FOMte3ewbG-LVy3k\&q=85\&s=0c9a264aec9a22ce5a20c4cc7980806d)

Every market has:

| Identifier | Description |
| - | - |
| **Condition ID** | Unique identifier for the market's resolvable condition |
| **Question ID** | Hash of the market question used for resolution |
| **Token IDs** | ERC1155 token IDs that track positions — one for Yes and one for No |

> [!NOTE]
>
> Markets can only be traded via the CLOB if `enableOrderBook` is `true`. Some
> markets may exist onchain but not be available for order book trading.

### Market Example

A simple market might be:

> **"Will Bitcoin reach \$150,000 by December 2026?"**

This creates two outcome positions:

* **Yes position** - Redeemable for `$1` if Bitcoin reaches `$150k`
* **No position** - Redeemable for `$1` if Bitcoin doesn't reach `$150k`

## Events

An **event** is a container that groups one or more related markets together. Events provide organizational structure and enable multi-outcome predictions.

### Single-Market Events

When an event contains just one market, it creates a simple market pair. The event and market are essentially equivalent.

```
Event: Will Bitcoin reach $100,000 by December 2024?
└── Market: Will Bitcoin reach $100,000 by December 2024? (Yes/No)
```

### Multi-Market Events

When an event contains two or more markets, it groups related binary questions under one event. Some multi-market events represent mutually exclusive outcomes.

Polymarket links these mutually exclusive markets as a [negative-risk group](https://docs.polymarket.com/concepts/negative-risk.md). Exactly one market in the group resolves Yes, while every other market resolves No.

```
Event: Who will win the 2024 Presidential Election?
├── Market: Donald Trump? (Yes/No)
├── Market: Joe Biden? (Yes/No)
├── Market: Kamala Harris? (Yes/No)
└── Market: Other? (Yes/No)
```

## Identifying Markets

Every market and event has a unique **slug** that appears in the Polymarket URL:

```
https://polymarket.com/event/fed-decision-in-october
                              └── slug: fed-decision-in-october
```

You can use slugs to fetch specific markets or events from the API:

```bash theme={null}
# Fetch event by slug
curl "https://gamma-api.polymarket.com/events?slug=fed-decision-in-october"
```

## Sports Markets

Specifically for sports markets, outstanding limit orders are **automatically cancelled** once the game begins, clearing the order book at the official start time. However, game start times can shift — if a game starts earlier than scheduled, orders may not be cleared in time. Always monitor your orders closely around game start times.

***

## Next Steps

> **[Prices & Orderbook](https://docs.polymarket.com/concepts/prices-orderbook.md)**
>
> Learn how prices are determined and how the order book works.

> **[Fetching Market Data](https://docs.polymarket.com/market-data/overview.md)**
>
> Start querying markets and events from the API.


This documentation is built and hosted on [Mintlify](https://mintlify.com), a developer documentation platform.