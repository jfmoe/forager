# Rust core simplification audit

## Scope

Reviewed the complete working tree, including staged and unstaged code, across
`src/types.rs`, `src/attempt_log.rs`, `src/engine.rs`, `src/research.rs`, and their
tests. The audit focused on unused surface area and duplicated policy, not behavior
changes or speculative error handling.

## Accepted: remove the uncontracted disposition aggregate

- Owner and design surface: `attempt_log::bounded_attempt_summary` and
  `AttemptDisposition::as_str`.
- Production consumers: the summary producer emitted `by_disposition`; no runtime
  consumer read the field, and no CLI or specification contract named it.
- Non-production consumers: no test required the aggregate. Tests still cover the
  load-bearing `fallback.used`, terminal attempt projection, and trace levels.
- Ambiguous consumers: repository-wide searches found no script, release check, or
  smoke path reading the field.
- Change: deleted the aggregate construction, its JSON field, and the string
  projection method used only by that aggregate.
- Abandoned capability: callers can no longer obtain disposition counts from the
  bounded attempt summary. Raw attempt dispositions and all terminal semantics
  remain available.
- Net impact: 17 production lines removed across `src/attempt_log.rs` and
  `src/types.rs`; no dependency, documentation, or operational change.
- Risk and reintroduction: reintroduce only after a documented output consumer
  needs grouped counts. Derive them at that boundary instead of expanding the core
  type preemptively.
- Acceptance: attempt-log unit tests and search/fetch integration tests preserve
  fallback and terminal attribution.

## Rejected candidates

### Remove `ResearchPlan::plan_version`

Rejected on re-review. The repository has no in-tree production caller, but
`forager::types` is public and external Rust consumers are ambiguous rather than
absent. The getter and its integration-test call were restored so the broader
working-tree refactor can keep plan fields private without also removing read
access. This preserves an observable API and keeps this audit behavior-neutral.

### Share `successful_provider` between engine and research

Rejected under Rule of Three. There are two small private implementations with
different terminal contracts: one returns `&str` under an invariant, while the
other returns `Option`. A shared `pub(crate)` helper would enlarge the interface and
encode a false common policy for negligible deletion.

### Remove attempt-domain and recovery types

Rejected because `AttemptDisposition`, `AttemptTarget`, recovery manifests,
fan-out waves, and classifier decisions all have production consumers and express
documented runtime state. Their tests protect current behavior rather than orphaned
surface area.
