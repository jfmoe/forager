# CLI contracts

This document owns the contracts that hold across forager's commands: how output is rendered and
where it goes, how payloads stay small, what exit codes mean, the deadline, the contracts of the
core commands, and what `doctor` and `browser` check and report.

It does not describe commands one by one. For the arguments of a command run
`forager <command> --help`, which is the authority for argument parsing; for its options, output
fields and failure cases read the product skill's
[CLI reference](../../skills/forager/references/cli.md). The rules shared by all platform
commands are in [`platforms.md`](platforms.md), and the configuration surface is in
[`config.md`](config.md).

## Command surface

- A bare verb is a smart pipeline (`search`, `research`). An operation with a provider fallback
  chain is a top-level verb named after the operation (`fetch`, `map`). A command that names a
  provider is nested under it (`exa search`, `context7 docs`) and bypasses capability routing and
  cross-provider fallback. Platform commands are `platform <id> <op>`.
- Aliases are visible aliases listed in `--help`. Clap's subcommand prefix inference is off, so an
  abbreviation that is not a declared alias is an unknown command.

## Output channels and formats

- stdout carries the result, and only the result: on success the result itself, on a terminal
  failure the failure payload. Diagnostics, warnings, progress and logs go to stderr. A failed
  command also prints its payload on stdout, so an empty stderr does not mean success; the exit
  code decides.
- `--format` selects the stdout rendering. `json` is the default for every command that has the
  option. `markdown` is the readable rendering of the same result. `content` prints only the body
  a command retrieved, so only commands with a body accept it; the parser rejects it elsewhere.
  research renders its Research Evidence Index and unresolved gaps in both `markdown` and
  `content`, never evidence bodies or a synthesized answer.
- `--output FILE` tees: the rendered result is written to `FILE` and printed to stdout as usual.
  When the write fails, the command ends with exit 3 whatever its own result was; stdout still
  carries the full result, and JSON output gains `output_status: "failed"` and a bounded
  `output_error`. A Search Result Journal failure is different: the journal is a side channel, and
  its failure is a warning that never changes the result or the exit code.
- `--receipt` requires `--output` and is accepted wherever `--output` is. The file content is
  identical to the tee. Only when the command succeeds (exit 0) and the file was written does
  stdout become a one-line receipt `{"output_path", "bytes", "lines"}` describing the written
  file. A failed command still prints its full failure payload; a failed write behaves as it does
  without `--receipt`. The receipt lets a caller keep a long result in a file and read parts of
  it instead of loading the whole result.
- JSON preflight failures: once clap has parsed the arguments and `--format json` is selected, a
  failure of `search`, `fetch` or `research` before a rendered result (loading the configuration,
  reading stdin, reading, parsing or validating a research plan) is a single parseable JSON object
  on stdout `{error_kind, message, journal_ref, journal_status}`, with exit 2, 3 or 4. No valid
  journal configuration exists yet, so forager does not guess one or fall back to a default
  directory: `journal_ref` is `null` and `journal_status` is `"unavailable"`. Clap argument errors
  and panics keep clap's and Rust's channels. Every other command, and these three in other
  formats, report a preflight failure as one stderr line `argument_error: …`, `config_error: …` or
  `runtime_error: …`.
- Operational commands (`browser`) print their result as one JSON object on stdout and their
  progress and prompts on stderr; a failure after preflight is `{error_kind, message}` on stdout.

## Lean payloads

- Default stdout carries what a caller acts on and leaves the rest to the journal. A failure
  payload holds `error_kind`, a `message` cut to 500 characters, an attempt summary (`total`,
  counts `by_kind`, the `providers` tried, each list bounded and marked when truncated), the
  `capability_gaps` that are not empty, and `journal_ref` (nullable) with `journal_status`.
  Commands that write no journal report `journal_status: "not_applicable"`.
- The default failure payload targets 4 KiB, and forager treats a default failure payload over
  that target as a rendering error.
- research failures use one small, fixed shape whatever the size of the run: `error_kind`, the
  bounded `message`, the full `evidence_dir`, `summary_path` (`null` unless the Research Recovery
  Manifest is readable), the `gap_check` status and stop reason, `synthesis_policy`, and the
  journal pair. The shape never changes with encoded length and locators are never cut, so a very
  long legitimate path can exceed 4 KiB.
- Full `provider_attempts` are stored only in the journal. `--verbose` is the escape hatch that
  puts them inline on stdout, on success and on failure.

## Exit codes

| Code | Meaning |
| --- | --- |
| `0` | Success, including a valid empty result of a direct command (`items: []`). |
| `2` | Argument or caller input error, found before any request: clap errors, an invalid research plan, an invalid `config set` path or value, and option combinations a route cannot serve. |
| `3` | Configuration error: an unknown file key or `FORAGER_*` variable, an empty chain or order, a missing credential a command needs, unreadable stdin, or a failed `--output` write. |
| `4` | Terminal failure of the transport family: `auth`, `rate_limited`, `quota_exhausted`, `parameter`, `network`, `timeout`, `runtime`. |
| `5` | Terminal failure of the content family: `quality` and `evidence`. |

- Exit code `1` is unused. A panic exits `101`, which is outside the contract.
- Exit 2 is reserved for input forager rejects before sending. When an upstream service rejects a
  request, the attempt's `parameter` kind is a transport-family failure and exits 4.
- An empty result is never an error kind. A direct command with no matches succeeds; only the
  evidence pipeline turns missing evidence into `evidence` (exit 5).

## Deadlines

- `--timeout SECONDS` is the hard deadline of the whole command. Every retry, fallback and probe
  shares it; an implementation may give one attempt a shorter limit, but never more than what
  remains. A command that runs out reports `timeout` and keeps the attempts it completed: in full
  in the journal and as the summary on stdout.
- Waiting for a route's request window counts against the deadline; see the access policies in
  [`platforms.md`](platforms.md).
- How the budget is reserved so that a fallback stays reachable is in
  [`architecture.md`](architecture.md).

## `search`

### Capability declaration

`--capabilities` takes comma-separated capability IDs from the product skill's
[capability vocabulary](../../skills/forager/references/capability-vocabulary.json), or the
exclusive sentinel `none`. It has three states:

- omitted: undeclared, so the classifier chooses the capabilities; without a configured
  classifier, search falls back to the default Web route;
- `none`: main search only;
- a list: the caller's declaration is authoritative and forager never widens it (ADR-0004).

### Output roles

- `sources` holds only Primary Search Sources, the sources the main answer is attributed to. An
  empty `sources` means the answer cites nothing; forager adds no flag, and the caller treats such
  an answer as unverified.
- Every non-primary candidate is a Search Candidate in `extra_sources`. A candidate has a
  `provider`, a `capability` and `provider_data`, and nullable `title`, `url` and `summary`. `url`
  is a real HTTP(S) URL or `null`, never a fabricated locator. `summary` copies only a
  provider-native description, abstract or snippet, except that a search-side Web Fetch success
  uses a preview of its fetched content. `provider_data` is a typed snake_case allowlist each
  provider defines; it never passes through raw HTTP or MCP wrappers or page bodies.
- Main search runs alongside the supplemental capabilities. When main search fails, it alone
  decides the terminal state and the exit code, and the candidates already found are kept: the
  failure payload adds `extra_sources` without their `summary`, in order, as many as fit the
  4 KiB target, with `extra_sources_truncated`, and its attempt summary counts the supplemental
  attempts too (ADR-0016). Both fields are absent when there are no candidates. `--verbose` gives
  every candidate in full, and the journal keeps them all.

### Answer normalization

Every main-search backend's answer passes one normalizer after the response is complete. It first
removes closed `<think>…</think>` blocks (case-insensitive, across lines), then projects a
trailing explicit source heading block and inline `[[N]](URL)` citations with an HTTP(S) URL
into sources. Other markers, `sources(...)`, arbitrary `<details>` and guessed trailing links are
not parsed. A citation title made only of digits is the citation's number, not a page title, so
the source has no title. Sources are deduplicated by their redacted public URL. An answer that is
empty after normalization is a `runtime` failure. `platform x ask` uses the same normalizer.

## `research`

research is a file-based evidence pipeline, not an answer engine.

### Research plan injection

A plan is passed with `--plan FILE`, or `--plan -` for stdin. A caller's plan and a
classifier-generated plan share one type, Schema v1; the product skill's
[`research-plan.json`](../../skills/forager/references/research-plan.json) is a valid example.

- A caller's plan is authoritative and skips the classifier. Without `--plan` the classifier
  generates the plan; without a configured classifier research exits 3.
- Parsing is strict, and any violation exits 2: an unknown or missing field, an unrecognized
  `plan_version`, an empty `decomposition`, an empty or duplicate subquestion `id`, an empty
  `reason`, or a capability outside the plan vocabulary. Duplicate capabilities are folded,
  keeping their order. A future version is a new explicit branch, never a lenient upgrade.
- `decomposition[].required_capabilities` decides which capability seams a subquestion may cross.
  The plan vocabulary has two values, `docs_search` and `web_search`. A missing credential inside
  a declared seam does not block success; it is reported in `capability_gaps`. Declared
  capabilities route; availability is advisory.
- `web_fetch` is an engine invariant (fetch before claim): the engine fetches as evidence needs
  it. Declaring `web_fetch` in a plan exits 2 with a message saying that the engine runs it.
- `intent_signals` affect evidence strength and the cross-validation strategy inside the
  declared seams only. They never add or remove a capability and never reorder providers; the
  engine never departs silently from the configured order.

These rules are the authoritative definition of the research exemption in ADR-0004; they stand
beside its ordinary-search semantics without conflict.

### Budgets and evidence accounting

- `--budget` bounds the shape of a run. `quick`, `standard` (the default) and `deep` allow at
  most 2, 4 and 6 subquestions and at most 1, 2 and 3 evidence items per subquestion. The caps
  are upper bounds, not fill targets: a subquestion with one accepted evidence item is covered.
- A caller's plan with more subquestions than its budget allows exits 2. A classifier plan over
  the limit is truncated with a warning.
- The run fails when it collects fewer evidence items than it requires: 2 when the plan's
  `cross_validation_need` or `source_authority_need` is `high`, 1 otherwise, and never more than
  the plan's caps can hold (a diagnostic says when the requirement was lowered to fit). The
  failure is `evidence`, except that a run with no evidence whose attempts all failed, or that hit
  a quality failure, takes the terminal kind of its attempts.
- Known URLs are evidence directives outside the caps. A URL in a subquestion's text belongs to
  that subquestion; a URL only in the query belongs to the plan as a whole. Every known URL is
  attempted, and one that fails to fetch always records a gap, even when other evidence covers its
  subquestion.
- A gap means a subquestion ran out of candidates without evidence; a failed fetch of one
  candidate is only an attempt. `gap_check.status` is `closed` when there is no gap and no
  capability gap, and `degraded` otherwise.
- The engine runs once: it never refetches or re-plans. That loop belongs to the caller.

### Research Evidence Index

- Default JSON on success is the Research Evidence Index: identity, metadata and a readable path
  for each evidence item, plus the evidence directory, the plan path, the unconsumed candidates
  as a count and a path, the gap check, capability gaps, `synthesis_policy: "fetch_before_claim"`
  and the journal pair. Evidence bodies never appear on stdout, in the manifest or in the journal.
- One locator is fetched once; its evidence item lists every subquestion it covers.
- Citations bind to evidence item IDs: URL evidence as `[eN](URL)`, URL-less Context7 evidence as
  `[eN]`, attributed through the index item with the same ID.
- `evidence_dir` always holds `00-plan.json` (the full normalized plan), one `NN-evidence.md` per
  evidence item (its body), `candidates.json` (the full metadata of unconsumed candidates, each
  `is_evidence: false`) and `summary.json`, the Research Recovery Manifest. The manifest records
  the query, budget, plan source, capabilities, fallback, evidence identity, metadata and paths,
  coverage, gaps, capability gaps, the terminal state, attempts and `synthesis_policy`, but no
  evidence body.
- Success delivers the index; a failure is recoverable through the manifest. The two terminal
  states need not share one shape. When the manifest cannot be written, the original terminal
  state and the full `evidence_dir` stay, `summary_path` is `null`, and the existing diagnostic is
  printed.

## `fetch`

The `content` of a fetch is the provider-independent Markdown body only. Provider attempts and
diagnostics stay in their own fields and channels and never enter the body. URLs and PDFs share
the `web_fetch` chain and its failure semantics.

## `doctor`

`doctor` has two tiers.

- **Shallow** (`doctor`) covers every registered provider: masked configuration, whether
  credentials are present, reachability, configuration and directory permissions that are too
  broad, and an effective-value block with the same shape as `config list`. An HTTP provider is
  reachable when a GET to its endpoint gets any HTTP response; doctor waits only for the response
  headers, because some endpoints do not answer HEAD. A platform route that no default order
  lists takes part only when the user's platform order lists it; otherwise it reports
  `configured: false`. A browser route is checked by the browser transport's preflight check
  (the browser can be found and, when the route needs a login, the Browser Profile carries a login
  marker), without launching a browser; a failed check carries a `message` with the fixing
  command.
- The top-level `ok` is true when every provider with `configured: true` is reachable, and true
  with nothing configured. When it is false, JSON and Markdown both say so and the command exits
  4. Permission warnings never change `ok`.
- **Deep** (`doctor --provider ID`) probes one provider; the IDs come from the provider registry.
  A provider that needs credentials gets a credential check and a minimal live call; a platform
  route gets a minimal platform request, except that a route whose service has a free account
  endpoint checks every key there instead of spending quota. A provider whose prerequisites are missing, or a
  platform route no order enables, fails with `config` and exit 3 before any request, and a
  browser route launches no browser in that case. A failed probe exits 4. What each provider's
  probe sends and adds to the report is in the product skill.
- A provider with an access policy sends both the shallow check and the deep probe through the
  cross-process request window. When the window does not open in time, the shallow check reports
  the provider unreachable and the deep probe fails with `timeout`; neither sends the request.
- A doctor report only proves what it probed. It is not the recovery path for a configuration
  that cannot be loaded; that is a preflight failure with exit 3.

## `browser`

`forager browser` manages the browser environment that the browser transport uses (see
[`platforms.md`](platforms.md)) and follows the operational output convention above. Argument
and configuration errors are preflight failures (exit 2 or 3); other failures exit 4.

- `browser install` downloads the Managed Browser package this forager pins for the current
  host, with the address and SHA256 built in, verifies the hash, and installs it into the cache
  directory given in [`config.md`](config.md). When the pinned version is already installed it
  downloads nothing and exits 0; the result says whether it downloaded. A download failure is
  `network`, the deadline is `timeout`, and a hash mismatch (the download is deleted), a failed
  unpack and a host without a package are `runtime`. A configured `browser.path` does not stop the
  install; stderr says that browser commands launch `browser.path`.
- `browser login PLATFORM` accepts only a platform whose browser route needs a login; any other
  platform exits 2. It launches a headed browser on the login entry that the route's page logic
  declares, waits until the page logic observes a logged-in session, closes the browser normally,
  and writes the login marker and the window's screen size into the Browser Profile. Like a
  platform command, it takes the route's request window before the profile lock. No locatable
  browser is a configuration error (exit 3) whose message names `forager browser install` or
  `browser.path`. `--timeout` bounds the whole command under the browser transport's deadline
  rules: no login observed by the read deadline is `timeout`; a browser that fails to launch, a
  failed CDP handshake, a window closed before the login, and a profile another process holds are
  `runtime`.
