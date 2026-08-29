# Tests, JavaScript, and release simplification audit

## Scope

Reviewed all repository tests, fixtures, skills, JavaScript helpers, Cargo metadata,
and release automation in the complete working tree. Product smoke paths and
release gates were classified as production or operational consumers rather than
ordinary test-only code.

## Accepted: remove an undeclared MSRV and its restatement test

- Owner and design surface: Cargo package metadata and release-scaffolding tests.
- Production consumers: Cargo parsed `rust-version`, but the specification
  explicitly makes no MSRV commitment. CI and development already use the pinned
  toolchain.
- Non-production consumers: one static test restated the manifest value and Clippy
  configuration instead of observing product behavior.
- Ambiguous consumers: release jobs were inspected; none use `rust-version` as an
  artifact gate or compatibility promise.
- Change: deleted `package.rust-version` and the static manifest/lint assertion.
- Abandoned capability: Cargo no longer rejects older compilers at manifest parse
  time. This matches the documented no-MSRV policy; compilation remains the
  compatibility signal.
- Net impact: 33 lines removed, no dependency or runtime behavior added.
- Risk and reintroduction: restore the field only when the project adopts and tests
  a documented MSRV policy.
- Acceptance: Cargo metadata resolves under `--locked`; release-scaffolding tests
  continue to verify the actual CI and release gates.

## Accepted: narrow the Kimi script module surface

- Owner and design surface: `kimi-datasource.mjs` command implementation and its
  Node tests.
- Production consumers: the executable entry path uses all retained internals.
- Non-production consumers: tests import five behavior seams:
  `tokenNeedsRefresh`, `redactSecrets`, `processToolResponse`, `invokeTool`, and
  `formatToolResult`.
- Ambiguous consumers: repository-wide searches found no package entry point or
  external script importing the other fourteen exports.
- Change: made fourteen command internals private and removed both unused stdin
  injection points; `buildCallParams` and `readStdin` now use the real process
  boundary directly.
- Abandoned capability: tests and hypothetical library consumers can no longer
  call CLI orchestration internals as a JavaScript API. The skill is shipped as a
  command, not a library.
- Net impact: the export surface falls from nineteen names to five and the unused
  dependency-injection branch is gone; production behavior and dependencies are
  unchanged.
- Risk and reintroduction: export a helper only when a tracked production consumer
  appears. Keep process-level CLI tests as the default command contract.
- Acceptance: all eight Node tests pass, including stdin JSON and refresh retry.

## Accepted: remove a third capability-vocabulary identity test

- Owner and design surface: `tests/skill_contract.rs`.
- Production consumers: runtime vocabulary loading and classifier validation remain
  unchanged.
- Non-production consumers: classifier unit tests already compare the compiled
  identity and order against the vocabulary asset; the skill contract still checks
  installable assets and the research-plan example.
- Ambiguous consumers: none.
- Change: removed the second cross-layer identity traversal from the integration
  contract test.
- Abandoned capability: no runtime capability; only duplicate failure reporting.
- Net impact: 22 dedicated test lines removed after excluding the separate
  plan-version assertion.
- Risk and reintroduction: restore an integration check only if the asset and
  runtime stop sharing the existing classifier-level contract.
- Acceptance: classifier vocabulary unit tests and both remaining skill-contract
  tests pass.

## Rejected candidates

The command watchdog, test-runner lifecycle, release target manifest and artifact
gate, Kimi invocation/result seams, acceptance manifest, provider fixtures, and
YAML parser dependency were retained. Each has a current production, operational,
or materially distinct test consumer; deleting them would remove behavior or move
complexity rather than simplify it.
