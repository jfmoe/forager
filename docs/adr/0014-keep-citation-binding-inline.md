# Keep citation binding inline

## Decision

Citation Binding lives in the answer text. forager adds no parallel claim or binding structure to
its output.

- Main search keeps a provider's inline citations in the answer unchanged. An answer without
  inline citations is still a success: there is no binding status, no forced fallback and no
  rewritten answer.
- The research pipeline delivers evidence, not an answer. Its consumer cites evidence inline
  through the evidence item's identity, in the form the product skill's CLI reference defines.
  forager neither rewrites nor validates the consumer's final answer.
- A binding records attribution only. It is not proof that the cited content supports the claim,
  so the skill's risk-based fetch, verification and disclosure rules still apply.

## Why

- The positional information is already in the answer; a parallel structure would duplicate it
  and could disagree with it.
- Some providers return no positional citation metadata. Requiring a binding would make them
  unusable or force a fallback that costs a second search for the same answer.
- Validating that a cited page supports a claim is semantic work that a deterministic CLI cannot
  do reliably; pretending to would give the caller false confidence.

## Rejected alternatives

- **A `claims` or bindings array in JSON output.** It duplicates the answer's own citations and
  needs rendering rules in every format.
- **Falling back or rewriting when a provider returns no inline citations.** It spends another
  search to restyle an answer that is already usable.
- **Verifying claims against cited content in the CLI.** Semantic verification belongs to the
  agent that writes the answer.

## Revisit when

- A consumer needs machine-readable claim-to-source links that cannot be recovered from the
  answer text.
