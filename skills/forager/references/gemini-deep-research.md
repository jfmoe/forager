# Gemini Deep Research

Gemini Deep Research runs in the user's own logged-in Chrome, spends the account's Deep Research
quota, and takes about 5 to 20 minutes, sometimes up to 60. Enter this branch only when the user
explicitly asks for Gemini Deep Research. For exact syntax, exit codes, and adapter installation,
read the Gemini Deep Research section of [`cli.md`](cli.md#gemini-deep-research).

## Start once

Run `forager gemini research start 'QUESTION' --format json` once. It brings a Chrome window to
the front for up to a few minutes. Tell the user the returned `conversation_url` and Gemini's
`plan`: its title, steps, and `eta_text`.

Never rerun `start` on your own after a failure: every run can create a conversation and spend
quota.

- A failure with `conversation_url` names the conversation it created. Poll that conversation; if
  the message says the plan was not confirmed, ask the user to click "Start research" on that page
  first.
- A failure whose message says forager does not know whether Gemini received the question: ask
  the user to check the Gemini history before anything is started again.
- `auth`: the user signs in to Gemini in Chrome. `quota_exhausted`: the quota refreshes later.
  Report either and stop.

Starting is complete when `start` returned a conversation, or a failure has been reported to the
user with its recovery step.

## Poll until completed

Run `forager gemini research result 'CONVERSATION_URL' --format json` about every 2 minutes. Each
poll takes 15 to 30 seconds in a background window.

- `awaiting_confirmation`: the plan still waits for "Start research"; ask the user to click it on
  the conversation page, then keep polling.
- `running`: report `progress.sources_visited` and `progress.latest_thought` when they change,
  then keep polling.
- `completed`: read `report_path`. For a long report, list its headings with
  `grep -n '^#' REPORT_PATH` and read only the relevant line ranges; `sources_path` lists the
  sources as `[{id, title, url}]`.

Stop after about 60 minutes without `completed`, and give the user the conversation URL. A
`runtime` failure that says the Gemini response structure changed does not go away by polling;
report it and stop.

Polling is complete when the report file has been read, or the user has the conversation URL and
the reason polling stopped.

## Use the report

The report is a Delegated Research Report, not research evidence.

- Relay its findings as Gemini's conclusions, naming Gemini Deep Research and the conversation
  URL.
- Before stating one of its claims as fact, fetch the cited source with `forager fetch` and check
  that the source supports the claim.
- When the user wants forager's own research, place reliable report sources into the research
  plan as known-URL evidence directives, following [`research.md`](research.md); the report itself
  never counts as evidence.

This branch is complete when the user has the report's findings with attribution, and every claim
stated as fact rests on a fetched source.
