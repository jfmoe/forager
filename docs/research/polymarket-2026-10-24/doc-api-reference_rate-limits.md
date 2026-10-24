> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.com/llms.txt
> Use this file to discover all available pages before exploring further.

# Rate Limits

> Cloudflare IP-based request limits for Polymarket APIs

The limits on this page are IP-based and enforced using Cloudflare's throttling system. When you exceed the limit for any endpoint, requests are throttled (delayed/queued) rather than immediately rejected. Limits reset on sliding time windows.

> [!NOTE]
>
> CLOB order and cancellation requests also use separate [per-signer
> token-bucket limits](https://docs.polymarket.com/api-reference/trading-rate-limits.md).

***

## General

| Endpoint | Limit |
| - | - |
| General rate limiting | 15,000 req / 10s |
| Health check (`/ok`) | 100 req / 10s |

***

## Gamma API

Base URL: `https://gamma-api.polymarket.com`

| Endpoint | Limit |
| - | - |
| General | 4,000 req / 10s |
| `/events` | 500 req / 10s |
| `/markets` | 300 req / 10s |
| `/markets` + `/events` listing | 900 req / 10s |
| `/comments` | 200 req / 10s |
| `/tags` | 200 req / 10s |
| `/public-search` | 350 req / 10s |

***

## Data API

Base URL: `https://data-api.polymarket.com`

### v1

| Endpoint | Limit |
| - | - |
| General | 1,000 req / 10s |
| `/trades` | 200 req / 10s |
| `/positions` | 150 req / 10s |
| `/closed-positions` | 150 req / 10s |
| Health check (`/ok`) | 100 req / 10s |

### v2

| Endpoint | Limit |
| - | - |
| General (all `/v2` endpoints) | 800 req / 10s |
| `/v2/trades` | 300 req / 10s |
| `/v2/positions`, `/v2/positions/combos` | 200 req / 10s |
| `/v2/activity`, `/v2/activity/combos` | 200 req / 10s |
| `/v2/prices-history` | 200 req / 10s |
| `/v2/status` | 100 req / 10s |

***

## CLOB API

Base URL: `https://clob.polymarket.com`

### General

| Endpoint | Limit |
| - | - |
| General | 9,000 req / 10s |
| `GET` balance allowance | 200 req / 10s |
| `UPDATE` balance allowance | 50 req / 10s |

### Market Data

| Endpoint | Limit |
| - | - |
| `/book` | 1,500 req / 10s |
| `/books` | 500 req / 10s |
| `/price` | 1,500 req / 10s |
| `/prices` | 500 req / 10s |
| `/midpoint` | 1,500 req / 10s |
| `/midpoints` | 500 req / 10s |
| `/prices-history` | 1,000 req / 10s |
| Market tick size | 200 req / 10s |

### Ledger

| Endpoint | Limit |
| - | - |
| `/trades`, `/orders`, `/notifications`, `/order` | 900 req / 10s |
| `/data/orders` | 500 req / 10s |
| `/data/trades` | 500 req / 10s |
| `/notifications` | 125 req / 10s |

### Authentication

| Endpoint | Limit |
| - | - |
| API key endpoints | 100 req / 10s |

### Trading

Cloudflare applies both **burst** limits (short spikes allowed) and **sustained** limits (longer-term average) to trading endpoints.

| Endpoint | Burst Limit | Sustained Limit |
| - | - | - |
| `POST /order` | 5,000 req / 10s | 120,000 req / 10 min |
| `DELETE /order` | 5,000 req / 10s | 120,000 req / 10 min |
| `POST /orders` | 2,000 req / 10s | 21,000 req / 10 min |
| `DELETE /orders` | 2,000 req / 10s | 15,000 req / 10 min |
| `DELETE /cancel-all` | 250 req / 10s | 6,000 req / 10 min |
| `DELETE /cancel-market-orders` | 1,500 req / 10s | 21,000 req / 10 min |

***

## Bridge API

Base URL: `https://bridge.polymarket.com`

| Endpoint | Limit |
| - | - |
| General | 50 req / 10s |

***

## Other

| Endpoint | Limit |
| - | - |
| Relayer `/submit` | 25 req / 1 min |
| User PNL API | 200 req / 10s |


This documentation is built and hosted on [Mintlify](https://mintlify.com), a developer documentation platform.