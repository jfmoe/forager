> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.com/llms.txt
> Use this file to discover all available pages before exploring further.

# List tags



## OpenAPI

````yaml /api-spec/gamma-openapi.yaml get /tags
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
  /tags:
    get:
      tags:
        - Tags
      summary: List tags
      operationId: listTags
      parameters:
        - $ref: '#/components/parameters/limit'
        - $ref: '#/components/parameters/offset'
        - $ref: '#/components/parameters/order'
        - $ref: '#/components/parameters/ascending'
        - name: include_template
          in: query
          schema:
            type: boolean
        - name: is_carousel
          in: query
          schema:
            type: boolean
      responses:
        '200':
          description: List of tags
          content:
            application/json:
              schema:
                type: array
                items:
                  $ref: '#/components/schemas/Tag'
components:
  parameters:
    limit:
      name: limit
      in: query
      schema:
        type: integer
        minimum: 0
    offset:
      name: offset
      in: query
      schema:
        type: integer
        minimum: 0
    order:
      name: order
      in: query
      schema:
        type: string
      description: Comma-separated list of fields to order by
    ascending:
      name: ascending
      in: query
      schema:
        type: boolean
  schemas:
    Tag:
      type: object
      properties:
        id:
          type: string
        label:
          type: string
          nullable: true
        slug:
          type: string
          nullable: true
        forceShow:
          type: boolean
          nullable: true
        publishedAt:
          type: string
          nullable: true
        createdBy:
          type: integer
          nullable: true
        updatedBy:
          type: integer
          nullable: true
        createdAt:
          type: string
          format: date-time
          nullable: true
        updatedAt:
          type: string
          format: date-time
          nullable: true
        forceHide:
          type: boolean
          nullable: true
        isCarousel:
          type: boolean
          nullable: true

````

This documentation is built and hosted on [Mintlify](https://mintlify.com), a developer documentation platform.