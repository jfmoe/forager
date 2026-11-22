# Prefer Exa for Documentation Search and keep Context7 as one payload

## Decision

This ADR holds two decisions about Documentation Search and its Context7 provider.

**1. Exa first.** Automatic Documentation Search tries Exa before Context7. Context7 is called
automatically only when Exa returns no consumable candidate, or when the user configures another
order. A provider result with no normalized source is not consumable, so an empty Context7 answer
continues down the chain instead of ending it as an empty success. Within one research subquestion,
candidates are taken round-robin across the declared capabilities before the discovery limit
applies, so no capability can fill every slot.

**2. Context7 as one readable payload.** The direct `context7` commands use the official Context7
MCP and deliver one documentation body. forager does not invent structured fields the MCP does not
return, and does not parse the body's source lines into sources. The library identifier is a
reusable lookup key, not a fetchable URL; the product skill teaches callers to resolve it once and
reuse it.

## Why

For Exa first, a comparison of 14 queries checked against official pages, together with the
journal from 2026-08-26 to 2026-09-25:

- Key-fact coverage was 48% for Context7 and 93% for Exa, at median latencies of 8.2 s and 2.4 s.
  On vendor platforms, data APIs and app manuals, Context7 covered 25% and Exa 100%.
- Context7's library resolution almost always returns some library, so the old Context7-first
  chain never reached Exa: none of 233 journaled Documentation Search attempts did. In research,
  42% of Context7 attempts were rejected as thin, and some accepted bodies came from the wrong
  library.
- Research kept each subquestion's first candidates in arrival order. Documentation Search
  arrived before Web Search, so under the smallest budget its candidates took every slot and no
  web candidate was fetched; one sampled run ended with no evidence because of this.
- Exa returns URLs a caller can fetch directly, where Context7 returns library locators that need
  a second call.

For one payload, the official MCP returns formatted text, unlike Context7's REST API with its
structured snippets. Exposing snippet fields would duplicate the body and make a response shape
the MCP does not produce part of the CLI contract.

## Rejected alternatives

- **Relevance gates on Context7 results** (library-name overlap, a minimum snippet count, anchor
  terms in the body). With Exa first, Context7 is rarely reached automatically, so the tuned
  heuristics would seldom run.
- **Context7-specific capability declarations in the skill.** With Exa first, the existing
  vocabulary already serves vendor API and platform documentation.
- **Structured snippet fields, or sources parsed from the body.** They duplicate content and
  depend on formatting the MCP does not promise.

## Revisit when

- The journal shows off-topic Context7 evidence again; then reconsider the relevance gates.
- Exa's coverage of documentation queries drops below Context7's on a comparable sample.
- The Context7 MCP starts returning structured results.

## Supersedes

- ADR-0011, which kept Context7 MCP docs as one readable payload. It is decision 2 above, and its
  rule that an empty Context7 result does not end the automatic chain is part of decision 1.
