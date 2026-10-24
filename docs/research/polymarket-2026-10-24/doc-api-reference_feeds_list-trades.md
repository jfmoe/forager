> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.com/llms.txt
> Use this file to discover all available pages before exploring further.

# List trades

> Keyset-paginated trade feed in the standard
`{ data, pagination }` envelope.

Pass `?cursor=` from a prior response's `next_cursor` to fetch the next page;
it carries both the page size and the `(block_timestamp, sequence_id)` seek
anchor, so deep pages cost the same as shallow ones.

A malformed `condition` id is a `400` naming the value; a
well-formed one that matches nothing serves an empty `data` array, which
is the meaningful zero-state; absence means the market is not servable,
and only that.



## OpenAPI

````yaml https://data-api.polymarket.com/v2/openapi.json get /v2/trades
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
  /v2/trades:
    get:
      tags:
        - feeds
      summary: List trades
      description: >-
        Keyset-paginated trade feed in the standard

        `{ data, pagination }` envelope.


        Pass `?cursor=` from a prior response's `next_cursor` to fetch the next
        page;

        it carries both the page size and the `(block_timestamp, sequence_id)`
        seek

        anchor, so deep pages cost the same as shallow ones.


        A malformed `condition` id is a `400` naming the value; a

        well-formed one that matches nothing serves an empty `data` array, which

        is the meaningful zero-state; absence means the market is not servable,

        and only that.
      operationId: get_trades
      parameters:
        - name: user
          in: query
          description: Address to filter by; omit for the market/event/global feed.
          required: false
          schema:
            type:
              - string
              - 'null'
        - name: limit
          in: query
          description: >-
            First-page size. Ignored when `cursor` is supplied (the cursor's
            size wins).
          required: false
          schema:
            type:
              - integer
              - 'null'
            format: int32
            maximum: 1000
            minimum: 0
        - name: cursor
          in: query
          description: Opaque pagination cursor from a prior response's `next_cursor`.
          required: false
          schema:
            type:
              - string
              - 'null'
        - name: taker_only
          in: query
          description: |-
            Defaults to `true`: each fill is served once, on its taker side.
            `false` includes the maker rows too.
          required: false
          schema:
            type:
              - boolean
              - 'null'
        - name: filter_type
          in: query
          description: CASH or TOKENS; defaults to TOKENS.
          required: false
          schema:
            type:
              - string
              - 'null'
        - name: filter_amount
          in: query
          description: Minimum trade size; defaults to 0.01, and 0 means the same.
          required: false
          schema:
            type:
              - number
              - 'null'
            format: double
        - name: start
          in: query
          description: >-
            Window start on `block_timestamp`, epoch seconds (inclusive);
            honored

            on the `user` shape only. Omitted or `0` floors to three years back;

            `start=1` asks for full history. The `condition`/`event_id` shapes

            serve a fixed three-year window and the bare feed the rolling

            current-plus-previous month, ignoring both bounds.
          required: false
          schema:
            type:
              - integer
              - 'null'
            format: int64
        - name: end
          in: query
          description: |-
            Window end, epoch seconds (inclusive); honored on the `user` shape
            only; omitted or `0` means now plus one day.
          required: false
          schema:
            type:
              - integer
              - 'null'
            format: int64
        - name: condition
          in: query
          description: |-
            Condition id(s), comma-separated (at most 20 distinct values).
            `condition_id` / `conditionId` are accepted aliases.
          required: false
          schema:
            type:
              - string
              - 'null'
        - name: event_id
          in: query
          description: Event id(s), comma-separated (at most 20 distinct values).
          required: false
          schema:
            type:
              - string
              - 'null'
        - name: side
          in: query
          description: BUY or SELL.
          required: false
          schema:
            type:
              - string
              - 'null'
      responses:
        '200':
          description: A page of trades
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/TradesPage'
        '400':
          description: >-
            Invalid query params or cursor, a malformed 'condition' id, or a
            'user' that is a known protocol contract address
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ErrorResponse'
        '429':
          description: Service is at heavy-query capacity; retry after `Retry-After`
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
            the connection pool's acquire budget) or a serving dependency is
            unavailable; retry after `Retry-After`
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ErrorResponse'
components:
  schemas:
    TradesPage:
      type: object
      description: '`{ data, pagination }` envelope for `/v2/trades`.'
      required:
        - data
        - pagination
      properties:
        data:
          type: array
          items:
            $ref: '#/components/schemas/Trade'
          description: The page's rows.
        pagination:
          $ref: '#/components/schemas/Pagination'
          description: 'Paging envelope: follow `next_cursor` until `null`.'
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
    Trade:
      type: object
      description: A trade (`/v2/trades`).
      required:
        - proxy_wallet
        - side
        - token_id
        - condition_id
        - size
        - price
        - timestamp
        - title
        - slug
        - icon
        - event_slug
        - outcome
        - outcome_index
        - name
        - pseudonym
        - bio
        - profile_image
        - profile_image_optimized
        - transaction_hash
      properties:
        bio:
          type: string
          description: Profile bio text.
        condition_id:
          type: string
          description: On-chain condition id of the market (`0x` hex).
        event_slug:
          type: string
          description: Parent event slug.
        icon:
          type: string
          description: Market icon URL.
        name:
          type: string
          description: Profile display name of the wallet.
        outcome:
          type: string
          description: Label of the traded outcome (e.g. `Yes`).
        outcome_index:
          type: integer
          format: int32
          description: |-
            Index of the traded outcome within the market; `999` means the
            outcome could not be labeled.
        price:
          type: number
          format: double
          description: Execution price per share, in USDC.
        profile_image:
          type: string
          description: Profile image URL.
        profile_image_optimized:
          type: string
          description: Resized profile image URL, when one exists.
        proxy_wallet:
          type: string
          description: |-
            Proxy wallet the row belongs to; the address every wallet-keyed
            endpoint accepts as `user`.
        pseudonym:
          type: string
          description: Generated fallback handle for profiles without a display name.
        side:
          type: string
          description: '`BUY` or `SELL`, from this wallet''s perspective.'
        size:
          type: number
          format: double
          description: Filled quantity in shares; bare sizes are shares, never USD.
        slug:
          type: string
          description: Market slug; the URL segment on polymarket.com.
        timestamp:
          type: integer
          format: int64
          description: Block timestamp of the fill, epoch seconds.
        title:
          type: string
          description: Market question title (Gamma enrichment; empty when unenriched).
        token_id:
          type: string
          description: CLOB asset id of the traded outcome token.
        transaction_hash:
          type: string
          description: Hash of the settling transaction.
    Pagination:
      type: object
      required:
        - limit
        - offset
        - has_more
      properties:
        has_more:
          type: boolean
          description: |-
            Exact: `true` iff another page exists; probe-based, never inferred
            from page fullness.
        limit:
          type: integer
          format: int32
          description: Page size this page was served with.
          minimum: 0
        next_cursor:
          type:
            - string
            - 'null'
          description: Opaque, signed cursor for the next page; `null` on the last page.
        offset:
          type: integer
          format: int32
          description: |-
            Running item offset for display continuity across keyset pages (the
            cursor drives the actual seek; this is cosmetic; there is no total).
          minimum: 0
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