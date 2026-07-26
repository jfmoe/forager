# AnySearch Verified Domain Manifest

`assets/anysearch/verified-domain-manifest.json` is the only source of truth for Verified Vertical Domain support. Live Domain Discovery is observational data: it cannot edit the manifest, promote a domain, or make Automatic Domain Search available. Domain-less Vertical Discovery likewise exposes no final domain selection.

The manifest starts with an empty `verified_domains` collection. A future Verified Domain Contract must include separate `domain` and `sub_domain` values, an acceptance date, a canonical schema fingerprint, the accepted parameter schema, and `status=verified`. Candidate assessments are separate and use `status=discovered_unverified` with explicit gaps.

## Candidate matrix

| Candidate | Versioned fixture | Result shape | Current conclusion | Missing gates |
| --- | --- | --- | --- | --- |
| `academic.search` | discovery schema, valid request, missing `keywords`, provider error | URL result | discovered/unverified | sanitized live capture, complete independent live run, upstream stability window |
| `security.vuln` | discovery schema, valid request with `type`/`value`, missing required params, provider error | URL-less structured result | discovered/unverified | sanitized live capture, complete independent live run, upstream stability window |
| `finance.fundamental` | discovery schema, valid request with `cn_code`/`symbol`/`type`, missing required params, provider error | URL-less structured result | discovered/unverified | sanitized live capture, complete independent live run, upstream stability window |
| `code.doc` | discovery schema, valid request with `library`, missing required param, provider error | URL result | discovered/unverified | sanitized live capture, complete independent live run, upstream stability window |

The fixtures under `tests/fixtures/anysearch/` are sanitized synthetic transport fixtures, not live acceptance evidence. Their schema fingerprints are checked against the manifest in ordinary offline tests. `security.cve` is intentionally absent and has no compatibility mapping; the candidate is `security.vuln`.

## Live acceptance

The complete live contract is defined by C14–C16 in [`docs/spec/forager/05-acceptance.md`](spec/forager/05-acceptance.md): explicit `academic.search`, domain-less Vertical Discovery, and Domain Discovery. Credentials come only from `providers.anysearch.keys` and its `FORAGER_` overlay.

Live results remain observational. Promotion requires a reviewed, sanitized evidence update covering every matrix gate, including stable provider-error classification and the upstream stability/version decision.
