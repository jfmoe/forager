# v0.1.1 switch gate

Acceptance date: 2026-07-28 (Asia/Shanghai)

Release: [v0.1.1](https://github.com/jfmoe/forager/releases/tag/v0.1.1)

Release workflow: [30288281134](https://github.com/jfmoe/forager/actions/runs/30288281134)

Switch gate: **open**

## Release artifact

- The public Release is not a draft or prerelease.
- The release gate reported `passed` for all five supported targets.
- The clean installer installed `forager 0.1.1` as a Mach-O arm64 executable outside the repository.
- The downloaded `forager-aarch64-apple-darwin.tar.xz` matched its published SHA-256 file.
- The tag was created by `github-actions[bot]` through release-plz and points to `22570d43177e4d1218ff36eb55d7809a755bda09`.

## Credentials and live gates

- The migrated configuration contains credentials for xAI, OpenAI-compatible, Tavily, Firecrawl, Jina, Context7, Exa, and AnySearch.
- The configuration directory and file modes are `0700` and `0600`.
- `doctor --timeout 60` returned `ok=true`, with all eight providers configured and no permission warnings. Jina's shallow reachability check was transiently false.
- L0.1-L0.8 then ran `doctor --provider <provider> --timeout 120` separately for xAI, OpenAI-compatible, Tavily, Firecrawl, Jina, Context7, Exa, and AnySearch. All eight deep probes passed, covering nine checks across SSE, HTTP, and MCP transports.
- Manual P1 returned a non-empty answer with ten sources and both `docs_search` and `web_search`.
- Manual P2 used a classifier-generated plan, returned verified evidence and a non-empty final answer, closed its gap check, and wrote its journal.
- Manual C14 returned three `academic.search` results for the explicit `academic.search` domain pair.
- `smoke --live --timeout 600` returned `ok=true`: 19 passed, 0 failed, 0 deferred, and 0 unconfigured. P1, P2, and C01-C17 all passed. C06 passed on its second transient-retry attempt; every other case passed on its first attempt.

## Frozen research quality gate

All three questions ran with the public v0.1.1 binary and caller-authored plans:

| Question | Subquestions covered | Citations | Unique citation URLs | Degraded | Gaps |
| --- | ---: | ---: | ---: | --- | --- |
| Rust 的 async drop 现状与最新提案是什么？ | 2/2 | 3 | 3 | no | closed |
| 对比 figment 与 config-rs 的分层覆盖模型，给出出处 | 2/2 | 3 | 3 | no | closed |
| 近一个月各大社区关于 Coding Agent 的讨论 | 3/3 | 3 | 3 | no | closed |

Rubric:

- Relevance: pass. Each evidence item maps to its requested library, proposal, or named community.
- Source deduplication: pass. Citation counts equal normalized unique URL counts for every answer.
- Citation support: pass with one noted limitation. Every citation URL appears in the final answer and each main finding includes its source. Jina received a Reddit 403 page rather than the discussion body, so that item establishes the source location but is not treated as substantive Reddit discussion evidence.

The independent forced-X command selected only xAI, disabled fallback, and enabled only `x_search`. It returned a non-empty answer, 13 sources, and 11 unique `x.com` discussion URLs.

## Journal and redaction

- Research journals contain both `.result` and `.execution`.
- The result and execution surfaces contain the same evidence counts.
- The execution surface records caller plan provenance, capability coverage, provider attempts, deadlines, and terminal attribution.
- A synthetic URL query parameter was replaced with `********` in the returned answer, source URL, output file, and journal result source. The original user query remains verbatim by contract.

## Setup

- A fresh interactive run exercised language selection followed by stages 2-4.
- The second run preserved the existing main credential while incrementally updating only the selected model and Exa credential.
- The resulting directory and file modes were `0700` and `0600`.
- No temporary setup files remained.

## Skill integration

- The original Claude Code attempts remain recorded as non-passing history: one isolated-profile authentication failure and two pre-inference `429 Service Unavailable` responses with zero tokens and zero tool calls.
- The final gate used an isolated Git project, HOME, Codex profile, forager config, and state directory. `npx skills add jfmoe/forager --agent codex --skill forager -y --copy` installed only the `forager` skill at project scope.
- The skill installed from the repository was byte-identical to the skill at v0.1.1. `PATH` began with the clean public-Release binary, which reported `forager 0.1.1` and Mach-O arm64; its archive passed the published SHA-256 check.
- Codex CLI 0.145.0 session `019fa5d8-cb53-7d51-9f48-00cee5d92765` read the installed skill and research references, generated a strict Schema v1 `research-plan.json`, and ran `forager research "Rust 的 async drop 现状与最新提案是什么？" --plan research-plan.json --budget standard --format json --output research-result.json`.
- The command exited 0. Both the Agent-authored plan and result parsed as JSON, `plan_source` was `caller`, three evidence items were collected, `capability_gaps` was empty, and the journal was written.
- The result disclosed one unresolved `open-questions` evidence gap and marked itself degraded. This does not replace or weaken the separately completed frozen research quality gate above: the Skill integration gate proves that a real installed Agent generated a plan, executed the public Release, reached a successful terminal result, and reported its evidence limits without human substitution.

All automated and manual evidence is tied to v0.1.1. With the supported real-Agent Skill session complete, every acceptance item has passed and the retirement switch gate is open.
