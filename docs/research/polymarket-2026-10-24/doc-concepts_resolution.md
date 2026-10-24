> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.com/llms.txt
> Use this file to discover all available pages before exploring further.

# Resolution

> How markets are resolved and winning positions redeemed

When the outcome of an event becomes known, the market is **resolved**. Resolution determines which outcome won, allowing holders of winning tokens to redeem them for \$1 each. Losing tokens become worthless.

Polymarket uses two **Resolution Sources**, UMA for prediction markets and Chainlink TWAP for up-down markets.

## UMA-Resolved Markets

Prediction markets use the **UMA Optimistic Oracle** for decentralized, permissionless resolution.

![](https://mintcdn.com/polymarket-292d1b1b/FOMte3ewbG-LVy3k/images/core-concepts/resolution-lifecycle.png?fit=max\&auto=format\&n=FOMte3ewbG-LVy3k\&q=85\&s=6726569af3efd6f4fda54528c8eb0d0a)

### Resolution Rules

Every market has pre-defined resolution rules that specify:

* **Resolution source** — Where the outcome will be determined from (e.g., official announcements, specific websites)
* **End date** — When the market is eligible for resolution
* **Edge cases** — How ambiguous situations should be handled

> [!WARNING]
>
> Always read the resolution rules before trading. The market title describes
> the question, but the **rules** define how it resolves.

**1. Proposal**

Anyone can propose a resolution by:

1. Selecting the winning outcome
2. Posting a bond (typically \$750 pUSD)
3. Submitting the proposal to the UMA Oracle

If the proposal is correct and undisputed, the proposer receives their bond back plus a reward.

> [!WARNING]
>
> If you propose incorrectly or too early, you lose your entire bond. Only
> propose if you're confident in the outcome and understand the process.

**2. Challenge Period**

After a proposal, there's a **2-hour challenge period** where anyone can dispute the outcome.

* **If no dispute**: The proposal is accepted and the market resolves
* **If disputed**: A new proposal round begins. If the second proposal is also disputed, the resolution escalates to UMA's DVM (Data Verification Mechanism) for a token holder vote.

There are three possible resolution flows:

1. **No dispute** — Propose then Resolve (fastest, \~2 hours)
2. **One dispute** — Propose, Challenge, second Propose, Resolve (second proposal accepted)
3. **Two disputes** — Propose, Challenge, second Propose, second Challenge, Resolve via DVM vote

**3. Dispute - If Challenged**

To dispute a proposal:

1. Post a counter-bond (same amount as proposer, typically \$750)
2. The dispute triggers a new proposal round, or if already in the second round, a debate period

During the **24-48 hour debate period**, evidence can be submitted in UMA's Discord channels (`#evidence-rationale` and `#voting-discussion`).

**4. UMA Vote**

After the debate period, UMA token holders vote on the correct outcome. The voting process takes approximately 48 hours.

| Outcome | Result | Bond Distribution |
| - | - | - |
| **Proposer wins** | Original proposal accepted | Proposer gets bond back + half of disputer's bond |
| **Disputer wins** | Proposal rejected, new proposal needed | Disputer gets bond back + half of proposer's bond |
| **Too Early** | Event hasn't concluded yet | Disputer gets bond back + half of proposer's bond |
| **Unknown/50-50** | Neither outcome applicable (rare) | Market resolves 50/50 — each token redeems for \$0.50; disputer gets bond back + half of proposer's bond |

### Clarifications

In rare cases, unforeseen circumstances require clarification of the rules after trading begins. Polymarket may issue an **"Additional context"** update that proposers and voters should consider during resolution.

Clarifications:

* Cannot change the fundamental intent of the question
* Are published onchain via the bulletin board contract
* Should be considered by UMA voters when resolving disputes

> [!TIP]
>
> If you believe a clarification is needed, request it in the [Polymarket
> Discord](https://discord.com/invite/polymarket) `#market-review` channel.

### Resolution Timeline

| Phase | Duration |
| - | - |
| Challenge period | 2 hours |
| Debate period (if disputed) | 24-48 hours |
| UMA voting (if disputed) | \~48 hours |

**Undisputed resolution**: \~2 hours after proposal

**Disputed resolution**: 4-6 days total

See [Resolution Contracts](https://docs.polymarket.com/resources/contracts.md#resolution-contracts) for UMA contract addresses.

## Chainlink TWAP–Resolved Up-Down Markets

Each market specifies an asset pair, a start time, and a duration. For example, a market may ask whether BTC/USD moves Up or Down during a specific one-hour window. The TWAP at the start sets the **price to beat**. When the duration ends, the TWAP at that time sets the **final price**. Both values come from the same **Chainlink TWAP stream**. A **time-weighted average price (TWAP)** represents an asset's average price over a defined lookback window.

![](https://mintcdn.com/polymarket-292d1b1b/_aswwXEhQSghbFEe/images/core-concepts/chainlink-twap-resolution.png?fit=max\&auto=format\&n=_aswwXEhQSghbFEe\&q=85\&s=1a7a30851b1155e88dbf05322a395a41)

### How Resolution Works

The market resolves **Up** when the final price is **equal to or greater** than the price to beat, and **Down** when it is **lower**. Because the inputs and comparison are fixed, the same observations always produce the same result without UMA's proposal-and-dispute process.

## After Resolution

Once a market resolves:

* **Trading stops** — You can no longer buy or sell tokens for this market
* **Winning tokens** become redeemable for \$1.00 each
* **Losing tokens** become worthless (\$0.00)

## Resources

* [UMA Oracle Portal](https://oracle.uma.xyz/) — View and interact with proposals
* [UMA Documentation](https://docs.uma.xyz/) — Learn more about the Optimistic Oracle
* [Chainlink TWAP Prices](https://docs.polymarket.com/market-data/chainlink-twap.md) — Learn how to access Chainlink-computed TWAP prices
* [Chainlink TWAP Documentation](https://chain.link/article/twap-and-vwap-pricing-mechanisms) — Learn how time-weighted average pricing works
* [Polymarket Discord](https://discord.com/invite/polymarket) — Discuss resolutions and request clarifications


This documentation is built and hosted on [Mintlify](https://mintlify.com), a developer documentation platform.