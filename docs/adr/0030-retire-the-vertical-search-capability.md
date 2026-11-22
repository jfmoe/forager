# Retire the vertical search capability

## Decision

forager has no vertical search capability. The capability, its only provider AnySearch, the
`anysearch` command, their configuration, the Verified Domain Manifest, and their entries in
doctor, smoke and setup are removed. The capabilities that search and research can plan are
Documentation Search, Web Search and Web Fetch. Old capability declarations, configuration keys,
environment variables and the old command are rejected as unknown input, with no alias or
compatibility layer.

## Why

The local journal from 2026-09-19 to 2026-10-18 (443 searches) and the agent session logs showed
heavy use and little contribution:

- 152 searches (34%) included vertical search, and the classifier chose it in 61 of its 92 plans.
- Its results went only into the supplementary sources, never into the main answer. It added 1.27
  sources per search on average; 38 calls returned nothing and 8 returned a "No relevant results
  found." placeholder. Of 193 results, 5 URLs appeared in the main answer.
- The results that sessions cited again were mostly site home pages such as `ssrn.com/` and
  `csindex.com.cn/`. Academic sources are covered by the arXiv, SSRN and Google Scholar platforms
  (ADR-0019).
- The domain-level search was never called in the session logs; the Verified Domain Manifest was
  always empty, so structured domain search never switched on.
- The capability ran through the CLI, configuration, the classifier, research, doctor, smoke,
  setup, release verification and the documentation, and every related change had to carry that
  whole chain.

## Rejected alternatives

- **Removing AnySearch but keeping an empty vertical search slot.** The classifier, research plans
  and documents would keep offering an option that can never be covered, and every selection would
  only produce a capability gap. Reintroducing the capability through a new ADR costs less.
- **Routing the queries it served to platforms automatically.** Documentation Search and Web
  Search now cover those queries, and platforms are called only by explicit command (ADR-0019).

## Revisit when

- A recurring vertical search need appears that neither Web Search nor the existing platforms can
  cover.
- A vertical search provider offers a stable, accepted domain-level contract whose results the main
  answer or research evidence would actually use.
