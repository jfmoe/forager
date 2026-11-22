# Keep the research engine deterministic and orchestrate from the skill

## Decision

**The engine.** `research` is a deterministic, single-pass evidence pipeline: plan, discover,
fetch, then account for coverage and deliver. It performs no semantic synthesis and never calls
main search. Budget tiers bound its shape (subquestions, evidence per subquestion, discovery
breadth) rather than one global counter, and known URLs in the plan are evidence directives that
are always attempted. The engine delivers evidence; it does not answer.

**The skill.** Everything that needs judgement runs in the agent layer, guided by the product
skill:

- **Cost ladder.** Every request goes down `direct retrieval → ordinary search → research` and
  takes the cheapest branch that can finish it. Escalation happens only on an observed shortfall
  of the cheaper branch, or when the request's shape rules that branch out, never in anticipation.
- **Orientation.** One ordinary search precedes every research plan; an escalation from ordinary
  search reuses the result it already has. Orientation only shapes the plan, correcting the
  decomposition and supplying known URLs. It is never cited as evidence.
- **Bounded recovery.** The ordinary-search recovery chain (retry, Exa fallback, fetch) has small
  fixed caps per step, and an error that is not a timeout or network failure leaves it at once.
- **Refetch after research.** A key claim without fetched evidence is either backed by fetching a
  disclosed candidate or a similar page, or downgraded to an unverified candidate. The loop ends
  when every key claim is evidenced or downgraded. It never reruns research and never uses main
  search to backfill evidence.

## Why

- The plan is fixed before any in-engine stage runs, so an orientation stage inside the engine
  could not prevent a wrong decomposition. Read before the plan is written, the same search shapes
  the subquestions and contributes the high-value URLs as known-URL directives.
- Main-search answers are model-synthesized. Admitting them into a pipeline whose contract is
  fetch before claim would let unverified text pose as evidence.
- A deterministic engine is reproducible and testable offline; judgement about when evidence is
  enough belongs to the agent that writes the final answer.
- Research is the most expensive branch, and the engine is single-pass, so recovery work lands in
  the agent layer either way. The ladder and the caps keep that cost in proportion to the request.

## Rejected alternatives

- **An orientation stage inside the engine**, run in parallel with discovery. It runs too late to
  change the plan and brings synthesized content into the evidence path.
- **A refetch loop inside the engine.** The engine cannot judge which claim lacks support; the
  agent can, and it already holds the disclosed candidates.
- **Numeric per-subquestion, per-tool or per-round quotas for the refetch loop.** A policy that
  ends when every key claim is evidenced or downgraded bounds the loop without arbitrary numbers.
- **A mechanical synthesis report from the engine.** It presented an excerpt list as an answer.

## Revisit when

- Research is used mostly without an agent in the loop. A bare CLI call plans from the query text
  alone, without orientation, and decomposes ambiguous queries worse; that is accepted while the
  skill workflow is the main caller.
- Agents are seen escalating to research, or looping in recovery, despite the ladder and caps.

## Supersedes

- ADR-0012, the research engine as a deterministic evidence pipeline without main search. It
  assigned orientation to the skill layer, and this ADR held the other half of that boundary;
  they are one decision.
