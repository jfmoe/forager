---
status: accepted
---

# Treat caller capability declarations as authoritative

A Caller Capability Declaration is the complete and authoritative capability set for any ordinary search that supplies one. forager does not supplement it from local rules, URL recognition, or the classifier; `none` therefore remains empty even when the query contains a URL. Calls without a declaration use the configured classifier or its specified degradation path, while provider selection, same-capability fallback, unconditional main search, and the separate research evidence workflow remain unchanged.

This supersedes ADR-0003 because merging lower-fidelity keyword rules into an Agent's complete semantic decision caused false capabilities, including Vertical Search for documentation requests that merely asked for accurate code. The routing decision records the caller as the only decision source for this path, making the declared authority observable rather than presenting unused inputs as decision sources.

Research plan injection is the first authoritative definition inside this ADR's research exemption. Its rules are specified in [`docs/spec/forager/02-cli.md`](../spec/forager/02-cli.md#权威规则补充决议-a1经-57-r2-收窄) and coexist with this decision's ordinary-search semantics.
