> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.com/llms.txt
> Use this file to discover all available pages before exploring further.

# Get prices history

> Retrieve historical price data for a market.



## OpenAPI

````yaml /api-spec/clob-openapi.yaml get /prices-history
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
  /prices-history:
    get:
      tags:
        - Markets
      summary: Get prices history
      description: Retrieve historical price data for a market.
      operationId: getPricesHistory
      parameters:
        - name: market
          in: query
          required: true
          description: The market (asset id) to query.
          schema:
            type: string
        - name: startTs
          in: query
          required: false
          description: Filter by items after this unix timestamp.
          schema:
            type: number
            format: double
        - name: endTs
          in: query
          required: false
          description: Filter by items before this unix timestamp.
          schema:
            type: number
            format: double
        - name: interval
          in: query
          required: false
          description: Time interval for data aggregation.
          schema:
            type: string
            enum:
              - max
              - all
              - 1m
              - 1w
              - 1d
              - 6h
              - 1h
        - name: fidelity
          in: query
          required: false
          description: Accuracy of the data expressed in minutes. Default is 1 minute.
          schema:
            type: integer
      responses:
        '200':
          description: Successful response with price history
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/PricesHistoryResponse'
        '400':
          description: Bad Request - Missing or invalid query parameters
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ErrorResponse'
        '500':
          description: Internal server error
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ErrorResponse'
      security: []
components:
  schemas:
    PricesHistoryResponse:
      type: object
      properties:
        history:
          type: array
          items:
            $ref: '#/components/schemas/MarketPrice'
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
    MarketPrice:
      type: object
      properties:
        t:
          type: integer
          format: uint32
        p:
          type: number
          format: float

````

This documentation is built and hosted on [Mintlify](https://mintlify.com), a developer documentation platform.