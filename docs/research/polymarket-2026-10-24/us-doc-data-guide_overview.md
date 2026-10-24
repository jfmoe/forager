> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Data Guide Overview

> Connect to the Polymarket Exchange for read-only market data consumption

This guide is for users who want to connect to the Polymarket Exchange for **read-only market data consumption** without trading functionality.

## Who Is This For?

The Data Guide is designed for:

* **Data vendors** building market data products
* **Research teams** analyzing prediction market data
* **Analytics platforms** displaying market information
* **Developers** building read-only applications

## What You Can Access

With read-only access, you can consume:

| Data Type | Description | Access Method |
| - | - | - |
| **Market Data** | Real-time quotes, BBO, L2 order book | REST API, gRPC Streaming |
| **Reference Data** | Instruments, symbols, metadata | REST API |
| **Market Statistics** | OHLC, last trade price, volume, open interest | REST API, gRPC Streaming |

## Available Endpoints

### REST API

| Endpoint | Description |
| - | - |
| `/v1/orderbook/{symbol}/bbo` | Best bid/offer for a symbol |
| `/v1/orderbook/{symbol}` | L2 order book depth |
| `/v1/refdata/instruments` | List all instruments |
| `/v1/refdata/symbols` | List all symbols |
| `/v1/refdata/metadata` | Instrument metadata |

### gRPC Streaming

For real-time data, use the [Market Data Stream](https://docs.polymarket.us/streaming-endpoints/market-data-stream.md):

* Subscribe to BBO updates
* Subscribe to L2 order book changes
* Receive market statistics updates (OHLC, last trade, volume)


This documentation is built and hosted on [Mintlify](https://mintlify.com), a developer documentation platform.