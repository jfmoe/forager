# Keep citation binding inline without a new output schema

Forager will represent Citation Binding in answer text rather than add `claims`, `citation_bindings`, or another parallel JSON structure. Main search preserves provider-generated inline citations such as xAI's `[[N]](URL)` unchanged; when a provider returns no inline citation, the result remains successful without a binding status, forced fallback, or rewritten answer. This avoids duplicating information already present in the answer and keeps providers without positional citation metadata usable.

The Research Evidence Pipeline continues to deliver evidence rather than synthesize an answer. Its skill-layer consumer must cite evidence inline as `[eN](URL)`, using the corresponding `evidence_items[].id` and URL. Forager does not rewrite or validate the consumer's final answer. Citation Binding records attribution only, not proof that the cited content supports a claim, so the existing risk-based fetch, verification, and disclosure rules remain unchanged.

## Consequences

- JSON, Markdown, and content output gain no new binding fields or rendering rules.
- The output-contract work following this decision may remove redundant fields without reserving a claim-binding structure.
- Hard enforcement, semantic claim verification, and normalization of missing provider citations remain outside the CLI contract.
