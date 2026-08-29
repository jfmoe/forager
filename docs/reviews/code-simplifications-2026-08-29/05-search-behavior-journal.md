# Search behavior and journal audit

## Scope

A fresh subagent exercised the current `forager 0.3.0` working-tree binary against
real providers. The three scenarios ran serially to avoid creating artificial
fallback, credential-cursor, or latency interactions. The main reviewer then read
the generated outputs, journals, plans, and evidence independently.

Artifacts remain under `/tmp/forager-behavior-audit.dYP19h`. The run created four
new result journals: three scenario records plus one ordinary-search rerun needed to
capture an explicit process exit code after the first observer lost its session.

## Scenario results

### Ordinary main search

- Invocation shape: caller-declared `capabilities=none`, JSON stdout, and tee output.
- Terminal: exit code 0; non-empty answer; five primary sources on the conclusive
  rerun; no extra sources or capability gaps.
- Journal: `search_result_1787974287622141000_78486_0.json`; schema v1,
  `source=caller`, empty capability set, one successful `xai/main_search` SSE
  attempt, terminal attribution `ok`, and 39,284 of 180,000 ms consumed.
- The original successful run produced
  `search_result_1787973840568742000_77129_0.json` with the same attempt topology.
  Its missing observed exit code was an outer-runner evidence gap, closed by the
  exact rerun; it was not a product failure.
- Historical comparison:
  `search_result_1787972384837627000_71998_0.json` has the same result/execution
  topology and invariants. Its successful provider was `openai_compatible` instead
  of `xai`, an expected dynamic main-chain outcome.

### Documentation search

- Invocation shape: caller-declared `docs_search`, one extra source, JSON stdout,
  and tee output.
- Terminal: exit code 0; non-empty answer, five primary sources, one typed Context7
  documentation candidate, and no capability gap.
- Journal: `search_result_1787973982950913000_77374_0.json`; schema v1,
  `source=caller`, capability set `[docs_search]`, successful `xai/main_search`
  followed by successful `context7/docs_search`, terminal attribution `ok`, and
  46,929 of 180,000 ms consumed.
- Historical comparison:
  `search_result_1787668757226519000_2803_0.json` has the same topology and
  provider/seam sequence. The older record predates the additive attempt
  `disposition` field; all current disposition/error invariants hold.

### Classifier-planned quick research

- Invocation shape: classifier-generated plan, quick budget, explicit evidence
  directory, JSON stdout, and tee output.
- Terminal: exit code 0; Schema v1 plan with two subquestions; two verified Context7
  evidence files; four disclosed unconsumed candidates; no capability gap;
  mechanical gap state `closed/evidence_converged`.
- Journal: `search_result_1787974060763401000_77539_0.json`; schema v1,
  `source=classifier`, capabilities `[docs_search, web_fetch]`, terminal attribution
  `ok`, and 25,755 of 600,000 ms consumed. One transient Context7 network failure is
  followed by a successful retry; every attempt's disposition and error fields are
  internally consistent.
- The Evidence Index metadata, paths, gap state, candidate locator, and synthesis
  policy exactly match the journal record. The plan, both evidence bodies,
  `candidates.json`, and `summary.json` are readable.
- Historical comparison:
  `search_result_1787972363459802000_71925_0.json` has the same journal and artifact
  contract. Its classifier selected a broader capability set for a different query,
  so the provider/seam sequence differs as expected.

## Semantic evidence check

The two initial research evidence files covered `JoinHandle::abort` but did not
support the `CancellationToken` half of the comparison. This does not contradict the
CLI contract: `verified` means a non-empty fetched body, and the documented Evidence
Index explicitly delegates semantic verification to the consuming agent.

The agent correctly refused to treat `gap_check=closed` as semantic proof. A direct
Context7 query using an already-discovered library ID also returned unrelated abort
material despite exit code 0. The agent then reused the official
`CancellationToken` URL discovered by the orientation search and fetched it through
the normal Web Fetch chain. That command exited 0 through Tavily and returned 31,809
characters covering cooperative cancellation, `child_token` directionality,
`DropGuard`, and `run_until_cancelled`. This closes the sample's evidence gap without
rerunning search or research, as required by the Skill.

The recovered evidence supports only the documented cancellation and drop/future
semantics. It does not justify a broader claim that arbitrary application cleanup is
guaranteed to run.

## Output, security, and filesystem checks

- Every stdout and tee pair is byte-identical and parses as JSON.
- Search and research stderr files are empty.
- The journal directory is mode 0700; all four new journals are mode 0600.
- New journals contain schema v1, matching query/status metadata, ordered attempts,
  non-exhausted deadlines, and `terminal_attribution=ok`.
- Credential/header and URL-query secret pattern scans found no leak. No raw
  credential was read for comparison.
- Provider-direct Context7 and fetch commands created no result journal, matching
  the documented journal ownership boundary.

## Conclusion

No actionable implementation finding remains. Runtime routing and recovery vary
with provider and query content, while the stable stdout, tee, journal, attempt,
artifact, permission, and redaction contracts match both current specifications and
comparable local history. The semantic evidence shortfall was observable and was
handled at the intended agent layer rather than being mistaken for a successful
research answer.
