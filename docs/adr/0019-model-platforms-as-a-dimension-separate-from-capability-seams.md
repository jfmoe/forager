# Model platforms as a dimension separate from capability seams

## Decision

A **Platform** is a built-in external content source with its own identity space, such as arXiv,
and it is a dimension beside the Capability Seam, not a provider of one. Its **Platform Routes**
are providers: they reuse provider identity, the provider configuration section, the credential
pool, doctor and smoke, and the routes of one platform form a fallback chain in the order the
user configures. A result never falls back to another platform. Users and agents reach a platform
only through an explicit `forager platform <id> <op>` command; every platform provides search and
fetch, and its options are typed flags.

forager supports only the platforms and routes that ship with a release. Configuration cannot
define a platform or a generic route. Each platform and route is registered once in the catalog,
and configuration defaults, validation, construction, doctor probes, smoke cases and a
consistency test derive from that registration. A new platform or route is a code change: the
compiler names every dispatch site it must handle, and the platform checklist test checks what the
compiler cannot, such as whether the facts declared in separate places agree.

## Why

- A request for a platform names the platform as the requirement: "papers on arXiv", "this post
  and its replies". A seam's chain lets one provider replace another by order, so as seam
  providers a paper search could come back as a Reddit post.
- Platforms have their own identity spaces (arXiv IDs and versions, post IDs) and their own
  parameters (categories, submission dates) that a shared seam request cannot express.
- User-defined routes would bypass the registration points and the typed options, which are what
  keep configuration checks, doctor and smoke in step with the code. Exhaustive matches make a
  missed dispatch site a compile error, but they cannot tell whether a route landed in the right
  case or whether the catalog, registration, declaration and configuration schema agree, so the
  checklist test checks those.

## Rejected alternatives

- **Platforms as providers of a discovery seam.** The seam's first accepted result ends the chain,
  so platforms would replace each other, and the shared request would lose each platform's
  identity and options.
- **Letting search route to a platform automatically.** A platform is
  chosen by the caller who names it; automatic routing would be a separate decision with its own
  evidence, and none has been made.
- **User-defined platforms or generic MCP and CLI routes in configuration.** They would skip the
  registration that derives validation, doctor and smoke, and could not carry typed options.
- **Time windows and sort order as shared platform parameters.** Their meaning differs from
  platform to platform, so each platform declares its own; the shared layer is the query and limit
  for search and the ref and depth for fetch.

## Revisit when

- A content source is needed that a release cannot ship as a built-in platform, and enough users
  need it to justify a stable extension contract.
- A recurring query need appears that only automatic routing to a platform would serve.
