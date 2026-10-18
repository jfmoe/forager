## Agent skills

### Issue tracker

Issues and specs are tracked in this repository's GitHub Issues. See `docs/agents/issue-tracker.md`.

### Triage labels

Use the five default canonical triage labels. See `docs/agents/triage-labels.md`.

### Domain docs

This repository uses a single-context domain documentation layout. See `docs/agents/domain.md`.

### Rust code quality

Before writing, modifying, or reviewing Rust, use `rust-code-quality`: read its `SKILL.md` and all relevant chapters in the same turn. During `code-review`, add it to the Standards axis without replacing its existing requirements.

### Source file size

Split source files by responsibility, not merely to satisfy a line limit. `tests/repository_rules.rs` caps Rust sources at 1000 lines and holds files already over it to their listed ceiling; raising a ceiling or listing a new file needs its reason in the commit message. Keep unit tests inline by default; when a test module overwhelms the production code, move it to a sibling test module file.

### Target hygiene

Stale build artifacts are removed by the last step of a release (`docs/agents/release.md`). Do not disable incremental compilation globally. Prefer a separate `CARGO_TARGET_DIR` for cross-compilation artifacts so host and target builds do not accumulate in one directory.

### Full local test suite

Run `cargo nextest run --all-targets --all-features --locked --no-fail-fast --status-level fail && cargo test --doc --locked` for complete local validation (install with `brew install cargo-nextest`). nextest runs the test binaries in parallel and ends with a summary that names every failed test; read failures from that summary instead of filtering the output. A test that fails under the full suite but passes alone is flaky: find its cause before closing the task.

### Windows-only CI

The `windows-permissions` job is CI-only. On non-Windows hosts, do not report a missing local run as a risk; report only an observed CI failure, and claim success only from a successful remote run.

### Merging and releasing

Merge to `main`, release, and finish a release by `docs/agents/release.md`. A release task closes only after every step of its completion section.

## Architecture

The six-group physical layering and the five-layer one-way dependency discipline are specified in `docs/spec/forager/04-architecture.md`; the rules below bind daily changes to it.

### Module placement

Every module under `src/` belongs to exactly one of the six responsibility groups (`cli/`, `core/`, `capabilities/`, `evidence/`, `infra/`, `ops/`). Place a new module by the responsibility it owns — record new owned concepts in `GLOSSARY.md` first — never by convenience. Directory depth stays at two levels.

### Dependency direction

Dependencies flow one way, top to bottom: entry → cli → orchestration (`core`/`evidence`/`ops`) → capability infrastructure (`capabilities`/`infra`) → `types`. Never add an upward edge: the Capability Catalog never depends on config, and provider adapters never import each other laterally. Cross-module sharing is `pub(crate)` and moves up to the single module that owns that responsibility.

### Public surface

`lib.rs` exposes exactly three facade modules: `app`, `config`, `types`. Everything else stays crate-private. Widening the public surface requires a spec change first, not a drive-by `pub`.

### Platforms

Before you add or change a platform or a platform route, read `docs/spec/forager/07-platforms.md`. Complete its integration checklist and keep the new-platform checklist test (`src/core/platform_checklist.rs`) passing. Before installing, changing, or live-testing an OpenCLI adapter, read `docs/agents/opencli.md`.

### Compatibility fixes

Prefer solving a compatibility problem through the general mechanism that owns the behavior over a special case for one site, URL, or sample. When a special case seems the only option, discuss it first.

### Spec-code lockstep

A change that adds, moves, or removes a module, or alters a module's ownership or dependency direction, updates `docs/spec/forager/04-architecture.md` in the same commit. `tests/repository_rules.rs` fails while a module or OpenCLI adapter file goes unnamed in that chapter; ownership and dependency claims remain the reviewer's to check. A spec claim the code no longer satisfies is corrected or deleted immediately — never left to drift.
