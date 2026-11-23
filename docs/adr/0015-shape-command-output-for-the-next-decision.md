# Shape each command's output for the next decision

## Decision

forager has no generic context budget for successful output: no character, byte or token budget
option, no default context limit, no automatic stdout truncation that spills the full content to a
file, and no summary-then-full two-stage retrieval. The full rendered result goes to stdout, and
`--output` tees it. Callers and the skill manage long output.

Instead, each command's default output is modelled for what its caller decides next, and material
that is already persisted or only diagnostic is delivered by reference:

- **search** keeps the answer and its primary sources apart from every other candidate, and each
  candidate carries the summary its provider returned, so an agent can choose which candidates are
  worth a fetch. Echoes of the invocation and diagnostics live in the journal; `--verbose` is the
  explicit way to see provider attempts inline.

The byte caps of ADR-0006 are memory safety rails, not a context budget, and the deadline limits
time, not size.

## Why

- A generic budget cannot know which part of a result matters: cutting a search answer, a
  candidate list or a page body at the same size loses different and unpredictable things, and
  the caller then pays a second call to get back what was cut.
- What a caller needs next differs by command. A search consumer needs enough of each candidate
  to choose which to fetch, not every candidate's body. Modelling each command keeps output small
  without hiding anything the next step needs.
- Bodies already written to disk or recorded in the journal would only be duplicated on stdout.
- An answer-shaped excerpt list looks like a synthesized answer but is not one, which invites
  citing it.

## Rejected alternatives

- **A budget option or default limit with spillover to a file.** Every caller would have to learn
  a two-stage protocol to recover content that a command-specific shape never cuts.
- **Truncating provider summaries to one uniform length.** Provider-native summaries already serve
  as discovery snippets; a second cut removes the signal used to choose a fetch.

## Revisit when

- Callers routinely receive successful outputs too large to consume even after the per-command
  shaping, in a way the skill cannot handle by persisting with `--output` and reading fragments.

## Supersedes

- ADR-0010, which kept stdout without a separate agent context budget. Its decision is the first
  half of this one; it is merged because command-specific shaping is what replaced the budget.
