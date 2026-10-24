> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.com/llms.txt
> Use this file to discover all available pages before exploring further.

# Get market price

> Retrieves the best market price for a specific token ID and side (bid or ask).
Returns the best bid price for BUY side or best ask price for SELL side.




## OpenAPI

````yaml /api-spec/clob-openapi.yaml get /price
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
  /price:
    get:
      tags:
        - Market Data
      summary: Get market price
      description: >
        Retrieves the best market price for a specific token ID and side (bid or
        ask).

        Returns the best bid price for BUY side or best ask price for SELL side.
      operationId: getPrice
      parameters:
        - name: token_id
          in: query
          description: Token ID (asset ID)
          required: true
          schema:
            type: string
          example: 0xabc123def456...
        - name: side
          in: query
          description: Order side
          required: true
          schema:
            type: string
            enum:
              - BUY
              - SELL
          example: BUY
      responses:
        '200':
          description: Successfully retrieved market price
          content:
            application/json:
              schema:
                type: object
                required:
                  - price
                properties:
                  price:
                    type: number
                    format: double
                    description: Market price as a decimal number
                    example: 0.45
              example:
                price: 0.45
        '400':
          description: Bad request - Invalid token id or side
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ErrorResponse'
              examples:
                invalid_token_id:
                  summary: Invalid token id
                  value:
                    error: Invalid token id
                invalid_side:
                  summary: Invalid side
                  value:
                    error: Invalid side
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