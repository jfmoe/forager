# Leave orchestration to the calling agent

## Decision

forager offers deterministic atomic operations and does not orchestrate an investigation. There is
no `research` command, no Research Evidence Pipeline and no planner. An agent that needs a
multi-source investigation splits the question itself, runs `search`, `fetch` and the platform
commands, and judges each source on whether it bears on the question. The product skill teaches
that workflow.

forager also does not choose capabilities for the caller. There is no classifier: no model call
decides what an ordinary search runs. A search without a Caller Capability Declaration runs main
search only (ADR-0004).

forager does not keep a second Documentation Search provider behind Exa. Context7 is retired as a
provider and as direct commands, and Exa alone serves a declared Documentation Search. The
Documentation Search capability seam has a provider order, and a result with no consumable
candidate counts as a failed attempt rather than an empty success, so a provider added to the
order later is reached when Exa finds nothing usable.

The agent layer follows these rules:

- **Cost ladder.** Every request goes down `direct retrieval → ordinary search → multi-source
  investigation` and takes the cheapest branch that can finish it. Escalation happens only on an
  observed shortfall of the cheaper branch, or when the request's shape rules that branch out,
  never in anticipation.
- **Bounded recovery.** The ordinary-search recovery chain (retry, Exa fallback, fetch) has small
  fixed caps on each step, and an error that is not a timeout or a network failure leaves it at
  once.
- **A main-search answer is never evidence.** A claim that needs a source is backed by fetched
  content; the answer only orients the next step.

## Why

- **It was not used.** From 2026-10-24 to 2026-11-23 the Search Result Journal recorded one
  `research` run against 171 searches, and no run where the classifier wrote the plan.
- **What it added was bookkeeping.** Over an agent calling `search` and then `fetch`, research
  added deduplication, per-subquestion caps and evidence numbering, which the agent can do itself.
  Its pipeline module was 1,367 lines with a 3,020-line test file, and its contract was among the
  heaviest parts of the product skill to document.
- **Its coverage status counted, it did not judge.** A run reported no remaining gap once it held
  enough evidence items. Two real investigations found secondary paraphrases, aggregator pages and
  unrelated reports behind such a status and had to reread every source. Judging relevance is
  semantic work that a deterministic CLI cannot do reliably (ADR-0014); the agent that writes the
  answer has to read the sources anyway.
- **The ladder keeps cost in proportion.** A multi-source investigation is the most expensive
  branch, and its recovery work lands in the agent layer, so escalation has to be earned by an
  observed shortfall.
- **Capped recovery stops a failing search from spiralling.** Retrying an authentication,
  configuration or quota error cannot succeed, and an uncapped chain spends quota and time on a
  search that already failed.
- **Main-search answers are model-synthesized.** Citing one as evidence would let unverified text
  pose as a source.
- **The classifier decided for almost nobody.** Over the same 30 days, 159 of the 171 searches
  declared their capabilities and the classifier planned the other 12, at a median of 7.7 s
  (at most 11.6 s) before main search could start. Its callers are agents that the product skill
  already teaches to declare, so the classifier cost a model endpoint, a credential, a `setup` step
  and journal fields for a decision the caller makes better from the request it has read.
- **Context7 was never reached.** Over the same 30 days the journal recorded no Context7 attempt
  as the Documentation Search fallback, against 10 Exa attempts, and no session outside forager's
  own development ever ran its direct commands; the last development call was on 2026-09-25.
- **Exa answers documentation queries better.** In a comparison of 14 queries checked against
  official pages, Context7 covered 48% of key facts and Exa 93%, at median latencies of 8.2 s and
  2.4 s. On vendor platforms, data APIs and app manuals, Context7 covered 25% and Exa 100%. Exa
  also returns URLs a caller can fetch, where Context7 returned library locators that needed a
  second call.
- **Context7 cost a provider of its own.** A 434-line provider with an 806-line test file, an MCP
  transport no other provider used, a configuration section, a `setup` prompt, a doctor probe and
  two live cases, all kept for a fallback that did not run.
- **The accepted cost.** Research discovered candidates through Supplemental Web Search without
  running main search. Through `search`, each part of an investigation pays one main search, a
  median of 31 s over the same 30 days. An undeclared search gets no supplemental sources of any
  kind; a caller that wants them has to declare them.

## Rejected alternatives

- **Keeping research and requiring a caller plan.** It removes only classifier planning and keeps
  the whole evidence contract, its files and its documentation for one run a month, with the same
  count-based coverage status.
- **Making the coverage status judge relevance.** That moves semantic judgement into the CLI and
  gives the caller a confidence it has not earned.
- **Keeping the classifier only as an opt-in.** It still leaves the module, its configuration, its
  `setup` step and its journal fields to maintain for a path the product skill never takes.
- **A batch fetch that writes each page to a file.** It would restore research's concurrent
  fetching, but parallel `fetch` calls that each write their page to a file already do that
  without a new contract for partial failure.
- **Keeping Context7 as an unused fallback.** It keeps a credential, a transport and a live
  check alive for a path the journal never shows, and its library resolution almost always returns
  some library, so it hides a miss rather than filling it.
- **Keeping only the direct Context7 commands.** Nobody outside forager's development called them.

## Revisit when

- Agents repeat the same investigation bookkeeping and get it wrong in a way a deterministic step
  would prevent, or a caller without an agent in the loop needs multi-source investigations.
- The main search each part pays becomes the dominant cost of investigations. A search mode that
  skips main search is the next option, not a planner.
- Agents are seen escalating to an investigation, or looping in recovery, despite the ladder and
  the caps.
- Undeclared searches become a large share of the journal and their answers show capabilities the
  request needed and did not get, for example because callers other than agents taught by the
  product skill start using `search`.
- Exa's coverage of documentation queries falls clearly below another documentation provider's
  on a comparable sample, or Exa stops serving them. A second provider then joins the
  Documentation Search order.

## Supersedes

- ADR-0013, which kept the research engine deterministic and orchestrated it from the skill. The
  engine is retired; the cost ladder, the recovery caps and the rule that a main-search answer is
  never evidence are restated above.
- ADR-0017, which put Exa before Context7 in Documentation Search and kept Context7 docs as one
  readable payload. Context7 is retired; the rule that a result with no consumable candidate does
  not end the chain is restated above.
