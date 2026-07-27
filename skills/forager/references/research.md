# Research

Read [`capability-vocabulary.json`](capability-vocabulary.json) and
[`research-plan.json`](research-plan.json). Choose `quick` for an atomic bounded question,
`standard` for normal multi-source work, and `deep` for an explicit deep or broad investigation.

Create a complete Schema v1 plan adapted to the request. Each subquestion must have a unique,
non-empty `id`, `question`, and `reason`, plus the complete `required_capabilities` set drawn from
`docs_search`, `web_search`, and `vertical_search`. The engine supplies `web_fetch`.

Store the exact plan in `PLAN_JSON`, then run:

```console
printf '%s' "$PLAN_JSON" | forager research "QUERY" --plan - --budget BUDGET --format json
```

Synthesize the answer from `evidence_items` and preserve their URLs. Treat discovery results as
evidence only after the engine fetches or reads their contents. Disclose every unresolved
`gap_check` and `capability_gaps` entry, including its effect on coverage.

Complete research when every subquestion is supported by fetched evidence or represented by a
disclosed gap. On terminal failure, preserve any reported evidence and gaps, then use the Diagnose
or configure branch in `SKILL.md`.
