> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Get Price History

> Returns book-derived Yes and No display prices for one market. Use the market slug as `symbol`.

Supported fixed profiles:
- `INTERVAL_1H` with `fidelity=1`: approximately one-minute points for the last hour
- `INTERVAL_6H` with `fidelity=1`: approximately one-minute points for the last six hours
- `INTERVAL_1D` with `fidelity=5`: five-minute points for the last day
- `INTERVAL_1W` with `fidelity=180`: three-hour points for the last week
- `INTERVAL_1M` with `fidelity=180`: three-hour points for the last 30 days
- `INTERVAL_ALL` with `fidelity=180`: three-hour points for newer markets and daily points for longer histories
- `INTERVAL_LIVE` with `fidelity=1`: from 15 minutes before the event starts through now

For a custom range, provide both Unix timestamps and use `fidelity=1`. Custom ranges are intended for short windows of up to 24 hours and return stored observations, which may be irregular or more frequent than once per minute.

`longPrice` is the Yes display price, normally derived from the best ask. `shortPrice` is the No display price, normally derived from one minus the best bid. They can sum to more than 1 because they preserve the bid-ask spread; these are not individual trades.

The endpoint supports one market per request. Cache identical responses for at least 30 seconds, stagger refreshes across markets, and observe the [20 requests/second/IP public limit](/api-reference/rate-limits). For live updates after loading history, use the [Markets WebSocket](/api-reference/websocket/markets).



## OpenAPI

````yaml /api-reference/oapi-schemas/price-history-schema.json get /v1/price-history
openapi: 3.0.3
info:
  title: Price History API
  version: 1.0.0
  description: Historical display prices for Polymarket US markets.
servers:
  - url: https://gateway.polymarket.us
    description: Production server
security: []
paths:
  /v1/price-history:
    get:
      tags:
        - Price History
      summary: Get Price History
      description: >-
        Returns book-derived Yes and No display prices for one market. Use the
        market slug as `symbol`.


        Supported fixed profiles:

        - `INTERVAL_1H` with `fidelity=1`: approximately one-minute points for
        the last hour

        - `INTERVAL_6H` with `fidelity=1`: approximately one-minute points for
        the last six hours

        - `INTERVAL_1D` with `fidelity=5`: five-minute points for the last day

        - `INTERVAL_1W` with `fidelity=180`: three-hour points for the last week

        - `INTERVAL_1M` with `fidelity=180`: three-hour points for the last 30
        days

        - `INTERVAL_ALL` with `fidelity=180`: three-hour points for newer
        markets and daily points for longer histories

        - `INTERVAL_LIVE` with `fidelity=1`: from 15 minutes before the event
        starts through now


        For a custom range, provide both Unix timestamps and use `fidelity=1`.
        Custom ranges are intended for short windows of up to 24 hours and
        return stored observations, which may be irregular or more frequent than
        once per minute.


        `longPrice` is the Yes display price, normally derived from the best
        ask. `shortPrice` is the No display price, normally derived from one
        minus the best bid. They can sum to more than 1 because they preserve
        the bid-ask spread; these are not individual trades.


        The endpoint supports one market per request. Cache identical responses
        for at least 30 seconds, stagger refreshes across markets, and observe
        the [20 requests/second/IP public limit](/api-reference/rate-limits).
        For live updates after loading history, use the [Markets
        WebSocket](/api-reference/websocket/markets).
      operationId: PriceHistoryService_GetPriceHistory
      parameters:
        - name: symbol
          in: query
          required: true
          description: Market slug.
          schema:
            type: string
        - name: fixedInterval
          in: query
          required: false
          description: Fixed history window. Do not combine with timestamp parameters.
          schema:
            type: string
            enum:
              - INTERVAL_ALL
              - INTERVAL_1M
              - INTERVAL_1W
              - INTERVAL_1D
              - INTERVAL_6H
              - INTERVAL_1H
              - INTERVAL_LIVE
        - name: timestamp.startTimestamp
          in: query
          required: false
          description: >-
            Custom range start in Unix seconds. Requires timestamp.endTimestamp
            and cannot be combined with fixedInterval.
          schema:
            type: integer
            format: int64
        - name: timestamp.endTimestamp
          in: query
          required: false
          description: >-
            Custom range end in Unix seconds. Requires timestamp.startTimestamp
            and cannot be combined with fixedInterval.
          schema:
            type: integer
            format: int64
        - name: fidelity
          in: query
          required: true
          description: >-
            Fidelity in minutes. Use the documented value for the selected fixed
            interval, or 1 for a timestamp range.
          schema:
            type: integer
            format: int32
            minimum: 1
      responses:
        '200':
          description: >-
            Price history. Unknown symbols and markets without stored
            observations return an empty history array.
          headers:
            Cache-Control:
              description: Responses are publicly cacheable for 30 seconds.
              schema:
                type: string
                example: public, max-age=30
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/GetPriceHistoryResponse'
        '400':
          description: Missing or invalid request parameters.
        '429':
          description: Public rate limit exceeded.
        '500':
          description: Internal server error.
components:
  schemas:
    GetPriceHistoryResponse:
      type: object
      properties:
        history:
          type: array
          items:
            $ref: '#/components/schemas/PriceHistoryPoint'
    PriceHistoryPoint:
      type: object
      properties:
        timestamp:
          type: integer
          format: int64
          description: Observation time in Unix seconds.
        longPrice:
          type: number
          format: float
          description: Yes display price, normally derived from the best ask.
        shortPrice:
          type: number
          format: float
          description: No display price, normally derived from one minus the best bid.

````

This documentation is built and hosted on [Mintlify](https://mintlify.com), a developer documentation platform.