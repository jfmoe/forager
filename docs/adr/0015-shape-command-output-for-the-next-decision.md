# Shape each command's output for the next decision

## Decision

forager has no generic context budget for successful output: no character, byte or token budget
option, no default context limit, no automatic stdout truncation that spills the full content to a
file, and no summary-then-full two-stage retrieval. The full rendered result goes to stdout, and
`--output` tees it. Callers and the skill manage long output.

Instead, each command's default output is modelled for what its caller decides next, and material
that is already persisted or only diagnostic is delivered by reference:

- **research** is a file-backed evidence delivery command. Its default result is a Research
  Evidence Index: identity, metadata and a readable path for each evidence item, with no bodies.
  The plan, the unconsumed candidates and a body-free Research Recovery Manifest are files the
  result points to. research emits no answer of its own; its consumer synthesizes one and cites
  the evidence (ADR-0014).
- **search** keeps the answer and its primary sources apart from every other candidate, and each
  candidate carries the summary its provider returned, so an agent can choose which candidates are
  worth a fetch. Echoes of the invocation and diagnostics live in the journal; `--verbose` is the
  explicit way to see provider attempts inline.

The byte caps of ADR-0006 are memory safety rails, not a context budget, and the deadline limits
time, not size.

## Why

- A generic budget cannot know which part of a result matters: cutting a search answer, an
  evidence index or a page body at the same size loses different and unpredictable things, and
  the caller then pays a second call to get back what was cut.
- What a caller needs next differs by command. A research consumer needs to know which evidence
  exists and where, not to receive every body at once; a search consumer needs enough of each
  candidate to choose which to fetch. Modelling each command keeps both small without hiding
  anything the next step needs.
- Bodies already written to disk or recorded in the journal would only be duplicated on stdout.
- research used to return an answer-shaped excerpt list. It looked like a synthesized answer but
  was not one, which invited citing it.

## Rejected alternatives

- **A budget option or default limit with spillover to a file.** Every caller would have to learn
  a two-stage protocol to recover content that a command-specific shape never cuts.
- **Inlining evidence bodies in the research result.** It multiplies the payload by the evidence
  count for content the consumer reads selectively.
- **Truncating provider summaries to one uniform length.** Provider-native summaries already serve
  as discovery snippets; a second cut removes the signal used to choose a fetch.

## Revisit when

- Callers routinely receive successful outputs too large to consume even after the per-command
  shaping, in a way the skill cannot handle by persisting with `--output` and reading fragments.

## Supersedes

- ADR-0010, which kept stdout without a separate agent context budget. Its decision is the first
  half of this one; it is merged because command-specific shaping is what replaced the budget.
