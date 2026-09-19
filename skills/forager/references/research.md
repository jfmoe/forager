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
Their maximum subquestion counts are 2, 4, and 6 respectively.

Create a complete Schema v1 plan adapted to the request. Each subquestion has a unique, non-empty
`id`, `question`, and `reason`, plus the complete `required_capabilities` set drawn from
`docs_search`, `web_search`, and `vertical_search`. The engine supplies `web_fetch`.

Use only the schema's exact intent-signal values: `recency_requirement` is `none`, `recent`, or
`current`; `docs_api_intent` is boolean; `source_authority_need` and `cross_validation_need` are
`normal` or `high`; and `claim_risk` is `medium` or `high`. Put dates and other precision in the
query or subquestions instead of inventing enum values.

Store the exact plan in `PLAN_JSON`, then run:

```console
printf '%s' "$PLAN_JSON" | forager research "QUERY" --plan - --budget BUDGET --format json
```

Execution is complete when the command exits and every returned path needed for synthesis is
readable.

## Synthesize from the evidence index

Use the Research Evidence Index as a directory, not as an answer. Read the necessary body from
every cited `evidence_items[].path` and check that it supports the claim. Cite URL evidence as
`[eN](URL)` and documentation evidence as `[eN]` when it has no URL, where `eN` matches the
evidence item's `id`. Citation Binding expresses attribution, not semantic verification.

Disclose every unresolved `gap_check` and `capability_gaps` entry and its effect on coverage.

For each key claim without fetched support, fetch an already-known reliable HTTP(S) URL when one is
available. Otherwise read `unconsumed_candidates.path`, select a matching disclosed candidate, and
fetch it. Only when neither exists, run one `forager exa similar` round from a reliable URL and fetch
the selected result. An unfetched candidate may appear only as a disclosed unverified candidate.
Supplemental fetches are outside the evidence index: cite their actual URLs as supplemental sources
and never fabricate an `eN` identity for them. Keep this recovery loop in the agent layer. Reuse
completed work rather than repeating the same research plan or main query to fill retrieval gaps.

## Follow new questions

When evidence reveals a new mechanism, useful search term, or unresolved objection that existing
material cannot answer, state the new question, the evidence that prompted it, and the decision it
could change. Within the authorized scope and remaining task budget, route that question through
the cost ladder in `SKILL.md`. Use a focused ordinary search when sufficient; use a new research
plan only when the new question requires decomposition or multiple-source verification.

Retain earlier evidence and gaps. A new query must add information, not merely rename a failed
query or seek a preferred conclusion. It does not reset retry limits, access permissions, or the
task budget. Keep evidence indexes from separate runs distinct; cite supplemental sources by URL
and identify the originating run when using index locators. Stop when further searches are unlikely
to change the decision, the budget is reached, or access is blocked; report the remaining limits.

## Close or recover

Complete research when every key claim is supported by fetched evidence or represented by a
disclosed gap or unverified candidate. On terminal failure, read the Research Recovery Manifest
when `summary_path` is not null, then consume its readable paths, completed evidence, and gaps. If
the manifest is unavailable, consume the other reported readable paths and gaps before using
Diagnose or configure in `SKILL.md`.
