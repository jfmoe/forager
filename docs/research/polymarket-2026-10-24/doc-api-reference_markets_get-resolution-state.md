> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.com/llms.txt
> Use this file to discover all available pages before exploring further.

# Get resolution state

> Complete resolution state by one selector family.

Supply exactly one of `question_id`, `condition`, or `event_id`. Condition and
event selectors accept comma-separated values and return condition-grain
rows in deterministic order, preferring native V2 lifecycle, then UMA
lifecycle, then terminal CTF state. Misses return `{ "data": [] }`.



## OpenAPI

````yaml https://data-api.polymarket.com/v2/openapi.json get /v2/resolutions
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
  /v2/resolutions:
    get:
      tags:
        - markets
      summary: Get resolution state
      description: >-
        Complete resolution state by one selector family.


        Supply exactly one of `question_id`, `condition`, or `event_id`.
        Condition and

        event selectors accept comma-separated values and return condition-grain

        rows in deterministic order, preferring native V2 lifecycle, then UMA

        lifecycle, then terminal CTF state. Misses return `{ "data": [] }`.
      operationId: get_resolutions
      parameters:
        - name: question_id
          in: query
          description: One UMA question identifier (`0x` plus 64 hexadecimal characters).
          required: false
          schema:
            type:
              - string
              - 'null'
        - name: condition
          in: query
          description: >-
            Comma-separated Gamma condition identifiers (at most 20 distinct
            values).
          required: false
          schema:
            type:
              - string
              - 'null'
        - name: event_id
          in: query
          description: >-
            Comma-separated positive Gamma event IDs (at most 20 distinct
            values).
          required: false
          schema:
            type:
              - string
              - 'null'
      responses:
        '200':
          description: Resolution rows for the selected question, markets, or events
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/Envelope_Vec_Resolution'
        '400':
          description: Missing, mixed, empty, or invalid selector
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
            the connection pool's acquire budget) or a serving dependency is
            unavailable; retry after `Retry-After`
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ErrorResponse'
components:
  schemas:
    Envelope_Vec_Resolution:
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
              One non-paginated `/v2/resolutions` row. UMA lifecycle rows
              populate the

              numeric-string price fields; direct question lookups omit
              `condition_id`,

              while condition/event lookups retain both the selected condition
              and backing

              UMA question. Native V2 and terminal CTF rows populate condition
              lifecycle,

              payout, provenance, and finality fields where those sources
              provide them.
            required:
              - status
              - extended_review
              - was_disputed
              - new_version_q
              - transaction_hash
              - log_index
              - last_update_timestamp
            properties:
              condition_id:
                type:
                  - string
                  - 'null'
                description: >-
                  Condition id the row answers for; absent on question-keyed
                  rows.
              expected_settlement_time:
                type:
                  - string
                  - 'null'
                description: >-
                  Estimated settlement time in RFC3339 UTC, not a guaranteed
                  resolution

                  deadline. Omitted when timing is unavailable; clients must not
                  derive

                  a deadline from `last_update_timestamp` or assume that an
                  unresolved

                  vote settles in its first round.
              extended_review:
                type: boolean
                description: >-
                  True while a managed proposal sits past its normal expiry in
                  extended

                  review; always false outside that window.
              last_update_timestamp:
                type: string
                description: >-
                  Latest lifecycle change: an epoch-seconds string on
                  question-keyed

                  rows, RFC3339 UTC on condition-keyed rows.
              log_index:
                type: string
                description: >-
                  Log index of the latest lifecycle event, as a numeric string;
                  empty

                  where `transaction_hash` is empty.
              market_type:
                type:
                  - string
                  - 'null'
                description: >-
                  BINARY, INCREMENTAL_NEGRISK or ATOMIC_NEGRISK; condition-keyed
                  rows only.
              new_version_q:
                type: boolean
                description: Whether the question rules were updated after posing.
              payouts:
                type:
                  - array
                  - 'null'
                items:
                  type: integer
                  format: int64
                description: >-
                  Per-outcome payout in micro-USDC per share, `[outcome0,
                  outcome1]`;

                  present on resolved condition-keyed rows.
              price:
                type:
                  - string
                  - 'null'
                description: Final settlement price, same conventions as `proposed_price`.
              proposed_price:
                type:
                  - string
                  - 'null'
                description: >-
                  Price of the first proposal as a numeric string; `69` means
                  unset.

                  Present on question-keyed rows only.
              question_id:
                type:
                  - string
                  - 'null'
                description: >-
                  UMA question id serving the row; absent on condition-keyed
                  rows.
              reporter:
                type:
                  - string
                  - 'null'
                description: 'Reporter family that resolved it: UMA_OO, CHAINLINK or EOA.'
              reproposed_price:
                type:
                  - string
                  - 'null'
                description: >-
                  Price of the second proposal, same conventions as
                  `proposed_price`.
              resolution_source:
                type:
                  - string
                  - 'null'
                description: >-
                  `reported` (an oracle reported it) or `derived` (a neg-risk
                  sibling

                  resolution no client can reconstruct).
              resolved_at:
                type:
                  - string
                  - 'null'
                description: When the condition resolved, RFC3339 UTC.
              resolved_block:
                type:
                  - integer
                  - 'null'
                format: int64
                description: Block the condition resolved at.
              settlement_time_basis:
                type:
                  - string
                  - 'null'
                description: >-
                  How `expected_settlement_time` was derived:
                  `managed_proposal_expiration` or `proposal_expiration` (end of
                  a live proposal's challenge window), `liveness` (end of a
                  proposal window on the v2 oracle) or `dvm_round_estimate`
                  (earliest end of the voting round a disputed market can settle
                  in); omitted when no estimate applies.
              status:
                type: string
                description: >-
                  Lifecycle state: initialized, posed, proposed, challenged,
                  reproposed,

                  disputed or resolved; condition-keyed rows can also serve
                  active and

                  arbitration.
              transaction_hash:
                type: string
                description: >-
                  Transaction of the latest lifecycle event; empty on
                  condition-keyed

                  rows without one.
              was_arbitrated:
                type:
                  - boolean
                  - 'null'
                description: Whether arbitration was triggered on the request.
              was_disputed:
                type: boolean
                description: Whether the resolution was disputed at any point.
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