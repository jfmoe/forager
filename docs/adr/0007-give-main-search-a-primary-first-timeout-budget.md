# Give main search a primary-first timeout budget

## Decision

A search runs against one end-to-end deadline, and no stage holds a reservation in it.

- **Main search is primary-first.** The first main-search backend may spend the whole remaining
  budget on its attempt. Fallback backends hold no reserved slice; they use what is left after the
  primary fails.
- **Hangs are caught by a read timeout, not by slicing.** The shared HTTP client has a fixed idle
  read timeout. A stream that keeps producing bytes may run as long as the deadline allows; a
  silent stream fails within the read timeout as a retryable network error.
- **Auxiliary seams keep fair slicing.** The classifier keeps its stage cap and its model-chain
  slices, and every other provider chain divides the remaining budget evenly across its remaining
  configured providers under each provider's own per-attempt cap. In those seams the chain is the
  retry: a single-provider chain gets no in-seam timeout retry.
- **Rotation and retry are separate quotas.** A timeout never rotates credentials (ADR-0005).
  Rotations do not consume the retry quota, so the worst case for one provider is its credential
  count plus its retries. Backoff is linear and without jitter.

The defaults and the slot rules are in `docs/reference/config.md` and `docs/reference/testing.md`.

## Why

- Fair slicing killed slow successes. On 2026-08-01 a 56.3-second slice cut an xAI request about
  3 ms before it would have completed, to keep a slice for a fallback that shared the same failure
  domain and could not have done better.
- Fallback earns its keep on fast failures (milliseconds), and the leftover budget covers those.
- With the few retries a single-user CLI makes, exponential backoff and jitter buy nothing and
  spend deadline.

## Rejected alternatives

- **An even split across main-search backends, models and transports.** It cut slow but healthy
  primaries short, as above.
- **Reserving budget for the fallback backend.** It would protect against a hung primary at the
  cost of every slow success; the journal up to 2026-10-28 shows no hung primary.
- **Exponential backoff with jitter.** No contention between many clients to spread out.

## Revisit when

- The journal shows a silently hung primary using up the deadline before a fallback backend could
  run. Then consider reserving budget for the fallback, or fewer primary retries when another
  backend follows.
- Accepted until then: a hung primary spends up to one read timeout per attempt before fallbacks
  run, and a fallback that shares the primary's endpoint, model and credentials gives no
  isolation. Fallback independence is a configuration concern, not a budget one.
