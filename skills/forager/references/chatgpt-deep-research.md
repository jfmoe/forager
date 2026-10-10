# ChatGPT Deep Research

ChatGPT Deep Research runs in the user's own logged-in Chrome, spends the account's Deep Research
quota, and takes several minutes, sometimes up to 60. Enter this branch only when the user
explicitly asks for ChatGPT Deep Research. For exact syntax, exit codes, and adapter installation,
read the ChatGPT Deep Research section of [`cli.md`](cli.md#chatgpt-deep-research).

## Start once

Run `forager chatgpt research start 'QUESTION' --format json` once. It brings a Chrome window to
the front for two to four minutes, because it keeps the page open until the plan's countdown starts
the research, and it leaves the ChatGPT composer in chat mode; tell the user both side effects. Tell the user the returned `conversation_url` and ChatGPT's `plan`: its title and
steps.

Never rerun `start` on your own after a failure: every run can create a conversation and spend
quota.

- A failure with `conversation_url` names the conversation it created; poll that conversation.
  - The plan has not started: its countdown may still start it, so poll before anything else.
  - ChatGPT replied with text instead of a plan: show the user the quoted reply, ask them to
    answer it on the conversation page, then poll.
- A failure whose message says nothing was sent: no conversation exists. Report the reason; when
  the message says the draft was not cleared, ask the user to empty the ChatGPT composer.
- A failure whose message says forager does not know whether ChatGPT received the question: ask
  the user to check the ChatGPT history before anything is started again.
- `auth`: the user signs in to ChatGPT in Chrome. `quota_exhausted`: the quota resets later.
  Report either and stop.

Starting is complete when `start` returned a conversation, or a failure has been reported to the
user with its recovery step.

## Poll until completed

Run `forager chatgpt research result 'CONVERSATION_URL' --format json` about every 2 minutes. Each
poll takes 15 to 30 seconds in a background window.

- `awaiting_confirmation`: the plan waits for its countdown, or for the user to click "Start";
  keep polling, and ask the user to click "Start" on the conversation page if it still waits
  after a few minutes.
- `running`: report `plan_title` and `progress.steps_completed` of `progress.steps_total` when
  they change, then keep polling.
- `completed`: read `report_path`. For a long report, list its headings with
  `grep -n '^#' REPORT_PATH` and read only the relevant line ranges; `sources_path` lists the
  sources as `[{id, title, url}]`.

Stop after about 60 minutes without `completed`, and give the user the conversation URL. A
`runtime` failure that says the ChatGPT response structure changed does not go away by polling;
report it and stop.

Polling is complete when the report file has been read, or the user has the conversation URL and
the reason polling stopped.

## Use the report

The report is a Delegated Research Report, not research evidence.

- Relay its findings as ChatGPT's conclusions, naming ChatGPT Deep Research and the conversation
  URL.
- The report can come in another language or script than the question (a Simplified Chinese
  prompt has produced a Traditional Chinese report). When that matters, convert it yourself or
  ask the user to request the language in a follow-up on the conversation page.
- Before stating one of its claims as fact, fetch the cited source with `forager fetch` and check
  that the source supports the claim.
- When the user wants forager's own research, place reliable report sources into the research
  plan as known-URL evidence directives, following [`research.md`](research.md); the report itself
  never counts as evidence.

This branch is complete when the user has the report's findings with attribution, and every claim
stated as fact rests on a fetched source.
