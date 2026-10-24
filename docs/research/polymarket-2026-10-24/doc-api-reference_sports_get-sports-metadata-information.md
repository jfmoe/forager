> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.com/llms.txt
> Use this file to discover all available pages before exploring further.

# Get sports metadata information



## OpenAPI

````yaml /api-spec/gamma-openapi.yaml get /sports
openapi: 3.0.3
info:
  title: Markets API
  version: 1.0.0
  description: REST API specification for public endpoints used by the Markets service.
servers:
  - url: https://gamma-api.polymarket.com
    description: Polymarket Gamma API Production Server
security: []
tags:
  - name: Gamma Status
    description: Gamma API status and health check
  - name: Sports
    description: Sports-related endpoints including teams and game data
  - name: Tags
    description: Tag management and related tag operations
  - name: Events
    description: Event management and event-related operations
  - name: Markets
    description: Market data and market-related operations
  - name: Comments
    description: Comment system and user interactions
  - name: Series
    description: Series management and related operations
  - name: Profiles
    description: User profile management
  - name: Search
    description: Search functionality across different entity types
paths:
  /sports:
    get:
      tags:
        - Sports
      summary: Get sports metadata information
      operationId: getSportsMetadata
      responses:
        '200':
          description: >-
            List of sports metadata objects containing sport configuration
            details, visual assets, and related identifiers
          content:
            application/json:
              schema:
                type: array
                items:
                  $ref: '#/components/schemas/SportsMetadata'
components:
  schemas:
    SportsMetadata:
      type: object
      properties:
        sport:
          type: string
          description: The sport identifier or abbreviation
        image:
          type: string
          format: uri
          description: URL to the sport's logo or image asset
        resolution:
          type: string
          format: uri
          description: >-
            URL to the official resolution source for the sport (e.g., league
            website)
        ordering:
          type: string
          description: Preferred ordering for sport display, typically "home" or "away"
        tags:
          type: string
          description: >-
            Comma-separated list of tag IDs associated with the sport for
            categorization and filtering
        series:
          type: string
          description: >-
            Series identifier linking the sport to a specific tournament or
            season series

````

This documentation is built and hosted on [Mintlify](https://mintlify.com), a developer documentation platform.