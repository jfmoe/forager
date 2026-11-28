# Report a forager problem

File an issue only for a forager defect, and only after the user has seen the draft and agreed.

## 1. Classify

- **Configuration**: exit 3, a missing or rejected key, a spent quota. Repair it with
  [`diagnostics.md`](diagnostics.md); nothing to file.
- **Upstream outage or block**: `network`, `timeout`, `rate_limited`, a server error, a
  verification page, or a site restricting the account or network address, while forager behaves
  as its files describe. Tell the user what failed and that it lies outside forager; nothing to
  file.
- **Expected behaviour**: first check whether the platform's file or the branch file lists what
  you saw as expected, such as rank gaps, lagging counts, a field that is `null`, an empty result
  or a cut-off list. If it does, explain it to the user; nothing to file.
- **forager defect**: forager contradicts its skill files or `--help` (a missing or mistyped
  field, a wrong exit code, a ref it returned and then refuses), crashes (exit 101), or fails the
  same way again when the upstream answers normally elsewhere, such as in a browser.

The step is done when the problem has exactly one class. Only a defect goes on.

## 2. Collect the evidence

- `forager --version` and the operating system.
- The exact command, with keys, tokens (an `xsec_token` in a URL included) and anything private
  removed, and its exit code.
- The failure payload from stdout.
- The attempt log of one rerun with `FORAGER_LOG__LEVEL=attempts` (it goes to stderr) and
  `--verbose`. Rerun only a route that costs no quota and risks no account; for any other, ask the
  user first.
- The affected provider's entry from `forager doctor --health --format json`.
- Where the records are, for the maintainer to ask about: a search's `journal_ref` (the Search
  Result Journal, under `~/.local/state/forager/journal` unless `journal.dir` says otherwise) and
  the Invocation Telemetry in `~/.local/state/forager/telemetry` (under `$XDG_STATE_HOME/forager`
  when that is set). Name the paths only: the journal holds queries and answers, and telemetry
  names the hosts fetched.

The step is done when every item is collected or noted as unavailable.

## 3. Search existing issues

`gh issue list --repo jfmoe/forager --state all --search 'KEYWORDS'`, with the command, the error
kind and the provider as keywords. When an open issue matches, the draft becomes a comment on it
with what is new.

## 4. Draft, show, file

Draft a title that names the command and the symptom, and a body with: what you ran, what you
expected and why (cite the skill file or `--help`), what happened, and the evidence above. Show
the user the whole draft and say it will be public. File only when the user agrees:
`gh issue create --repo jfmoe/forager --title 'TITLE' --body-file FILE`, or `gh issue comment` on
the matching issue. Without `gh`, give the user the text and
`https://github.com/jfmoe/forager/issues/new`.

Reporting is complete when the issue is filed with the user's agreement, or the user has the text
and the URL.
