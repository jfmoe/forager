> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.com/llms.txt
> Use this file to discover all available pages before exploring further.

# Get data freshness

> How fresh the data behind this API is.

`/readyz` answers whether this pod can reach its datastore; this answers how
far behind chain head what you are reading is. Two halves, because each is
blind exactly where the other sees:

* `serving`; the projections that turn ingested rows into what the feeds
  read (`activity_feed` enrichment, `custody_balances` ingestion, the PnL
  engine).
  Each carries its own clock, so this is the only half that can see
  "everything stopped". `lag_seconds` is the worst age across them and
  `worst` names which one, so a single stalled projection cannot hide behind
  two healthy ones.
* `ingestion`; one crawl cursor per `(contract, event)`, summarised as the
  tail, the furthest-behind block, and the worst few cursors BY NAME. Every
  reading here is relative or positional, so under a total stall it reads
  green; that is why `serving` exists alongside it.

Streams that are dormant by design are dropped from `ingestion` rather than
reported, because they sit arbitrarily far back forever and would otherwise
fill the list and push the stream you care about off it.

Takes no parameters and is not paginated. Served from a snapshot refreshed
in the background, never computed on the request: `computed_at` and
`age_seconds` say how old it is, so a stalled refresher or a datastore
outage shows up as a growing age rather than a 500. Before the first refresh
lands there is nothing to serve and this answers `503`; "not measured yet"
and "measured, no lag" are different claims.



## OpenAPI

````yaml https://data-api.polymarket.com/v2/openapi.json get /v2/status
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
  /v2/status:
    get:
      tags:
        - service
      summary: Get data freshness
      description: >-
        How fresh the data behind this API is.


        `/readyz` answers whether this pod can reach its datastore; this answers
        how

        far behind chain head what you are reading is. Two halves, because each
        is

        blind exactly where the other sees:


        * `serving`; the projections that turn ingested rows into what the feeds
          read (`activity_feed` enrichment, `custody_balances` ingestion, the PnL
          engine).
          Each carries its own clock, so this is the only half that can see
          "everything stopped". `lag_seconds` is the worst age across them and
          `worst` names which one, so a single stalled projection cannot hide behind
          two healthy ones.
        * `ingestion`; one crawl cursor per `(contract, event)`, summarised as
        the
          tail, the furthest-behind block, and the worst few cursors BY NAME. Every
          reading here is relative or positional, so under a total stall it reads
          green; that is why `serving` exists alongside it.

        Streams that are dormant by design are dropped from `ingestion` rather
        than

        reported, because they sit arbitrarily far back forever and would
        otherwise

        fill the list and push the stream you care about off it.


        Takes no parameters and is not paginated. Served from a snapshot
        refreshed

        in the background, never computed on the request: `computed_at` and

        `age_seconds` say how old it is, so a stalled refresher or a datastore

        outage shows up as a growing age rather than a 500. Before the first
        refresh

        lands there is nothing to serve and this answers `503`; "not measured
        yet"

        and "measured, no lag" are different claims.
      operationId: get_status
      responses:
        '200':
          description: Serving and ingestion freshness
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/Envelope_ServiceStatus'
        '503':
          description: >-
            No freshness measured yet, a timeout, or a serving dependency
            outage; retry after `Retry-After` when present
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ErrorResponse'
components:
  schemas:
    Envelope_ServiceStatus:
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
          type: object
          description: >-
            `/v2/status` payload.


            Served from a snapshot refreshed in the background, never computed
            on the

            request. `computed_at`/`age_seconds` make that explicit rather than
            implicit:

            if the refresher wedges or its datastore goes away, the answer keeps
            being

            served with a growing age instead of turning into a 500; the right
            degraded

            mode for the endpoint you call when something is already wrong.
          required:
            - computed_at
            - age_seconds
            - serving
            - ingestion
          properties:
            age_seconds:
              type: integer
              format: int64
              description: >-
                How old the snapshot is, in seconds. Normally under the refresh

                interval; a value that keeps climbing means the refresher is not

                completing, and the freshness figures below are that stale ON
                TOP of

                whatever lag they report.
            computed_at:
              type: string
              description: When this snapshot was taken, RFC 3339 in UTC.
            ingestion:
              $ref: '#/components/schemas/IngestionFreshness'
            serving:
              $ref: '#/components/schemas/ServingFreshness'
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
    IngestionFreshness:
      type: object
      description: >-
        Ingestion health: one cursor per `(contract, event)` stream.


        Every figure here is over the LIVE streams only. Streams that are
        dormant by

        design sit arbitrarily far behind forever; they are dropped outright
        rather

        than reported and flagged, because there is nothing a consumer could do
        with

        them and including them is precisely what makes the list useless.
      required:
        - cursors
        - chain_id
        - lagging
      properties:
        chain_id:
          type: integer
          format: int64
          description: >-
            The chain id this service is configured for. Echoed so the pairing
            above

            is readable in one response.
        cursors:
          type: integer
          description: |-
            How many live streams were found. `0` means no ingestion cursors are
            visible to this API at all.
          minimum: 0
        lagging:
          type: array
          items:
            $ref: '#/components/schemas/CursorLag'
          description: The furthest-behind live streams, `most_lagged` first.
        max_synced_block:
          type:
            - integer
            - 'null'
          format: int64
          description: |-
            The tail: the furthest-along cursor. Every `behind_max` is measured
            against it, and so is each serving mechanism's `blocks_behind`.
        min_synced_block:
          type:
            - integer
            - 'null'
          format: int64
          description: >-
            The furthest-behind LIVE stream's block. Deliberately asymmetric
            with

            `max_synced_block` above, which is over ALL cursors: the tail must
            not

            move with the dormant cut, or excluding a stream would redefine the

            distance every other stream is measured against.
        most_lagged:
          oneOf:
            - type: 'null'
            - $ref: '#/components/schemas/CursorLag'
              description: >-
                The single furthest-behind live stream. A stall confined to one
                stream

                while every sibling sits at head is invisible in any aggregate
                figure.
        network:
          type:
            - string
            - 'null'
          description: >-
            The chain the ingestion cursors were written for, as the datastore

            itself names it; NOT as this service is configured. Compared against

            `chain_id`, it catches a datastore pointed at the wrong chain, which

            otherwise presents as data that is merely wrong. `null` when no
            stream

            declares one.
    ServingFreshness:
      type: object
      description: >-
        What API consumers actually experience: the projections behind the
        feeds.


        One headline number and the name of whichever mechanism produced it, so
        a

        single stalled projection cannot hide behind two healthy ones; the same

        shape as `ingestion.most_lagged`, for the same reason.
      required:
        - mechanisms
      properties:
        lag_seconds:
          type:
            - integer
            - 'null'
          format: int64
          description: |-
            The worst age across `mechanisms`. `null` only when no mechanism
            reported at all.
        mechanisms:
          type: array
          items:
            $ref: '#/components/schemas/ServingMechanism'
          description: |-
            Every mechanism that produced a candidate freshness row, in a fixed
            order; `worst` names the culprit.
        worst:
          type:
            - string
            - 'null'
          description: Which mechanism `lag_seconds` came from.
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
    CursorLag:
      type: object
      description: One ingestion stream's distance from the furthest-along stream.
      required:
        - source
        - block
        - behind_max
      properties:
        behind_max:
          type: integer
          format: int64
          description: >-
            How many blocks this stream trails the furthest-along stream; `0`
            for

            the leader.
        block:
          type: integer
          format: int64
          description: Last block the stream has ingested through.
        source:
          type: string
          description: The stream this cursor belongs to, as `<contract>_<event>`.
    ServingMechanism:
      type: object
      description: One serving mechanism's freshness.
      required:
        - name
        - age_seconds
      properties:
        age_seconds:
          type: integer
          format: int64
          description: Seconds since it last advanced, by its own clock.
        blocks_behind:
          type:
            - integer
            - 'null'
          format: int64
          description: |-
            How far behind the ingestion tail it has projected. Absent for a
            mechanism that records a time but no block.
        name:
          type: string
          description: >-
            What this mechanism produces: `activity_feed`, `custody_balances`,
            `pnl`.

````

This documentation is built and hosted on [Mintlify](https://mintlify.com), a developer documentation platform.