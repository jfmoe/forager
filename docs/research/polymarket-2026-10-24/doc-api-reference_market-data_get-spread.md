> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.com/llms.txt
> Use this file to discover all available pages before exploring further.

# Get spread

> Retrieves the spread for a specific token ID.
The spread is the difference between the best ask and best bid prices.




## OpenAPI

````yaml /api-spec/clob-openapi.yaml get /spread
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
  /spread:
    get:
      tags:
        - Market Data
      summary: Get spread
      description: |
        Retrieves the spread for a specific token ID.
        The spread is the difference between the best ask and best bid prices.
      operationId: getSpread
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
          description: Successfully retrieved spread
          content:
            application/json:
              schema:
                type: object
                required:
                  - spread
                properties:
                  spread:
                    type: string
                    description: Spread as a string
                    example: '0.02'
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