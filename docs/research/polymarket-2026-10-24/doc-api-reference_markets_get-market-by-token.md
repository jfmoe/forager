> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.com/llms.txt
> Use this file to discover all available pages before exploring further.

# Get market by token

> Returns the parent market for a given token ID. Useful when you have
a token ID and need to resolve its parent market without knowing the
condition ID in advance.




## OpenAPI

````yaml /api-spec/clob-openapi.yaml get /markets-by-token/{token_id}
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
  /markets-by-token/{token_id}:
    get:
      tags:
        - Markets
      summary: Get market by token
      description: |
        Returns the parent market for a given token ID. Useful when you have
        a token ID and need to resolve its parent market without knowing the
        condition ID in advance.
      operationId: getMarketByToken
      parameters:
        - name: token_id
          in: path
          required: true
          description: The token ID to look up the parent market for
          schema:
            type: string
      responses:
        '200':
          description: Successfully retrieved market
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/MarketByTokenResponse'
        '400':
          description: Invalid market - empty token_id
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ErrorResponse'
        '404':
          description: Market not found for token
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
components:
  schemas:
    MarketByTokenResponse:
      type: object
      description: >-
        Response for GET /markets-by-token/{token_id} — condition ID and both
        token IDs in the market.
      required:
        - condition_id
        - primary_token_id
        - secondary_token_id
      properties:
        condition_id:
          type: string
          description: The condition ID of the market containing the given token
          example: '0xbd31dc8a20211944f6b70f31557f1001557b59905b7738480ca09bd4532f84af'
        primary_token_id:
          type: string
          description: The primary (Yes) token ID
          example: >-
            71321045679252212594626385532706912750332728571942532289631379312455583992563
        secondary_token_id:
          type: string
          description: The secondary (No) token ID
          example: >-
            52114319501245915516055106046884209969926127482827954674443846427813813222426
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