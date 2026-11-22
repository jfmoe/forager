# One credential pool model for every keyed provider

## Decision

Every provider whose registration says it uses credentials, and the classifier, holds them in a
Provider Credential Pool. A single credential is the one-element case of the same pool; there is
no second shape for one key. A provider registered as not using credentials has no pool, never
claims or rotates, and always counts as configured. A provider registered with optional
credentials runs anonymously when its pool cannot serve it (ADR-0029).

- A request claims the next credential from a cursor shared by all local processes and advances
  it at once, without rolling back on failure.
- Only a rate-limit or quota failure rotates to another credential before retrying, and each
  credential is tried at most once per request. A rate-limited credential is not backed off and
  retried. Other failures follow the provider's retry and fallback rules.
- No output or error contains a credential's characters. Configuration and doctor views show a
  fixed, irreversible mask; the cursor state holds only non-sensitive indices.

## Why

- Two credential systems with several configuration shapes (a single key, a key list, a
  JSON-encoded list), replacement priorities and a provider allowlist between them mean every
  provider has to be checked against each shape.
- Advancing without rollback keeps concurrent processes from piling onto the same credential and
  needs no coordination after the request ends.
- A rate-limited credential recovers on the upstream's schedule, not within one request; trying
  the next credential is the only retry that can succeed in time.

## Rejected alternatives

- **Separate single-key and multi-key shapes.** Two code paths and a precedence rule for the same
  fact, with no behaviour that the one-element pool does not already give.
- **Rolling the cursor back after a failed attempt.** Concurrent requests would reuse a failing
  credential, and the rollback would need its own cross-process transaction.
- **Backing off and retrying the same credential on a rate limit.** It burns the deadline waiting
  on a limit that outlasts the request.

## Revisit when

- A provider's terms forbid rotating across credentials. The pool model stays, but such a provider
  is restricted to one credential in its configuration, as YouTube's Data API already is.
