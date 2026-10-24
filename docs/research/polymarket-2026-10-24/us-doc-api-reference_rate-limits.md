> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Rate Limits

> API rate limits and how to stay within them.

Exceeding a rate limit returns `429 Too Many Requests`. Limits are enforced per source IP and Cloudflare location, so requests using different API keys from the same IP share counters at that location.

***

## Limits

The Retail API has a general limit of **25 requests/s**, shared across endpoints except quote creation and deletion.

Exceptions:

* `POST /v1/combos` and `POST /v1/rfqs`: **1 request/s**, shared.
* `POST /v1/rfqs/quotes`: **100 requests/s**.
* `DELETE /v1/rfqs/{rfqId}/quotes/{quoteId}`: **100 requests/s**.
* `GET /v1/rfqs`: **1 request/s**.
* `GET /v1/rfqs/quotes`: **1 request/s**.
* `GET /v1/rfqs/trades`: **1 request/s**.
* `GET /v1/rfqs/user-id`: **1 request/s**.

***

## When you're rate limited

```json theme={null}
{ "status": 429, "message": "Too Many Requests" }
```

Stop immediately, wait at least 1 second, then retry with exponential backoff:

```python theme={null}
import time

def make_request_with_retry(fn, max_retries=3):
    for attempt in range(max_retries):
        response = fn()
        if response.status_code != 429:
            return response
        time.sleep(2 ** attempt)
    raise Exception("Max retries exceeded")
```

***

## Latency stopgap on orders

During periods of increased latency, Polymarket US applies a **5-second stopgap** to inbound orders. If an order has been received by Polymarket US but has not been processed within 5 seconds, we reject it to protect you from a bad fill at a stale price.

> [!WARNING]
>
> These rejects carry the message **`Global Rate Limit Exceeded`**, but they are **not** an actual rate limit. You do **not** need to throttle your traffic in response to them. Treat them as a transient latency reject, not a signal to back off.

What it applies to:

* **New orders** — rejected if not processed within 5 seconds.
* **Order modifications via cancel/replace** — also subject to the stopgap.
* **Pure cancels are not affected** — a standalone cancel is never rejected by this stopgap.

You can always cancel an order before you have received an acknowledgement, and even before it has been processed.

***

## Use WebSocket instead of polling

The single most effective way to stay within limits is to stop polling and use WebSocket streams. One persistent connection replaces hundreds of repeated REST calls.

| Don't poll | Use instead |
| - | - |
| `GET /v1/orders/open` repeatedly | `/v1/ws/private` - `SUBSCRIPTION_TYPE_ORDER` |
| `GET /v1/portfolio/positions` repeatedly | `/v1/ws/private` - `SUBSCRIPTION_TYPE_POSITION` |
| `GET /v1/account/balances` repeatedly | `/v1/ws/private` - `SUBSCRIPTION_TYPE_ACCOUNT_BALANCE` |
| `GET /v1/markets/{slug}/bbo` repeatedly | `/v1/ws/markets` - `SUBSCRIPTION_TYPE_MARKET_DATA_LITE` |
| `GET /v1/markets/{slug}/book` repeatedly | `/v1/ws/markets` - `SUBSCRIPTION_TYPE_MARKET_DATA` |

***

## Cache reference data

Market and event metadata changes infrequently. Fetch it once on startup and refresh periodically rather than on every request.

```python theme={null}
import time

class MarketCache:
    def __init__(self, client, ttl=300):
        self._client = client
        self._cache = {}
        self._ttl = ttl
        self._last_refresh = None

    def get(self, slug):
        if self._needs_refresh():
            self._refresh()
        return self._cache.get(slug)

    def _needs_refresh(self):
        return not self._last_refresh or (time.time() - self._last_refresh) > self._ttl

    def _refresh(self):
        markets = self._client.markets.list({"limit": 100, "active": True})
        self._cache = {m["slug"]: m for m in markets["markets"]}
        self._last_refresh = time.time()
```

***

## For automated systems

If you're running an automated trading system and need higher limits for production:

1. Document your use case and expected request volume
2. Email [support@polymarket.us](mailto:support@polymarket.us)
3. Include which endpoints you need higher limits for


This documentation is built and hosted on [Mintlify](https://mintlify.com), a developer documentation platform.