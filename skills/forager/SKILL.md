---
name: forager
description: "Search current web and X/Twitter sources with the local forager CLI. Use for known-URL retrieval, site mapping, official/API documentation lookup, vertical discovery, source-backed fact checking, or deep research."
metadata:
  forager: ">=0.1.0"
---

# forager

Use the local `forager` CLI as the execution layer. This Skill requires `forager >=0.1.0`.

Only read `references/cli.md` when ordinary `search` or `research` fails and CLI diagnosis or
recovery details are needed, or when the user explicitly asks to use another `forager` command or
inspect its parameters. Do not load it for a routine `search` or `research`. Treat
`forager <command> --help` as the final authority for argument parsing.

Before declaring capabilities for `search` or `research`, read
`references/capability-vocabulary.json`. Declare capabilities; the CLI owns provider selection,
ordering, credential rotation, and same-capability fallback.

## Ordinary search

For every `forager search`, declare the complete set of supplemental capabilities required beyond
main search. The declaration is authoritative: forager executes it as given.

1. Select every required capability from the vocabulary, preserving its canonical order.
2. Use `none` when main search alone is sufficient.
3. Run `forager search "QUERY" --capabilities CAPABILITIES --format json`.

Examples:

```console
forager search "latest React API changes" --capabilities docs_search,web_search --format json
forager search "summarize https://example.com/report.pdf" --capabilities web_fetch --format json
forager search "explain how solar panels work" --capabilities none --format json
```

Complete ordinary search when the command succeeds and every reported `capability_gaps` entry is
disclosed with its effect on coverage.

## Direct operations

Use `forager fetch URL --format json` when the task is limited to reading or extracting a known
URL or PDF. Use ordinary search with `web_fetch` when that source contributes to a broader answer.

Use `forager map URL --instructions "GOAL" --format json` to discover pages or site structure
under a known domain.

Commands under `exa`, `context7`, and `anysearch` bypass capability routing. Reserve them for a
user-requested provider or an operation available only through its direct command. Automatic
fallback remains with `search` and `research`.

Complete a direct operation when it returns the requested content or page set, or its observed
failure is reported under the failure boundary.

## Research

Use `research` when the request requires decomposition, multi-source verification, or explicitly
asks for deep research. Otherwise use ordinary search.

Create a complete Schema v1 plan following `references/research-plan.json`. Adapt both
`intent_signals` and `decomposition` to the request. Store the exact JSON in `PLAN_JSON`, then pipe
it through standard input:

```console
printf '%s' "$PLAN_JSON" | forager research "QUERY" --plan - --budget standard --format json
```

Each subquestion carries a complete `required_capabilities` declaration drawn from `docs_search`,
`web_search`, and `vertical_search`. The research engine supplies `web_fetch` for URL extraction
and fetch-before-claim. Use only Schema v1 fields, with unique non-empty `id` values and non-empty
`question` and `reason` values.

After the command completes, synthesize the answer from `evidence_items` and preserve their source
URLs. Discovery results become evidence only after forager fetches their contents. Disclose
unresolved `gap_check` and `capability_gaps`.

Complete research when every subquestion is supported by fetched evidence or represented by a
disclosed gap.

## Failure boundary

If `forager` is missing or its version is below the minimum, report the observed command or version
and stop this Skill branch.

For configuration errors, run `forager config path`, then `forager config list` to locate the
invalid value. Repair a parseable document with `forager config set` or `forager config unset`; edit
the reported path when its TOML syntax is damaged.

For provider authentication, availability, or connectivity failures, run
`forager doctor --provider PROVIDER --format json`. Report redacted diagnostic fields and the
relevant recovery command.

A successful `search` or `research` result may contain advisory `capability_gaps`. Keep usable
results, disclose each missing capability and its effect on coverage, and leave the affected part
of the request unverified.

Complete failure handling when the user has an actionable diagnosis and the reported verification
state matches the available evidence.
