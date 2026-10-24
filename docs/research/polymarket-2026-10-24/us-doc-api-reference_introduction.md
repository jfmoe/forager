> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Introduction

> Overview of the Polymarket US API

The Polymarket US API is split into two parts: an authenticated API for trading and a public API for reading market data. For support, contact [support@polymarket.us](mailto:support@polymarket.us).

## Authenticated API

```
https://api.polymarket.us
```

Use the authenticated API to trade. This is where you place orders, check your positions, and manage your account. Every request requires an API key - see [Authentication](https://docs.polymarket.us/api-reference/authentication.md) to get set up.

| Group | What you get |
| - | - |
| **Orders** | Place, modify, cancel, and query orders |
| **Portfolio** | View positions and trading activity |
| **Account** | Check balances and buying power |

The authenticated API also provides two WebSocket endpoints for real-time streaming:

| Endpoint | Purpose |
| - | - |
| `wss://api.polymarket.us/v1/ws/private` | Real-time order, position, and balance updates |
| `wss://api.polymarket.us/v1/ws/markets` | Real-time order book and trade streaming |

## Public API

```
https://gateway.polymarket.us
```

Use the public API to browse what's available on Polymarket US. No API key needed. This is where you fetch markets, events, series, sports data, and search results. If you're building something that displays market information - prices, odds, order books - this is all you need.

| Group | What you get |
| - | - |
| **Markets** | List markets, get order books, BBO, settlement prices, and price history |
| **Events** | List events, get event details |
| **Series** | List series (e.g., NFL 2025-26 Season) |
| **Sports** | Leagues, teams, game schedules |
| **Search** | Full-text search across events and markets |


This documentation is built and hosted on [Mintlify](https://mintlify.com), a developer documentation platform.