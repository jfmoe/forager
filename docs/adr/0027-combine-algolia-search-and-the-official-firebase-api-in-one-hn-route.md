# Combine Algolia search and the official Firebase API in one HN route

## Decision

The `hn` platform has one anonymous route, `hn_api`, which combines two public endpoints inside
the route: Algolia's HN Search API and Hacker News's official Firebase API. It provides search,
fetch and the Platform Operation Stories.

- **One route reaches both endpoints, not two routes.** The platform chain takes the whole result
  from the first accepted route and never merges results, so two routes could not combine
  Algolia's tree with Firebase's ranking in one fetch. The platform contract therefore allows a
  route to reach several official endpoints of one platform, each with its own URL setting, all
  sharing the route's access policy.
- **Firebase only enhances.** When an enhancing request (ranking, live counts, deletion markers,
  story titles) fails, the route keeps Algolia's result and reports a diagnostic instead of
  failing. The one exception is the Stories list itself, which only Firebase provides.
- **A constant number of requests.** Firebase ranking is read only for the root item: top-level
  comments follow HN's page ranking and deeper levels follow time. Each operation sends at most a
  few requests whatever the number of comments; only Stories fetches items one by one when Algolia
  is missing some.
- **Threads are delivered as files.** `thread` is HN fetch's default depth; the discussion is
  written as a Markdown file and stdout carries only its path and counts (ADR-0015). A platform
  declares how its `thread` depth is delivered: as a file when the route returns a Native body
  (HN), inline otherwise (X).
- **Ranked lists are a separate Platform Operation, Stories.** It takes no query, which changes the
  required input; items carry their rank, and it issues no cursor, because the list keeps changing
  and paging could not be reproduced.

## Why

Measured on 2026-10-17:

- Algolia searches by keyword, type, author, story, time and points, returning at most the first
  1,000 hits per query, and its item endpoint returns the whole comment tree in one request (3,768
  nodes, about 2 MB, 3.8 seconds). Children in the tree are ordered by ID, which is posting time,
  not by HN's page ranking, and counts and new items lag by about three minutes. Each IP may send
  at most 10,000 requests an hour.
- Firebase reads only by ID: an item's children are its direct replies in page-ranking order,
  counts are live, and there are six official ranked lists (top, new, best, ask, show, job). Its
  documentation says there is currently no rate limit. It cannot search, and reading a tree takes
  one request per comment.
- Each endpoint has what the other lacks: search and the whole tree only in Algolia; ranking, live
  counts and the lists only in Firebase.
- A large thread renders to around a million characters, which inline output would push into the
  caller's context.

## Rejected alternatives

- **Algolia only.** Comments come in time order, so the top of the file does not show the most
  valued discussion, and there are no front-page lists.
- **Firebase only.** No search, and a tree of thousands of comments takes thousands of requests.
- **Two routes, one per endpoint.** The chain does not merge results, and Firebase alone as a
  fallback cannot even search.
- **Ranking nested replies too.** Every node with replies would cost a Firebase request, so the
  request count would grow with the thread.
- **Scraping HN's HTML pages.** Large threads are paged, the nesting is hard to parse reliably,
  and it is not an official interface.

## Revisit when

- Firebase starts limiting requests, or Algolia's per-IP limit is hit. Firebase's "no rate limit"
  is the current state, not a promise, and an IP blocked by Algolia has to appeal by email; the
  shared access policy keeps a margin below Algolia's limit across processes.
- Callers need a complete mirror of HN pages. Threads leave out deleted and dead comments and may
  miss the last few minutes of comments, and search and Stories counts may lag by minutes; the
  platform reference tells callers this.
