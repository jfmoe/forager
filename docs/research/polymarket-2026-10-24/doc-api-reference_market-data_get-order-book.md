> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.com/llms.txt
> Use this file to discover all available pages before exploring further.

# Get order book

> Retrieves the order book summary for a specific token ID.
Includes bids, asks, market details, and last trade price.




## OpenAPI

````yaml /api-spec/clob-openapi.yaml get /book
openapi: 3.1.0
info:
  title: Polymarket CLOB API
  description: Polymarket CLOB API Reference
  license:
    name: MIT
    identifier: MIT
  version: 1.0.0
servers:
  - url: https://clob.polymarket.com
    description: Production CLOB API
  - url: https://clob-staging.polymarket.com
    description: Staging CLOB API
security: []
tags:
  - name: Trade
    description: Trade endpoints
  - name: Markets
    description: Market data endpoints
  - name: Account
    description: Account and authentication endpoints
  - name: Notifications
    description: User notification endpoints
  - name: Rewards
    description: Rewards and earnings endpoints
  - name: Rebates
    description: Maker rebate endpoints
paths:
  /book:
    get:
      tags:
        - Market Data
      summary: Get order book
      description: |
        Retrieves the order book summary for a specific token ID.
        Includes bids, asks, market details, and last trade price.
      operationId: getBook
      parameters:
        - name: token_id
          in: query
          description: Token ID (asset ID)
          required: true
          schema:
            type: string
          example: 0xabc123def456...
      responses:
        '200':
          description: Successfully retrieved order book
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/OrderBookSummary'
              example:
                market: '0x1234567890123456789012345678901234567890'
                asset_id: 0xabc123def456...
                timestamp: '1234567890'
                hash: a1b2c3d4e5f6...
                bids:
                  - price: '0.45'
                    size: '100'
                  - price: '0.44'
                    size: '200'
                asks:
                  - price: '0.46'
                    size: '150'
                  - price: '0.47'
                    size: '250'
                min_order_size: '1'
                tick_size: '0.01'
                neg_risk: false
                last_trade_price: '0.45'
        '400':
          description: Bad request - Invalid token id
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ErrorResponse'
              example:
                error: Invalid token id
        '404':
          description: Not found - No orderbook exists for the requested token id
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ErrorResponse'
              example:
                error: No orderbook exists for the requested token id
        '500':
          description: Internal server error
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ErrorResponse'
              example:
                error: error getting the orderbook
      security: []
components:
  schemas:
    OrderBookSummary:
      type: object
      required:
        - market
        - asset_id
        - timestamp
        - hash
        - bids
        - asks
        - min_order_size
        - tick_size
        - neg_risk
        - last_trade_price
      properties:
        market:
          type: string
          description: Market condition ID
          example: '0x1234567890123456789012345678901234567890'
        asset_id:
          type: string
          description: Token ID (asset ID)
          example: 0xabc123def456...
        timestamp:
          type: string
          description: Timestamp of the order book snapshot
          example: '1234567890'
        hash:
          type: string
          description: Hash of the order book summary
          example: a1b2c3d4e5f6...
        bids:
          type: array
          description: List of bid orders (sorted by price descending)
          items:
            $ref: '#/components/schemas/OrderSummary'
        asks:
          type: array
          description: List of ask orders (sorted by price ascending)
          items:
            $ref: '#/components/schemas/OrderSummary'
        min_order_size:
          type: string
          description: Minimum order size
          example: '1'
        tick_size:
          type: string
          description: Minimum price increment (tick size)
          example: '0.01'
        neg_risk:
          type: boolean
          description: Whether negative risk is enabled for this market
          example: false
        last_trade_price:
          type: string
          description: Last trade price
          example: '0.45'
    ErrorResponse:
      type: object
      required:
        - error
      properties:
        error:
          type: string
          description: Error message
        code:
          type: string
          description: Machine-readable error code, when provided
        retry_after_seconds:
          type: integer
          description: Number of seconds to wait before retrying, when provided
    OrderSummary:
      type: object
      required:
        - price
        - size
      properties:
        price:
          type: string
          description: Order price
          example: '0.45'
        size:
          type: string
          description: Order size
          example: '100'

````

This documentation is built and hosted on [Mintlify](https://mintlify.com), a developer documentation platform.