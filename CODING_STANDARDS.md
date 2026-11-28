# Coding standards

The points that reviews of this repository caught more than once, and the product skill's review
points, that no test can check. The rules themselves live in `AGENTS.md` and the procedures in
`docs/agents/`.

A point enters this file the second time a review catches the same kind of mistake. When a point
becomes checkable by a machine, it turns into a repository rule test and leaves this file.

## Tests

- **A test runs the logic it is named after.** Follow its input to the assertion: a record that is
  filtered out before it is parsed, a field under a missing parent, or an empty collection lets
  the assertion pass with nothing checked.
- **A file that one program writes and another reads has one test that hands the real output to
  the reader.** Two tests that each build the file by hand agree only with themselves, so the
  format can drift on one side without a red run.
- **A helper that a second test module needs moves to the shared support modules.** A copy starts
  equal and then changes on one side only; one review found about nineteen copies of the
  sample-loading helpers.

## Failures nobody watches

- **Each way an unattended job can fail reaches a person.** For every exit of a scheduled script,
  job or workflow, name who is told and how: a failure of the notifying tool itself, a missing
  program, an unexpected exit code, a run cut short, and a subject that was retired while its
  issue was open. Such a path runs only when something is already wrong, so no ordinary run shows
  that it is silent.

## Product skill

No test reads the skill's wording, so these are judged in review. A change can be correct and
still make the skill worse; reject it when it breaks one of them. `docs/agents/product-skill.md`
says where each fact goes and when a change needs a fresh-agent trial.

- **Each sentence changes what an agent does.** Delete a sentence an agent would follow without
  being told, or that explains without changing an action. Every sentence is loaded by every task
  that reads its file, and agents often read only a file's first lines, so a no-op pushes a live
  rule out of the part they read.
- **The skill describes what an agent can observe, not how forager is built.** An agent acts on a
  command, an exit code, a field or a message; a route's internals give it nothing to act on, and
  they go stale without a test noticing, because no test reads the skill.
- **One platform's fact lives in that platform's file.** An agent reads only the files its task
  routes to, so the fact anywhere else is loaded by tasks that do not need it and missed by the
  one that does. A fact shared by every platform lives once in `shared.md`.
- **A pointer states when to follow it.** An agent decides whether to open a file from the
  pointer's condition alone: "read X" with no condition is followed by every task or by none, and
  trials found agents loading thousands of characters a task did not need from conditions that
  were too broad.

## Rules with another owner that reviews keep catching

Their wording and reasons stay with the owner; check the change against it.

- An upstream record in a test is derived from a stored sample, and a sample is read through the
  shared reader: `docs/agents/testing.md`, "Upstream samples".
- A retired test's commit message names what proves its rule now: `docs/agents/testing.md`,
  "Retiring or rewriting a test".
- A test module larger than the code it tests is a sibling `<module>_tests.rs`: `AGENTS.md`, "Rust
  conventions".
- A document states no value the code states and carries no change history: `AGENTS.md`, "Writing
  documents".
