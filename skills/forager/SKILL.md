---
name: forager
description: "Fetch known URLs, search the live web, and read platform items (arXiv, SSRN, Google Scholar, Hacker News, Polymarket, YouTube, X, Reddit, Xiaohongshu 小红书) with the local forager CLI. Use for reading a given page or PDF, questions that need current or source-backed answers, official documentation lookup, multi-source research, and forager configuration errors or defects."
---

# forager

Run the local `forager` CLI for every retrieval below; it picks providers and falls back on its
own. If it is not installed, say so and stop; if a command or option named here is missing, report
`forager --version` and ask the user to upgrade. `forager <command> --help` gives every option.

## Choose the branch

Take the cheapest branch that can finish the task, and move on only when its shape or an observed
shortfall shows the current one cannot.

1. **Direct retrieval**: the task gives a URL or PDF, or asks which pages a site has. Read
   [`references/direct-retrieval.md`](references/direct-retrieval.md).
2. **Ordinary search**: one live search can answer it. Read
   [`references/ordinary-search.md`](references/ordinary-search.md) before you run the search.
3. **Investigation**: the question needs decomposition or several sources. Split it into parts;
   run `forager search` for a part that needs discovery, `forager fetch` for each source that may
   answer it, and the platform searches the table below matches. Judge a source by whether it
   bears on the question. Cite only fetched content and name each part no source answered. Stop
   when more searches are unlikely to change the answer, the budget is spent, or access is
   blocked, and say what remains open.

## Platforms

A platform is one of these nine services and nothing else. A WeChat article, a Discord question or a
page on any other site goes through direct retrieval or ordinary search.

- **Match a row, add its search.** When the task asks what a row of the table below answers best,
  run that platform beside your branch, whether or not the user named it, after reading its file
  (each builds on [`references/platforms/shared.md`](references/platforms/shared.md)). When the
  task matches no row, run no platform command.
- **Opt-in**: the route is off until the user enables it. When the task would gain from it and
  forager refuses because the route is off, tell the user what the platform would add, what
  enabling it needs, and what it risks (their account or network address, against the site's
  terms), then wait for their answer, continuing from the other sources meanwhile. Enabling,
  installing and logging in stay with the user.
- **Key**: the user's key or quota pays for each request; spend it only on what the task needs.

| Platform | Answers best | Cost |
| --- | --- | --- |
| [arXiv](references/platforms/arxiv.md) | Preprints in physics, maths, CS, quantitative finance; full text | Free |
| [SSRN](references/platforms/ssrn.md) | Working papers in finance, economics, law; abstracts | Free |
| [Google Scholar](references/platforms/scholar.md) | Papers across publishers, citation counts, who cites a paper | Key: SerpApi quota |
| [Hacker News](references/platforms/hn.md) | What practitioners think of a tool, product or technical idea | Free |
| [Polymarket](references/platforms/polymarket.md) | How likely an event is: live market probabilities | Free |
| [YouTube](references/platforms/youtube.md) | Talks, lectures, tutorials; what a video says or shows | Key: YouTube and Gemini; captions opt-in |
| [X](references/platforms/x.md) | What accounts post on X now; reactions to news | Key: main search's xAI key |
| [Reddit](references/platforms/reddit.md) | User experience and community discussion in English | Opt-in: user's login |
| [Xiaohongshu 小红书](references/platforms/xiaohongshu.md) | Chinese consumer reviews, lifestyle and travel experience | Opt-in: user's login |

## Run each command to completion

A command is done only when it exits. When the runner hands back a handle, wait on it until exit
before you read output, diagnose or retry. A search can run a minute or longer: keep forager's
default `--timeout` and give the runner more. Judge the outcome by the exit code; a failure prints
its payload on stdout too.

## Configure, diagnose or report

- A configuration, credential or quota error, or a request to set up or check forager: read
  [`references/diagnostics.md`](references/diagnostics.md).
- A result that looks wrong, a command that fails without a configuration cause, or a request to
  report a forager problem: read [`references/feedback.md`](references/feedback.md).
