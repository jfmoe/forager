> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.com/llms.txt
> Use this file to discover all available pages before exploring further.

# Get batch prices history

> Retrieve historical price data for multiple markets in a single request.



## OpenAPI

````yaml /api-spec/clob-openapi.yaml post /batch-prices-history
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
  /batch-prices-history:
    post:
      tags:
        - Markets
      summary: Get batch prices history
      description: Retrieve historical price data for multiple markets in a single request.
      operationId: getBatchPricesHistory
      requestBody:
        required: true
        content:
          application/json:
            schema:
              $ref: '#/components/schemas/BatchPricesHistoryRequest'
      responses:
        '200':
          description: Successful response with price history for each market
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/BatchPricesHistoryResponse'
        '400':
          description: Bad Request - Missing or invalid parameters
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
    BatchPricesHistoryRequest:
      type: object
      required:
        - markets
      properties:
        markets:
          type: array
          items:
            type: string
          description: List of market asset ids to query. Maximum 20.
          maxItems: 20
        start_ts:
          type: number
          format: double
          description: Filter by items after this unix timestamp (seconds).
        end_ts:
          type: number
          format: double
          description: Filter by items before this unix timestamp (seconds).
        interval:
          type: string
          enum:
            - max
            - all
            - 1m
            - 1w
            - 1d
            - 6h
            - 1h
          description: Time interval for data aggregation.
        fidelity:
          type: integer
          description: Accuracy of the data expressed in minutes. Default is 1 minute.
    BatchPricesHistoryResponse:
      type: object
      properties:
        history:
          type: object
          additionalProperties:
            type: array
            items:
              $ref: '#/components/schemas/MarketPrice'
          description: Map of market asset id to array of price data points.
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