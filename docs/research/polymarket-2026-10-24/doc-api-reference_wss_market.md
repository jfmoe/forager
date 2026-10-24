> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.com/llms.txt
> Use this file to discover all available pages before exploring further.

# Market Channel

> Public WebSocket for real-time orderbook, price, and market lifecycle updates.



## AsyncAPI

````yaml asyncapi.json market
id: market
title: Market Channel
description: >-
  Public channel for real-time market data. Subscribe by providing asset IDs
  (token IDs). Receive orderbook snapshots, price updates, trade executions, and
  market lifecycle events.
servers:
  - id: production
    protocol: wss
    host: ws-subscriptions-clob.polymarket.com
    bindings: []
    variables: []
address: /ws/market
parameters: []
bindings: []
operations:
  - &ref_3
    id: subscribe
    title: Subscribe
    description: Send initial subscription request to receive market data
    type: receive
    messages:
      - &ref_14
        id: subscriptionRequest
        contentType: application/json
        payload:
          - name: Subscription Request
            description: Initial subscription message sent after connecting
            type: object
            properties:
              - name: assets_ids
                type: array
                description: Asset IDs (token IDs) to subscribe to
                required: true
                properties:
                  - name: item
                    type: string
                    required: false
              - name: type
                type: string
                description: Must be 'market'
                required: true
              - name: initial_dump
                type: boolean
                description: >-
                  Whether to send an initial orderbook snapshot on subscribe.
                  Defaults to true.
                required: false
              - name: level
                type: integer
                description: Subscription level. Defaults to 2.
                enumValues:
                  - 1
                  - 2
                  - 3
                required: false
              - name: custom_feature_enabled
                type: boolean
                description: Enable best_bid_ask, new_market, and market_resolved events.
                required: false
        headers: []
        jsonPayloadSchema:
          type: object
          description: Initial subscription request payload
          required:
            - assets_ids
            - type
          properties:
            assets_ids:
              type: array
              description: Asset IDs (token IDs) to subscribe to
              items:
                type: string
                x-parser-schema-id: <anonymous-schema-2>
              x-parser-schema-id: <anonymous-schema-1>
            type:
              type: string
              const: market
              description: Must be 'market'
              x-parser-schema-id: <anonymous-schema-3>
            initial_dump:
              type: boolean
              description: >-
                Whether to send an initial orderbook snapshot on subscribe.
                Defaults to true.
              default: true
              x-parser-schema-id: <anonymous-schema-4>
            level:
              type: integer
              enum:
                - 1
                - 2
                - 3
              description: Subscription level. Defaults to 2.
              default: 2
              x-parser-schema-id: <anonymous-schema-5>
            custom_feature_enabled:
              type: boolean
              description: Enable best_bid_ask, new_market, and market_resolved events.
              default: false
              x-parser-schema-id: <anonymous-schema-6>
          x-parser-schema-id: SubscriptionRequest
        title: Subscription Request
        description: Initial subscription message sent after connecting
        example: |-
          {
            "assets_ids": [
              "65818619657568813474341868652308942079804919287380422192892211131408793125422",
              "52114319501245915516055106046884209969926127482827954674443846427813813222426"
            ],
            "type": "market"
          }
        bindings: []
        extensions:
          - id: x-parser-unique-object-id
            value: subscriptionRequest
    bindings: []
    extensions: &ref_0
      - id: x-parser-unique-object-id
        value: market
  - &ref_4
    id: updateSubscription
    title: Update Subscription
    description: Dynamically subscribe or unsubscribe from assets without reconnecting
    type: receive
    messages:
      - &ref_15
        id: subscriptionRequestUpdate
        contentType: application/json
        payload:
          - name: Subscription Update
            description: Subscribe or unsubscribe from assets without reconnecting
            type: object
            properties:
              - name: operation
                type: string
                enumValues:
                  - subscribe
                  - unsubscribe
                required: true
              - name: assets_ids
                type: array
                required: true
                properties:
                  - name: item
                    type: string
                    required: false
              - name: level
                type: integer
                enumValues:
                  - 1
                  - 2
                  - 3
                required: false
              - name: custom_feature_enabled
                type: boolean
                required: false
        headers: []
        jsonPayloadSchema:
          type: object
          description: Dynamically update asset subscriptions
          required:
            - operation
            - assets_ids
          properties:
            operation:
              type: string
              enum:
                - subscribe
                - unsubscribe
              x-parser-schema-id: <anonymous-schema-7>
            assets_ids:
              type: array
              items:
                type: string
                x-parser-schema-id: <anonymous-schema-9>
              x-parser-schema-id: <anonymous-schema-8>
            level:
              type: integer
              enum:
                - 1
                - 2
                - 3
              x-parser-schema-id: <anonymous-schema-10>
            custom_feature_enabled:
              type: boolean
              x-parser-schema-id: <anonymous-schema-11>
          x-parser-schema-id: SubscriptionRequestUpdate
        title: Subscription Update
        description: Subscribe or unsubscribe from assets without reconnecting
        example: |-
          {
            "operation": "subscribe",
            "assets_ids": [
              "71321045679252212594626385532706912750332728571942532289631379312455583992563"
            ]
          }
        bindings: []
        extensions:
          - id: x-parser-unique-object-id
            value: subscriptionRequestUpdate
    bindings: []
    extensions: *ref_0
  - &ref_5
    id: ping
    title: Ping
    description: Send PING every 10 seconds to keep the connection alive
    type: receive
    messages:
      - &ref_16
        id: ping
        contentType: text/plain
        payload:
          - type: string
            const: PING
            x-parser-schema-id: <anonymous-schema-12>
            name: Ping
            description: Client heartbeat — send every 10 seconds
        headers: []
        jsonPayloadSchema:
          type: string
          const: PING
          x-parser-schema-id: <anonymous-schema-12>
        title: Ping
        description: Client heartbeat — send every 10 seconds
        example: '{}'
        bindings: []
        extensions:
          - id: x-parser-unique-object-id
            value: ping
    bindings: []
    extensions: *ref_0
  - &ref_6
    id: pong
    title: Pong
    description: Server responds to PING with PONG
    type: send
    messages:
      - &ref_17
        id: pong
        contentType: text/plain
        payload:
          - type: string
            const: PONG
            x-parser-schema-id: <anonymous-schema-13>
            name: Pong
            description: Server heartbeat response
        headers: []
        jsonPayloadSchema:
          type: string
          const: PONG
          x-parser-schema-id: <anonymous-schema-13>
        title: Pong
        description: Server heartbeat response
        example: '{}'
        bindings: []
        extensions:
          - id: x-parser-unique-object-id
            value: pong
    bindings: []
    extensions: *ref_0
  - &ref_7
    id: receiveBook
    title: Orderbook Snapshot
    description: Full orderbook snapshot sent on subscribe or after a trade
    type: send
    messages:
      - &ref_18
        id: book
        contentType: application/json
        payload:
          - name: Orderbook Snapshot
            description: Full aggregated orderbook for an asset
            type: object
            properties:
              - name: event_type
                type: string
                description: book
                required: true
              - name: asset_id
                type: string
                description: Asset ID (token ID)
                required: true
              - name: market
                type: string
                description: Condition ID of the market
                required: true
              - name: bids
                type: array
                description: Aggregated buy orders by price level
                required: true
                properties:
                  - name: price
                    type: string
                    description: Price level (e.g., '0.50')
                    required: true
                  - name: size
                    type: string
                    description: Total size at this price level
                    required: true
              - name: asks
                type: array
                description: Aggregated sell orders by price level
                required: true
                properties:
                  - name: price
                    type: string
                    description: Price level (e.g., '0.50')
                    required: true
                  - name: size
                    type: string
                    description: Total size at this price level
                    required: true
              - name: timestamp
                type: string
                description: Unix timestamp in milliseconds
                required: true
              - name: hash
                type: string
                description: Hash of the orderbook content
                required: true
        headers: []
        jsonPayloadSchema:
          type: object
          description: Full orderbook snapshot
          required:
            - event_type
            - asset_id
            - market
            - bids
            - asks
            - timestamp
            - hash
          properties:
            event_type:
              type: string
              const: book
              x-parser-schema-id: <anonymous-schema-14>
            asset_id:
              type: string
              description: Asset ID (token ID)
              x-parser-schema-id: <anonymous-schema-15>
            market:
              type: string
              description: Condition ID of the market
              x-parser-schema-id: <anonymous-schema-16>
            bids:
              type: array
              description: Aggregated buy orders by price level
              items: &ref_1
                type: object
                description: Aggregated order at a price level
                required:
                  - price
                  - size
                properties:
                  price:
                    type: string
                    description: Price level (e.g., '0.50')
                    x-parser-schema-id: <anonymous-schema-18>
                  size:
                    type: string
                    description: Total size at this price level
                    x-parser-schema-id: <anonymous-schema-19>
                x-parser-schema-id: OrderSummary
              x-parser-schema-id: <anonymous-schema-17>
            asks:
              type: array
              description: Aggregated sell orders by price level
              items: *ref_1
              x-parser-schema-id: <anonymous-schema-20>
            timestamp:
              type: string
              description: Unix timestamp in milliseconds
              x-parser-schema-id: <anonymous-schema-21>
            hash:
              type: string
              description: Hash of the orderbook content
              x-parser-schema-id: <anonymous-schema-22>
          x-parser-schema-id: BookEvent
        title: Orderbook Snapshot
        description: Full aggregated orderbook for an asset
        example: |-
          {
            "event_type": "book",
            "asset_id": "65818619657568813474341868652308942079804919287380422192892211131408793125422",
            "market": "0xbd31dc8a20211944f6b70f31557f1001557b59905b7738480ca09bd4532f84af",
            "bids": [
              {
                "price": "0.48",
                "size": "30"
              },
              {
                "price": "0.49",
                "size": "20"
              },
              {
                "price": "0.50",
                "size": "15"
              }
            ],
            "asks": [
              {
                "price": "0.52",
                "size": "25"
              },
              {
                "price": "0.53",
                "size": "60"
              },
              {
                "price": "0.54",
                "size": "10"
              }
            ],
            "timestamp": "1757908892351",
            "hash": "0xabc123..."
          }
        bindings: []
        extensions:
          - id: x-parser-unique-object-id
            value: book
    bindings: []
    extensions: *ref_0
  - &ref_8
    id: receivePriceChange
    title: Price Change
    description: >-
      Delta update to orderbook price levels when an order is placed or
      cancelled
    type: send
    messages:
      - &ref_19
        id: priceChange
        contentType: application/json
        payload:
          - name: Price Change
            description: Orderbook price level delta update
            type: object
            properties:
              - name: event_type
                type: string
                description: price_change
                required: true
              - name: market
                type: string
                description: Condition ID of the market
                required: true
              - name: price_changes
                type: array
                required: true
                properties:
                  - name: asset_id
                    type: string
                    required: true
                  - name: price
                    type: string
                    description: Price level affected
                    required: true
                  - name: size
                    type: string
                    description: New aggregate size (0 means level removed)
                    required: true
                  - name: side
                    type: string
                    enumValues:
                      - BUY
                      - SELL
                    required: true
                  - name: hash
                    type: string
                    description: Hash of the order that caused this change
                    required: true
                  - name: best_bid
                    type: string
                    required: false
                  - name: best_ask
                    type: string
                    required: false
              - name: timestamp
                type: string
                description: Unix timestamp in milliseconds
                required: true
        headers: []
        jsonPayloadSchema:
          type: object
          description: One or more price level updates
          required:
            - event_type
            - market
            - price_changes
            - timestamp
          properties:
            event_type:
              type: string
              const: price_change
              x-parser-schema-id: <anonymous-schema-23>
            market:
              type: string
              description: Condition ID of the market
              x-parser-schema-id: <anonymous-schema-24>
            price_changes:
              type: array
              items:
                type: object
                description: Individual price level change
                required:
                  - asset_id
                  - price
                  - size
                  - side
                  - hash
                properties:
                  asset_id:
                    type: string
                    x-parser-schema-id: <anonymous-schema-26>
                  price:
                    type: string
                    description: Price level affected
                    x-parser-schema-id: <anonymous-schema-27>
                  size:
                    type: string
                    description: New aggregate size (0 means level removed)
                    x-parser-schema-id: <anonymous-schema-28>
                  side:
                    type: string
                    enum:
                      - BUY
                      - SELL
                    x-parser-schema-id: <anonymous-schema-29>
                  hash:
                    type: string
                    description: Hash of the order that caused this change
                    x-parser-schema-id: <anonymous-schema-30>
                  best_bid:
                    type: string
                    x-parser-schema-id: <anonymous-schema-31>
                  best_ask:
                    type: string
                    x-parser-schema-id: <anonymous-schema-32>
                x-parser-schema-id: PriceChangeMessage
              x-parser-schema-id: <anonymous-schema-25>
            timestamp:
              type: string
              description: Unix timestamp in milliseconds
              x-parser-schema-id: <anonymous-schema-33>
          x-parser-schema-id: PriceChangeEvent
        title: Price Change
        description: Orderbook price level delta update
        example: |-
          {
            "event_type": "price_change",
            "market": "0x5f65177b394277fd294cd75650044e32ba009a95022d88a0c1d565897d72f8f1",
            "price_changes": [
              {
                "asset_id": "71321045679252212594626385532706912750332728571942532289631379312455583992563",
                "price": "0.5",
                "size": "200",
                "side": "BUY",
                "hash": "56621a121a47ed9333273e21c83b660cff37ae50",
                "best_bid": "0.5",
                "best_ask": "1"
              }
            ],
            "timestamp": "1757908892351"
          }
        bindings: []
        extensions:
          - id: x-parser-unique-object-id
            value: priceChange
    bindings: []
    extensions: *ref_0
  - &ref_9
    id: receiveLastTradePrice
    title: Last Trade Price
    description: Trade execution notification
    type: send
    messages:
      - &ref_20
        id: lastTradePrice
        contentType: application/json
        payload:
          - name: Last Trade Price
            description: Trade execution event
            type: object
            properties:
              - name: event_type
                type: string
                description: last_trade_price
                required: true
              - name: asset_id
                type: string
                required: true
              - name: market
                type: string
                required: true
              - name: price
                type: string
                description: Trade execution price
                required: true
              - name: size
                type: string
                description: Trade size
                required: true
              - name: fee_rate_bps
                type: string
                description: Fee rate in basis points
                required: false
              - name: side
                type: string
                description: From taker's perspective
                enumValues:
                  - BUY
                  - SELL
                required: true
              - name: timestamp
                type: string
                description: Unix timestamp in milliseconds
                required: true
              - name: transaction_hash
                type: string
                required: false
        headers: []
        jsonPayloadSchema:
          type: object
          description: Last trade price event
          required:
            - event_type
            - asset_id
            - market
            - price
            - size
            - side
            - timestamp
          properties:
            event_type:
              type: string
              const: last_trade_price
              x-parser-schema-id: <anonymous-schema-34>
            asset_id:
              type: string
              x-parser-schema-id: <anonymous-schema-35>
            market:
              type: string
              x-parser-schema-id: <anonymous-schema-36>
            price:
              type: string
              description: Trade execution price
              x-parser-schema-id: <anonymous-schema-37>
            size:
              type: string
              description: Trade size
              x-parser-schema-id: <anonymous-schema-38>
            fee_rate_bps:
              type: string
              description: Fee rate in basis points
              x-parser-schema-id: <anonymous-schema-39>
            side:
              type: string
              enum:
                - BUY
                - SELL
              description: From taker's perspective
              x-parser-schema-id: <anonymous-schema-40>
            timestamp:
              type: string
              description: Unix timestamp in milliseconds
              x-parser-schema-id: <anonymous-schema-41>
            transaction_hash:
              type: string
              x-parser-schema-id: <anonymous-schema-42>
          x-parser-schema-id: LastTradePriceEvent
        title: Last Trade Price
        description: Trade execution event
        example: |-
          {
            "event_type": "last_trade_price",
            "asset_id": "114122071509644379678018727908709560226618148003371446110114509806601493071694",
            "market": "0x6a67b9d828d53862160e470329ffea5246f338ecfffdf2cab45211ec578b0347",
            "price": "0.456",
            "size": "219.217767",
            "fee_rate_bps": "0",
            "side": "BUY",
            "timestamp": "1750428146322",
            "transaction_hash": "0xeeefffggghhh"
          }
        bindings: []
        extensions:
          - id: x-parser-unique-object-id
            value: lastTradePrice
    bindings: []
    extensions: *ref_0
  - &ref_10
    id: receiveTickSizeChange
    title: Tick Size Change
    description: Market tick size update when price approaches limits
    type: send
    messages:
      - &ref_21
        id: tickSizeChange
        contentType: application/json
        payload:
          - name: Tick Size Change
            description: Market tick size update event
            type: object
            properties:
              - name: event_type
                type: string
                description: tick_size_change
                required: true
              - name: asset_id
                type: string
                required: true
              - name: market
                type: string
                required: true
              - name: old_tick_size
                type: string
                required: true
              - name: new_tick_size
                type: string
                required: true
              - name: timestamp
                type: string
                required: true
        headers: []
        jsonPayloadSchema:
          type: object
          description: Tick size change event
          required:
            - event_type
            - asset_id
            - market
            - old_tick_size
            - new_tick_size
            - timestamp
          properties:
            event_type:
              type: string
              const: tick_size_change
              x-parser-schema-id: <anonymous-schema-43>
            asset_id:
              type: string
              x-parser-schema-id: <anonymous-schema-44>
            market:
              type: string
              x-parser-schema-id: <anonymous-schema-45>
            old_tick_size:
              type: string
              x-parser-schema-id: <anonymous-schema-46>
            new_tick_size:
              type: string
              x-parser-schema-id: <anonymous-schema-47>
            timestamp:
              type: string
              x-parser-schema-id: <anonymous-schema-48>
          x-parser-schema-id: TickSizeChangeEvent
        title: Tick Size Change
        description: Market tick size update event
        example: |-
          {
            "event_type": "tick_size_change",
            "asset_id": "65818619657568813474341868652308942079804919287380422192892211131408793125422",
            "market": "0xbd31dc8a20211944f6b70f31557f1001557b59905b7738480ca09bd4532f84af",
            "old_tick_size": "0.01",
            "new_tick_size": "0.001",
            "timestamp": "1757908892351"
          }
        bindings: []
        extensions:
          - id: x-parser-unique-object-id
            value: tickSizeChange
    bindings: []
    extensions: *ref_0
  - &ref_11
    id: receiveBestBidAsk
    title: Best Bid/Ask
    description: 'Best bid and ask update (requires custom_feature_enabled: true)'
    type: send
    messages:
      - &ref_22
        id: bestBidAsk
        contentType: application/json
        payload:
          - name: Best Bid/Ask
            description: >-
              Best bid and ask price update — requires custom_feature_enabled:
              true
            type: object
            properties:
              - name: event_type
                type: string
                description: best_bid_ask
                required: true
              - name: asset_id
                type: string
                required: true
              - name: market
                type: string
                required: true
              - name: best_bid
                type: string
                required: true
              - name: best_ask
                type: string
                required: true
              - name: spread
                type: string
                required: true
              - name: timestamp
                type: string
                required: true
        headers: []
        jsonPayloadSchema:
          type: object
          description: Best bid/ask event — requires custom_feature_enabled
          required:
            - event_type
            - asset_id
            - market
            - best_bid
            - best_ask
            - spread
            - timestamp
          properties:
            event_type:
              type: string
              const: best_bid_ask
              x-parser-schema-id: <anonymous-schema-49>
            asset_id:
              type: string
              x-parser-schema-id: <anonymous-schema-50>
            market:
              type: string
              x-parser-schema-id: <anonymous-schema-51>
            best_bid:
              type: string
              x-parser-schema-id: <anonymous-schema-52>
            best_ask:
              type: string
              x-parser-schema-id: <anonymous-schema-53>
            spread:
              type: string
              x-parser-schema-id: <anonymous-schema-54>
            timestamp:
              type: string
              x-parser-schema-id: <anonymous-schema-55>
          x-parser-schema-id: BestBidAskEvent
        title: Best Bid/Ask
        description: 'Best bid and ask price update — requires custom_feature_enabled: true'
        example: |-
          {
            "event_type": "best_bid_ask",
            "market": "0x0005c0d312de0be897668695bae9f32b624b4a1ae8b140c49f08447fcc74f442",
            "asset_id": "85354956062430465315924116860125388538595433819574542752031640332592237464430",
            "best_bid": "0.73",
            "best_ask": "0.77",
            "spread": "0.04",
            "timestamp": "1766789469958"
          }
        bindings: []
        extensions:
          - id: x-parser-unique-object-id
            value: bestBidAsk
    bindings: []
    extensions: *ref_0
  - &ref_12
    id: receiveNewMarket
    title: New Market
    description: 'New market creation event (requires custom_feature_enabled: true)'
    type: send
    messages:
      - &ref_23
        id: newMarket
        contentType: application/json
        payload:
          - name: New Market
            description: 'New market creation event — requires custom_feature_enabled: true'
            type: object
            properties:
              - name: event_type
                type: string
                description: new_market
                required: true
              - name: id
                type: string
                description: Market ID
                required: true
              - name: question
                type: string
                required: true
              - name: market
                type: string
                description: Condition ID
                required: true
              - name: slug
                type: string
                required: true
              - name: description
                type: string
                required: false
              - name: assets_ids
                type: array
                required: true
                properties:
                  - name: item
                    type: string
                    required: false
              - name: outcomes
                type: array
                required: true
                properties:
                  - name: item
                    type: string
                    required: false
              - name: event_message
                type: object
                description: Parent event metadata for grouped markets
                required: false
                properties:
                  - name: id
                    type: string
                    required: false
                  - name: ticker
                    type: string
                    required: false
                  - name: slug
                    type: string
                    required: false
                  - name: title
                    type: string
                    required: false
                  - name: description
                    type: string
                    required: false
              - name: timestamp
                type: string
                required: true
              - name: tags
                type: array
                required: false
                properties:
                  - name: item
                    type: string
                    required: false
              - name: condition_id
                type: string
                description: Condition ID
                required: false
              - name: active
                type: boolean
                description: Whether the market is active
                required: false
              - name: clob_token_ids
                type: array
                description: CLOB token IDs for the market
                required: false
                properties:
                  - name: item
                    type: string
                    required: false
              - name: sports_market_type
                type: string
                description: Sports market type such as spread or moneyline
                required: false
              - name: line
                type: string
                description: Betting line value, or an empty string when not applicable
                required: false
              - name: game_start_time
                type: string
                description: >-
                  Game start time in RFC3339 format, or an empty string when not
                  applicable
                required: false
              - name: order_price_min_tick_size
                type: string
                description: Minimum tick size for order prices
                required: false
              - name: group_item_title
                type: string
                description: Display title for the group item
                required: false
        headers: []
        jsonPayloadSchema:
          type: object
          description: New market creation event — requires custom_feature_enabled
          required:
            - event_type
            - id
            - question
            - market
            - slug
            - assets_ids
            - outcomes
            - timestamp
          properties:
            event_type:
              type: string
              const: new_market
              x-parser-schema-id: <anonymous-schema-56>
            id:
              type: string
              description: Market ID
              x-parser-schema-id: <anonymous-schema-57>
            question:
              type: string
              x-parser-schema-id: <anonymous-schema-58>
            market:
              type: string
              description: Condition ID
              x-parser-schema-id: <anonymous-schema-59>
            slug:
              type: string
              x-parser-schema-id: <anonymous-schema-60>
            description:
              type: string
              x-parser-schema-id: <anonymous-schema-61>
            assets_ids:
              type: array
              items:
                type: string
                x-parser-schema-id: <anonymous-schema-63>
              x-parser-schema-id: <anonymous-schema-62>
            outcomes:
              type: array
              items:
                type: string
                x-parser-schema-id: <anonymous-schema-65>
              x-parser-schema-id: <anonymous-schema-64>
            event_message: &ref_2
              type: object
              description: Parent event metadata for grouped markets
              properties:
                id:
                  type: string
                  x-parser-schema-id: <anonymous-schema-66>
                ticker:
                  type: string
                  x-parser-schema-id: <anonymous-schema-67>
                slug:
                  type: string
                  x-parser-schema-id: <anonymous-schema-68>
                title:
                  type: string
                  x-parser-schema-id: <anonymous-schema-69>
                description:
                  type: string
                  x-parser-schema-id: <anonymous-schema-70>
              x-parser-schema-id: EventMessage
            timestamp:
              type: string
              x-parser-schema-id: <anonymous-schema-71>
            tags:
              type: array
              items:
                type: string
                x-parser-schema-id: <anonymous-schema-73>
              x-parser-schema-id: <anonymous-schema-72>
            condition_id:
              type: string
              description: Condition ID
              x-parser-schema-id: <anonymous-schema-74>
            active:
              type: boolean
              description: Whether the market is active
              x-parser-schema-id: <anonymous-schema-75>
            clob_token_ids:
              type: array
              description: CLOB token IDs for the market
              items:
                type: string
                x-parser-schema-id: <anonymous-schema-77>
              x-parser-schema-id: <anonymous-schema-76>
            sports_market_type:
              type: string
              description: Sports market type such as spread or moneyline
              x-parser-schema-id: <anonymous-schema-78>
            line:
              type: string
              description: Betting line value, or an empty string when not applicable
              x-parser-schema-id: <anonymous-schema-79>
            game_start_time:
              type: string
              description: >-
                Game start time in RFC3339 format, or an empty string when not
                applicable
              x-parser-schema-id: <anonymous-schema-80>
            order_price_min_tick_size:
              type: string
              description: Minimum tick size for order prices
              x-parser-schema-id: <anonymous-schema-81>
            group_item_title:
              type: string
              description: Display title for the group item
              x-parser-schema-id: <anonymous-schema-82>
          x-parser-schema-id: NewMarketEvent
        title: New Market
        description: 'New market creation event — requires custom_feature_enabled: true'
        example: |-
          {
            "event_type": "new_market",
            "id": "1031769",
            "question": "Will NVIDIA (NVDA) close above $240 end of January?",
            "market": "0x311d0c4b6671ab54af4970c06fcf58662516f5168997bdda209ec3db5aa6b0c1",
            "slug": "nvda-above-240-on-january-30-2026",
            "description": "This market will resolve to \"Yes\" if the official closing price for NVIDIA (NVDA) on the final trading day of January 2026 is higher than the listed price. Otherwise, this market will resolve to \"No\".",
            "assets_ids": [
              "76043073756653678226373981964075571318267289248134717369284518995922789326425",
              "31690934263385727664202099278545688007799199447969475608906331829650099442770"
            ],
            "outcomes": [
              "Yes",
              "No"
            ],
            "event_message": {
              "id": "125819",
              "ticker": "nvda-above-in-january-2026",
              "slug": "nvda-above-in-january-2026",
              "title": "Will NVIDIA (NVDA) close above ___ end of January?",
              "description": "This market will resolve to \"Yes\" if the official closing price for NVIDIA (NVDA) on the final trading day of January 2026 is higher than the listed price. Otherwise, this market will resolve to \"No\"."
            },
            "timestamp": "1766790415550",
            "tags": [
              "stocks"
            ],
            "condition_id": "0x311d0c4b6671ab54af4970c06fcf58662516f5168997bdda209ec3db5aa6b0c1",
            "active": true,
            "clob_token_ids": [
              "76043073756653678226373981964075571318267289248134717369284518995922789326425",
              "31690934263385727664202099278545688007799199447969475608906331829650099442770"
            ],
            "sports_market_type": "",
            "line": "",
            "game_start_time": "",
            "order_price_min_tick_size": "0.01",
            "group_item_title": "NVDA above $240"
          }
        bindings: []
        extensions:
          - id: x-parser-unique-object-id
            value: newMarket
    bindings: []
    extensions: *ref_0
  - &ref_13
    id: receiveMarketResolved
    title: Market Resolved
    description: 'Market resolution event (requires custom_feature_enabled: true)'
    type: send
    messages:
      - &ref_24
        id: marketResolved
        contentType: application/json
        payload:
          - name: Market Resolved
            description: 'Market resolution event — requires custom_feature_enabled: true'
            type: object
            properties:
              - name: event_type
                type: string
                description: market_resolved
                required: true
              - name: id
                type: string
                required: true
              - name: market
                type: string
                description: Condition ID
                required: true
              - name: assets_ids
                type: array
                required: true
                properties:
                  - name: item
                    type: string
                    required: false
              - name: winning_asset_id
                type: string
                required: true
              - name: winning_outcome
                type: string
                required: true
              - name: event_message
                type: object
                description: Parent event metadata for grouped markets
                required: false
                properties:
                  - name: id
                    type: string
                    required: false
                  - name: ticker
                    type: string
                    required: false
                  - name: slug
                    type: string
                    required: false
                  - name: title
                    type: string
                    required: false
                  - name: description
                    type: string
                    required: false
              - name: timestamp
                type: string
                required: true
              - name: tags
                type: array
                required: false
                properties:
                  - name: item
                    type: string
                    required: false
        headers: []
        jsonPayloadSchema:
          type: object
          description: Market resolution event — requires custom_feature_enabled
          required:
            - event_type
            - id
            - market
            - assets_ids
            - winning_asset_id
            - winning_outcome
            - timestamp
          properties:
            event_type:
              type: string
              const: market_resolved
              x-parser-schema-id: <anonymous-schema-83>
            id:
              type: string
              x-parser-schema-id: <anonymous-schema-84>
            market:
              type: string
              description: Condition ID
              x-parser-schema-id: <anonymous-schema-85>
            assets_ids:
              type: array
              items:
                type: string
                x-parser-schema-id: <anonymous-schema-87>
              x-parser-schema-id: <anonymous-schema-86>
            winning_asset_id:
              type: string
              x-parser-schema-id: <anonymous-schema-88>
            winning_outcome:
              type: string
              x-parser-schema-id: <anonymous-schema-89>
            event_message: *ref_2
            timestamp:
              type: string
              x-parser-schema-id: <anonymous-schema-90>
            tags:
              type: array
              items:
                type: string
                x-parser-schema-id: <anonymous-schema-92>
              x-parser-schema-id: <anonymous-schema-91>
          x-parser-schema-id: MarketResolvedEvent
        title: Market Resolved
        description: 'Market resolution event — requires custom_feature_enabled: true'
        example: |-
          {
            "event_type": "market_resolved",
            "id": "1031769",
            "market": "0x311d0c4b6671ab54af4970c06fcf58662516f5168997bdda209ec3db5aa6b0c1",
            "assets_ids": [
              "76043073756653678226373981964075571318267289248134717369284518995922789326425",
              "31690934263385727664202099278545688007799199447969475608906331829650099442770"
            ],
            "winning_asset_id": "76043073756653678226373981964075571318267289248134717369284518995922789326425",
            "winning_outcome": "Yes",
            "timestamp": "1766790415550",
            "tags": [
              "stocks"
            ]
          }
        bindings: []
        extensions:
          - id: x-parser-unique-object-id
            value: marketResolved
    bindings: []
    extensions: *ref_0
sendOperations:
  - *ref_3
  - *ref_4
  - *ref_5
receiveOperations:
  - *ref_6
  - *ref_7
  - *ref_8
  - *ref_9
  - *ref_10
  - *ref_11
  - *ref_12
  - *ref_13
sendMessages:
  - *ref_14
  - *ref_15
  - *ref_16
receiveMessages:
  - *ref_17
  - *ref_18
  - *ref_19
  - *ref_20
  - *ref_21
  - *ref_22
  - *ref_23
  - *ref_24
extensions:
  - id: x-parser-unique-object-id
    value: market
securitySchemes: []

````

This documentation is built and hosted on [Mintlify](https://mintlify.com), a developer documentation platform.