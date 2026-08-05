# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- separate search-side Web Fetch previews and Vertical Discovery results, and remove `validation_results` ([#101](https://github.com/jfmoe/forager/issues/101))

## [0.1.2](https://github.com/jfmoe/forager/compare/v0.1.1...v0.1.2) - 2026-08-02

### Added

- parallelize intra-stage fan-outs ([#84](https://github.com/jfmoe/forager/pull/84))
- cap response bodies and narrow redaction ([#81](https://github.com/jfmoe/forager/pull/81))
- prioritize primary search timeout budget ([#80](https://github.com/jfmoe/forager/pull/80))
- protect provider credentials with Secret type ([#78](https://github.com/jfmoe/forager/pull/78))

### Fixed

- *(smoke)* require statuspage outage JSON ([#75](https://github.com/jfmoe/forager/pull/75))
- redact provider error URLs
- *(openai)* reject truncated SSE responses
- unify private permission enforcement ([#63](https://github.com/jfmoe/forager/pull/63))
- *(config)* serialize configuration writes
- *(smoke)* avoid polling and journal pollution ([#61](https://github.com/jfmoe/forager/pull/61))
- *(config)* make endpoint URL defaults intrinsic
- *(anysearch)* cache verified domain manifest ([#57](https://github.com/jfmoe/forager/pull/57))
- implement Error for ResearchError ([#56](https://github.com/jfmoe/forager/pull/56))
- configure shared HTTP client ([#53](https://github.com/jfmoe/forager/pull/53))
- bound CI jobs and fixture accepts ([#52](https://github.com/jfmoe/forager/pull/52))
- isolate journal writes from cleanup failures ([#51](https://github.com/jfmoe/forager/pull/51))
- report research summary write failures ([#50](https://github.com/jfmoe/forager/pull/50))
- prevent retry duration overflow ([#49](https://github.com/jfmoe/forager/pull/49))
- *(net)* rotate credentials after HTTP 402

### Other

- clarify Windows CI verification
- complete public type documentation ([#70](https://github.com/jfmoe/forager/pull/70))
- *(providers)* unify execution loops ([#83](https://github.com/jfmoe/forager/pull/83))
- unify provider seam chains ([#82](https://github.com/jfmoe/forager/pull/82))
- stabilize response stall timeout ([#81](https://github.com/jfmoe/forager/pull/81))
- drive config leaves from schema ([#79](https://github.com/jfmoe/forager/pull/79))
- split configuration modules
- authenticate release-plz with GitHub App ([#76](https://github.com/jfmoe/forager/pull/76))
- consolidate HTTP fixtures ([#74](https://github.com/jfmoe/forager/pull/74))
- enforce clippy pedantic policy ([#73](https://github.com/jfmoe/forager/pull/73))
- consolidate pull request gates ([#72](https://github.com/jfmoe/forager/pull/72))
- cover journal persistence and cleanup ([#69](https://github.com/jfmoe/forager/pull/69))
- record intra-stage search concurrency decision ([#43](https://github.com/jfmoe/forager/pull/43)) ([#66](https://github.com/jfmoe/forager/pull/66))
- record primary-first timeout budget ADR and skill guidance ([#65](https://github.com/jfmoe/forager/pull/65))
- record output security architecture decision ([#41](https://github.com/jfmoe/forager/pull/41))
- validate acceptance tests against Cargo registry ([#60](https://github.com/jfmoe/forager/pull/60))
- remove duplicate transport fixture manifest ([#59](https://github.com/jfmoe/forager/pull/59))
- consolidate shared utility functions
- remove redundant classifier clones
- document installation and distribution
- require manual release dispatch
- record smartsearch archive
- record retirement cutover ([#23](https://github.com/jfmoe/forager/pull/23))
- open v0.1.1 switch gate
- record all L0 deep probes
- record v0.1.1 switch gate evidence

## [0.1.1](https://github.com/jfmoe/forager/compare/v0.1.0...v0.1.1) - 2026-07-27

### Fixed

- cover every research subquestion ([#26](https://github.com/jfmoe/forager/pull/26))
- resolve release assets outside a checkout
- make smoke test fixtures portable on Windows ([#26](https://github.com/jfmoe/forager/pull/26))

## [0.1.0](https://github.com/jfmoe/forager/releases/tag/v0.1.0) - 2026-07-27

### Added

- gate clean release assets ([#20](https://github.com/jfmoe/forager/pull/20))
- split forager skill workflows
- align model prompts and agent contracts
- deliver installable forager skill ([#21](https://github.com/jfmoe/forager/pull/21))
- add live smoke acceptance matrix ([#18](https://github.com/jfmoe/forager/pull/18))
- add offline smoke readiness check ([#17](https://github.com/jfmoe/forager/pull/17))
- add registry-backed provider doctor
- generate plans for bare research ([#15](https://github.com/jfmoe/forager/pull/15))
- execute caller research plans
- drive bare search with classifier
- honor caller capability declarations
- add OpenAI-compatible search fallback ([#11](https://github.com/jfmoe/forager/pull/11))
- complete xai default search invocation ([#10](https://github.com/jfmoe/forager/pull/10))
- add Tavily site mapping
- add web fetch capability
- deliver AnySearch acceptance surface
- add Context7 documentation search
- *(exa)* add similar page discovery
- add Exa search network runtime ([#4](https://github.com/jfmoe/forager/pull/4))
- add incremental setup wizard ([#3](https://github.com/jfmoe/forager/pull/3))
- add strict editable configuration ([#2](https://github.com/jfmoe/forager/pull/2))
- establish config path and release scaffolding ([#1](https://github.com/jfmoe/forager/pull/1))

### Fixed

- cap Tavily map provider timeout
- decode live AnySearch domain contracts
- make live classifier and research probes reliable
- match live provider protocol shapes

### Other

- stop freezing skill prompt prose
- seal tier zero and tier one regression gates ([#19](https://github.com/jfmoe/forager/pull/19))
- configure engineering agent skills
- Add Rust formatting and lint components
- Initial forager scaffold
# Changelog

All notable changes to forager will be recorded in this file.
