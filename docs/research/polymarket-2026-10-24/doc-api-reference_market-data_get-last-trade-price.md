> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.com/llms.txt
> Use this file to discover all available pages before exploring further.

# Get last trade price

> Retrieves the last trade price and side for a specific token ID.
Returns default values of "0.5" for price and empty string for side if no trades found.




## OpenAPI

````yaml /api-spec/clob-openapi.yaml get /last-trade-price
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
  /last-trade-price:
    get:
      tags:
        - Market Data
      summary: Get last trade price
      description: >
        Retrieves the last trade price and side for a specific token ID.

        Returns default values of "0.5" for price and empty string for side if
        no trades found.
      operationId: getLastTradePrice
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
          description: Successfully retrieved last trade price
          content:
            application/json:
              schema:
                type: object
                required:
                  - price
                  - side
                properties:
                  price:
                    type: string
                    description: Last trade price
                    example: '0.45'
                  side:
                    type: string
                    description: Last trade side (BUY or SELL)
                    enum:
                      - BUY
                      - SELL
                      - ''
                    example: BUY
        '400':
          description: Bad request - Invalid token id
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ErrorResponse'
              example:
                error: Invalid token id
        '500':
          description: Internal server error
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ErrorResponse'
              example:
                error: Internal server error
      security: []
components:
  schemas:
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

````

This documentation is built and hosted on [Mintlify](https://mintlify.com), a developer documentation platform.