# Research

## Orient before planning

Run one ordinary search before writing the plan. When this branch follows an ordinary-search
escalation, reuse that existing result instead. Treat the search result as orientation material
only: use it to improve decomposition and place reliable discovered URLs into subquestions as
known-URL evidence directives. Claims require evidence from the Research Evidence Pipeline.

Orientation is complete when the result has shaped every subquestion and every useful known URL is
attached to the plan without being treated as evidence.

## Plan and run

Read [`capability-vocabulary.json`](capability-vocabulary.json) and
[`research-plan.json`](research-plan.json). Choose `quick` for a small research-grade plan,
`standard` for normal multi-source work, and `deep` for an explicit deep or broad investigation.

Create a complete Schema v1 plan adapted to the request. Each subquestion has a unique, non-empty
`id`, `question`, and `reason`, plus the complete `required_capabilities` set drawn from
`docs_search`, `web_search`, and `vertical_search`. The engine supplies `web_fetch`.

Store the exact plan in `PLAN_JSON`, then run:

```console
printf '%s' "$PLAN_JSON" | forager research "QUERY" --plan - --budget BUDGET --format json
```

Execution is complete when the command exits and every returned path needed for synthesis is
readable.

## Synthesize from the evidence index

Use the Research Evidence Index as a directory, not as an answer. Read only the necessary bodies
from `evidence_items[].path`, then synthesize each supported claim with `[eN](URL)`, where `eN`
matches the evidence item's `id`. Citation Binding expresses attribution, not semantic
verification; check that the cited body supports the claim.

Disclose every unresolved `gap_check` and `capability_gaps` entry and its effect on coverage.

For each key claim without fetched support, read `unconsumed_candidates.path`, select a matching
disclosed candidate, and fetch it. When none fits, run `forager exa similar` from a reliable URL and
fetch the selected result. An unfetched candidate may appear only as a disclosed unverified
candidate. Keep this supplement loop in the agent layer and re-run neither `forager research` nor
main search.

Complete research when every key claim is supported by fetched evidence or represented by a
disclosed gap or unverified candidate. On terminal failure, preserve and consume any reported
evidence paths and gaps before using Diagnose or configure in `SKILL.md`.
