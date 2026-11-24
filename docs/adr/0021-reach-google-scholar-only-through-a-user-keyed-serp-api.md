# Reach Google Scholar only through a user-keyed SERP API

## Decision

The `scholar` platform reaches Google Scholar only through a third-party SERP API. Its one route
is `serpapi`, SerpApi's Google Scholar engine. The user registers and configures their own key;
forager ships no key and pays for no quota, and the interface is designed around SerpApi's free
plan.

- **Keys go into the Provider Credential Pool (ADR-0005)**, with no new credential mechanism. An
  exhausted key and a rate-limited key both make the same request move to the next key.
- **Present by default, idle without a key.** The route is in the default order, but without a key
  it is unconfigured: platform commands fail before any request and name the missing key, and
  plain search never calls it.
- **Spend as little quota as possible.** Every successful search costs quota, including a
  legitimate empty result and a missing cluster. Search asks for a full page by default, because a
  page costs the same whatever its size; fetch reads one page of a cluster with no automatic
  paging and only at metadata depth, leaving the body to the target platform or Web Fetch. Quota
  state is not persisted. Plain doctor sends one keyless request that costs nothing, a targeted
  doctor asks the free account endpoint about each key, and only the route's Live Checks, and only
  with a key configured, spend quota.
- **Named after the vendor**, not the platform: one SerpApi account's quota and throughput are
  shared across engines, so another SerpApi engine can reuse the same key pool and
  credential cursor.
- **Keys travel in the URL query**, the only place SerpApi accepts them, so the route strips URLs
  from HTTP client errors and redacts key values from its own messages before they reach an
  attempt.

## Why

- Google Scholar has no official public search API. Its help pages rule out bulk access and ask
  automated software to follow robots.txt, which disallows `/scholar`. Scraping it cannot be stable
  infrastructure, and a documented SERP API already returns structured results, including citation
  counts and version clusters.
- SERP APIs bill per call. forager is a local CLI with no server to hold a shared key safely, and
  it should not carry the user's cost or terms liability.
- The free plan is small (250 searches per account per month on 2026-10-07), so every design
  choice above saves calls. Not persisting quota state costs one unbilled 429 round trip when an
  exhausted key comes up.

## Rejected alternatives

- **Scraping Google Scholar directly.** It breaks the site's robots.txt and help-page rules and
  would not be stable.
- **A browser route.** Not built; one would be opt-in like every route that carries terms risk
  (ADR-0020).
- **A shared forager key.** No safe place to keep it, and forager would carry the cost and the
  terms liability.
- **Suggesting several free accounts to stretch the quota.** That may count as abusing the free
  plan, so the product skill says only that each key carries its own quota.
- **Sorting Scholar search by date.** The upstream date order ignores the year range, so the two
  could not be combined.
- **An author option for Scholar search.** The query's own `author:` operator already applies
  to the whole query.
- **Cited-by as a search option, and a review-only filter on cited-by.** Cited-by changes the required
  input and what a result means, so it is its own operation. The review filter was never tested
  together with the citation lookup.

## Revisit when

- SerpApi stops the service or changes its terms; the platform has no fallback to another
  platform, so it would be unavailable.
- Result quality limits start to matter: author names may be abbreviated or cut, cluster identity
  partly relies on an undocumented result ID encoding, and a cluster's first version is not always
  the published one.
- Google Scholar offers an official API.
