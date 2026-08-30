# Implementation closure

## Decision ledger

| # | Candidate | Class | Decision | Result |
|---|---|---|---|---|
| 1 | Attempt `by_disposition` aggregate | Code quality | Accepted | Deleted unused output and projection |
| 2 | Public `ResearchPlan::plan_version` getter | Code quality | Rejected | External Rust consumers are ambiguous; getter and integration call retained |
| 3 | Shared `successful_provider` helper | Code quality | Rejected | Two callers have different contracts; sharing enlarges the interface |
| 4 | Generic endpoint-default marker | Code quality | Rejected | Required by partial-table Serde defaults |
| 5 | Hand-written config temp lifecycle | Code quality | Accepted | Replaced with existing `tempfile` APIs |
| 6 | Duplicate provider names and fetch constructors | Code quality | Accepted | Folded into `ProviderId` and one constructor |
| 7 | Undeclared MSRV plus static restatement test | Code quality | Accepted | Removed metadata and brittle test |
| 8 | Duplicate capability-vocabulary identity test | Code quality | Accepted | Removed redundant traversal |

All accepted work is structural simplification. No candidate is classified as a
bug fix, and no rare-scenario defensive branch was added. Existing security and
resource-lifecycle behavior at filesystem, network, credential, process, and
serialization boundaries was retained.

## Complexity result

The audit-specific implementation diff touches code, test, and manifest files.
The additions are replacement glue around existing `tempfile` and typed provider
identities; they do not introduce a new abstraction.

Complexity decreased in four observable dimensions:

- fewer duplicate representations of provider identity;
- less owned resource-lifecycle machinery;
- fewer tests that restate implementation or metadata instead of behavior.

## Verification ledger

| Check | Status | Evidence |
|---|---|---|
| Rust formatting | Passed | `cargo fmt --all -- --check` |
| Patch whitespace | Passed | `git diff --check HEAD` |
| Targeted Rust tests | Passed | `cargo test --locked --lib --test config_commands --test config_path --test setup --test fetch --test search --test doctor --test smoke --test skill_contract --test release_scaffolding` (277 tests) |
| Reviewer re-review | Findings handled | Restored public getter and narrowed provider-name ownership claim |
| Clippy | Passed | `cargo clippy --all-targets --all-features --locked -- -D warnings` |
| Full local suite | Passed | `cargo test --all-targets --all-features --locked --no-fail-fast -- --quiet` (425 tests) |
| Cargo metadata | Passed | `cargo metadata --locked --format-version 1 --no-deps` |
| Live provider E2E | Passed | `cargo run --locked --quiet -- smoke --live --timeout 600` (19 passed; 0 failed, deferred, or unconfigured; every case passed on its first attempt) |
| Common search behavior and journal audit | Passed | Ordinary search, documentation search, and classifier-planned quick research matched current contracts and comparable local journals; agent-layer evidence recovery also completed |

## Baseline and scope

The complete `HEAD` plus staged, unstaged, and untracked working tree was reviewed.

## Re-review finding disposition

- Rust core P2: accepted. `ResearchPlan::plan_version` and its external integration
  call were restored; the candidate is now explicitly rejected because public Rust
  consumers are ambiguous.
- Provider/config low: accepted. The report now limits `ProviderId` ownership to
  registry and shared adapters; provider-specific transports retain local protocol
  labels without creating a reverse dependency.
- Tests/release: no finding.
