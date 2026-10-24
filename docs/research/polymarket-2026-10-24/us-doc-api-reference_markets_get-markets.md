> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Get Markets

> Retrieve all markets



## OpenAPI

````yaml /api-reference/oapi-schemas/markets-schema.json get /v1/markets
openapi: 3.0.3
info:
  title: protos/gateway/market/v1/market.proto
  version: 1.0.0
servers:
  - url: https://gateway.polymarket.us
    description: Production server
security: []
tags:
  - name: MarketService
paths:
  /v1/markets:
    get:
      tags:
        - Markets
      summary: Get Markets
      description: Retrieve all markets
      operationId: MarketService_GetMarkets
      parameters:
        - name: limit
          description: Page size
          in: query
          required: false
          schema:
            type: integer
            format: int32
        - name: offset
          description: Page offset
          in: query
          required: false
          schema:
            type: integer
            format: int32
        - name: orderBy
          description: Order by fields
          in: query
          required: false
          explode: true
          schema:
            type: array
            items:
              type: string
        - name: orderDirection
          description: Order direction
          in: query
          required: false
          schema:
            type: string
        - name: id
          description: Market IDs
          in: query
          required: false
          explode: true
          schema:
            type: array
            items:
              type: integer
              format: int32
        - name: slug
          description: Market slugs
          in: query
          required: false
          explode: true
          schema:
            type: array
            items:
              type: string
        - name: archived
          description: Whether market is archived
          in: query
          required: false
          schema:
            type: boolean
        - name: active
          description: Whether market is active
          in: query
          required: false
          schema:
            type: boolean
        - name: closed
          description: Whether market is closed
          in: query
          required: false
          schema:
            type: boolean
        - name: volumeNumMin
          description: Minimum volume number
          in: query
          required: false
          schema:
            type: number
            format: double
        - name: volumeNumMax
          description: Maximum volume number
          in: query
          required: false
          schema:
            type: number
            format: double
        - name: startDateMin
          description: Minimum start date
          in: query
          required: false
          schema:
            type: string
        - name: startDateMax
          description: Maximum start date
          in: query
          required: false
          schema:
            type: string
        - name: endDateMin
          description: Minimum end date
          in: query
          required: false
          schema:
            type: string
        - name: endDateMax
          description: Maximum end date
          in: query
          required: false
          schema:
            type: string
        - name: relatedTags
          description: Whether related tags are enabled
          in: query
          required: false
          schema:
            type: boolean
        - name: gameId
          description: Game ID
          in: query
          required: false
          schema:
            type: string
        - name: sportsMarketTypes
          description: Sports market types
          in: query
          required: false
          explode: true
          schema:
            type: array
            items:
              type: string
              enum:
                - SPORTS_MARKET_TYPE_MONEYLINE
                - SPORTS_MARKET_TYPE_SPREAD
                - SPORTS_MARKET_TYPE_TOTAL
                - SPORTS_MARKET_TYPE_PROP
                - SPORTS_MARKET_TYPE_FUTURE
                - SPORTS_MARKET_TYPE_DRAWABLE_OUTCOME
        - name: includeTag
          description: Whether to include tag
          in: query
          required: false
          schema:
            type: boolean
        - name: categories
          description: Categories
          in: query
          required: false
          explode: true
          schema:
            type: array
            items:
              type: string
        - name: marketTypes
          description: Market types
          in: query
          required: false
          explode: true
          schema:
            type: array
            items:
              type: string
        - name: includeHidden
          description: 'Include hidden markets (default: false)'
          in: query
          required: false
          schema:
            type: boolean
        - name: tagIds
          description: Tag IDs
          in: query
          required: false
          explode: true
          schema:
            type: array
            items:
              type: integer
              format: int32
      responses:
        '200':
          description: List of markets
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/gateway.market.v1.GetMarketsResponse'
        '500':
          description: Internal server error
          content:
            application/json:
              schema: {}
components:
  schemas:
    gateway.market.v1.GetMarketsResponse:
      type: object
      properties:
        markets:
          type: array
          items:
            $ref: '#/components/schemas/gateway.market.v1.Market'
          description: List of markets
      description: Response containing list of markets
    gateway.market.v1.Market:
      type: object
      properties:
        id:
          type: string
          description: Unique market identifier
        question:
          type: string
          description: Market question
          nullable: true
        slug:
          type: string
          description: Market slug for URL
          nullable: true
        endDate:
          type: string
          description: Market end date
          nullable: true
        category:
          type: string
          description: Market category
          nullable: true
        startDate:
          type: string
          description: Market start date
          nullable: true
        image:
          type: string
          description: Market image URL
          nullable: true
        imageDisplayType:
          $ref: '#/components/schemas/gateway.market.v1.ImageDisplayType'
        description:
          type: string
          description: Market description
          nullable: true
        active:
          type: boolean
          description: Whether market is active
          nullable: true
        marketType:
          type: string
          description: 'Type of market (deprecated). Options: moneyline, spreads, totals'
          nullable: true
          deprecated: true
        closed:
          type: boolean
          description: Whether market is closed
          nullable: true
        createdAt:
          type: string
          description: Creation timestamp
          nullable: true
        updatedAt:
          type: string
          description: Last update timestamp
          nullable: true
        archived:
          type: boolean
          description: Whether market is archived
          nullable: true
          deprecated: true
        orderPriceMinTickSize:
          type: number
          format: decimal
          description: Minimum tick size for order price
          nullable: true
        gameStartTime:
          type: string
          description: Game start time
          nullable: true
          deprecated: true
        manualActivation:
          type: boolean
          description: Whether manual activation is required
          nullable: true
          deprecated: true
        sportsMarketType:
          type: string
          description: Sports market type
          nullable: true
        line:
          type: number
          format: decimal
          description: Line value
          nullable: true
        marketSides:
          type: array
          items:
            $ref: '#/components/schemas/gateway.market.v1.MarketSide'
          description: Market sides
        outcomes:
          type: string
          description: Outcomes JSON
          nullable: true
          deprecated: true
        outcomePrices:
          type: string
          description: Outcome prices JSON
          nullable: true
          deprecated: true
        ep3Status:
          type: string
          description: Raw upstream EP3 instrument state (deprecated). Use status.
          nullable: true
        status:
          $ref: '#/components/schemas/gateway.market.v1.MarketStatus'
        assetPriceTerms:
          allOf:
            - $ref: '#/components/schemas/gateway.market.v1.AssetPriceTerms'
          description: >-
            Typed terms of an automated crypto price market. Null on every other
            market, including the hand-listed year-end crypto markets.
        sportsMarketTypeV2:
          allOf:
            - $ref: '#/components/schemas/gateway.market.v1.SportsMarketType'
          deprecated: true
        hidden:
          type: boolean
          description: Whether market is hidden
          nullable: true
        tags:
          type: array
          items:
            $ref: '#/components/schemas/gateway.tags.v1.Tag'
          description: Associated tags
        title:
          type: string
          description: Market title
          nullable: true
        subtitle:
          type: string
          description: Market subtitle
          nullable: true
        color:
          type: string
          description: Market color
          nullable: true
        darkColor:
          type: string
          description: Market dark mode color
          nullable: true
        subjectId:
          type: integer
          format: int32
          description: Subject ID
          nullable: true
        subject:
          $ref: '#/components/schemas/gateway.market.v1.Subject'
        feeCoefficient:
          type: number
          format: decimal
          description: Fee coefficient
          nullable: true
        spreadTotalSuffix:
          type: string
          description: Spread/total suffix for UI display (e.g. points, goals, runs)
          nullable: true
        sortOrder:
          type: integer
          format: int32
          description: Sort order for market within event
          nullable: true
        bestBidQuote:
          $ref: '#/components/schemas/gateway.types.v1.Amount'
        bestAskQuote:
          $ref: '#/components/schemas/gateway.types.v1.Amount'
        metadata:
          type: object
          description: >-
            Opaque metadata object — internal-use only, shape is not part of the
            public contract
          nullable: true
        titleShort:
          type: string
          description: >-
            Compact display title computed at serialization time (e.g. 'DET -5.5
            F5', 'Over 5.5 F5'). Falls back to title.
          nullable: true
        ep3SyncedAt:
          type: string
          description: Timestamp when this market was last synced from EP3
          nullable: true
        minimumTradeQty:
          type: number
          format: decimal
          description: >-
            Minimum order quantity in contracts (e.g. 0.01 = 1% of a contract,
            1.0 = one whole contract).
          nullable: true
        volume:
          type: number
          format: decimal
          description: Market volume (lifetime, in shares)
          nullable: true
        volume24hr:
          type: number
          format: decimal
          description: 24-hour market volume (in shares)
          nullable: true
        volume1wk:
          type: number
          format: decimal
          description: One week market volume (in shares)
          nullable: true
        volume1mo:
          type: number
          format: decimal
          description: One month market volume (in shares)
          nullable: true
        volume1yr:
          type: number
          format: decimal
          description: One year market volume (in shares)
          nullable: true
        rulesDisclaimer:
          type: string
          description: >-
            Short 'understanding the rules' disclaimer for this market type,
            computed at serialization time. Omitted when the market's
            sports_market_type has no disclaimer.
          nullable: true
        rulesDisclaimerPopup:
          type: boolean
          description: >-
            When true, clients should surface rules_disclaimer as a popup sheet
            the user acknowledges before buying, rather than inline. Only set
            when rules_disclaimer is present.
          nullable: true
        comboEnabled:
          type: boolean
          description: Whether this market is combo-enabled
          nullable: true
      description: Market information and configuration
    gateway.market.v1.ImageDisplayType:
      type: string
      enum:
        - IMAGE_DISPLAY_TYPE_HEADSHOT
        - IMAGE_DISPLAY_TYPE_LOGO
        - IMAGE_DISPLAY_TYPE_FLAG
        - IMAGE_DISPLAY_TYPE_ARTWORK
    gateway.market.v1.MarketSide:
      type: object
      properties:
        id:
          type: string
          description: Market side ID
        marketSideType:
          $ref: '#/components/schemas/gateway.market.v1.MarketSideType'
        identifier:
          type: string
          description: Market side identifier
          nullable: true
        createdAt:
          type: string
          description: Creation timestamp
          nullable: true
        updatedAt:
          type: string
          description: Last update timestamp
          nullable: true
        description:
          type: string
          description: Market side description
          nullable: true
        price:
          type: string
          description: Market side price
          nullable: true
        marketId:
          type: integer
          format: int32
          description: Market ID
        long:
          type: boolean
          description: Whether market side is the long or short side of the market
          nullable: true
        teamId:
          type: integer
          format: int32
          description: Team ID
          nullable: true
        team:
          allOf:
            - $ref: '#/components/schemas/gateway.market.v1.Team'
          deprecated: true
        quote:
          $ref: '#/components/schemas/gateway.types.v1.Amount'
        tradable:
          type: boolean
          description: Whether this side can be traded
          nullable: true
      description: Market position information
    gateway.market.v1.MarketStatus:
      type: string
      enum:
        - MARKET_STATUS_UNSPECIFIED
        - MARKET_STATUS_OPEN
        - MARKET_STATUS_CLOSED
        - MARKET_STATUS_RESOLVING
        - MARKET_STATUS_RESOLVED
        - MARKET_STATUS_HALTED
      description: >-
        MarketStatus is the normalized trading lifecycle for a market, derived
        by the

        gateway. Values may grow; treat unrecognized values as

        MARKET_STATUS_UNSPECIFIED. Pre-trading upstream states have no
        counterpart

        here and read as MARKET_STATUS_UNSPECIFIED.

         - MARKET_STATUS_RESOLVING: Past expiry, final settlement not yet published — payouts are pending.
         - MARKET_STATUS_RESOLVED: Final settlement published; the outcome will not change.
    gateway.market.v1.AssetPriceTerms:
      type: object
      properties:
        marketType:
          $ref: '#/components/schemas/gateway.market.v1.AssetPriceMarketType'
        asset:
          $ref: '#/components/schemas/gateway.types.v1.AssetId'
        indexSymbol:
          type: string
          description: >-
            Reference index the market settles on, e.g. BRTI. Empty on commodity
            (gold and silver) markets, which settle on a Chainlink spot stream.
        horizon:
          type: string
          description: Supply-side horizon slug. Display and grouping only.
        windowStart:
          type: string
          format: date-time
          description: >-
            Start of the settlement window. Always set for UP_DOWN. Null when
            the market has no settlement window (ladders).
          nullable: true
        windowEnd:
          type: string
          format: date-time
          description: >-
            End of the measurement window and scheduled settlement instant. Set
            on every automated crypto family, including ABOVE_BELOW and RANGE
            ladders. Use this rather than endDate for the settlement time.
            ONE_TOUCH may settle Yes before this instant.
          nullable: true
        priceToBeat:
          allOf:
            - $ref: '#/components/schemas/gateway.types.v1.NullableAmount'
          description: >-
            Null until stamped a few seconds after windowStart, and on every
            market that is not UP_DOWN.
        strike:
          allOf:
            - $ref: '#/components/schemas/gateway.types.v1.NullableAmount'
          description: Null on every market that is not ABOVE_BELOW or ONE_TOUCH.
        rangeLower:
          allOf:
            - $ref: '#/components/schemas/gateway.types.v1.NullableAmount'
          description: >-
            Null on every market that is not a RANGE, and on the open side of a
            tail range.
        rangeUpper:
          allOf:
            - $ref: '#/components/schemas/gateway.types.v1.NullableAmount'
          description: >-
            Null on every market that is not a RANGE, and on the open side of a
            tail range.
        rangeKind:
          $ref: '#/components/schemas/gateway.market.v1.AssetPriceRangeKind'
        direction:
          $ref: '#/components/schemas/gateway.market.v1.AssetPriceDirection'
        settlementPrice:
          allOf:
            - $ref: '#/components/schemas/gateway.types.v1.NullableAmount'
          description: >-
            Null until resolved, and stays null on a ONE_TOUCH market that
            resolved No: no price was touched.
        priceDecimals:
          type: integer
          format: int32
          description: >-
            Decimal places every price of the market carries, e.g. 2 for BTC and
            gold, 3 for silver. Always read this field: it can differ between
            assets, and between market types of one asset, for example 4 for SOL
            Up/Down. Show prices at exactly this many places.
        assetName:
          type: string
          description: >-
            Display name of the asset, e.g. Gold. Empty when the asset has none:
            show the symbol.
        priceUnit:
          type: string
          description: >-
            Unit one price is quoted per, e.g. oz for gold and silver. Empty
            when the asset needs no unit, as crypto.
        nextWindowStart:
          type: string
          format: date-time
          nullable: true
          description: >-
            Start of the next window of this market's length that the asset's
            trading hours allow. Equals windowEnd on crypto windows. On gold and
            silver it is later than windowEnd when trading closes after this
            window (the daily break, the weekend or a holiday). Null when
            unknown.
      description: >-
        Terms of a market settled against an external reference (index) price.
        Fields a market type does not use are null, never zero-filled. Prices
        are decimal strings exactly as the supply side stamped them.
      nullable: true
    gateway.market.v1.SportsMarketType:
      type: string
      enum:
        - SPORTS_MARKET_TYPE_MONEYLINE
        - SPORTS_MARKET_TYPE_SPREAD
        - SPORTS_MARKET_TYPE_TOTAL
        - SPORTS_MARKET_TYPE_PROP
        - SPORTS_MARKET_TYPE_FUTURE
        - SPORTS_MARKET_TYPE_DRAWABLE_OUTCOME
    gateway.tags.v1.Tag:
      type: object
      properties:
        id:
          type: string
          description: Unique tag identifier
        label:
          type: string
          description: Tag label
          nullable: true
        slug:
          type: string
          description: Tag slug for URL
          nullable: true
        createdAt:
          type: string
          description: Creation timestamp
          nullable: true
        updatedAt:
          type: string
          description: Last update timestamp
          nullable: true
        image:
          type: string
          description: Tag image URL
          nullable: true
        tradable:
          type: boolean
          description: Whether the tag is tradable
          nullable: true
        league:
          $ref: '#/components/schemas/gateway.tags.v1.TagLeague'
        sport:
          $ref: '#/components/schemas/gateway.tags.v1.TagSport'
        parentId:
          type: integer
          format: int32
          description: Parent tag ID
          nullable: true
        subtags:
          type: array
          items:
            type: object
            description: Nested object (recursive)
          description: Child subtags
        hub:
          $ref: '#/components/schemas/gateway.hubs.v1.Hub'
    gateway.market.v1.Subject:
      type: object
      properties:
        id:
          type: integer
          format: int32
          description: Subject ID
        name:
          type: string
          description: Subject name
        displayName:
          type: string
          description: Subject display name
          nullable: true
        description:
          type: string
          description: Subject description
          nullable: true
        subjectType:
          type: string
          description: Subject type (nominee, player, team, candidate, golf_player)
        image:
          type: string
          description: Subject image URL
          nullable: true
        color:
          type: string
          description: Subject color
          nullable: true
        darkColor:
          type: string
          description: Subject dark mode color
          nullable: true
        createdAt:
          type: string
          description: Creation timestamp
          nullable: true
        updatedAt:
          type: string
          description: Last update timestamp
          nullable: true
        slug:
          type: string
          description: Subject slug for URL
          nullable: true
      description: Subject information
    gateway.types.v1.Amount:
      type: object
      properties:
        value:
          type: string
          format: decimal
          example: 123.45
          description: The amount as a decimal string.
        currency:
          type: string
          description: The currency code
      description: Represents a monetary amount with its currency.
      required:
        - value
        - currency
    gateway.market.v1.MarketSideType:
      type: string
      enum:
        - MARKET_SIDE_TYPE_ERC1155
        - MARKET_SIDE_TYPE_INSTRUMENT
    gateway.market.v1.Team:
      type: object
      properties:
        id:
          type: integer
          format: int32
        name:
          type: string
        abbreviation:
          type: string
        league:
          type: string
        record:
          type: string
        logo:
          type: string
        alias:
          type: string
        safeName:
          type: string
        homeIcon:
          type: string
          nullable: true
        awayIcon:
          type: string
          nullable: true
        colorPrimary:
          type: string
          nullable: true
        providerId:
          type: integer
          format: int32
          nullable: true
        ordering:
          type: string
          nullable: true
        longIcon:
          type: string
          nullable: true
        shortIcon:
          type: string
          nullable: true
        displayAbbreviation:
          type: string
          nullable: true
        ranking:
          type: string
          format: int64
          nullable: true
        conference:
          type: string
          nullable: true
        providerIds:
          type: array
          items:
            $ref: '#/components/schemas/gateway.market.v1.SportsTeamProvider'
        longIconDark:
          type: string
          nullable: true
        shortIconDark:
          type: string
          nullable: true
        color:
          $ref: '#/components/schemas/gateway.market.v1.ResolvedColor'
        imageDisplayType:
          $ref: '#/components/schemas/gateway.market.v1.ImageDisplayType'
    gateway.market.v1.AssetPriceMarketType:
      type: string
      enum:
        - ASSET_PRICE_MARKET_TYPE_UNSPECIFIED
        - ASSET_PRICE_MARKET_TYPE_UP_DOWN
        - ASSET_PRICE_MARKET_TYPE_ABOVE_BELOW
        - ASSET_PRICE_MARKET_TYPE_RANGE
        - ASSET_PRICE_MARKET_TYPE_ONE_TOUCH
        - ASSET_PRICE_MARKET_TYPE_DATE
        - ASSET_PRICE_MARKET_TYPE_FIRST_TO_HIT
      description: >-
        How the market settles. ASSET_PRICE_MARKET_TYPE_UNSPECIFIED is returned
        for a market type newer than this schema: render the terms that are
        present.
    gateway.types.v1.AssetId:
      type: object
      properties:
        assetClass:
          $ref: '#/components/schemas/gateway.types.v1.AssetClass'
        symbol:
          type: string
          example: btc
          description: >-
            Lower-case symbol within the class, e.g. btc or gold. Matches
            [a-z0-9-], at most 64 characters.
      description: >-
        Identifies one priced asset. The canonical string form is class:symbol,
        e.g. crypto:btc or commodity:gold.
    gateway.types.v1.NullableAmount:
      type: object
      properties:
        value:
          type: string
          format: decimal
          example: 123.45
          description: The amount as a decimal string.
        currency:
          type: string
          description: The currency code
      description: Represents a monetary amount with its currency.
      required:
        - value
        - currency
      nullable: true
    gateway.market.v1.AssetPriceRangeKind:
      type: string
      enum:
        - ASSET_PRICE_RANGE_KIND_UNSPECIFIED
        - ASSET_PRICE_RANGE_KIND_INTERIOR
        - ASSET_PRICE_RANGE_KIND_BELOW
        - ASSET_PRICE_RANGE_KIND_ABOVE
      description: >-
        Which side of a RANGE market is open. ASSET_PRICE_RANGE_KIND_UNSPECIFIED
        is returned on every market that is not a RANGE.
    gateway.market.v1.AssetPriceDirection:
      type: string
      enum:
        - ASSET_PRICE_DIRECTION_UNSPECIFIED
        - ASSET_PRICE_DIRECTION_UP
        - ASSET_PRICE_DIRECTION_DOWN
      description: >-
        ONE_TOUCH direction. ASSET_PRICE_DIRECTION_UNSPECIFIED is returned on
        every market that is not a ONE_TOUCH.
    gateway.tags.v1.TagLeague:
      type: object
      properties:
        id:
          type: integer
          format: int32
        name:
          type: string
        sportId:
          type: integer
          format: int32
        tagId:
          type: integer
          format: int32
          nullable: true
        image:
          type: string
          nullable: true
        resolution:
          type: string
          nullable: true
        ordering:
          type: string
          nullable: true
        activeSeriesId:
          type: integer
          format: int32
          nullable: true
        isOperational:
          type: boolean
          nullable: true
        automaticResolution:
          type: boolean
          nullable: true
        createdAt:
          type: string
          nullable: true
        slug:
          type: string
        abbreviation:
          type: string
          nullable: true
    gateway.tags.v1.TagSport:
      type: object
      properties:
        id:
          type: integer
          format: int32
        name:
          type: string
        tagId:
          type: integer
          format: int32
          nullable: true
        createdAt:
          type: string
          nullable: true
        slug:
          type: string
        image:
          type: string
          nullable: true
    gateway.hubs.v1.Hub:
      type: object
      properties:
        id:
          type: string
          description: Opaque hub ID for navigation
        title:
          type: string
          description: Hub display title
        image:
          type: string
          description: Hub banner image URL. Empty when absent
        subtitle:
          type: string
          description: Hub subtitle. Empty when absent
        icon:
          type: string
          description: Hub icon URL. Empty when absent
        status:
          $ref: '#/components/schemas/gateway.hubs.v1.HubStatus'
        startTime:
          type: string
          format: date-time
          description: Start of the hub's real-world event. Unset means no countdown
      description: Hub carries the display data for a link to a unified hub.
    gateway.market.v1.SportsTeamProvider:
      type: object
      properties:
        provider:
          $ref: '#/components/schemas/gateway.market.v1.Provider'
        providerId:
          type: string
          description: The provider id
    gateway.market.v1.ResolvedColor:
      type: object
      properties:
        light:
          type: string
          description: Hex color for light mode
        dark:
          type: string
          description: Hex color for dark mode
    gateway.types.v1.AssetClass:
      type: string
      enum:
        - ASSET_CLASS_CRYPTO
        - ASSET_CLASS_COMMODITY
      description: AssetClass namespaces asset symbols.
    gateway.hubs.v1.HubStatus:
      type: string
      enum:
        - HUB_STATUS_ACTIVE
        - HUB_STATUS_ARCHIVED
      description: |-
        HubStatus matches the public unified Hub lifecycle.

         - HUB_STATUS_ACTIVE: The hub serves and appears in feed placements.
         - HUB_STATUS_ARCHIVED: The hub is retired and no longer serves.
    gateway.market.v1.Provider:
      type: string
      enum:
        - PROVIDER_SPORTSDATAIO
        - PROVIDER_SPORTRADAR
        - PROVIDER_OPTICODDS
        - PROVIDER_PANDASCORE
        - PROVIDER_INFRONT
        - PROVIDER_ENETPULSE
        - PROVIDER_UFC
        - PROVIDER_ODDSPAPI
        - PROVIDER_BETER
        - PROVIDER_CHAMPION_DATA
        - PROVIDER_ALTSPORTSDATA
        - PROVIDER_GRID

````

This documentation is built and hosted on [Mintlify](https://mintlify.com), a developer documentation platform.