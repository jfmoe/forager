> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.com/llms.txt
> Use this file to discover all available pages before exploring further.

# Get a token's price history

> The price-history series for one outcome token, or
a single point-in-time observation.

Three window forms, exactly one per request: `start` (with optional `end`),
`interval`, or `as_of`. Points are served oldest-first; pass `?cursor=` from
a prior response's `next_cursor` for the next page.

`interval=max` (or `all`) serves the market's whole life, back to 2022 for
the oldest markets, at 12-hour buckets unless `bucket_seconds` says
otherwise; explicit `start`/`end` windows cap at 15 days, so the presets
are the long-range path.

The series ends with a **terminal point**: the latest observation inside the
window, which is a real tick and so can fall between bucket boundaries.
Nothing may assume uniform spacing (sparse data already forbids that), and
each point's `resolution_seconds` says what window it was observed in.

**Availability is graded by resolution.** The 3-hour and 12-hour series are
permanent (back to 2022-11-18); the fine grains are windowed, with these
MINIMUMS: 1-minute data at least 7 days, 5-minute at least 60, 30-minute at
least 90. Floors, not exact horizons: data expires in multi-day chunks, so a
grain usually reaches a few days further back than its minimum. Coverage is
measured per request, never assumed from a table, so `bucket_seconds=60`
over a window starting a month ago is an incompatible pair: both values are
valid, and there is no data at their intersection.

The two ways of asking behave differently on purpose:

* **`bucket_seconds` sent** is served exactly as asked, so over a window
  whose resolution has expired you get an empty page rather than a silent
  substitution.
* **`bucket_seconds` omitted** lets the server choose a width that can
  actually serve the window: the finest tier grain whose series stays under
  ~2,500 points, except that a window wider than 12 hours never defaults to
  the 60-second grain (ask for `bucket_seconds=60` explicitly when you want
  it over a longer window), and
  coarsened further if that grain does not reach the window's start. A
  window up to 12 hours keeps 1-minute detail (an hour 60 points, 12 hours
  720); beyond that it moves to a materialized tier (a day 288, three days
  864, a week 2,016, fifteen days 720). Send `bucket_seconds` when you want
  a specific density instead.

A RESOLVED market's series ends with one settlement point: the on-chain
payout of the requested token (0 lost, 1 won, fractional for voided or
split resolutions) at `resolution_seconds` 0, placed one bucket edge past
the last stored row and clamped to an explicit window's `end`. It appears
only on the final page of a paginated walk, and only once the resolution
is settled on-chain; an unresolved market's series is unchanged.

`resolution_seconds` on every point reports what it was served at, so read it
instead of assuming the width a recent window would have given you. For a
multi-day window prefer a grain-aligned `bucket_seconds` (300, 1800, 10800,
43200) or omit it; a 60-second width over days is the one shape that is both
slow and, past 7 days, empty.



## OpenAPI

````yaml https://data-api.polymarket.com/v2/openapi.json get /v2/prices-history
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
  /v2/prices-history:
    get:
      tags:
        - markets
      summary: Get a token's price history
      description: >-
        The price-history series for one outcome token, or

        a single point-in-time observation.


        Three window forms, exactly one per request: `start` (with optional
        `end`),

        `interval`, or `as_of`. Points are served oldest-first; pass `?cursor=`
        from

        a prior response's `next_cursor` for the next page.


        `interval=max` (or `all`) serves the market's whole life, back to 2022
        for

        the oldest markets, at 12-hour buckets unless `bucket_seconds` says

        otherwise; explicit `start`/`end` windows cap at 15 days, so the presets

        are the long-range path.


        The series ends with a **terminal point**: the latest observation inside
        the

        window, which is a real tick and so can fall between bucket boundaries.

        Nothing may assume uniform spacing (sparse data already forbids that),
        and

        each point's `resolution_seconds` says what window it was observed in.


        **Availability is graded by resolution.** The 3-hour and 12-hour series
        are

        permanent (back to 2022-11-18); the fine grains are windowed, with these

        MINIMUMS: 1-minute data at least 7 days, 5-minute at least 60, 30-minute
        at

        least 90. Floors, not exact horizons: data expires in multi-day chunks,
        so a

        grain usually reaches a few days further back than its minimum. Coverage
        is

        measured per request, never assumed from a table, so `bucket_seconds=60`

        over a window starting a month ago is an incompatible pair: both values
        are

        valid, and there is no data at their intersection.


        The two ways of asking behave differently on purpose:


        * **`bucket_seconds` sent** is served exactly as asked, so over a window
          whose resolution has expired you get an empty page rather than a silent
          substitution.
        * **`bucket_seconds` omitted** lets the server choose a width that can
          actually serve the window: the finest tier grain whose series stays under
          ~2,500 points, except that a window wider than 12 hours never defaults to
          the 60-second grain (ask for `bucket_seconds=60` explicitly when you want
          it over a longer window), and
          coarsened further if that grain does not reach the window's start. A
          window up to 12 hours keeps 1-minute detail (an hour 60 points, 12 hours
          720); beyond that it moves to a materialized tier (a day 288, three days
          864, a week 2,016, fifteen days 720). Send `bucket_seconds` when you want
          a specific density instead.

        A RESOLVED market's series ends with one settlement point: the on-chain

        payout of the requested token (0 lost, 1 won, fractional for voided or

        split resolutions) at `resolution_seconds` 0, placed one bucket edge
        past

        the last stored row and clamped to an explicit window's `end`. It
        appears

        only on the final page of a paginated walk, and only once the resolution

        is settled on-chain; an unresolved market's series is unchanged.


        `resolution_seconds` on every point reports what it was served at, so
        read it

        instead of assuming the width a recent window would have given you. For
        a

        multi-day window prefer a grain-aligned `bucket_seconds` (300, 1800,
        10800,

        43200) or omit it; a 60-second width over days is the one shape that is
        both

        slow and, past 7 days, empty.
      operationId: get_prices_history
      parameters:
        - name: token_id
          in: query
          description: >-
            Outcome token id (the CLOB asset id the chart is keyed by).
            Required.

            `tokenId` is an accepted alias.
          required: false
          schema:
            type:
              - string
              - 'null'
        - name: start
          in: query
          description: >-
            Window start, epoch seconds, INCLUSIVE. Alone it means "up to the

            present", and is capped at 15 days back from now.


            Pass `end` too when paging: a window that tracks the present keeps

            growing at the tip, and the 15-day cap is re-checked on every page,
            so a

            walk that starts near the cap can outlive it.
          required: false
          schema:
            type:
              - integer
              - 'null'
            format: int64
        - name: end
          in: query
          description: >-
            Window end, epoch seconds, EXCLUSIVE. Requires `start`; on its own
            it

            would ask for every point ever recorded up to `end`.
          required: false
          schema:
            type:
              - integer
              - 'null'
            format: int64
        - name: interval
          in: query
          description: >-
            Relative window INSTEAD of `start`/`end`: `max`, `all`, `1m`, `1w`,

            `1d`, `6h`, `1h`, matched case-sensitively. An empty `interval=` is
            a

            malformed value, not an absent one.


            `max`/`all` serve the market's WHOLE life (back to 2022 for the
            oldest

            markets) at 12-hour buckets by default; at `bucket_seconds` of 10800
            or

            43200 the window is unbounded, while finer widths keep a 30-day
            window.

            The other intervals take a width sized to their own span when

            `bucket_seconds` is omitted (`1h`/`6h`/`1d` 60, `1w` 300, `1m`
            1800), and

            each still floors an explicit value: 600 for `max`/`all`/`1m`, 300
            for

            `1w`.
          required: false
          schema:
            type:
              - string
              - 'null'
        - name: bucket_seconds
          in: query
          description: >-
            Bucket width in SECONDS (60…86400). `bucketSeconds` is an accepted
            alias.


            OMIT it and the server sizes the width to the window: the finest
            tier

            grain (60, 300, 1800, 10800, 43200) that keeps the series under
            ~2,500

            points, except that an explicit window wider than 12 hours never

            defaults to the 60s grain (send `bucket_seconds=60` when you want

            second-by-minute detail over a longer window), and coarsened further
            if

            that grain no longer reaches the window's start.

            So a window up to 12 hours keeps 1-minute detail, half a day through
            a

            week defaults to 300, fifteen days to 1800; interval presets keep
            their

            own span-derived defaults (`1d` 60s, `1w` 300, `1m` 1800, and the

            unbounded `max`/`all` 43200).


            SEND it and it is served exactly as asked, which for a window older
            than

            that resolution's retention means an empty page rather than a silent

            substitution. An explicit value is also floored per interval: 600
            for

            `max`/`all`/`1m`, 300 for `1w`.
          required: false
          schema:
            type:
              - integer
              - 'null'
            format: int64
        - name: as_of
          in: query
          description: >-
            Point-in-time read, epoch seconds, INCLUSIVE: the latest observation
            at

            or before this instant. Cannot be combined with a window. `asOf` is
            an

            accepted alias.
          required: false
          schema:
            type:
              - integer
              - 'null'
            format: int64
        - name: limit
          in: query
          description: >-
            First-page size; defaults to the cap (10,000). Ignored when `cursor`
            is

            supplied (the cursor's size wins).
          required: false
          schema:
            type:
              - integer
              - 'null'
            format: int32
            maximum: 10000
            minimum: 0
        - name: cursor
          in: query
          description: Opaque pagination cursor from a prior response's `next_cursor`.
          required: false
          schema:
            type:
              - string
              - 'null'
      responses:
        '200':
          description: A page of the price-history series
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/PricesHistoryPage'
        '400':
          description: Invalid query params or cursor
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ErrorResponse'
        '429':
          description: Per-client rate limit; retry after `Retry-After`
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
            Timed out at the Timescale statement timeout, no pooled connection
            within the acquire budget, or the reader is unavailable; retry after
            `Retry-After`
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ErrorResponse'
components:
  schemas:
    PricesHistoryPage:
      type: object
      description: |-
        `{ data, pagination }` envelope for `/v2/prices-history`. The terminal
        point (the latest observation inside the window) is part of the series
        and lands on the final page, so a client that wants the freshest value
        on a multi-page series follows the cursor to the end.
      required:
        - data
        - pagination
      properties:
        data:
          type: array
          items:
            $ref: '#/components/schemas/PricePoint'
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
    PricePoint:
      type: object
      description: One served point.
      required:
        - timestamp
        - price
        - resolution_seconds
      properties:
        price:
          type: number
          format: double
          description: Price in the range 0…1. Always a JSON number.
        resolution_seconds:
          type: integer
          format: int64
          description: >-
            Width of the window this price was observed in: `0` for an exact
            tick,

            the bucket width for an aggregate. So the price was observed within

            `[timestamp, timestamp + resolution_seconds)`, and a caller can tell

            whether that is precise enough.


            Per-point rather than per-response because points in one response

            genuinely differ: grid points carry the requested bucket width, the

            series' terminal point is an exact tick at `0`, and a tier-degraded

            `as_of` carries its tier's width.


            This is the ONLY thing the payload says about provenance,
            deliberately.
        timestamp:
          type: integer
          format: int64
          description: >-
            The OBSERVATION's own time, epoch seconds, never the time that was

            asked for. A raw tick reports the tick; an aggregate reports its
            bucket

            start.
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