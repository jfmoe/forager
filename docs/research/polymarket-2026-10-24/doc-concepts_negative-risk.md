> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.com/llms.txt
> Use this file to discover all available pages before exploring further.

# Negative Risk Markets

> Capital-efficient trading for multi-outcome events

**Negative risk** is a mechanism for multi-outcome events where only one outcome can win. It enables capital-efficient trading by allowing positions across all outcomes within an event to be related through a **conversion** operation.

## How It Works

In a standard multi-outcome event, each market is independent. If you want to bet against one outcome, you must buy that outcome's No positions—but those No positions have no relationship to the other outcomes.

Negative risk changes this. In a neg risk event:

* A **No share** in any market can be converted into **1 Yes share in every other market**

### Example

Consider an event: "Who will win the 2024 Presidential Election?" with three outcomes:

| Outcome | Your Position |
| - | - |
| Trump | — |
| Harris | — |
| Other | 1 No |

With negative risk, that 1 No on "Other" can be converted into:

| Outcome | After Conversion |
| - | - |
| Trump | 1 Yes |
| Harris | 1 Yes |
| Other | — |

This is capital-efficient because betting against one outcome is economically equivalent to betting *for* all other outcomes.

## Contract Addresses

Neg risk markets use different contracts than standard markets:

See [Contracts](https://docs.polymarket.com/resources/contracts.md) for the contracts used by each position system.

## Augmented Negative Risk

Standard negative risk requires the complete set of outcomes to be known at market creation. But sometimes new outcomes emerge after trading begins (e.g., a new candidate enters a race). Augmented negative risk applies to both CTF and Polymarket Protocol V2 markets.

**Augmented negative risk** solves this with:

| Outcome Type | Description |
| - | - |
| **Named outcomes** | Known outcomes (e.g., "Trump", "Harris") |
| **Additional outcomes** | Outcomes that can become available after the event starts |
| **Explicit Other** | Catches any outcome not explicitly named |

### Trading Rules for Augmented Neg Risk

> [!WARNING]
>
> Only trade on **named outcomes**. Placeholder outcomes should be ignored until
> they are named or until resolution occurs. The Polymarket UI does not display
> unnamed outcomes.

* If the correct outcome at resolution is not named, the market resolves to "Other"
* The "Other" outcome's definition changes as placeholders are clarified—avoid trading it directly

## Technical Details

### Conversion Mechanics

Conversion is atomic. Polymarket Protocol V2 uses the Router and NegRiskModule.
CTF uses the Neg Risk Adapter.

1. You hold 1 No position for Outcome A
2. Call the conversion operation for the market's position system
3. You receive 1 Yes position for every other outcome in the event

## Next Steps

> **[Markets & Events](https://docs.polymarket.com/concepts/markets-events.md)**
>
> Understand how multi-market events are structured.

> **[Positions & Tokens](https://docs.polymarket.com/concepts/positions-tokens.md)**
>
> Learn about position operations like split, merge, and redeem.


This documentation is built and hosted on [Mintlify](https://mintlify.com), a developer documentation platform.