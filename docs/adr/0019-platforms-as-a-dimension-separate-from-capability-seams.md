# Platforms are a dimension separate from Capability Seams

forager adds **Platform** as a second dimension beside the Capability Seam. A Platform is a built-in external content source with its own identity space, such as arXiv. Each Platform is structurally like a seam: its **Platform Routes** are providers, and the routes of one Platform form a fallback chain in the order that `platforms.<id>.order` sets. A result never falls back to another Platform. Users and agents reach a Platform through `forager platform <id> <op>`; every Platform provides `search` and `fetch`, and its options are typed flags.

## Why a Platform is not a Vertical Search provider

Vertical Search is a fallback chain in which the first accepted result ends the chain. A request for a Platform names the Platform as the requirement ("papers on arXiv", "this post and its replies"). If Platforms were Vertical Search providers, they would replace each other by chain order, and a paper search could return a Reddit post. Platforms also have their own identity spaces (arXiv IDs and versions, post IDs) and their own parameters (categories, submission dates) that the shared Vertical Search request cannot express. Vertical Search therefore stays a domain-based discovery capability in which the provider selects the sources.

## Only built-in Platforms

forager supports only Platforms and routes that ship with a release. Configuration cannot define a Platform or a generic MCP or CLI route. Each Platform and route is registered once in the catalog, and configuration defaults, validation, construction, doctor probes, smoke cases, and a consistency test all derive from that registration. User-defined routes would bypass these registration points and the typed options, so a new Platform or route is a code change that follows `docs/spec/forager/07-platforms.md`.

## Consequences

- A route is a provider, so it reuses provider identity, the provider configuration section, doctor, and smoke. A route ID never uses the bare Platform name (`arxiv_api`, not `arxiv`).
- A route that needs no credentials has no `keys` leaf and is always configured (ADR 0005).
- The registration points are spread over several modules and the type system cannot find a missing one. The new-platform checklist test reports each missing point and names the checklist item in the platform specification.
