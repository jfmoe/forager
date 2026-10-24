> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.com/llms.txt
> Use this file to discover all available pages before exploring further.

# SDKs & APIs

> Choose how to integrate with Polymarket.

Choose the interface that best fits your application. Use an SDK for a unified,
typed integration, or work with the APIs directly when you need lower-level
control.

> **[TypeScript SDK](https://docs.polymarket.com/getting-started/typescript.md)**
>
> Build services, bots, scripts, and applications with `@polymarket/client`.

> **[Python SDK](https://docs.polymarket.com/getting-started/python.md)**
>
> Build services, notebooks, scripts, and data workflows with
> `polymarket-client`.

> **[API](https://docs.polymarket.com/getting-started/api.md)**
>
> Build directly against Polymarket REST APIs and WebSocket streams.

> [!NOTE]
>
> A unified Rust SDK is in development. For now, see the [Rust migration
> guide](https://docs.polymarket.com/migrate/clob-sdk-to-unified-sdk.md#rust).

## Compare Options

| | TypeScript SDK | Python SDK | API |
| - | - | - | - |
| Best for | Node.js applications, bots, and services | Services, scripts, and notebooks | Unsupported runtimes and low-level control |
| Data models | Typed models | Typed models | Raw JSON |
| Polymarket surfaces | One client interface | One client interface | Service-specific APIs |
| Authentication and signing | SDK helpers | SDK helpers | Implement directly |
| Pagination | Consistent paginator | Consistent paginator | Varies by API |
| Realtime | SDK subscriptions | SDK subscriptions | Connect to each WebSocket surface |

## Existing Integrations

Already using an earlier Polymarket SDK? Follow the [SDK migration
guide](https://docs.polymarket.com/migrate/clob-sdk-to-unified-sdk.md) to update your integration.


This documentation is built and hosted on [Mintlify](https://mintlify.com), a developer documentation platform.