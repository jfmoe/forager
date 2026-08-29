# Provider and configuration simplification audit

## Scope

Reviewed the complete working tree across configuration loading/editing, provider
registration, network boundaries, credential pools, doctor, smoke, and integration
tests. The audit treated filesystem permissions and atomic replacement as required
behavior, not removable defensive code.

## Accepted: replace hand-written temporary-file lifecycle

- Owner and design surface: `config::edit` atomic writes and
  `config::location` writability probes.
- Production consumers: config set/unset, setup, and default path resolution use
  these paths. Their atomicity, same-directory placement, cleanup, and private
  permissions remain required.
- Non-production consumers: config command, path, and setup tests exercise
  permissions, no-clobber creation, cleanup, lock ownership, and failure behavior.
- Ambiguous consumers: setup and smoke paths were inspected and classified as
  production-facing commands.
- Change: replaced the custom create/write/flush/sync/rename/remove sequence with
  the already-used `tempfile::NamedTempFile`, `persist`, `persist_noclobber`, and
  `close` lifecycle. Private permissions are applied before secrets are written and
  again after commit.
- Abandoned capability: none. The dependency covers the same-directory temporary
  file, no-clobber persistence, and cleanup semantics previously maintained here.
- Net impact: 28 production lines removed across the two modules, with no new
  dependency. Cleanup and failure ownership move to a maintained library API.
- Risk and reintroduction: custom lifecycle code is warranted only if a required
  platform semantic cannot be represented by `tempfile`. The current permission,
  atomicity, and cleanup tests are the re-evaluation gate.
- Acceptance: config command, config path, and setup integration tests pass,
  including permission and temporary-file cleanup cases.

## Accepted: centralize registry and shared-adapter names in `ProviderId`

- Owner and design surface: provider registry, supplemental search, web-fetch
  construction, app doctor wiring, and smoke projections.
- Production consumers: registry construction, credential lookup, supplemental
  search, shared web-fetch output, capability checks, doctor, and smoke use the
  canonical registry ID names. Provider-specific transports still own protocol
  labels locally.
- Non-production consumers: registry and provider integration tests compare the
  same public names.
- Ambiguous consumers: smoke projections were checked and use the canonical names,
  not independent aliases.
- Change: deleted `ProviderRegistration.name`, routed registry and shared-adapter
  projections through `ProviderId::name`, stored `ProviderId` in supplemental
  search, and collapsed the Jina/Tavily/Firecrawl web-fetch wrappers into one
  constructor.
- Abandoned capability: registry entries can no longer assign a name that diverges
  from their ID. No current contract or consumer supports such aliases.
- Net impact: 51 production lines removed across provider modules and their app/
  smoke call sites; one source of provider identity replaces several synchronized
  string representations.
- Risk and reintroduction: introduce a distinct display or protocol name only when
  a provider genuinely has two identities. Model it as a named concept rather than
  restoring a duplicate registry field.
- Acceptance: provider registry unit tests plus fetch, search, doctor, and smoke
  integration tests pass.

## Rejected candidates

### Remove the generic endpoint-default marker

Rejected because the generic `Endpoint<D>` marker preserves provider-specific
default URLs when a partially specified provider table is deserialized. Moving all
defaults into `Providers::default` would not cover Serde's field-level defaulting
for a present table and would regress an existing configuration contract. This is
load-bearing type-level policy, not speculative generality.

### Collapse provider capability and execution seams

Rejected because registry operations, credential pools, MCP/HTTP adapters, byte
caps, deadlines, retry slices, and credential serialization all have distinct
production consumers and boundary semantics. A wrapper reduction here would move
complexity rather than delete it.
