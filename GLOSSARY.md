# forager

forager organizes its providers by retrieval capability, and organizes external content sources
that have their own identity space as platforms.

## Language

**Capability Seam**:
A stable contract defined around one retrieval capability and shared only by the providers that
offer that capability; different capabilities need not have the same operations.
_Avoid_: universal provider interface, extra search channel

**Documentation Search Capability**:
The capability that searches documentation of external libraries, SDKs, APIs, frameworks and
official technical sources. An ordinary technical question, or one that merely mentions a
technical name, is not by itself a need for this capability.
_Avoid_: general technical search, docs provider

**Supplemental Web Search Capability**:
Web discovery beside main search, for recency, news, region, policy, market data, cross-validation
or stronger sourcing. It is not the main search that every ordinary search runs.
_Avoid_: main search, all web access, web provider

**Primary Search Source**:
A source that the main-search answer is attributed to, reported in search's `sources`. It differs
from the candidates that other capabilities produce and nobody has consumed yet, and being cited
does not make it verified evidence.
_Avoid_: supplemental source, verified evidence, all search sources

**Search Candidate**:
A candidate that a capability other than main search produces during an ordinary search, for the
caller to choose its next action from; reported in `extra_sources`. A candidate is located by a URL,
and it is not evidence until the caller retrieves its body.
_Avoid_: Supplemental Search Candidate, primary source, verified evidence, merged source

**Web Fetch Capability**:
The capability that reads, extracts or verifies the body of a known source; its input is a known
URL. It does not discover links.
_Avoid_: web search, link discovery

**Normalized Fetch Content**:
The provider-independent Markdown body that the Web Fetch Capability accepts after provider
decoding and the quality gate. It contains no transport wrapper or diagnostics, and different
providers are not promised to choose the same body boundaries.
_Avoid_: raw provider response, cleaned content

**Default Search Invocation**:
One request through `forager search`, forager's default aggregate search entry point, that reaches a
terminal state, whether it succeeded, failed or timed out. Provider-direct commands, fetches,
diagnostics and other dedicated operations are not Default Search Invocations.
_Avoid_: successful search, provider search, all commands

**Multi-source Investigation**:
An investigation that the calling agent runs itself: it splits a question that one ordinary search
cannot settle, calls forager several times across searches and direct retrieval, and judges each
source on whether it bears on the question. forager has no command or pipeline for it, and every
call it receives is an ordinary search or a direct retrieval. An ordinary search is a single
Default Search Invocation whose answer and candidates only orient the caller; splitting,
escalating and judging evidence happen in the caller.
_Avoid_: deep research, research pipeline, forager-side planning

**Search Result Journal**:
The local record, on by default and in order of occurrence, of the result and the execution of
every Default Search Invocation. The result side holds the query, the answer and the sources; the
execution side holds the plan summary, the provider attempt chain, the terminal attribution, the
budget and the capability gaps. It never stores request or response headers, request bodies, raw
response bodies, credentials or tool traces. It serves review and diagnosis; it is not a debug
message stream and not a cache of search responses.
_Avoid_: debug log, response cache, search history

**Attempt Trace**:
The read-only facts derived from a provider attempt chain: the terminal attribution, whether a
fallback happened, the provider that succeeded, and a bounded summary. Every consumer (the Search
Result Journal, the stderr attempt log and exit-code attribution) reads its projection instead of
interpreting the raw chain again. A fallback happened when the run degraded at all, including a
model change within one provider.
_Avoid_: per-call-site interpretation, raw attempt scan

**Caller Capability Declaration**:
The complete and authoritative capability set that a caller declares for one ordinary
`forager search`; an empty set is a valid declaration. A search without a declaration is an empty
declaration and runs main search only. forager never adds to it from local rules or URL
recognition. A declaration names Capability Seams and never selects providers.
_Avoid_: provider override, router hint, capability patch

**Capability Vocabulary**:
The single vocabulary of the capabilities a caller may declare: each one's identity, canonical
order, selection meaning and example phrases. Identity and order are compiled into the CLI, and
`--capabilities` validation reads them there; the product skill's vocabulary file adds selection
meaning and example phrases, and a product skill contract test binds its identities and order to
the compiled ones. Provider fallback and routing control flow are not part of it.
_Avoid_: Skill capability list, routing rules DSL, provider registry

**Citation Binding**:
A machine-readable link between an inline marker in an answer and one source.
It expresses attribution; it does not mean the system has verified that the evidence supports the
statement.
_Avoid_: claim verification, source list

**Provider Credential Pool**:
The credentials of a provider that uses them, configured as a TOML `keys` array; every such
provider has this shape, and a single credential is a one-element array. A provider that needs no
credentials has no `keys`. At run time the pool rotates through its credentials and, on a quota
or rate-limit failure, switches to another credential within the same request. A provider whose
registration says it needs no credentials never selects or rotates one and always counts as
configured. A provider whose credentials are optional also has `keys`; when they are empty it runs
anonymously and likewise always counts as configured.
_Avoid_: key pool, key rotation, API key list, high-availability credential pool

**Anonymous Fallback**:
The rule by which a provider with optional credentials sends one more request without a
credential. With an empty Provider Credential Pool it requests anonymously from the start. When
the pool, following its own rules (switching credentials on quota and rate limits, stopping on an
authentication failure), ends in an authentication, quota or rate-limit failure, the provider
requests once more anonymously in the time left in its slot. Failures unrelated to credentials do
not trigger it, it happens at most once per run, and the anonymous attempt is marked `anonymous`
(ADR-0029).
_Avoid_: keyless mode, free tier fallback, anonymous retry

**Access Policy**:
The request pace a provider declares for its endpoint: a minimum interval and a maximum
concurrency. Every forager process sharing one state directory shares that pace and concurrency.
When the shared state is unavailable nothing is sent, and a request that cannot get its turn
within the budget ends in a timeout.
_Avoid_: throttle, retry backoff, provider quota

**Capability Catalog**:
The single source of the legal providers of each Capability Seam. Default configuration order,
order validation, provider construction, doctor probes and smoke cases all derive from it; there
is no second, parallel list of which provider belongs to which seam.
_Avoid_: provider registry, BACKENDS allowlist, per-call-site provider list

**Provider HTTP Read Contract**:
The read rules that every provider response passes at forager's network boundary: error
attribution from the HTTP status and from quota hints in rate-limit responses, a hard cap on
response size, UTF-8-safe truncation of fetched bodies, bounded error bodies, and redaction of
credentials and URLs. A provider adapter only builds its request and decodes its own success
format; it never assembles these rules itself.
_Avoid_: per-adapter read recipe, raw reqwest handling

**Platform**:
A built-in external content source with its own identity space, such as arXiv, SSRN or Google
Scholar. It stands beside the Capability Seams, and platforms never fall back to one another. A
platform is not a provider.
_Avoid_: platform provider, custom platform

**Platform Route**:
One way to reach a Platform. A route is a provider, and the routes of one platform form a
fallback chain in their configured order. The Capability Catalog declares which routes serve each
Platform Operation; a route that serves no operation of a platform is not one of its routes. A
route's identity is never the bare platform name.
_Avoid_: platform backend, platform provider list

**Route Transport**:
How a Platform Route reaches its source, declared in the provider's registration: HTTP (configured
with `url`, plus `keys` when the route needs credentials), or forager's browser transport. A route
on the browser transport is a browser route; its only setting is `timeout`, and its registration
also declares the browser language. It uses the Browser Profile of the platform it serves, which
needs a login. A browser route runs only when the user adds it to the platform's order
(ADR-0020).
_Avoid_: route kind, backend type, process route

**Page Logic**:
A browser route's own knowledge of its site: which pages one browser command opens, which native
input it performs, and which requests it waits for and keeps. It produces observations (page
facts, captured requests and responses, the reason it could not finish). Judging those
observations into a result or a failure is a pure function that needs no browser, and it also
belongs to the route. The browser transport itself has no page logic.
_Avoid_: adapter, browser script, scraper

**Managed Browser**:
The browser (fingerprint-chromium) that forager downloads, pins to one version and checks by
SHA256. `forager browser install` installs it into forager's cache under a versioned directory,
and forager runs only the pinned version. When `browser.path` is set, the local browser it names
replaces it.
_Avoid_: bundled Chrome, system browser

**Browser Profile**:
A persistent browser environment dedicated to one platform, kept in the state directory and named
after the platform id. It holds the platform's login state, a fingerprint seed generated once when
it is created, and the host's screen size at login. Only one process can use it at a time. A login
marker says only that a login happened, not that the session is still valid; forager never reads
or exports the cookies it holds.
_Avoid_: browser session, cookie store, user data dir

**Platform Ref**:
The typed identity of a Platform entity: the platform, a kind of the platform's own, and an id,
optionally with a version. An SSRN ref has no version, because SSRN keeps a paper's DOI when it
revises the paper, so no version can be derived. A ref and its canonical URL can each be derived
from the other. Kinds divide only by identity space or by the shape of what is retrieved, never
by conversational role.
_Avoid_: platform URL, raw id

**Access Token**:
An access parameter needed to open a platform entity that is not part of its identity, such as
`xsec_token` in a Xiaohongshu note URL. One entity gets different tokens at different times and
from different entry points, and a token works only in the user's own logged-in session. It never
enters the Platform Ref, the canonical URL, a Page Cursor or the Search Result Journal; it appears
only in the access link that the route builds (`access_url`), is masked in debug output and cannot
be serialized.
_Avoid_: credential, key, session token, ref version

**Scholar Cluster**:
The identity under which Google Scholar groups the versions of one paper (the publisher's page,
preprints, third-party copies), written as a decimal cluster ID. A Google Scholar Platform Ref
points to the cluster, not to one of its versions. The first version in a cluster is not
necessarily the published one, and a cluster can include other papers.
_Avoid_: scholar paper id, result id, version

**SERP API Route**:
A Platform Route that reads search engine result pages through a third-party search results API,
such as `serpapi`. It is named after the vendor, and its key, supplied by the user, joins the
Provider Credential Pool; one account's quota and throughput are shared across engines. Every
successful search spends the user's quota, and forager keeps no quota state (ADR-0021).
_Avoid_: scholar scraper, scholar_serpapi

**Platform Operation**:
An operation of a Platform. Every Platform offers search and fetch. An operation that changes the
kind of result or the required input is a new Platform Operation, not an option: for example
Google Scholar's Cited-by, Comments on Xiaohongshu and YouTube, Ask on X and YouTube, Hacker News
Stories, and YouTube's Videos and Transcribe. Each operation's routes are declared in the
Capability Catalog.
_Avoid_: platform option, sub-command flag

**Cited-by**:
The Platform Operation that lists the works Google Scholar counts as citing a paper. Its result
has the same shape as search, and both a paper nobody cites and a paper that does not exist give
an empty list.
_Avoid_: citations, references, backlinks

**Comments**:
The Platform Operation that lists the top-level comments of a platform item. Each platform defines
how far replies and further pages go: Xiaohongshu can expand the first page of replies to the
first few comments and issues no cursor, and YouTube includes only the replies returned with the
comments. An item without comments gives an empty list; comments being turned off is not an empty
list.
_Avoid_: replies (the replies themselves), thread

**Ask**:
The Platform Operation in which a model writes an answer from platform content alone. The result
is one answer with its sources, not a list of items, and the whole answer is Relayed Content. Each
platform defines what the answer rests on: on X, the posts the model found on X; on YouTube, the
picture and sound of one video.
_Avoid_: X answer search, summary, main search, video analysis

**Videos**:
The Platform Operation that lists the videos in a YouTube playlist or channel. Each item carries
its position in the collection, and a channel's videos are newest first.
_Avoid_: playlist fetch, channel search, uploads

**Transcript**:
A timestamped document rendered from one caption track of a video, labelled with the track's
language and origin: captions the publisher uploaded, captions the platform recognized
automatically, or captions a model generated. A Transcript of captions already on the platform is
the video's `full_text`; a model-generated Transcript is delivered only by Transcribe.
_Avoid_: subtitles dump, captions file

**Transcribe**:
The Platform Operation in which a model watches and listens to a video and generates a Transcript.
The whole result is Relayed Content. It runs only when the caller asks for it explicitly and never
stands in automatically for missing platform captions.
_Avoid_: ASR fallback, generated full text

**Precheck**:
The step in which a model operation (YouTube's Ask and Transcribe) reads the same video through
the platform's Platform Fetch chain before it calls the model. If that read fails, or the video
is live, the command fails without calling the model. The item read is handed to the model route,
which never reads the platform itself, and the precheck's attempts come before the model route's.
_Avoid_: preflight (the checks made before any request), metadata check, validation

**Stories**:
The Platform Operation that lists the current items of one of Hacker News's official lists,
ordered by their rank on the list; it issues no cursor.
_Avoid_: front page search, HN feed

**Polymarket Event**:
A question or topic presented to people on Polymarket, containing one or more Polymarket Markets;
the default kind of a Polymarket platform item. Exactly one market of an event made of mutually
exclusive markets resolves Yes, but the markets' Market-Implied Probabilities need not sum to 1.
_Avoid_: question, market group

**Polymarket Market**:
A question within a Polymarket Event that resolves on its own. It has two or more outcomes (usually
Yes and No), and each outcome trades separately on the order book.
_Avoid_: contract, outcome, event

**Market-Implied Probability**:
The midpoint between the best bid and the best ask for one outcome of a Polymarket Market, when
both sides of its order book have orders. It does not exist when either side is empty or the
market has stopped trading; a resolved market gives its payout value, not a probability. It
expresses how traders price the outcome now; it is not a calibrated forecast and not the price the
website shows.
_Avoid_: odds, chance, displayed price, outcome price

**Price History**:
The Platform Operation that lists the price series of one outcome of a Polymarket Market over a
period. The series holds only market prices; for a resolved market the payout value is given
separately, never mixed into the series. A price series is not a history of the Market-Implied
Probability: the platform does not say whether a historical point is a trade price or a midpoint.
_Avoid_: probability history, chart, price feed

**Platform Fetch**:
The operation that retrieves one platform item by its Platform Ref. It is not the Web Fetch
Capability, although its body step can reuse Web Fetch. The platform route first retrieves the
metadata and settles the actual version; a full-text depth then retrieves the body from the source
the route declares: a list of URLs, tried in turn through one Web Fetch chain, or a platform-native
body that the route read and checked itself within the same attempt (Native, such as a Xiaohongshu
note), which skips Web Fetch (ADR-0022). The full text is written to a local file, and the result
refers to it only by path.
_Avoid_: web fetch, platform download

**Content Depth**:
How much content a platform item result holds: `metadata` (bibliographic data only, no abstract),
`snippet`, `abstract`, `full_text` or `thread`. Each Platform defines what the depths it supports
mean, and there is no global order between depths. An abstract depth is never the full text, and a
snippet is never the abstract. For SSRN the requested depth is a minimum, and the item's `depth`
records what the route actually obtained.
_Avoid_: detail level, verbosity

**Relayed Content**:
Item content a Platform Route obtains through a model's retelling: the model's tool read the
original, and forager receives only the text the model wrote. The route can confirm the item's
identity from checkable facts, but the body and the author are not guaranteed verbatim; for
example the X posts read through xAI, and the answers and captions a model writes after watching a
YouTube video.
_Avoid_: platform-native content, verbatim text, quoted post

**Page Cursor**:
An opaque paging token for platform search results. It fully restores the original request and the
position of the next page, and continues only on the route and Platform Operation that issued it.
No cursor only means the results cannot be continued in another command; it does not prove they
were all read. Xiaohongshu orders results differently on every visit, so its search never issues a
cursor.
_Avoid_: offset, page token

**SSRN Search Criteria**:
The search scope, authors and institutions, dates, metadata conditions and sort order that one
SSRN search asks for. Every Platform Route applies them in full, and a condition with a similar
name but a different meaning never stands in for another.
_Avoid_: raw provider parameters, best-effort filters

**Spec**:
The description of one change, kept in a GitHub issue and closed when the change ships. An issue
is never the source of a current fact: on delivery, what a CLI user can observe goes into the
product skill, how it works stays in the code, and a hard-to-reverse trade-off goes into an ADR.
_Avoid_: standing spec, spec chapter

**Upstream Sample**:
One stored body of one upstream response, or one recorded observation of a browser route, that
tests replay instead of reaching the upstream. Its record says whether it was captured from a real
response or written by hand, and when its shape was seen. A sample shows how the upstream answered
then; it never proves the upstream still answers that way.
_Avoid_: fixture (the local server that serves a sample), mock, golden file

**Shape**:
One response structure of one upstream endpoint. Each shape is stored once as an Upstream Sample;
an error, an empty result or another page of the same endpoint is a variant of that shape.
_Avoid_: response type, endpoint

**Command Contract Test**:
A test that spawns the real forager binary to prove what only the binary seen from outside can
show. A rule that a test in the crate or a binary unit test can observe is not a command contract.
_Avoid_: end-to-end test, integration test, offline end-to-end test
