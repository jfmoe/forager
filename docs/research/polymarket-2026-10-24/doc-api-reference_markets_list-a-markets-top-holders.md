> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.com/llms.txt
> Use this file to discover all available pages before exploring further.

# List a market's top holders

> Top holders of a market, netted per user and grouped by
outcome token.

`?condition=` (CSV of `condition_id`) is required (400 otherwise). `?limit=`
(default 100, max 1000; v2 family-uniform) sizes the page **per outcome
token**; `?min_balance=` filters the served amount.

`?include_pnl=true` opts each holder row into the position economics
(entry price and cost, current price and value, realized, unrealized and
total profit) and switches the amounts to per-side gross balances, so a
wallet holding both outcomes appears under both tokens. The default
response is unchanged. This mode serves one market's table at a time:
exactly one `condition` and `limit` at most 100 (400 otherwise, including
via a cursor that carries a wider window).

A malformed condition id is a `400` naming the value; a well-formed one
that resolves to no holdings serves an empty `data` array, which is the
meaningful zero-state.

Deeper pages come only from `?cursor=` (which carries the page window and
overrides `limit`).

Page walks advance every token group together, so a group leaves `data`
once its token is exhausted while others keep paging; no rows are lost,
but merge pages by `token_id`, not by array position.



## OpenAPI

````yaml https://data-api.polymarket.com/v2/openapi.json get /v2/holders
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
  /v2/holders:
    get:
      tags:
        - markets
      summary: List a market's top holders
      description: >-
        Top holders of a market, netted per user and grouped by

        outcome token.


        `?condition=` (CSV of `condition_id`) is required (400 otherwise).
        `?limit=`

        (default 100, max 1000; v2 family-uniform) sizes the page **per outcome

        token**; `?min_balance=` filters the served amount.


        `?include_pnl=true` opts each holder row into the position economics

        (entry price and cost, current price and value, realized, unrealized and

        total profit) and switches the amounts to per-side gross balances, so a

        wallet holding both outcomes appears under both tokens. The default

        response is unchanged. This mode serves one market's table at a time:

        exactly one `condition` and `limit` at most 100 (400 otherwise,
        including

        via a cursor that carries a wider window).


        A malformed condition id is a `400` naming the value; a well-formed one

        that resolves to no holdings serves an empty `data` array, which is the

        meaningful zero-state.


        Deeper pages come only from `?cursor=` (which carries the page window
        and

        overrides `limit`).


        Page walks advance every token group together, so a group leaves `data`

        once its token is exhausted while others keep paging; no rows are lost,

        but merge pages by `token_id`, not by array position.
      operationId: get_holders
      parameters:
        - name: condition
          in: query
          description: >-
            Comma-separated `condition_id`s (at most 20 distinct values; exactly

            one with `include_pnl=true`). Required. `condition` is the unified
            key

            across v2 (same as the feeds and `/v2/oi`); `condition_id` /

            `conditionId` are accepted aliases.
          required: false
          schema:
            type:
              - string
              - 'null'
        - name: limit
          in: query
          description: |-
            Rows per outcome token; defaults to 100, maximum 1000 (100 with
            `include_pnl=true`).
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
          description: >-
            Opaque pagination cursor from a prior response's `next_cursor`;
            carries

            the per-token `(limit, offset)` window and overrides `limit`. Paging

            past the first page is cursor-only.
          required: false
          schema:
            type:
              - string
              - 'null'
        - name: min_balance
          in: query
          description: >-
            Minimum net balance in shares (default 0), applied post-netting.
            With

            `include_pnl=true` it applies to the served per-side gross amount

            instead.
          required: false
          schema:
            type:
              - number
              - 'null'
            format: double
        - name: include_pnl
          in: query
          description: >-
            Opt into per-holder position economics (default `false`;
            `includePnl`

            is an accepted alias). Adds `avg_price`, `entry_cost_usdc`,

            `current_price`, `current_value`, `realized_pnl`, `unrealized_pnl`
            and

            `total_pnl` to every holder row, and switches the served amounts to

            per-side gross balances: a wallet holding both outcomes appears
            under

            both tokens with per-side sizes and that side's economics, matching

            `/v2/positions` for the same wallet and token. This mode serves one

            market's table at a time: exactly one `condition`, `limit` at most
            100.
          required: false
          schema:
            type:
              - boolean
              - 'null'
      responses:
        '200':
          description: A page of top holders per outcome token
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/HoldersPage'
        '400':
          description: >-
            Missing 'condition', invalid params, or invalid cursor, or a
            malformed 'condition' id; with 'include_pnl', also more than one
            'condition' or a limit above 100 (including a cursor that carries a
            wider window)
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
    HoldersPage:
      type: object
      description: |-
        `{ data, pagination }` envelope for `/v2/holders`; an offset-cursor
        feed (per outcome token): the offset lives behind the opaque token,
        never on the wire.
      required:
        - data
        - pagination
      properties:
        data:
          type: array
          items:
            $ref: '#/components/schemas/MetaHolder'
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
    MetaHolder:
      type: object
      description: >-
        One outcome token's holder group in `/v2/holders`: the `token_id` and
        its

        holders, top-N by net balance. A multi-market request interleaves tokens

        across the page, so merge groups by `token_id`, not by array position.
      required:
        - token_id
        - holders
      properties:
        holders:
          type: array
          items:
            $ref: '#/components/schemas/Holder'
          description: |-
            Top holders of that token, amount descending (net by default,
            per-side gross with `include_pnl=true`).
        token_id:
          type: string
          description: The outcome token this group ranks.
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
    Holder:
      type: object
      description: >-
        One `/v2/holders` row: a market holder enriched with their public
        profile,

        netted across the market's outcome tokens by default or at per-side
        gross

        grain with the position economics when `include_pnl=true`.

        `profile_image_optimized` is always empty.
      required:
        - proxy_wallet
        - bio
        - token_id
        - pseudonym
        - amount
        - display_username_public
        - outcome_index
        - name
        - profile_image
        - profile_image_optimized
        - verified
      properties:
        amount:
          type: number
          format: double
          description: >-
            Holding in shares. By default the NET figure (a fully hedged wallet

            nets to `0.0` and only appears at `min_balance=0`); with

            `include_pnl=true` the per-side GROSS figure, so each side of a
            hedged

            wallet appears under its own token with that side's full size.
        avg_price:
          type:
            - number
            - 'null'
          format: double
          description: >-
            Historical entry price per share. Served only with
            `include_pnl=true`.
        bio:
          type: string
          description: Profile bio text.
        current_price:
          type:
            - number
            - 'null'
          format: double
          description: |-
            Current price of the held outcome token, bounded to `[0, 1]`. Served
            only with `include_pnl=true`.
        current_value:
          type:
            - number
            - 'null'
          format: double
          description: >-
            Mark value of the holding: `amount` times `current_price`. Served
            only

            with `include_pnl=true`.
        display_username_public:
          type: boolean
          description: Whether the profile chose to show its name publicly.
        entry_cost_usdc:
          type:
            - number
            - 'null'
          format: double
          description: >-
            Cost basis of the held size in USDC, excluding entry fees. Served
            only

            with `include_pnl=true`.
        name:
          type: string
          description: Profile display name of the wallet.
        outcome_index:
          type: integer
          format: int32
          description: >-
            Index of the held outcome within the market; `999` means
            unlabelable.
        profile_image:
          type: string
          description: Profile image URL.
        profile_image_optimized:
          type: string
          description: Resized profile image URL, when one exists.
        proxy_wallet:
          type: string
          description: The holding wallet.
        pseudonym:
          type: string
          description: Generated fallback handle for profiles without a display name.
        realized_pnl:
          type:
            - number
            - 'null'
          format: double
          description: |-
            Profit already locked in by sells and redemptions. Served only with
            `include_pnl=true`.
        token_id:
          type: string
          description: Outcome token held.
        total_pnl:
          type:
            - number
            - 'null'
          format: double
          description: >-
            Total profit and loss; always `realized_pnl + unrealized_pnl`.
            Served

            only with `include_pnl=true`.
        unrealized_pnl:
          type:
            - number
            - 'null'
          format: double
          description: |-
            Mark-to-market profit on the held size: `current_value` minus
            `entry_cost_usdc`. Served only with `include_pnl=true`.
        verified:
          type: boolean
          description: Profile verification badge.

````

This documentation is built and hosted on [Mintlify](https://mintlify.com), a developer documentation platform.