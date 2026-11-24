# Architecture

This document covers what no single file shows: how the code is layered, which way dependencies point, how a request moves through the layers, the shape of the error model, where the system is meant to be extended, and the question each test layer answers. It stops at module groups. For anything finer, read the group that owns it.

## Priorities

When design goals conflict, they rank in this order: distribution and startup experience, then type safety, then architectural stability under long-running agent-driven change. Runtime throughput is not a goal.

## Shape

The project is one crate with a binary and a library. The library exposes exactly three facade modules, `app`, `config` and `types`; every other module is private to the crate. The binary parses arguments, renders output and maps errors to exit codes. Output formatting stays on the binary side.

`src/` holds six groups, each owning one kind of responsibility:

- **`cli/`**: the argument tree, command dispatch and the `app` facade. Dispatch runs the browser preflight check and writes the journal at the end of a command.
- **`core/`**: orchestration of one invocation. This covers the search fan-out, the shared chain executor, route planning for each platform operation, and attempt attribution.
- **`capabilities/`**: everything that reaches a provider. This covers the Capability Catalog and platform catalog, the HTTP boundary, credential pools, Access Policies, and the provider and route adapters, including the browser transport.
- **`evidence/`**: what an invocation leaves behind: the Search Result Journal and the stderr attempt log.
- **`infra/`**: shared foundations: configuration, the shared types, redaction, secure file writing and cross-process state files.
- **`ops/`**: operator commands outside a search: doctor, the Live Check runner behind `doctor --live`, and browser install and login.

## Dependency direction

Dependencies point one way, from top to bottom:

```
entry (binary)
  → cli
    → orchestration: core, evidence, ops
      → capability infrastructure: capabilities, infra
        → types
```

Nothing depends on a layer above it. When two modules need the same thing, it moves to the single module that owns that responsibility and is shared as `pub(crate)`. The rules below are enforced by review only; no test checks them:

- The Capability Catalog depends neither on configuration nor on provider adapters. Access Policies do not depend on the catalog.
- Provider adapters never import each other. A Platform Route never imports a capability provider.
- The browser transport, the shared Responses transport and the transcript formatting depend on no route adapter.
- A route adapter builds requests, decodes responses and declares where the full text lives. It does not plan, fall back or retry across routes.
- `types` is the base. It does no IO and is the single definition point of every shape that crosses a layer. Parsing and validating refs, requests and options are pure functions there, and `types` does not depend on the argument parser.
- The HTTP boundary owns the only HTTP client. Status codes become error kinds there, once; a route may only add a reason hook.
- Configuration checks, preflight checks, doctor, the Live Check runner and the platform checklist branch on a route's Route Transport (HTTP or browser), never on its provider ID. A browser route declares its browser environment in its registration (ADR-0036).

## Providers and platforms

Each Capability Seam has one trait with its own outcome type. A provider is one struct, and the same instance can join several chains. The provider registry is the only place that records a provider's identity, credentials, transport, Access Policy, per-key account check and Live Checks.

A Platform is a dimension beside the Capability Seams, not another seam (ADR-0019). A Platform Route is a provider, and one route can serve both a seam and a platform. The platform catalog is the only source of which routes belong to which platform, and it declares the routes of every Platform Operation: search, fetch, and each operation of a platform's own. A platform can have several routes that split its evidence into layers; one layer never falls back to another (ADR-0028).

Each Platform Route is declared once, in its own adapter: how it is built from its runtime configuration, the request checks planning runs before it chooses the route, and what doctor and the browser login add for it. The built route hands out its own seam implementations, extra endpoint checks and per-key account checks. The provider factory reaches a route only through that declaration, with one lookup by route configuration and one by provider identity.

Every module that dispatches on the provider identity, on a configuration keyed by it, or on a paged operation names each case in its matches, and a lint in each such module keeps it so. The runtime configuration holds one named field per route. A provider or paged operation that a dispatch site misses is therefore a compile error.

## How a request flows

**Search.** The caller's capability declaration is authoritative, and a search without one runs main search only (ADR-0004). Main search and the supporting capabilities then fan out inside one deadline (ADR-0016). Their attempts are merged, and dispatch writes one journal record at the terminal state (ADR-0002).

**A platform operation.**

1. Dispatch runs the browser preflight check when a browser route could serve the request.
2. Planning is pure: the usable routes are the configured order, intersected with the operation's route set in the platform catalog and with the configured routes. An operation that one route serves gets that route's own configuration, or a configuration error when the order leaves the route out or it is unconfigured.
3. The shared chain executor runs them. An empty result is a legitimate outcome, not a failure. A request never falls back to another platform.
4. Platform fetch has two stages. The route returns metadata and declares where the full text lives; orchestration then reads it through the global Web Fetch chain, or uses the native body the route already read (ADR-0022).
5. A model operation reads what it needs from the platform during orchestration first. A model route never reads the platform itself.

## State

Only state that separate processes must share is written to disk: credential cursors, Access Policy reservations and the Browser Profile lock, through one shared state-file mechanism. Short-lived policy stays in the process. A browser command takes its route's access permit before it locks the profile.

## Browser transport

The browser transport knows no site; all site knowledge is in a route's Page Logic. Page Logic sends no request of its own: it reads the requests the page makes and the page state through read-only scripts, and drives the page with native input (ADR-0036).

## Error model

Error kinds fall into two families, transport and content. Configuration errors belong to neither, because they occur only before any provider runs. Exit codes follow the phase and the family:

- before any request: argument errors exit 2, configuration errors exit 3;
- after requests: transport failures exit 4, content failures exit 5.

Attribution stays inside one chain. It takes each provider's final attempt and picks the highest by one total order of kinds, in which every content kind ranks above every transport kind. The order is defined once, in `core/`.

Invariants that hold for every command:

- An invocation reaches exactly one terminal state.
- The exit code is chosen by the error family.
- A failure in side work (the journal, a credential cursor, the `--output` copy) does not change the main terminal state, except that an explicitly requested output file that cannot be written exits 3.
- Success and failure payloads each have a bound.
- A caller's capability declaration is authoritative.

## Extension points

- **A provider** is registered once, in the provider registry, with its Live Checks, and needs its configuration section, leaves and runtime configuration. The compiler then names each match in the provider factory and the runtime configuration that must place it.
- **A platform or route** needs a catalog entry, its type leaf, the route adapter with its one route declaration, a provider registration, and its configuration section and runtime configuration. The compiler names the factory's two lookups and each projection of the runtime configuration, but cannot tell whether a route was placed in the right case. The platform checklist test checks what the compiler cannot, such as whether facts declared in separate places agree, and whether the platform has sample refs, an access-window test and, for each operation, a route that declares a Live Check. Run it; each failure names the rule it breaks and what to add.
- **A platform operation served by one route** is an inherent method on that route. The platform catalog lists the operation and its route; the route's module declares how the operation reaches that method; `core/` declares the operation and shapes its result, and the one orchestration entry that every such operation shares plans and runs it. When a second route appears, the operation becomes a trait.
- **A command** is placed by kind:
  - the bare verb is the smart pipeline;
  - an operation with a fallback chain is a top-level verb;
  - a command that names a provider is nested under that provider and bypasses routing;
  - a platform command is `platform <id> <op>`.

## Test layers

The test layers are ordered from the library outward to real sites. Each one faces a different part of the system, and each step outward costs more time and sees less detail, so a rule sits at the innermost layer that sees it break; the testing procedure places each test.

- **In-crate tests** face the library, every module group below the binary, in process. They see forager's own logic against stored upstream samples and a local server, and nothing that only the binary or a process adds.
- **Binary unit tests** face the binary's presentation, in process. They see how a result or a failure leaves the binary, and nothing that takes a process.
- **Command contract tests** face the binary from outside, as one test binary that runs it against local stand-ins. They see what every command shares at the process boundary and that each command's arguments and result pass through it, and not the detail beneath, which they reach through only a few samples.
- **Rule tests** face the repository's documents and layout, and see nothing of forager's behaviour.
- **Release and permission tests** face what CI and a release rely on: the workflows, the release configuration and scripts, and private files on each operating system. They do not see a published release.
- **Real-browser tests** face the browser transport and Page Logic inside the Managed Browser, on local pages, and stay off by default.
- **Live Checks** face real sites and keys through `forager doctor --live`, as canaries of each provider's operations, run by hand after a release and after a change to an adapter. Each declares a Live Tier, which decides whether it may run without a person present. They are the only layer that reaches an upstream; every other layer trusts the samples as they were observed.

A test that spawns the binary goes through the shared test environment, which sends its HTTP through a proxy on a closed port, so a route its configuration missed fails instead of reaching a site. An in-crate test, one compiled into the crate under the test build, gets its HTTP client from the HTTP boundary, which under that build ignores proxy settings and refuses any host that is not loopback; its default state location and default journal directory resolve to a temporary directory owned by the test process. The command contract tests link the library without the test build, so these guarantees do not exist in them; they send no request through the HTTP boundary themselves, only to local fixtures.
