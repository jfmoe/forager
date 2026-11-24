# Writing and running tests

Each rule is proven once, at the cheapest layer that can observe it breaking. Place a test by the question it answers, not by where similar tests happen to be.

## Which layer a test belongs to

| Layer | Answers | Proves |
| --- | --- | --- |
| In-crate tests | Did my change break forager's own logic? | Decisions; response decoding against stored samples; request construction and HTTP semantics against the local fixture server; validation and argument rules; pacing, back-off and deadlines; the platform-independent rules of planning, fallback and attribution |
| Binary unit tests | Does the binary present a result correctly? | Rendering of command outputs and the mapping from errors to exit codes |
| Command contract tests | Does the real binary keep its outer contract? | Each command's outer path and the items of the [outer checklist](#the-outer-checklist) |
| Rule tests | Do the repository's documents and layout follow their rules? | The repository rules and the product skill contract |
| Real-browser tests | Do the browser transport and Page Logic work in the Managed Browser? | Off by default; [`browser-routes.md`](browser-routes.md) runs them |

Start in the crate and move outward only when the rule is a fact of the outer layer.

- **Decoding** goes next to the adapter: give a stored sample to the adapter's highest in-process function that takes an upstream body, or, for a browser route, to the Page Logic judgement that takes an observation. Start no server, because no HTTP fact is under test.
- **Request construction and HTTP semantics** (status mapping, headers, query encoding, redirects, deadlines) go through the real HTTP stack against the shared fixture server, in the crate. Build the route as the provider factory does, or run the orchestration over a configuration document.
- **Validation before any request** stays in the crate. Argument rules (conflicts, ranges, defaults) are proven by parsing the command's definition in process, which covers the argument wiring without spawning the binary.
- **Platform-independent rules** (an empty or unusable route set, a missing key, cursor tampering and resumption, opt-in hints, fallback and attribution) are proven once, in the module that implements them, not once per platform. The command contract layer keeps at most two representatives of each rule: one on an HTTP route and one on a browser route. A platform's own facts that a rule's test touches, such as how its route maps a page position onto its request, belong to that route's tests.
- **A command contract test** spawns the binary and proves only what nothing cheaper can: the command's outer path or an outer checklist item. A test whose rule is not on the checklist belongs in the crate.
- **The command contract tests are one binary**, `tests/command_contract/`, organised by domain, and they reach the binary only through the shared test environment in `tests/support/`. A new contract test is a module of that binary, never a new file directly under `tests/`: every test binary links the library again and rebuilds after any source change, and a repository rule fixes the set of binaries.

### Waiting on real time

No in-crate test waits on real time, because a suite that sleeps grows slower with every test and its duration stops reflecting work.

- In the test build, a route's limiter has no minimum interval, so a test that builds routes through the provider factory or the orchestration never waits on an Access Policy. Nothing outside the test build can build such a limiter, so no release can bypass an Access Policy.
- A test about pacing holds `paced_limiters()` while it builds the route, and either runs on the paused Tokio clock or is arranged so that a window fails at once instead of being awaited.
- A test may combine the paused clock with a real loopback socket only when it waits for the request to arrive before it advances the clock. Otherwise the paused clock jumps ahead while the response is in flight and the outcome depends on scheduling.
- No in-crate test waits out a deadline either. On the paused clock any deadline works; on the real clock the test makes the deadline decide at once, as a window that cannot fit it does, or sets it far longer than the test runs.
- The spawned binary keeps production pacing, so a command contract test pays one interval for every paced request after the first. Pick the operation that makes the fewest paced requests.
- A command contract test may wait on pacing, or on a deadline that the CLI takes in whole seconds, only when it is on the exemption list in the runner configuration, whose entry says in one sentence why no operation without a wait proves its rule. A new entry needs such a reason; otherwise the rule moves into the crate or to an operation that does not wait. The list is kept short, because each entry is time every run pays.
- The runner terminates any other test that runs past its limit and names it in the summary, so a slow test is caught when it is written. The CI profile's limit is higher, because CI runners have far fewer cores.
- The shared test environment's own deadlines stay under the limit that applies to the test, so a test that waits for something that never comes fails with its own diagnosis instead of a termination: the commands a test starts are killed once they use up a share of that limit, which the exemption list raises for the tests it names, and a fixture still missing a request when the test finishes it fails naming how many requests arrived. A test that waits without being on the list therefore fails at the command deadline, with the command's output.

## Selecting a subset

A test's path names what it belongs to. A *word* is one `_`-separated part of a module name in the test's path. In the crate, a provider's tests live in its adapter's modules, which carry the provider ID, and a platform's types live in `types::platform_<platform>`. In the command contract binary, a platform's tests live in `platforms::<platform>`, and on a platform with several routes one level further down, in a module named after the route that serves the command, such as `platforms::youtube::youtube_web`. A module about one provider elsewhere carries its ID, such as `web::exa_search` or `doctor::serpapi`, and a module about a shared mechanism, such as `access_windows`, names that mechanism; a test in a module that covers several providers, such as the doctor tests at the root of `doctor`, carries no provider's word, so no provider filter selects it. Because placement sets the words, moving a test to the right module is enough to make the filters find it.

One filter selects a provider's or a platform's tests, with `IDS` replaced:

```sh
cargo nextest run -E 'test(/(^|[:_])(IDS)(_\w*)?::/)'
```

- **A provider:** its ID, plus each platform that it is the only route of. The `xai` provider is `xai|x`, because it is the X platform's only route; `youtube_web` is `youtube_web`, because YouTube has other routes. `gemini_api` alone also selects YouTube's ask and transcribe commands, because it is the only route that serves them.
- **A platform:** its ID, plus each of its routes that serves nothing else. Google Scholar is `scholar|serpapi`. X is `x` alone: the `xai` route also backs main search, and listing it would select main-search tests.

The routes of each platform are in the platform catalog and in the product skill. Words match whole, so the X filter never selects `xai`, and `hn` never selects a module that merely starts with the letters.

## Local and CI runs

The local default run runs every test the CI run does except the release artifact gate, which packages a release and takes longer than any one test may; apart from that the two profiles differ only in how long a test may take. The default run's summary counts the gate's tests among the skipped ones, beside the ignored real-browser tests. After a change to the release scripts, run the gate through the CI profile, because under the default profile naming its target selects no tests:

```sh
cargo nextest run --profile ci --locked --test release_artifact_gate
```

No profile retries a failed test, so a red run always means something; a test that fails only sometimes is fixed, not retried.

## Upstream samples

The sample store is `tests/samples/<provider>/`. Every upstream body a test replays comes from there, and the in-crate tests and the command contract tests read the same file, so an upstream change is fixed in one place. Tests locate samples relative to the crate root, because the runner does not fix the working directory.

- A *shape* is one response structure of one upstream endpoint, and it is stored once. Derive a variant (an error, an empty result, another page) from the base sample inside the test, or store it as a named variant whose record says `variant_of`.
- An input built to break a decoder, such as a truncated body or a page that is not XML, is not an upstream shape: write it in the test, because it records nothing the upstream did and no refresh would ever change it.
- Each directory's `samples.toml` has one record per sample: `file`, `shape`, `origin` (`captured` or `hand-written`), the `observed` date and, for a captured sample only, `obtained`, which says how it was captured. A repository rule fails, naming the sample, when a sample has no record or a record names no sample.
- `observed` is the date the shape was seen at the upstream. For a hand-written sample of unknown basis, it is the date the sample was written, and its origin is `hand-written`.

### Adding or refreshing a sample

1. Capture the response with a request a user of the route could make, or write the sample by hand from an observation.
2. Before committing, remove every key, token, account identifier and personal content, replacing each with a fake value of the same form. The repository is public and a published sample cannot be taken back. A private person's identifiers and content are always replaced; an official organisation's account and a public figure's well-known public content may stay, because publishing them again exposes no one.
3. Never commit a page behind a login as captured. Commit only a hand-written reconstruction based on what you observed, so no account's content or identifiers leave the machine.
4. Trim the sample to the fields the route reads and the structure around them, so a reader can see what the route depends on.
5. Write or update its record: the new `observed` date and, for a captured sample, how it was `obtained`.
6. Run the route's subset; a refreshed sample that changes a test's expected values belongs in its own commit that says what the upstream changed.

## The outer checklist

Only the real binary can prove these facts, and each has at least one command contract test. Before moving or retiring a command contract test, make sure every item it proves keeps another test.

- Each exit code: 0, 2, 3, 4 and 5.
- Each output format (JSON, Markdown, content) for each kind of output that offers it: search results, platform items, fetched bodies.
- Writing the result to an output file, and the receipt.
- A failed command still printing its failure payload as JSON on stdout.
- Environment variable overrides of configuration.
- The preflight output of a configuration error.
- The Search Result Journal being written when it is on and not written when it is off.
- The file names and the default location of a content directory.
- Environment isolation: the spawned binary reads only the configuration and state the test gives it, and a route its configuration missed fails instead of reaching a site.

Every command keeps at least one command contract test, and every platform keeps at least one successful path that its offline stand-in can produce. The browser platforms' stand-in does not speak the browser protocol, so for them the contract layer keeps the outermost path it can prove (preflight, launch arguments, refusal and timeout); their successful path is proven by the Page Logic tests in the crate and by the real-browser tests.

## What each layer does not prove

- **No offline layer** proves that an upstream still behaves as its sample shows. A sample is frozen at its `observed` date.
- **In-crate tests** do not prove exit codes, rendering, output files, environment overrides or the journal; the binary's outer path owns those.
- **Binary unit tests** do not prove argument parsing or anything that needs a process.
- **Command contract tests** do not prove decoding detail, because they replay a few samples, and do not prove a browser platform's successful path.
- **Real-browser tests** run against local pages, so they prove Page Logic's scripts, not a real site.

## Retiring or rewriting a test

A test may be retired or rewritten when it:

- is **fragile**: a behavior-preserving refactor forces edits, because it asserts private fields, internal names or call order;
- is a **change detector**: it mirrors the implementation and asserts no result, so it fails on every change and catches no bug;
- is **tautological**: it computes its expected value the way the code does, so it agrees with any bug;
- is **erratic**: its result depends on timing, arrival order, the wall clock or shared state, so a red run means nothing;
- is **slow**: most of its time is setup or waiting rather than the rule;
- is **duplicated**: another test proves the same rule with the same oracle and input class, so a rule change would edit both;
- proves **the wrong thing**: framework glue, a configuration literal, or that a retired behavior is absent.

A retirement or a change to an expected value never shares a commit with moved tests, so a reviewer can tell a moved proof from a changed one. Several retirements of one provider may share a commit, as may several changes to expected values; the message names, for each test, the rule it proved and what proves it now, or why the rule needs no proof, so a reviewer can check that no proof was lost. A commit that moves tests changes no expected value, and a behavior test is never weakened to make a move easier.
