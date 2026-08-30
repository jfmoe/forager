# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.5.0](https://github.com/jfmoe/forager/compare/v0.4.0...v0.5.0) - 2026-08-30

### Other

- close code quality reviews
- *(skill)* remove Kimi datasource
- *(chain)* isolate step preparation
- pin gate/SlicedEven incompatibility in the chain runner
- *(providers)* add capability catalogs
- *(net)* own provider HTTP reads
- share one typed chain runner across fallback loops
- merge research terminal state into one type
- own attempt-chain derived facts in attempt_trace

## [0.4.0](https://github.com/jfmoe/forager/compare/v0.3.0...v0.4.0) - 2026-08-29

### Fixed

- *(release)* clear expected doctor exit on Windows
- *(release)* align artifact gate with doctor exit contract

### Other

- simplify suite contracts
- [**breaking**] tighten audited contracts

## [0.3.0](https://github.com/jfmoe/forager/compare/v0.2.0...v0.3.0) - 2026-08-09

### Added

- *(research)* consume Context7 evidence locators
- *(search)* unify search candidate contract

### Fixed

- close search fallback and live smoke gaps
- *(map)* enforce Tavily API bounds ([#143](https://github.com/jfmoe/forager/pull/143))
- *(anysearch)* restore narrow markdown decoding ([#142](https://github.com/jfmoe/forager/pull/142))
- *(mcp)* preserve optional provider contracts ([#141](https://github.com/jfmoe/forager/pull/141))
- *(search)* restore streaming provider contracts
- *(search)* normalize completed main answers
- *(search)* restore supplemental fallback contract
- *(search)* remove validation and restore source targets
- *(research)* stabilize failure recovery manifest
- *(cli)* restore runtime observability contracts
- *(cli)* restore JSON preflight terminals
- *(net)* enforce protocol response byte caps
- *(net)* disable shared client redirects

### Other

- *(skill)* sync bundled CLI contract ([#146](https://github.com/jfmoe/forager/pull/146))
- streamline acceptance validation
- *(net)* stall after the first response byte
- restore project agent instructions
- sync agent skills and migration audit

### Removed

- Removed the `search --validation` option and `search.validation` configuration key without aliases or compatibility shims. Legacy uses now fail as unknown CLI or configuration input.

### Migration

- Upgrade the CLI to `forager >=0.3.0` before installing or using the bundled skill.
- Search JSON no longer returns `vertical_results`; read every non-primary Search Candidate, including Vertical Discovery results, from `extra_sources`.
- Before or after upgrading, use `forager config path` to locate the configuration file and manually delete any persisted `search.validation` key; `config path` does not load the schema. The obsolete `SMART_SEARCH_VALIDATION_LEVEL` migration mapping is deleted rather than treated as a runtime input.
- Callers that omit `--extra-sources` or pass `0` should account for the restored Supplemental Web Search request target of 3. Documentation Search and Vertical Search still default to 1, explicit `1..=20` targets are unchanged, and values above 20 are rejected before configuration loading or network access.

## [0.2.0](https://github.com/jfmoe/forager/compare/v0.1.2...v0.2.0) - 2026-08-05

### Added

- *(research)* deliver file-backed evidence index ([#104](https://github.com/jfmoe/forager/pull/104))
- *(research)* account for known URLs and terminal gaps ([#103](https://github.com/jfmoe/forager/pull/103))
- *(research)* budget evidence per subquestion
- *(search)* separate fetch and vertical results ([#101](https://github.com/jfmoe/forager/pull/101))
- *(search)* separate supplemental candidates ([#100](https://github.com/jfmoe/forager/pull/100))
- normalize web fetch content contract ([#99](https://github.com/jfmoe/forager/pull/99))

### Fixed

- preserve docs fallback after empty Context7 sources ([#95](https://github.com/jfmoe/forager/pull/95))

### Other

- document v0.2.0 output migration ([#106](https://github.com/jfmoe/forager/pull/106))
- *(skill)* rewrite forager orchestration ([#105](https://github.com/jfmoe/forager/pull/105))
- define consumable search and research outputs
- define inline citation binding
- decide skill search orchestration contract
- decide research engine evidence pipeline contract
- record Context7 MCP output contract
- retain current stdout contract
- decide provider-first fetch content contract
- require post-release local validation

### Changed

- Web Fetch now returns provider-independent Normalized Fetch Content and uses the shared `Tavily → Firecrawl → Jina` chain for URLs, PDFs, search-side fetches, and research evidence ([#99](https://github.com/jfmoe/forager/issues/99)).
- Search `sources` now contains only Primary Search Sources. `extra_sources` contains only Supplemental Search Candidates with provider-native summaries, while `vertical_results` contains only structured Vertical Discovery results ([#100](https://github.com/jfmoe/forager/issues/100), [#101](https://github.com/jfmoe/forager/issues/101)).
- Search-side Web Fetch results now appear in `extra_sources` with the successful provider and a 300-character body preview; the preview limit does not truncate provider-native candidate summaries ([#101](https://github.com/jfmoe/forager/issues/101)).
- Search source and candidate `title` fields are now optional in JSON; Markdown uses the URL as the link label when a title is absent. Invocation fields `query`, `model`, selected `provider`, and `capabilities` now live in the Search Result Journal rather than default stdout.
- Context7 docs JSON now exposes one readable payload with `provider`, `library_id`, `query`, and `content`; `provider_attempts` remains available only with `--verbose`. Automatic Documentation Search continues to the next provider when Context7 has no normalized source ([#95](https://github.com/jfmoe/forager/issues/95)).
- Research is now a deterministic, single-pass evidence pipeline with per-subquestion caps, first-class known URLs, terminal coverage gaps, and a file-backed Research Evidence Index ([#102](https://github.com/jfmoe/forager/issues/102), [#103](https://github.com/jfmoe/forager/issues/103), [#104](https://github.com/jfmoe/forager/issues/104)).
- The bundled skill now routes through `direct retrieval → ordinary search → research`, orients with ordinary search before writing a research plan, reads evidence from `evidence_items[].path`, and binds synthesized claims as `[eN](URL)` ([#105](https://github.com/jfmoe/forager/issues/105)).

### Removed

- Search no longer returns `validation_results`, duplicates Supplemental Search Candidates into `sources`, or copies URL-bearing Vertical Discovery records into `extra_sources`.
- Context7 docs no longer returns `code_snippets`, `info_snippets`, `results`, `total`, or synthesized `sources`.
- Research no longer returns `evidence_items[].content`, mechanical `final_answer`, duplicate top-level `content`, derived `citations`, inline `research_plan`, or invocation and diagnostic echoes in its default output. No aliases, compatibility shims, feature flags, or duplicate legacy arrays remain.

### Migration

- Upgrade the CLI to `forager >= 0.2.0` before installing or using the bundled skill.
- Search callers must treat `sources` as answer attribution, select and fetch any needed `extra_sources`, consume Vertical Discovery only from `vertical_results`, handle an omitted `title`, read `query`/`model`/selected `provider`/`capabilities` from the journal, and request `--verbose` when inline `provider_attempts` are required.
- Research callers must read selected evidence bodies from `evidence_items[].path`, synthesize the answer themselves, cite evidence with `[eN](URL)`, and treat unconsumed candidates as unverified fetch inputs rather than evidence.
- Context7 callers must reuse `library_id` as an identifier rather than pass it to `fetch`; use absolute URLs present in the returned content when source-page verification is required.

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
