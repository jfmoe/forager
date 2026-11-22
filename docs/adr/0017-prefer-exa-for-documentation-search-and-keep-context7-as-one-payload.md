# Prefer Exa over Context7 for automatic Documentation Search

The default Documentation Search order changes from Context7 → Exa to Exa → Context7, and research shares each subquestion's discovery limit across the capabilities that subquestion declares. Context7 stays a supported provider and its direct `forager context7` commands are unchanged. It is now called automatically only when Exa returns no consumable candidate, or when a user configures a different order.

The evidence is the measured comparison in `docs/research/context7-necessity-evaluation.md`, based on 14 queries checked against official pages and the Search Result Journal from 2026-08-26 to 2026-09-25:

- Key-fact coverage was 48% for Context7 and 93% for Exa, with median latency of 8.2 s and 2.4 s. On vendor platforms, data APIs, and app manuals, Context7 covered 25% and Exa 100%.
- Context7's `resolve-library-id` almost always returns some library, so the previous chain never reached Exa: 0 of 233 journaled Documentation Search attempts did. In research, 42% of Context7 attempts were rejected as thin, and some accepted bodies came from the wrong library.
- Research kept each subquestion's first `discovery_limit` candidates in arrival order. Documentation Search was discovered before Web Search, so under the quick budget its candidates filled every slot and no web candidate was ever fetched. In the evaluation, one sampled run ended with zero evidence because of this, and another converged on a marketing page.

## Consequences

- Ordinary search with `docs_search` now returns Exa URL candidates, which a caller can fetch directly, instead of Context7 library locators that need a second `context7 docs` call. Research docs candidates are therefore usually URLs read through Web Fetch rather than Context7 query-docs bodies.
- Within one subquestion, discovery candidates are ranked round-robin across capability blocks (the first candidate of each capability, then the second of each, and so on) before the discovery limit is applied. Candidate order within one capability is preserved.
- Relevance gating of Context7 resolve results (library-name overlap, minimum snippet count) and anchor-term gating of query-docs bodies are not adopted. With Exa first, Context7 is rarely reached automatically, so these tuned heuristics would seldom apply. They should be reconsidered if journal data shows off-topic Context7 evidence again.
- The skill's capability vocabulary is unchanged. It also feeds the classifier prompt, and with Exa first, `docs_search` serves vendor API and platform documentation well, so callers need no Context7-specific declaration rules.
