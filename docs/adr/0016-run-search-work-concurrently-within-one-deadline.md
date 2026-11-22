# Run search work concurrently within one deadline

## Decision

forager runs independent provider work concurrently and keeps dependent steps in order.

- The classifier runs first, because it decides the capability set. Main search and the
  supplemental capabilities then start together, instead of supplemental work waiting for main
  search to succeed.
- Independent work fans out inside a search's supplemental stage and inside research discovery and
  candidate fetching. A provider fallback chain inside one seam stays strictly sequential
  (ADR-0007).
- Each fan-out point has its own fixed width in code, not a configuration key and not a shared
  global capacity.
- Concurrent branches share the one absolute deadline and do not split it. Main search keeps its
  primary-first budget; supplemental seams keep their per-attempt caps and chain slicing.
- Results merge in a deterministic order: main-search work first, then each branch as a block in
  launch order. Research fetches candidates in waves that never fetch beyond what the evidence caps
  can accept and never race candidates against each other.
- When main search fails, it alone decides the terminal state and the exit code, and the
  candidates and capability gaps already gathered are delivered instead of discarded.
- Credential claims are serialized within the process and run off the async executor.

The mechanics are under "Search concurrency" in `docs/reference/architecture.md`, and the failure
payload is in `docs/reference/cli.md`.

## Why

- Over 2026-08-26 to 2026-09-25 the journal showed main search (xAI) taking a median of 57 s,
  while the supplemental stage then added a median of 5.6 s (p90 8.2 s) of pure waiting.
- When main search failed, candidates the supplemental providers could have returned in seconds
  were never requested, so the caller got nothing after waiting out the deadline.
- With concurrent branches, the serial-era reason for slicing the deadline between siblings is
  gone: every branch sees the same remaining time.
- Claiming a credential inline (a file lock, an fsync and a rename) stalled executor threads, and
  concurrent claims timed out on each other's lock and collapsed rotation onto the first
  credential.
- The width trades speed against provider rate limits, and no user is better placed than the
  default to judge that trade.

## Rejected alternatives

- **Supplemental capabilities after main search succeeds** (the earlier boundary). It added the
  supplemental latency to every search and returned nothing when main search failed.
- **One global concurrency limit shared by nested fan-outs.** It needs cross-branch coordination
  for a peak that stays small; per-point limits let nested fan-outs briefly exceed the width
  instead.
- **Unordered completion or racing candidates.** Faster tails, but merge order and journal content
  would vary run to run, and racing spends provider quota on discarded results.
- **A current-thread runtime.** All fan-out is I/O multiplexing a single thread could serve, but
  synchronous parsing and decoding run on executor threads; switching waits for tail-latency
  measurements under fan-out.

## Revisit when

- Concurrent branches that share a provider cause rate-limit failures, including simultaneous
  rotations after matching rate limits; a per-provider gate is the next option.
- Head-of-line blocking in fetch waves shows up as a real cost. A slow fetch delays the next wave,
  and under the smallest research budget each subquestion fetches one candidate at a
  time.
- Supplemental quota spent on searches whose main search fails becomes significant. It is accepted
  now because the candidates reach the caller and the supplemental set is usually small.

## Supersedes

- ADR-0008, intra-stage search concurrency. ADR-0016 had already replaced its sequential boundary
  between main search and supplemental capabilities; its remaining rules (per-point limits, the
  shared deadline, deterministic merge, claim serialization) are one concurrency decision with it.
