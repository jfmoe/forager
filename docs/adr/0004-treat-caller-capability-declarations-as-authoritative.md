# Treat caller capability declarations as authoritative

## Decision

When an ordinary search carries a Caller Capability Declaration, that declaration is the complete
and only capability set for the invocation. forager does not add capabilities from local rules or
URL recognition, so an empty declaration stays empty even when the query contains a URL. Main
search always runs and is not part of the declaration, and callers name capabilities, never
providers.

A search without a declaration is an empty declaration: it runs main search and no supplemental
seam. Provider selection and same-capability fallback are unaffected. The routing decision recorded
for the invocation names the caller as its source when there is a declaration, and the main-search
default when there is none, so both the declared authority and the default are observable.

## Why

- The caller is an agent that has already read the request and the capability vocabulary; its
  decision is semantic, while keyword rules and URL matching are lower-fidelity signals.
- Merging the two in practice added capabilities the request did not need: a documentation
  request that merely asked for accurate code was also routed to an unrelated capability by a
  keyword rule. Each false capability costs provider quota and adds noise to the candidates.
- Treating the declaration as complete also removes a preliminary routing call: the agent sends
  its decision with the search instead of asking forager to route first.
- Callers are agents that the product skill teaches to declare, so an undeclared search is rare.
  Running main search alone keeps it fast and spends no supplemental quota on a need nobody
  stated; a caller that wants more declares it.

## Rejected alternatives

- **Union of local rules and the declaration** (the earlier design). It guaranteed that a
  rule-confirmed need such as a known URL could never be suppressed, at the cost of false
  capabilities on every request where a rule over-matched. The agent can declare the URL need
  itself, so the guarantee was not worth the noise.
- **A fixed supplemental default for an undeclared search**, such as always adding Supplemental
  Web Search. It spends quota on every undeclared call whether or not the request needs current
  sources, and it makes the empty declaration and no declaration behave differently.
- **A data-driven routing rules language.** Routing knowledge stays in the code-owned catalog;
  a rules language would add a second place to change behaviour with no caller that needs it.

## Revisit when

- Journal entries show agents regularly omitting a capability their request clearly needed, so
  that a rule-based safety net would recover more than it adds.

## Supersedes

- ADR-0003, which merged the caller's declaration with local rules. Its merge produced false
  capabilities; its remaining content (the declaration's shape) is restated above or owned by the
  product skill.
