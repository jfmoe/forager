> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.com/llms.txt
> Use this file to discover all available pages before exploring further.

# Look up tokens

> Resolves token ids, or the tokens of conditions, to their condition,
module, outcome, market, event and neg-risk question index in one call.
Exactly one selector per request.

`?token_id=`: a comma-separated list of up to 50 decimal token ids (alias
`token_ids`); elements are trimmed, leading zeros stripped and duplicates
collapsed to their first position. `data` holds one row per known id in
request order; an unknown id is simply absent (never a `404`), and an
all-unknown request returns an empty array.

`?condition=`: a comma-separated list of up to 10 condition selectors
(aliases `condition_id`, `conditionId`), each `0x` plus 62 or 64 hex
digits, lower-cased and deduplicated. A ConditionalTokens condition id
returns the market's tokens (for a market carried over to polymarket-v2,
its whole family: the ConditionalTokens pair, then the polymarket-v2
pair); a neg-risk market id returns every market of the structure; a
polymarket-v2 condition id (the value a combo leg carries as
`leg_condition_id`) returns that condition only; the 66-character
condition id of a native polymarket-v2 market returns that market; the
on-chain event id of a polymarket-v2 neg-risk structure returns every
question of it. `data` holds the rows grouped by selector in request
order, then by question index, market, module and outcome slot; an
unknown value contributes nothing.

A malformed value, more values than the cap, both selectors or neither is
a `400`. `condition_id` is the market's condition id in its 66-character
form, the value the other routes carry; `structural_condition_id` is the
on-chain polymarket-v2 condition id in the same 66-character form, filled
on every polymarket-v2 token, null on `v1_ctf`, and accepted back as a
`condition` selector. `module` names the contract family: `v1_ctf` is the
ConditionalTokens market (module 0) and every other value is a
polymarket-v2 module. Reference fields never change; `closed`,
`resolved`, `final_price` and `outcome` may lag by up to a minute. Not
paginated: `data` is the row array with no `pagination`.



## OpenAPI

````yaml https://data-api.polymarket.com/v2/openapi.json get /v2/tokens
openapi: 3.1.0
info:
  title: Polymarket Data API v2
  description: >-
    The Polymarket Data API: wallet portfolios, trade and activity feeds, market
    state and ranked boards.


    ## Conventions every endpoint shares


    - **Envelope**: every response wraps its payload in `data` (paged routes add
    `pagination`). A documented miss is `data: null` or an empty list, never an
    error.

    - **Pagination is cursor-only**: follow `pagination.next_cursor` until
    `null`; `has_more` is exact, and there is no `offset` query parameter
    (sending one is a `400`). Cursors are signed, typed per endpoint, and
    opaque. The feeds (`trades`, `activity`, combo activity) are keyset walks,
    stable across concurrent writes; the boards, `holders` and most
    combo-position sorts are offset walks behind the opaque token, so a page
    taken across a data refresh can skip or repeat rows. Where a cursor binds
    its cohort (the boards, positions, combo positions), resuming bare is fine,
    restating the same values is fine, and contradicting them is a `400`. The
    `trades`/`activity` feed cursors carry only the seek anchor and page size
    (plus the sort direction on activity): re-send identical filters on every
    page, because changing one mid-walk re-anchors silently.

    - **Rate limiting**: `429` with `Retry-After` is the busy signal for heavy
    queries. A heavy query may first be queued briefly for a capacity slot; the
    `429` arrives only if that short wait ends unserved. Each caller also has a
    per-client request allowance, and bursts past it get the same `429` with
    `Retry-After` sized to the remaining wait. Retry after the given delay. A
    request that could not get a database connection within its budget is NOT a
    `429`: it is a `503` `request_timeout` with `Retry-After`, because the
    shortage is on the server side, not in the caller's rate.

    - **Identifiers**: `condition` (aliases `condition_id`, `conditionId`) is
    the unified query key for on-chain 0x condition ids; `market_id` fields
    carry Gamma's own market ids; `event_id` takes Gamma event ids; `token_id`
    is the CLOB asset id (the key on `/v2/prices-history`).

    - **Params** accept both snake_case and camelCase spellings.

    - **Units**: bare `volume`/`size` values are **shares**; `_usdc` suffixed
    fields are USD; `taker_` prefixed volumes are one-side.

    - **Sentinels**: `outcome_index: 999` means the outcome could not be
    labeled; a missing or `null` numeric field means unavailable, never zero.

    - **Windows on `/v2/trades?user=` and `/v2/activity`**: an omitted or `0`
    `start` floors to three years back (`start=1` asks for full history); an
    omitted or `0` `end` is now plus one day. The other `/v2/trades` shapes
    ignore `start`/`end`: `condition`/`event_id` serve a fixed three-year window
    and the bare feed serves the rolling current-plus-previous month. Other
    windowed routes treat omitted/`0` bounds as unbounded; each documents its
    own rule. `/v2/prices-history` is the strict one, where a `0` bound is a
    `400`.

    - **Errors**: every unsuccessful response is JSON with a human-readable
    `error`, stable `code`, `retryable` flag, and opaque `trace_id`; validation
    failures may also name `parameter`. Codes map to statuses as follows:
    `invalid_request` = `400`, `not_found` = `404`, `method_not_allowed` =
    `405`, `rate_limited` = `429`, `internal` = `500`, and both
    `request_timeout` (the request deadline, the datastore's statement timeout,
    or the connection pool's acquire budget) and `dependency_unavailable` =
    `503`. A `429` or `503` that is worth retrying carries `Retry-After` in
    seconds. Every response, successful or not, echoes the same id in the
    `x-trace-id` header; supply it when reporting a failure so operators can
    correlate it with telemetry.

    - **Auth**: none. All data routes are public; no API key or token is
    required.
  contact:
    name: Polymarket
  license:
    name: MIT
    identifier: MIT
  version: 0.1.0
servers:
  - url: https://data-api.polymarket.com
    description: Production
  - url: https://data-api-rs.stage.pmd.use1.polymarket.sh
    description: Staging
security: []
tags:
  - name: wallet
    description: >-
      Everything anchored on one wallet: positions (base and combos), portfolio
      value, PnL history, the profile card, trading volume, and token approvals.
      Pass the proxy wallet as `user`. One exception cuts across sections:
      `/v2/positions` with `condition` alone (no `user`) answers the market-wide
      holders question.
  - name: feeds
    description: >-
      The high-traffic keyset feeds: trades, activity and combo activity. Filter
      by `user`, `condition` or `event_id`; page with `next_cursor`.
  - name: markets
    description: >-
      Market and event state, keyed by on-chain `condition` ids, Gamma
      `event_id`s or a CLOB `token_id`: open interest, holders, per-event taker
      volume, resolution lifecycle, and price history.
  - name: boards
    description: >-
      Ranked, windowed boards: the PnL/volume leaderboard, biggest single wins,
      and the builder standings and volume buckets. Cursors pin the board they
      were minted on.
  - name: service
    description: >-
      Service metadata: data freshness: the serving watermark, its lag, and
      per-stream ingestion cursors.
paths:
  /v2/tokens:
    get:
      tags:
        - markets
      summary: Look up tokens
      description: >-
        Resolves token ids, or the tokens of conditions, to their condition,

        module, outcome, market, event and neg-risk question index in one call.

        Exactly one selector per request.


        `?token_id=`: a comma-separated list of up to 50 decimal token ids
        (alias

        `token_ids`); elements are trimmed, leading zeros stripped and
        duplicates

        collapsed to their first position. `data` holds one row per known id in

        request order; an unknown id is simply absent (never a `404`), and an

        all-unknown request returns an empty array.


        `?condition=`: a comma-separated list of up to 10 condition selectors

        (aliases `condition_id`, `conditionId`), each `0x` plus 62 or 64 hex

        digits, lower-cased and deduplicated. A ConditionalTokens condition id

        returns the market's tokens (for a market carried over to polymarket-v2,

        its whole family: the ConditionalTokens pair, then the polymarket-v2

        pair); a neg-risk market id returns every market of the structure; a

        polymarket-v2 condition id (the value a combo leg carries as

        `leg_condition_id`) returns that condition only; the 66-character

        condition id of a native polymarket-v2 market returns that market; the

        on-chain event id of a polymarket-v2 neg-risk structure returns every

        question of it. `data` holds the rows grouped by selector in request

        order, then by question index, market, module and outcome slot; an

        unknown value contributes nothing.


        A malformed value, more values than the cap, both selectors or neither
        is

        a `400`. `condition_id` is the market's condition id in its 66-character

        form, the value the other routes carry; `structural_condition_id` is the

        on-chain polymarket-v2 condition id in the same 66-character form,
        filled

        on every polymarket-v2 token, null on `v1_ctf`, and accepted back as a

        `condition` selector. `module` names the contract family: `v1_ctf` is
        the

        ConditionalTokens market (module 0) and every other value is a

        polymarket-v2 module. Reference fields never change; `closed`,

        `resolved`, `final_price` and `outcome` may lag by up to a minute. Not

        paginated: `data` is the row array with no `pagination`.
      operationId: get_tokens
      parameters:
        - name: token_id
          in: query
          description: >-
            Token id(s), comma-separated (at most 50 distinct values): decimal

            ERC-1155 ids as they appear on receipts and on the `token_id` and

            `asset` fields of the other routes. Elements are trimmed, leading
            zeros

            stripped and duplicates collapsed to their first position.
            `token_ids`

            is an accepted alias. Exactly one of `token_id` and `condition` is

            required.
          required: false
          schema:
            type:
              - string
              - 'null'
        - name: condition
          in: query
          description: >-
            Condition selector(s), comma-separated (at most 10 distinct values),

            each `0x` followed by 62 or 64 hex digits: a ConditionalTokens

            condition id (for a market carried over to polymarket-v2, its whole

            family), a neg-risk market id (every market of the structure), a

            polymarket-v2 condition id (that condition only), the 66-character

            condition id of a native polymarket-v2 market, or the on-chain event

            id of a polymarket-v2 neg-risk structure (every question of it).

            Elements are trimmed, lower-cased and duplicates collapsed to their

            first position. `condition_id` and `conditionId` are accepted
            aliases.

            Exactly one of `token_id` and `condition` is required.
          required: false
          schema:
            type:
              - string
              - 'null'
      responses:
        '200':
          description: One row per token, in request order of the selector
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/Envelope_Vec_TokenRef'
        '400':
          description: >-
            Both selectors or neither; a malformed or over-cap 'token_id' or
            'condition' value; or an unsupported query param
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ErrorResponse'
        '429':
          description: Too many requests; retry after `Retry-After`
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
        '503':
          description: >-
            Timed out (the request deadline, the datastore statement timeout, or
            the connection pool's acquire budget) or a dependency is
            unavailable; retry after `Retry-After`
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ErrorResponse'
components:
  schemas:
    Envelope_Vec_TokenRef:
      type: object
      description: >-
        `{ "data": T }`; the envelope for endpoints that don't paginate.


        There is no `pagination` key: an aggregate or bounded list has no next
        page.

        Paginated feeds return a `*Page` shape (`{ data, pagination }`) instead.
      required:
        - data
      properties:
        data:
          type: array
          items:
            type: object
            description: >-
              One `/v2/tokens` row: the reference facts for one ERC-1155 token
              id.
            required:
              - token_id
              - condition_id
              - module
              - outcome_index
              - resolved
            properties:
              clob_index:
                type:
                  - integer
                  - 'null'
                format: int32
                description: >-
                  Position of the token in the market's CLOB token list; null
                  until the

                  settlement writer has placed the token, and always null
                  outside

                  `v1_ctf`.
              closed:
                type:
                  - boolean
                  - 'null'
                description: >-
                  Whether the market is closed; null when the market is not
                  mirrored.
              condition_id:
                type: string
                description: >-
                  The market's condition id in its 66-character form, the value
                  the

                  other routes carry on their rows.
              event_id:
                type:
                  - integer
                  - 'null'
                format: int32
                description: >-
                  Lowest event id the market belongs to; null when none is
                  mirrored.
              event_slug:
                type:
                  - string
                  - 'null'
                description: Slug of that event; null when none is mirrored.
              final_price:
                type:
                  - number
                  - 'null'
                format: double
                description: >-
                  Payout rate of the token once resolved (0 lost, 1 won,
                  fractions for

                  split payouts); null while unresolved.
              market_slug:
                type:
                  - string
                  - 'null'
                description: Market slug; null when the market is not mirrored.
              module:
                $ref: '#/components/schemas/TokenModule'
                description: |-
                  The contract family that issued the token: `v1_ctf` is the
                  ConditionalTokens market (module 0), every other value is a
                  polymarket-v2 module.
              neg_risk:
                type:
                  - boolean
                  - 'null'
                description: >-
                  Whether the market belongs to a neg-risk structure; null when
                  the

                  market is not mirrored.
              neg_risk_market_id:
                type:
                  - string
                  - 'null'
                description: >-
                  The neg-risk structure's market id; null outside neg-risk
                  markets.
              opposite_token_id:
                type:
                  - string
                  - 'null'
                description: The other token of a two-outcome condition; null otherwise.
              outcome:
                type:
                  - string
                  - 'null'
                description: >-
                  Outcome label from the market's outcome list; null when no
                  label slot

                  applies or the market is not mirrored.
              outcome_index:
                type: integer
                format: int32
                description: On-chain outcome slot of the token within its condition.
              question_index:
                type:
                  - integer
                  - 'null'
                format: int32
                description: >-
                  Index of the market inside its neg-risk structure: for
                  `v1_ctf` the

                  last byte of the question id, for `neg_risk` the last byte of
                  the

                  polymarket-v2 condition id; null outside neg-risk markets.
              resolved:
                type: boolean
                description: True once the token has an on-chain payout.
              structural_condition_id:
                type:
                  - string
                  - 'null'
                description: >-
                  The on-chain polymarket-v2 condition id of the token in its

                  66-character form (the id a combo leg carries as
                  `leg_condition_id`,

                  plus the `00` byte); null for `v1_ctf`, whose only id is

                  `condition_id`. On a native polymarket-v2 market it equals

                  `condition_id`; on a market carried over to polymarket-v2 it
                  is a

                  different value. Accepted as a `condition` selector.
              title:
                type:
                  - string
                  - 'null'
                description: Market question; null when the market is not mirrored.
              token_id:
                type: string
                description: Decimal ERC-1155 token id, as a string.
    ErrorResponse:
      type: object
      description: Error body returned by Data API endpoints for unsuccessful requests.
      required:
        - error
        - code
        - retryable
        - trace_id
      properties:
        code:
          $ref: '#/components/schemas/ErrorCode'
          description: Stable classification suitable for programmatic branching.
        error:
          type: string
          description: Human-readable error message.
        parameter:
          type:
            - string
            - 'null'
          description: Query or body parameter associated with a validation failure.
        retryable:
          type: boolean
          description: Whether an automated consumer may retry the request unchanged.
        trace_id:
          type: string
          description: Opaque identifier shared with structured logs and error telemetry.
    TokenModule:
      type: string
      description: >-
        The contract family that issued a token. `v1_ctf` is the
        ConditionalTokens

        market (module 0); every other value is a polymarket-v2 module.
      enum:
        - v1_ctf
        - binary
        - neg_risk
        - combo
    ErrorCode:
      type: string
      description: Stable machine-readable classification for Data API failures.
      enum:
        - invalid_request
        - unauthorized
        - not_found
        - method_not_allowed
        - request_timeout
        - rate_limited
        - dependency_unavailable
        - internal

````

This documentation is built and hosted on [Mintlify](https://mintlify.com), a developer documentation platform.