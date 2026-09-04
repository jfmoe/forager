## Agent skills

### Issue tracker

Issues and PRDs are tracked in this repository's GitHub Issues. See `docs/agents/issue-tracker.md`.

### Triage labels

Use the five default canonical triage labels. See `docs/agents/triage-labels.md`.

### Domain docs

This repository uses a single-context domain documentation layout. See `docs/agents/domain.md`.

### Rust code quality

Before writing, modifying, or reviewing Rust, use `rust-code-quality`: read its `SKILL.md` and all relevant chapters in the same turn. During `code-review`, add it to the Standards axis without replacing its existing requirements.

### Full local test suite

Run `cargo test --all-targets --all-features --locked --no-fail-fast -- --quiet` for complete local validation. `--no-fail-fast` continues running remaining targets but any failure still makes the command fail; `-- --quiet` reduces successful-test output while preserving failure details.

### Windows-only CI

The `windows-permissions` job is CI-only. On non-Windows hosts, do not report a missing local run as a risk; report only an observed CI failure, and claim success only from a successful remote run.

### Release completion

After publishing a release, update the local CLI, run `npx skills add jfmoe/forager -g -s forager -a '*' -y`, then run the live-provider end-to-end test before closing the task.

## Architecture

The six-group physical layering and the five-layer one-way dependency discipline are specified in `docs/spec/forager/04-architecture.md`; the rules below bind daily changes to it.

### Module placement

Every module under `src/` belongs to exactly one of the six responsibility groups (`cli/`, `core/`, `capabilities/`, `evidence/`, `infra/`, `ops/`). Place a new module by the responsibility it owns — record new owned concepts in `CONTEXT.md` first — never by convenience. Directory depth stays at two levels.

### Dependency direction

Dependencies flow one way, top to bottom: entry → cli → orchestration (`core`/`evidence`/`ops`) → capability infrastructure (`capabilities`/`infra`) → `types`. Never add an upward edge: the Capability Catalog never depends on config, and provider adapters never import each other laterally. Cross-module sharing is `pub(crate)` and moves up to the single module that owns that responsibility.

### Public surface

`lib.rs` exposes exactly three facade modules: `app`, `config`, `types`. Everything else stays crate-private. Widening the public surface requires a spec change first, not a drive-by `pub`.

### Spec-code lockstep

A change that adds, moves, or removes a module, or alters a module's ownership or dependency direction, updates `docs/spec/forager/04-architecture.md` in the same commit. A spec claim the code no longer satisfies is corrected or deleted immediately — never left to drift.
