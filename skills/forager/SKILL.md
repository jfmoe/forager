---
name: forager
description: "Use the local forager CLI for capability-oriented search, source retrieval, and evidence-backed research."
metadata:
  forager: ">=0.1.0"
---

# forager

Use the local `forager` CLI as the execution layer for current search, documentation lookup,
known-URL retrieval, vertical discovery, and evidence-backed research. This Skill requires
`forager >=0.1.0`.

Read `references/capability-vocabulary.json` before choosing capabilities. Select capabilities,
never providers. Provider choice, order, credential rotation, and same-capability fallback belong
to the CLI.

## Ordinary search

Produce a complete Caller Capability Declaration for every ordinary search. The declaration is
the full authoritative set; forager will not add capabilities based on keywords, URLs, validation,
or its classifier.

1. Select every required capability from the vocabulary, preserving its canonical order.
2. Use `none` when the complete set is empty.
3. Run `forager search "QUERY" --capabilities CAPABILITIES --format json`.

Examples:

```console
forager search "latest React API changes" --capabilities docs_search,web_search --format json
forager search "summarize https://example.com/report.pdf" --capabilities web_fetch --format json
forager search "explain binary search" --capabilities none --format json
```

## Research

For research, generate a Schema v1 plan shaped like
`references/research-plan.json`, adapt its subquestions to the request, save it as `plan.json`, and
run:

```console
forager research "QUERY" --plan plan.json --budget standard --format json
```

The plan's `required_capabilities` may contain only `docs_search`, `web_search`, and
`vertical_search`. It is the complete seam declaration for each subquestion. Do not put
`web_fetch` in the plan: URL extraction and fetch-before-claim are engine invariants. Do not add
unknown fields. Keep every `id` unique and non-empty, and every `reason` non-empty.

Use fetched evidence for consequential or time-sensitive claims. Treat discovery results as
candidates until forager has fetched and cited their contents.

## Failure boundary

If `forager` is missing, its version is below the minimum, configuration is invalid, or a required
capability has no configured provider, report the observed failure and the relevant recovery
command. Do not silently switch to an unrelated search path.

Read `references/migration.md` when installing this Skill or when `smart-search-cli` is also
present.
