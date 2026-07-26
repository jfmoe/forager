---
status: superseded by ADR-0004
---

# Let Skill callers declare ordinary-search capabilities

Smart Search will accept a Caller Capability Declaration for ordinary search and route diagnostics. An Agent that has already interpreted the Smart Search Skill sends one ordered capability CSV—or `none` for an empty declaration—directly with the search invocation, so the runtime can merge deterministic rules with that declaration and skip embeddings and classifier routing without adding a preliminary route call. Direct CLI invocations without a declaration retain the existing hybrid router, and the research executor retains its separate evidence workflow.

## Decision

- Caller Capability Declarations are accepted only by ordinary search and route diagnostics. They may name Documentation Search, Supplemental Web Search, Web Fetch, and Vertical Search capabilities; `main_search` remains unconditional and provider names are never accepted.
- A declaration is a single case-insensitive CSV string. Values are trimmed, deduplicated, and normalized to catalog order. The exclusive `none` sentinel represents a complete empty declaration; absence of the option continues to mean that Smart Search should run its configured intent router.
- Local deterministic rules remain authoritative. The final required capabilities are the union of rules and the caller declaration, so a caller cannot suppress known-URL, strict-validation, or other rule-confirmed needs. A declaration intentionally bypasses embeddings and classifier routing and is not reported as degradation.
- Explicit caller requirements execute even under fast validation. Existing provider selection and same-capability fallback remain owned by Smart Search, and the behavior of parallel extra sources is unchanged.
- The route diagnostic exposes the same caller-aware routing decision without becoming a required preflight for search. An explicit router-mode override cannot be combined with a Caller Capability Declaration because the two inputs compete for routing control.
- The Intent Routing Catalog is the single runtime owner of capability identity and order, selection and exclusion semantics, rule terms, and semantic examples. Routing algorithms, URL recognition, threshold and margin handling, merge policy, provider registries, and fallback control flow remain code-owned rather than becoming a data-driven rules language.
- The public and packaged Skill capability reference is generated from the Intent Routing Catalog. Agent-authored ordinary search commands always include a Caller Capability Declaration; offline deep plans also emit declarations on their generated search steps. The research executor does not accept the declaration.
- Direct CLI classifier routing remains available when no caller declaration exists. Its prompt is generated from the Intent Routing Catalog, treats rule and embedding results as non-authoritative evidence, returns a complete Classifier Capability Decision, preserves the existing structured output fields, and cannot select providers.
- Classifier model fallback is not added. A successful HTTP response with invalid classifier content continues to degrade to the remaining router results, and the existing single classifier configuration remains unchanged.

## Consequences

Agent-driven ordinary searches avoid a redundant semantic-router call and make their capability choice explicit in existing routing diagnostics, while human CLI usage remains backward compatible. Maintainers update capability selection knowledge once in the runtime catalog and regenerate the Skill contract, but must preserve the seam between capability selection and provider execution; generated Skill drift becomes a release-check failure.
