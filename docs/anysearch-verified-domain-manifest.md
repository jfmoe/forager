# AnySearch Verified Domain Manifest

The version-controlled Verified Domain Manifest is the only source of truth for Verified Vertical Domain support. Live Domain Discovery and domain-less Vertical Discovery are observational data: neither can edit the manifest, promote a domain, or make the Vertical Search Capability ready.

The manifest starts with an empty `verified_domains` collection. A future Verified Domain Contract must include separate `domain` and `sub_domain` values, an acceptance date, a canonical schema fingerprint, the accepted parameter schema, and `status=verified`. Candidate assessments are separate and use `status=discovered_unverified` with explicit gaps.

## First candidate matrix

| Candidate | Versioned mock evidence | Result shape | Current conclusion | Missing gates |
| --- | --- | --- | --- | --- |
| `academic.search` | discovery schema, valid request, missing `keywords`, provider error | URL result | discovered/unverified | sanitized live capture, complete independent live run, upstream stability window |
| `security.vuln` | discovery schema, valid request with `type`/`value`, missing required params, provider error | URL-less structured result | discovered/unverified | sanitized live capture, complete independent live run, upstream stability window |
| `finance.fundamental` | discovery schema, valid request with `cn_code`/`symbol`/`type`, missing required params, provider error | URL-less structured result | discovered/unverified | sanitized live capture, complete independent live run, upstream stability window |
| `code.doc` | discovery schema, valid request with `library`, missing required param, provider error | URL result | discovered/unverified | sanitized live capture, complete independent live run, upstream stability window |

The fixtures under `tests/fixtures/anysearch/` are sanitized synthetic transport fixtures, not live acceptance evidence. Their schema fingerprints are checked against the manifest in ordinary offline CI. `security.cve` is intentionally absent and rejected locally without a compatibility mapping; the candidate is `security.vuln`.

## Live acceptance

The authoritative live acceptance cases are [`C14`–`C16`](spec/forager/05-acceptance.md):

- C14 exercises `academic.search` with an explicit domain and sub-domain.
- C15 exercises domain-less Vertical Discovery.
- C16 exercises Domain Discovery.

Live checks use the unified provider `keys` configuration and `FORAGER_` environment overlay. Ordinary CI carries no live credentials. A live result is observational and cannot implement Automatic Domain Search or promote the manifest.

Promotion requires a reviewed, sanitized evidence update covering every matrix gate, including valid and invalid requests, stable provider-error classification, result shape, and the upstream stability or version decision.
