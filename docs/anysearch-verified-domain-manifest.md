# AnySearch Verified Domain Manifest

`assets/anysearch/verified-domain-manifest.json` is the only source of truth for Verified Vertical Domain support. Live Domain Discovery is observational data: it cannot edit the manifest, promote a domain, or make Automatic Domain Search available. Domain-less Vertical Discovery likewise exposes no final domain selection.

The manifest starts with an empty `verified_domains` collection. A future Verified Domain Contract must include separate `domain` and `sub_domain` values, an acceptance date, a canonical schema fingerprint, the accepted parameter schema, and `status=verified`. Candidate assessments are separate and use `status=discovered_unverified` with explicit gaps.

## Candidate matrix

| Candidate | Versioned fixture | Result shape | Current conclusion | Missing gates |
| --- | --- | --- | --- | --- |
| `academic.search` | discovery schema | Unverified | discovered/unverified | sanitized live capture, valid and invalid request evidence, provider error, result shape, upstream stability window |
| `security.vuln` | discovery schema | Unverified | discovered/unverified | sanitized live capture, valid and invalid request evidence, provider error, result shape, upstream stability window |
| `finance.fundamental` | discovery schema | Unverified | discovered/unverified | sanitized live capture, valid and invalid request evidence, provider error, result shape, upstream stability window |
| `code.doc` | discovery schema | Unverified | discovered/unverified | sanitized live capture, valid and invalid request evidence, provider error, result shape, upstream stability window |

The fixtures under `tests/fixtures/anysearch/` are versioned synthetic schema artifacts, not transport fixtures or live acceptance evidence. Candidate schema fingerprints only verify that these artifacts have not drifted; they are not runtime request-validation fingerprints. Because the manifest has no verified entry, the current runtime performs no Verified Domain schema validation and passes explicit unverified-domain parameters through unchanged. `security.cve` is intentionally absent and has no compatibility mapping; the candidate is `security.vuln`.

## Live acceptance

The complete live contract is defined by C14–C16 in [`docs/spec/forager/05-acceptance.md`](spec/forager/05-acceptance.md): explicit `academic.search`, domain-less Vertical Discovery, and Domain Discovery. Credentials come only from `providers.anysearch.keys` and its `FORAGER_` overlay.

Live results remain observational. The first promotion must atomically deliver reviewed, sanitized live evidence covering every matrix gate, a formal verified manifest entry, its canonical schema fingerprint, the runtime validator, and acceptance coverage. This includes stable provider-error classification and the upstream stability/version decision.
