# Thermo Pressure Pass

Strict structural-pressure overlay on top of a [full review](./full-review.md). **User-triggered only**, via an explicit "thermonuclear", "thermo-nuclear", or "extremely strict structural review" request. It adds investigation depth and a stricter approval bar; it does not change what counts as evidence.

## Precondition

A full review of the same target must be finished first — context intake, staged reading, and systematic matrix included. If that has not happened, run the full review now; do not start this pass on fast-review-level context. This pass only reads what a candidate issue needs: the diff, surrounding context, tests, and candidate canonical implementations.

## Control Conditions

- **Empty is a valid outcome.** A surface with no credible, simpler, and safe alternative ends in `No finding`. Never manufacture a finding to satisfy the pressure.
- **1,000 lines is a trigger, not a finding.** A diff pushing a file past 1,000 lines triggers an investigation of responsibility, boundaries, and control flow. The count alone is never a finding or a blocker.
- **Read-only.** This pass produces findings, not edits. Cross-file or architectural changes stay behind the full review's confirmation stop.
- **Concurrency claims need semantic evidence.** Parallelization or atomicity findings must state data dependencies, shared mutable state, ordering observability, error and cancellation behavior, backpressure, and rollback. "Looks parallelizable" is not evidence.
- **Strict tone, same bar.** Strictness raises investigation depth and explanation demands. Blockers and every other finding use the same verifiable evidence standard as the rest of the review.

## Check Surfaces

Work through each surface and answer its factual questions. Any surface may come back empty.

1. **Deletable complexity.** Is there a credible alternative model that deletes a whole branch, state, helper, mode, or layer — rather than moving the same code around? "Cleaner" without a structural mechanism is not a candidate. Before claiming something is deletable, prove the consumer side: search the exact symbol, config key, event, or wire string, and classify every material consumer as production, non-production (tests, docs, snapshots), or ambiguous. Unused-symbol tool output is discovery, not proof — it misses dynamic names, public interfaces, and configuration. Replacing hand-rolled infrastructure with a platform builtin or maintained dependency counts as deletion only when the swap removes the implementation plus its dedicated tests, and the residual semantics are named.
2. **Growth and cohesion.** Did the diff push a file past 1,000 lines, or mix responsibilities, abstraction levels, or control flow? Beyond the count, is there semantic evidence — or is the growth a cohesive constant table, a deep module, or a flat linear flow?
3. **Spaghetti and model.** Does a new conditional express a real variant, state, or policy — or is it a special case bolted onto an unrelated path? Is feature-rule ownership leaking into shared paths?
4. **Boundaries, types, wrappers.** Do casts, `any`/`unknown`, optionality, or ad-hoc object shapes hide an expressible invariant? Does each wrapper earn a stable boundary, a test seam, a cross-cutting concern, or domain meaning — or only indirection?
5. **Canonical ownership.** Does an existing helper, service, module, or layer with the same semantics and lifecycle already own this logic? Similar names are not duplication.
6. **Concurrency and atomicity.** Is there independent I/O, shared state, or a related invariant left half-applied? What are the data dependencies, ordering, task ownership, cancellation, error, backpressure, and rollback semantics?
7. **Behavior safety net.** What is the observable contract — API, serialization, side effects, exceptions, dynamic dispatch, concurrency timing? What do existing tests or characterization pin down, and what would a proposed restructuring leave unverified? Tests describe behavior; they do not prove the behavior is still required — a test that pins behavior consumer evidence shows to be obsolete is a deletion candidate, not a safety net.

## Blocker Evidence Framework

A thermo blocker must carry all five fields. If any is missing, report the item as a concern, an open question, or `No finding` instead:

1. **Locatable fact** — `path:line`, the diff, and the context showing the structure added or kept.
2. **Structural mechanism** — the causal account: coupling, concept count, change propagation, contract ambiguity, concurrency risk, or half-applied state.
3. **Credible counterfactual** — a smaller ownership or interface alternative that is more than "more elegant", with the net deletion stated: what code, dedicated tests, and docs disappear, minus the glue that remains. A wrapper that relocates the same complexity fails.
4. **Behavior and safety evidence** — the observable behavior the alternative preserves; existing test, characterization, type, integration, transaction, or concurrency verification; and the gaps that remain.
5. **Counter-evidence reviewed** — the possibility of a cohesive deep module, necessary domain complexity, a stable boundary, a test seam, deliberate serialization, or legitimate out-of-transaction work has been considered.

A 1,000-line crossing alone never satisfies this framework. Code-judo, parallelize, and atomicize proposals are equally bound by it.

## Output

Append a neutral record section after the full review's `Findings / Open Questions / Notes`:

```text
Thermo Pressure Pass

- Checked: deletable-complexity / growth / spaghetti / boundary / canonical-ownership /
  concurrency-atomicity / behavior-safety.
- [blocker|concern, confidence] path:line Title
  Fact: observable diff and context.
  Structural mechanism: concepts, coupling, control flow, or invariant risk added.
  Counter-case: legitimate explanations ruled out, or uncertainty remaining.
  Smallest safe alternative: replacement ownership/interface and the behavior it preserves.
  Evidence required: runnable verification, or the explicit gap.
- No finding: candidates investigated where no reliable, simpler, and safe alternative exists.
```

Order findings by: structural regressions, missed restructuring that deletes complexity, spaghetti growth, boundary and type problems, file growth, modularity, legibility. Prefer a few high-conviction findings over a long list. Approval requires: no structural regression, no visible simplification path left untaken, no unjustified file growth, no spaghetti branching, no magical abstraction, no boundary leak or canonical-helper duplication — each judged at the same evidence standard as any other finding.
