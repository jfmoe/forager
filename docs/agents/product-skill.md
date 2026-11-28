# Maintaining the product skill

The product skill, `skills/forager/`, is what an end user's agent reads to use forager. It ships alone, and an agent reads only the files its task routes it to. Every change answers one question: what must an agent read to finish this task? This procedure says where a fact goes, how to add a platform or a command, and how to prove with a fresh-agent trial that a change to routing or structure helped. The review points for the skill's wording are in `CODING_STANDARDS.md`, "Product skill".

## Where a fact goes

`AGENTS.md`, "Where each fact goes", decides whether a fact belongs to the skill at all. Inside the skill, the file is decided by the task that needs the fact:

| File | Holds |
| --- | --- |
| `SKILL.md`, the entry | The routing every task passes through: the branches, the platform table and the rules for adding a platform search, running a command to completion, and when to read diagnostics or feedback |
| `references/direct-retrieval.md`, `references/ordinary-search.md` | One branch each: its commands, the result fields it acts on, its recovery, and its completion criterion |
| `references/platforms/shared.md` | What holds for every platform: refs, content depth, file results, pages, shared recovery |
| `references/platforms/<platform>.md` | One platform: when to use it and which operation fits which need, its costs and opt-in steps, its refs, the result fields an agent acts on, and its failures |
| `references/diagnostics.md` | Configuration, keys, `setup`, `browser` and `doctor` |
| `references/feedback.md` | Telling a defect from configuration, an upstream outage or expected behaviour, and reporting a defect |

Within a file, the rules that decide the outcome come first, inside the first forty lines, because agents often read a file's start and stop; each step ends with its completion criterion. A fact that only some tasks need moves out of the entry into the file those tasks read.

## Adding a platform

The platform checklist (`cargo nextest run --lib platform_checklist`) fails until the platform has its skill file.

1. **Write the row.** Add the platform to the entry's table: a link to its file, the kind of question it answers best, phrased as a user would ask it so an agent matches a task to it without the platform being named, and its cost class: `Free`, `Key:` with whose key or quota, or `Opt-in:` with what the user must enable or log in to.
2. **Write the file.** Copy the structure of an existing platform file with the same cost class (`hn.md` for a free one, `scholar.md` for a keyed one, `reddit.md` for an opt-in one):
   - An opening paragraph: what the platform holds, what it costs, and the pointers to `shared.md` and to Failures.
   - For an opt-in route: what it adds, what enabling it needs, and what it risks, each one line, so the agent can tell the user as the entry's opt-in rule says.
   - One bullet per need, each naming the operation that serves it with its command, then the evidence rule (what to cite, what a result supports), then the step's completion criterion.
   - The ref and URL forms, under `## Refs` or beside the operation that takes them, then `## Result fields` and `## Failures`: only what changes what an agent does. A field whose name explains it, an option `--help` describes and a failure whose message names its fix stay out.
3. **Write the help.** The platform's one-line purpose in `forager platform --help` comes from its doc comment in the command definition; it says what the platform is for and what it costs, so an agent without the skill still finds it.
4. **Measure** the load of a task on the platform, as below.
5. **Run a trial**: the platform table changed, so run the seven standard tasks, as below. When the platform's routes are free or use the maintainer's configured key, also give one fresh agent a question the platform answers best without naming the platform, and check that it reads the platform's file and runs its search.

## Adding a command or an option

- **Syntax stays in `--help`.** A new option needs a description in the command definition; the skill names an option only where it changes what an agent does, such as when to set it.
- **A command an agent should reach for** goes into the file of the branch or platform whose tasks need it, with when to use it, the result fields it acts on and its recovery. When it gives some tasks a different route, the entry's routing changes, and the change needs a trial.
- **A command only a maintainer uses** goes into the procedure that uses it; the skill says at most when an agent should run it at the user's request, as `diagnostics.md` does for `doctor --live`.

## Budgets and load targets

- **Budgets** cap each file and the whole skill, in characters. The repository rule `the_product_skill_stays_within_its_size_budgets` holds them; their values are in its code. Raise one only in a commit whose message says what the extra room is for and why splitting or removing content did not serve.
- **Load targets** cap what one task loads: the entry plus every skill file the task must read. They are targets, not rules; no test checks them, and a trial reports them. A known-URL fetch or an ordinary search loads at most 9,000 characters; a single-platform task, entry plus `shared.md` plus the platform's file, at most 12,000.
- **Measure** a task's load from its route with `cat SKILL.md references/platforms/shared.md references/platforms/hn.md | wc -m`, run in the skill directory and naming the files the route reads. In a trial, add up the characters of the skill files the agent actually read, counting a partial read for what it read.

## Fresh-agent trials

A trial runs the standard tasks, each by a fresh agent, and judges each run against its checklist. It is the only proof that the skill works: no test reads the skill's wording.

### When a trial is required

Run one when a change touches the entry's routing (the branches, the platform rules, the conditions for reading a file), the platform table, the branch structure, or how content is split across files. A wording change inside one platform or branch file needs none. A trial of the skill before the change gives the baseline: the characters each task loaded and the mistakes it made.

### Setting up

1. Freeze a copy of the skill under test and a build of the binary in a scratch directory outside the repository, so the agent reads only the skill.
2. Copy the maintainer's configuration directory into the scratch directory and remove every opt-in route from its platform's order, so that forager itself refuses those routes as it would for an end user; `forager config list` with `FORAGER_CONFIG_DIR` set to the copy shows no opt-in route in any order. Task 6 uses an empty configuration directory instead. The trial thus uses no-credential routes and the maintainer's keys, and never opens an attended route.
3. Give each task its own directory holding a wrapper script named `forager`. The wrapper appends each invocation's arguments to a call log in that directory, sets `FORAGER_CONFIG_DIR` to the task's configuration directory, and runs the frozen binary, except that it refuses, with exit 3 and a message saying the command is unavailable in this session, every command that changes the configuration (`config set`, `config unset`, `setup`), that opens a browser (`browser`), and that runs real checks (`--live`). Judge an agent's reaction to such a refusal as a guard of the trial, not as forager's behaviour.
4. Start a fresh agent per task: a new session or a subagent with no other context. Give it only the skill's path, the wrapper's path as the forager command, the task sentence, and this sentence: "No one will answer questions during this session: where you would ask the user, write the question in your final answer and finish what you can without the answer."

### The standard tasks

Each task is given word for word. Every run is also judged on these, whatever the task: it ran each command to exit, read no skill file it did not need, ran no refused command a second time, and cited only fetched content.

1. "From https://doc.rust-lang.org/book/ch09-02-recoverable-errors-with-result.html, quote what the page says about where the `?` operator can be used."
   - Reads the entry and `direct-retrieval.md` only, within the 9,000 target.
   - Runs `forager fetch` on the URL and no search.
   - Quotes the page's text.
2. "What is the latest stable Rust release, and what are its headline changes?"
   - Reads the entry and `ordinary-search.md` only, within the 9,000 target.
   - Runs `forager search` with `--capabilities` declared, including `web_search`.
   - Answers from fetched official release notes and discloses every capability gap.
3. "Summarise the main objections raised in this discussion: https://news.ycombinator.com/item?id=8863"
   - Goes from the entry straight to `hn.md` and `shared.md`, without `direct-retrieval.md`, within the 12,000 target.
   - Runs `forager platform hn fetch` on the URL as given and reads the discussion file in parts.
   - Attributes each objection to its commenter as an opinion.
4. "What do engineers who have run SQLite as the main database of a production web service say about it? Give me the main arguments for and against, with sources."
   - Runs `forager platform hn search` beside the general search without being told to, and no platform that matches no row of the task.
   - When it tried Reddit and was refused, the answer's coverage note says in one or two sentences what Reddit would add, what enabling it needs and what it risks, and offers to enable it.
   - Gives arguments for and against, each with a fetched source.
5. "What are ordinary buyers in China saying about living with the Fujifilm X100VI: first-hand impressions, not professional reviews?"
   - Recognises Xiaohongshu as the task's main source.
   - Tells the user what Xiaohongshu would add, what enabling it needs and what it risks, and asks whether to enable it.
   - Changes no configuration and runs no browser command, and answers from the other sources meanwhile, saying what they cover.
6. "Searching YouTube through forager does not work for me. Find out why and tell me how to fix it." Run it with an empty configuration directory.
   - Reads the entry and `diagnostics.md`; reads `youtube.md` only for a platform's enabling step, which a missing key is not.
   - Finds the missing key from the error or `forager doctor`.
   - Tells the user how to set the key through stdin or `forager setup`, never asking for the key in the conversation and never setting it itself.
7. "`forager platform hn stories ask --limit 5` gives me items whose `rank` values skip numbers (1, 2, 4, 5, 7). I think that is a bug in forager. Deal with it."
   - Reads the entry, `feedback.md` and `hn.md`.
   - Classifies the gaps as expected behaviour from `hn.md` and explains them to the user.
   - Files no issue and drafts none.

### Judging and recording

From each agent's transcript and call log, record the skill files read with the characters loaded, the forager commands run, whether the answer is right, and every checklist item it missed, with what in the skill led it there. Keep the records in `.local/`. A missed item is a deviation: correct the skill, or record why it stands, in the issue that carries the change; rerun the tasks a correction touches.
