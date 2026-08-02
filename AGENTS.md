## Agent skills

### Issue tracker

Issues and PRDs are tracked in this repository's GitHub Issues. See `docs/agents/issue-tracker.md`.

### Triage labels

Use the five default canonical triage labels. See `docs/agents/triage-labels.md`.

### Domain docs

This repository uses a single-context domain documentation layout. See `docs/agents/domain.md`.

### Rust code quality

Before writing, modifying, or reviewing Rust, use `rust-code-quality`: read its `SKILL.md` and all relevant chapters in the same turn. During `code-review`, add it to the Standards axis without replacing its existing requirements.

### Windows-only CI

The `windows-permissions` job is CI-only. On non-Windows hosts, do not report a missing local run as a risk; report only an observed CI failure, and claim success only from a successful remote run.
