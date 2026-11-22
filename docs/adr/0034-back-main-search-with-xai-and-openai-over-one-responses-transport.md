# Back main search with xai and openai over one Responses transport

## Decision

Main search is a Capability Seam with two backends in independent failure domains: `xai` and
`openai`. Both speak the Responses API with a fixed web-search tool; `openai` may point at the
official API or at a gateway that speaks the same protocol. A backend without credentials is
skipped, so a user who does not configure `openai` sees no change, and reordering the backends
makes `openai` primary.

- **One transport.** The Responses transport (request body, stream reading, completion and
  terminal-failure handling, answer text and citation extraction) is a shared module named after
  the protocol, and the main-search execution path exists once. A backend supplies only its
  endpoint, model, tool item and tracking parameter. X platform search stays xai's own adapter.
- **A fixed request surface.** `openai` exposes only its endpoint, credentials and model. Tools,
  reasoning effort and web-search parameters are not configurable, and like `xai` it has no
  per-attempt timeout: the primary-first budget applies (ADR-0007).
- **Scope.** `openai` is not a platform route and does not serve the classifier.
- **No same-failure-domain machinery.** The retired `openai_compatible` backend and everything
  that existed only for it stay removed: the same-endpoint model chain, the in-process model
  circuit breaker, the stream-to-plain-HTTP transport fallback, the two-shape doctor probe, the
  chain's pre-run gate, and the configuration warning about fallbacks in the same failure domain.
  Its configuration keys, environment variables and doctor target are rejected as unknown input,
  with no alias.

The configuration surface is in `docs/reference/config.md`; the module split and the request
details are under the main-search sections of `docs/reference/architecture.md` and
`docs/reference/cli.md`.

## Why

- The retired fallback never helped. In the journal from 2026-09-19 to 2026-10-18 (443 searches)
  it was called in 13 searches, and all 39 of its attempts failed with the upstream's "service
  temporarily unavailable". It was configured with the same endpoint and model as `xai`, so it
  failed whenever `xai` did, while its machinery reached across providers, chain, configuration,
  doctor, smoke and setup, with about 900 lines in the provider alone.
- That data showed only that a fallback in the same failure domain is worthless, not that an
  independent one is; this is why the seam was kept for a later candidate.
- `openai` is independent: its upstream model provider is OpenAI rather than xAI, through a
  different endpoint. On 2026-10-28 a gateway test completed a streamed web-search request in about
  12 s with two search rounds, returned cited answer text, and emitted the same stream event
  sequence as xAI.
- In the 30 days to 2026-10-28 the journal had 274 searches with two main-search failures. One
  was three consecutive network attempts of under half a second each, the other a 30-second
  runtime failure. Both left enough deadline for an independent backend to answer.
- Two backends speak one protocol, so a protocol fix belongs in one place.

## Rejected alternatives

- **Keeping `openai_compatible` as the fallback.** It shared the primary's failure domain and never
  succeeded.
- **Removing the main-search seam while only `xai` remained.** A new candidate would have needed
  the structure rebuilt; keeping it cost a backend list with one valid value for a while.
- **Restoring the same-failure-domain warning.** `openai`'s default endpoint differs from `xai`'s;
  configuring both to the same endpoint and model takes deliberate effort.
- **Opening tools, reasoning effort or web-search parameters.** No current need; they can be added
  when one appears.
- **Per-backend copies of the Responses transport.** Each protocol fix would have to land twice.
- **Removing tools a gateway injects into requests.** That is the gateway's behaviour; handling it
  would tie forager to one gateway's quirks.

## Revisit when

- The two backends are seen failing together. The maintainer's two gateways resolve to the local
  proxy's fake IPs, so it is unknown whether they share a host; a host failure would take both
  down, though the upstream services remain independent.
- A third main-search candidate with its own failure domain passes acceptance.
- The cost of `openai` matters. One simple query with web search used about 15,600 tokens,
  including reasoning, on the tested model; more complex queries cost more.

## Supersedes

- ADR-0031, the retirement of the `openai_compatible` backend. Two of its points no longer hold
  (only `xai` valid, no backend fallback); the rest, the removed machinery and its evidence, is
  kept above.
