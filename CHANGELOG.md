# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.23.1](https://github.com/jfmoe/forager/compare/v0.23.0...v0.23.1) - 2026-11-28

### Fixed

- *(state)* leave a non-file under the sweep marker's name alone
- *(state)* repair state files whose content is not UTF-8
- *(browser)* remove old Managed Browser versions on install

### Other

- *(state)* name the record ownership check by what it returns
- *(state)* sweep expired records at most once an hour
- *(journal)* expire journal records by the time in their names
- *(dist)* strip the symbol table from release binaries
- *(state)* stop syncing credential cursor and Access Policy writes
- *(state)* let each state file say whether its write syncs
- *(credentials)* select a single key without the cursor state
- *(browser)* split placing the package out of install
- *(deps)* state the reqwest decision where Dependabot ignores it

## [0.23.0](https://github.com/jfmoe/forager/compare/v0.22.0...v0.23.0) - 2026-11-28

### Fixed

- *(doctor)* search Xiaohongshu by most comments in its Live Check
- *(doctor)* pass the Xiaohongshu comments check on a note without threads
- *(doctor)* read ten results in the Exa docs search Live Check

### Other

- bump the github-actions group across 1 directory with 6 updates

## [0.22.0](https://github.com/jfmoe/forager/compare/v0.21.0...v0.22.0) - 2026-11-28

### Added

- *(skill)* [**breaking**] rebuild the product skill around what an agent's task needs
- *(doctor)* keep one issue per provider from scheduled Live Checks
- *(ci)* build each release target and check advisories weekly
- *(doctor)* summarise real use and the schedules' freshness offline
- *(telemetry)* record every retrieval command's health signals locally
- *(log)* [**breaking**] collect every attempt once and narrow log.level to three values
- *(doctor)* assert every decoded field in Live Checks and record each run
- *(doctor)* [**breaking**] replace smoke with Live Checks run by doctor --live
- *(doctor)* [**breaking**] send no billed request from doctor for one provider
- *(doctor)* add an offline mode and check the state directories
- [**breaking**] remove Context7; Documentation Search is served by Exa alone
- [**breaking**] remove the classifier; an undeclared search runs main search only
- [**breaking**] remove the research command and its evidence pipeline

### Fixed

- *(skill)* route platform URLs, opt-in offers and config errors as trials need
- *(skill)* keep code values out of the skill and the deciding rules on top
- *(schedules)* signal every run the jobs cannot act on
- *(doctor)* name a command the pipeline issue can run and close a retired check's issue
- *(schedules)* notify only what the issue job did, after the workflows
- *(doctor)* name a provider that fails most of its calls in the health summary
- *(cli)* keep the exit code and telemetry when the reader closes the pipe
- *(telemetry)* classify an unwritten output copy and skipped routes
- *(doctor)* fail a contract field under a missing object and mark confirmation
- *(doctor)* tighten the Live Check contracts and keep reruns off browser routes
- *(youtube_web)* report a caption body that is not srv1 as invalid

### Other

- *(release)* stop pinning the app token action's version
- add the product skill's maintenance procedure and review points
- *(tests)* move the installable skill rule into the repository rules
- hold reqwest major updates out of Dependabot
- *(deps)* upgrade toml_edit to 0.25
- *(deps)* upgrade toml to 1.2
- *(deps)* upgrade nix to 0.32
- *(deps)* upgrade sha2 to 0.11
- open weekly Dependabot updates
- forbid unsafe code through the manifest's lint table
- replace fs2 with the standard library's file locking
- *(deps)* update the lockfile to the latest compatible versions
- move the toolchain to Rust 1.100.1
- add review standards for mistakes no test can catch
- *(agents)* reject a check command piped into a filter
- turn off debug info in the dev profile
- keep agents off manual-only checks and state the jobs' prerequisites
- *(schedules)* close the schedule rule's blind spots and bind the records and skill lists
- *(infra)* share one timestamped record directory between run records and telemetry
- *(doctor)* name a health entry by one rule and read telemetry's switch once
- *(doctor)* give doctor rendering its own module and name the fallback rule
- *(cli)* gather a result's presentation into one type
- *(test)* inline the telemetry tests and share the empty environment
- *(core)* lead every chain with earlier attempts in one place
- *(types)* move the Live Check rule vocabulary into the shared types
- *(doctor)* name the Live Check rules and the record error as types
- *(catalog)* define the opt-in route rule once
- *(doctor)* build both all-provider reports in one place
- *(doctor)* separate the local provider check from reachability
- *(runner)* exempt the access window test that sends to every route
- give each testing fact one owner and guard the contract layout
- close the gaps the offline test rebuild left in its own proofs
- *(tests)* read samples through one reader and share the environment
- expect what the stored samples hold where a test stopped hand-writing its record
- derive every valid upstream record from the sample store
- *(testing)* close the gaps a fresh agent fell into
- *(testing)* give rendering to the binary and every layer its limits
- *(testing)* describe each test layer by the question it answers
- *(runner)* terminate a test after 2 s by default and 6 s in CI
- prove the timing-dependent rules by an effect instead of the clock
- keep each rule's proof in the one test that owns it
- prove the failure exit codes, a fetched body's formats and the log level
- *(rate-limit)* stop waiting out the state lock in the test build
- *(rules)* fix the set of test binaries and forbid doc tests
- *(tests)* merge the command contract tests into one binary
- *(support)* retire the fixture server's unused parallel mode
- *(config)* check the Windows read side of private files in process
- *(fetch)* derive every stored web fetch variant through one helper
- *(search)* prove the command options reach the engine again
- *(search)* replay the main search failures from stored samples
- *(credentials)* stop waiting out the cursor lock in the test build
- tidy the seams and shared fixtures of the offline tests
- *(search)* prove the search command's own rules in the crate
- *(search)* open seams for in-crate search command tests
- *(search)* retire the main search tests that other tests already prove
- *(search)* prove the main search rules in the crate
- *(search)* open a seam for in-crate main search decoding tests
- *(search)* prove the supplemental search rules in the crate
- *(search)* open seams for in-crate supplemental search tests
- *(fetch)* retire the fetch attribution tests that the chain proves
- *(fetch)* prove the web fetch rules in the crate
- *(fetch)* open a seam for in-crate decoding tests
- *(exa)* expect the stored search result in the Markdown tee
- *(exa)* prove the Exa rules in the crate
- *(exa)* open a seam for in-crate decoding tests
- *(tavily)* prove the site map's rules in the crate
- *(tavily)* open a seam for in-crate site map tests
- *(reddit_browser)* keep sample records out of the projection rewrite
- *(browser)* exempt the browser routes' CLI timeout from the limit
- *(xiaohongshu_browser)* derive search failures from a search sample
- *(reddit_browser)* name the served local pages samples
- *(reddit_browser)* judge observations from the stored samples
- *(reddit_browser)* replace identifiers that may be real
- *(xiaohongshu_browser)* judge observations from the stored samples
- *(xiaohongshu_browser)* replace identifiers that may be real
- *(xiaohongshu_browser)* keep one refused input per Xiaohongshu command
- *(xiaohongshu_browser)* prove the missing-token refusal in the types
- *(hn_api)* exempt the content-directory fetch from the runner limit
- *(browser)* publish the stand-in's pid files by rename
- *(serpapi)* reject an invented Scholar profile URL
- *(xai)* keep one argument-wiring case per X command path
- *(xai)* prove the X ref hints in the platform types
- *(serpapi)* zero the search ID in the status-error sample
- *(hn_api)* give the shared-window test its widest deadline
- *(tests)* tidy the first-batch route tests after review
- *(testing)* drop the binary half of a filter that matches no binary
- *(hn_api)* retire the unwritable-thread fetch contract
- *(hn_api)* prove an unwritable full text fails in the crate
- *(samples)* replace X individuals and drop Scholar profile IDs
- *(gemini_api)* prove the Gemini route's rules in the crate
- *(gemini_api)* read every Gemini body from the stored samples
- *(gemini_api)* open seams for in-crate Gemini tests
- *(youtube_api)* replace the channels of individuals in the samples
- *(youtube_api)* prove the YouTube API route's rules in the crate
- *(youtube_api)* read every Data API body from the stored samples
- *(youtube_api)* open seams for in-crate YouTube API tests
- *(xai)* prove the X route's rules in the crate
- *(xai)* open seams for in-crate X tests
- *(serpapi)* read the success-protocol bodies from the stored samples
- *(serpapi)* prove the Scholar route's rules in the crate
- *(serpapi)* open seams for in-crate Scholar tests
- *(ssrn_crossref)* read every Crossref body from the stored samples
- *(ssrn_crossref)* retire the input-form fetch contract
- *(ssrn_crossref)* prove the SSRN route's rules in the crate
- *(ssrn_crossref)* open seams for in-crate SSRN tests
- *(hn_api)* read every Hacker News body from the stored samples
- *(hn_api)* keep only the empty list in the stories contract
- *(hn_api)* retire the link-story fetch contract
- *(hn_api)* prove the Hacker News route's rules in the crate
- *(hn_api)* open seams for in-crate Hacker News tests
- retire per-platform copies of the platform-independent rules
- prove the platform-independent planning and cursor rules once
- decide full-text delivery in one place and split planning tests
- *(testing)* list the tests allowed to wait and match the procedure to the suite
- restore lost proofs and close the sample rule's gaps
- *(test)* share the fixture server's request readers and runtime
- *(search)* prove that a search with the journal off writes none
- *(youtube_web)* keep only the outer facts of the fetch contract
- *(polymarket_api)* keep only the outer facts of the history contract
- *(polymarket_api)* retire the fetch contract that the crate proves
- *(polymarket_api)* keep only the outer facts of the search contract
- *(cli)* share one parser of the platform command in its tests
- name the spawned-binary layer the command contract
- *(architecture)* prove decoding in the crate against stored samples
- *(polymarket_api)* retire a history test that other tests prove
- *(polymarket_api)* read the stored Polymarket bodies as they are
- *(polymarket_api)* prove Polymarket's route rules in the crate from stored samples
- open seams for in-crate Polymarket tests
- *(youtube_web)* read every YouTube web body from the stored samples
- *(youtube_web)* prove YouTube web's route rules in the crate from stored samples
- *(cli)* split the YouTube fetch's argument checks from running it
- *(arxiv_api)* retire arXiv tests that other tests prove
- *(arxiv_api)* read every arXiv body from the stored feed
- prove arXiv's route rules in the crate from stored samples
- open seams for in-crate arXiv and orchestration tests
- *(cli)* name the first setup step number and content failures
- *(setup)* number the setup steps from their order
- *(journal)* type the search plan source
- drop names left over from retired definitions
- *(search)* make a search candidate's URL required
- state the compile-time dispatch rule as it stands
- *(checklist)* drop the check that a route is registered
- *(providers)* tighten names, visibility and dispatch
- *(checklist)* state only what the checklist checks beyond the compiler
- *(checklist)* require a live case for every catalog operation
- *(catalog)* derive operation-only routes from operation routes
- *(checklist)* retire the operation-only route rule
- *(platform)* run single-route operations through one entry
- *(catalog)* drop registration fields with one value per route
- *(checklist)* prove route declarations match their built routes
- *(providers)* declare every platform route once
- *(providers)* declare hn_api and reddit_browser once
- *(catalog)* define the provider identity once
- remove the acceptance manifest and the rules that read it
- let the Windows job compile without denying warnings
- run the target script behind the proxy and tidy test wording
- *(test)* read canary requests with the shared request reader
- keep only release-shape assertions with an incident or a script
- run one pull-request workflow with linux and windows jobs
- run the Windows read-side permission check in the Windows job
- run the suite through one nextest configuration
- answer fixture requests by route and keep the gate offline
- *(test)* share the fixture server with in-crate tests
- isolate in-crate tests from real state and real hosts
- let callers supply the credential and access state files
- state current rules where the retirements left old wording
- drop the unreachable parameter kind from JSON preflight
- prove an undefined provider section is a configuration error
- remove dead code left after the retirements
- prove the rejection rules with invented names, not retired ones
- reduce the Windows job to compile, permissions and artifact gate
- *(adr)* keep ADR-0038's lists unbroken
- prove surviving rules without Context7 as a sample
- prove surviving rules without the classifier as a sample
- *(rules)* drop the line-count ceiling on Rust sources
- rebuild the agent-facing documentation ([#295](https://github.com/jfmoe/forager/pull/295))

## [0.21.0](https://github.com/jfmoe/forager/compare/v0.20.0...v0.21.0) - 2026-11-21

### Added

- *(reddit)* [**breaking**] retire reddit_web and read Reddit only in the browser
- *(reddit)* read posts in the logged-in Managed Browser
- *(reddit)* search Reddit in the logged-in Managed Browser
- *(smoke)* run left-out routes under --opt-in and name why cases skip

### Fixed

- *(reddit)* gate the unix-only projection test imports
- *(smoke)* report the child's error in a failed live case
- *(reddit)* end both reads on a 5xx and stop hiding scroll failures
- *(reddit)* end a post read on a server error and tidy the review findings
- *(reddit)* hold reddit_browser to its spec on decoding and messages

### Other

- *(reddit)* state the ADR and research findings as current fact
- *(reddit)* state the search exit codes as current behavior
- *(browser)* share one page reading session across browser routes
- *(smoke)* fold the opt-in gate branches and split its test

## [0.20.0](https://github.com/jfmoe/forager/compare/v0.19.0...v0.20.0) - 2026-11-19

### Added

- *(xiaohongshu)* [**breaking**] run xiaohongshu_browser on the browser transport
- *(browser)* add browser transport, install and login commands

### Fixed

- *(xiaohongshu)* expand replies the page loaded while expanding
- *(browser)* gate the unix-only test import of WindowMode
- *(browser)* send the browser's stdout to /dev/null again
- *(browser)* say how a browser exited before the handshake
- *(xiaohongshu)* attribute click answers by when the request was sent
- *(xiaohongshu)* expand replies on each comment page as it arrives
- *(browser)* turn the wheel in small bursts and type CJK in groups
- *(browser)* aim at a steady target and re-aim before the press
- *(xiaohongshu)* stop on rate limits, 471, and API error codes
- *(browser)* let a wheel scroll settle before reading positions
- *(rate-limit)* gate the test-only state-file limiter to unix
- *(browser)* keep browser install off the blocking path and in time
- *(xiaohongshu)* classify searches cut short on the home page
- *(browser)* reap a dropped browser without blocking the runtime
- *(browser)* quote the browser's real last stderr line
- *(browser)* name an undecodable response body as such
- *(browser)* report why a hovered target no longer qualifies
- *(browser)* send no pointer step past the read deadline
- *(release)* verify aarch64 macOS artifacts on macos-15
- *(release)* build aarch64 macOS artifacts on macos-15

### Other

- *(spec)* drop a ticket reference from the Xiaohongshu classification
- *(research)* record the live matrix of the Xiaohongshu browser route
- *(browser)* move the browser's process group into its own module
- *(browser)* move the input pacing into its own module
- *(skill)* describe the new Xiaohongshu stop signals
- *(xiaohongshu)* expect the host's own fix when no browser is installed
- *(skill)* describe the Xiaohongshu route on the browser transport
- *(agents)* replace the OpenCLI guide with a browser-route guide
- *(adr)* record the own browser transport and fingerprint browser
- *(browser)* ignore real-browser tests instead of passing them empty
- *(browser)* rename the window choice to WindowMode
- *(browser)* share the duplicated helpers of the transport
- *(browser)* drop input and preflight paths nothing uses
- *(browser)* tolerate a stand-in hdiutil killed before it logs
- *(spec)* register the browser route config projections
- *(browser)* move login orchestration into ops
- *(browser)* build the base64 body fixture without an Option wrap
- *(xiaohongshu)* keep native input failures typed to the route boundary
- *(xiaohongshu)* judge page observations in pure functions

## [0.19.0](https://github.com/jfmoe/forager/compare/v0.18.0...v0.19.0) - 2026-11-17

### Other

- drop retired-adapter residue from OpenCLI docs
- collapse indirection left by the single OpenCLI route
- [**breaking**] retire the SSRN browser route and local-file full text
- prove OpenCLI deadline and process rules through xiaohongshu_browser
- prove OpenCLI process-route rules through xiaohongshu_browser
- [**breaking**] retire Gemini and ChatGPT delegated research

## [0.18.0](https://github.com/jfmoe/forager/compare/v0.17.0...v0.18.0) - 2026-10-28

### Added

- *(search)* add openai as a main search backend
- *(reddit)* search and read Reddit through its own pages

### Fixed

- *(deps)* update rustls to 0.23.46 for GHSA-2mjx-qc3c-rqvc

## [0.17.0](https://github.com/jfmoe/forager/compare/v0.16.0...v0.17.0) - 2026-10-24

### Added

- *(polymarket)* read Polymarket price histories
- *(polymarket)* search and fetch Polymarket markets

### Fixed

- *(polymarket)* suspend placeholder markets and tighten decoding

### Other

- *(polymarket)* design the Polymarket platform
- *(skill)* point YouTube file downloads at yt-dlp

## [0.16.0](https://github.com/jfmoe/forager/compare/v0.15.0...v0.16.0) - 2026-10-21

### Added

- *(chatgpt)* [**breaking**] operate the ChatGPT page with native input
- *(gemini)* [**breaking**] operate the Gemini page with native input
- *(xiaohongshu)* [**breaking**] operate the Xiaohongshu page with native input
- *(opencli)* [**breaking**] decode the OpenCLI 2.x output envelope

### Fixed

- *(gemini)* wait for the plan card until only the confirm time is left
- *(gemini)* keep the adapter contract at forager-gemini/2
- *(gemini)* stop on Google's unusual-traffic page
- *(gemini)* [**breaking**] judge a started research from the reopened conversation
- *(gemini)* reopen a conversation whose body was not captured
- *(chatgpt)* evaluate the Deep Research check as one script
- *(opencli)* hand every target candidate to the hit check
- *(opencli)* stop native input once the deadline passes
- *(xiaohongshu)* click only the button a pointer would hit

### Other

- *(spec)* credit envelope status check to opencli_route
- Merge branch 'opencli-2x' into worktree-agent-a22839134bd9b3af6
- Merge branch 'opencli-2x' into worktree-agent-a70cfdf48d83166e1
- *(research)* share the delegated research flow
- *(opencli)* share one route-bound OpenCLI invocation
- *(opencli)* document OpenCLI 2.x setup and native input limits
- *(smoke)* drop the SSRN browser live cases C22 and C23
- [**breaking**] retire the openai_compatible main-search backend
- *(smoke)* keep P1 unconfigured in the P2 journal test
- *(spec)* drop retired vertical search counts
- *(smoke)* state the P2 discovery requirement directly
- [**breaking**] retire vertical search and AnySearch

## [0.15.0](https://github.com/jfmoe/forager/compare/v0.14.0...v0.15.0) - 2026-10-18

### Added

- *(web-fetch)* prefer Jina over Tavily and let Jina run anonymously

### Fixed

- *(x)* verify fetch by its thread fetch and require the tool
- *(config)* reject quoted strings in config set and fix its help

### Other

- *(agents)* shorten the compatibility fixes rule
- *(agents)* fix compatibility issues in the owning mechanism
- *(research)* compare ways to read X articles and Jina vs Tavily
- *(research)* record why X fetch rejects replies
- *(research)* record Reddit integration research and evidence
- skip pull request CI for documentation-only changes
- *(skill)* state that failures render on stdout

## [0.14.0](https://github.com/jfmoe/forager/compare/v0.13.0...v0.14.0) - 2026-10-18

### Added

- *(youtube)* ask about and transcribe videos via gemini_api
- *(youtube)* list a video's comments and a collection's videos
- *(youtube)* read captions as a transcript via youtube_web
- *(youtube)* search and fetch YouTube videos through the Data API

### Fixed

- *(transcript)* fold whitespace inside a cue by the joining rule
- *(transcript)* join CJK cues without a space
- *(youtube)* name a missing video in comments and strip API markup
- *(youtube)* read a video's only caption track when its language is unknown
- *(youtube)* build the youtube_web enable hint from the current order
- *(youtube)* keep a channel's uploads playlist in its cursor
- *(youtube)* report only an ASR-derived original language
- *(youtube)* stop guessing live for unconfirmed live content

### Other

- *(core)* split the ask run by platform
- name the precheck and unify shared module imports
- split the platform catalog and YouTube smoke tests
- *(providers)* give key account checks a shared owner
- *(core)* pair the YouTube precheck with the YouTube ask
- *(providers)* share one in-attempt refusal shape
- *(youtube)* render YouTube transcripts through one binding
- *(providers)* share count and UTC-second timestamp helpers
- *(adr)* record the YouTube platform's three evidence routes
- *(youtube)* record YouTube integration research and terms

## [0.13.0](https://github.com/jfmoe/forager/compare/v0.12.0...v0.13.0) - 2026-10-17

### Added

- *(hn)* search, fetch, and list Hacker News stories

## [0.12.0](https://github.com/jfmoe/forager/compare/v0.11.2...v0.12.0) - 2026-10-17

### Added

- *(x)* search, fetch, and ask X through xAI x_search

### Other

- *(adr)* record the X platform through xAI x_search

## [0.11.2](https://github.com/jfmoe/forager/compare/v0.11.1...v0.11.2) - 2026-10-11

### Fixed

- *(ssrn)* arm the capture before opening the paper page

## [0.11.1](https://github.com/jfmoe/forager/compare/v0.11.0...v0.11.1) - 2026-10-11

### Fixed

- *(smoke)* stop one live case from starving the rest

### Other

- *(agents)* record the release procedure and OpenCLI dev loop
- check two repository rules and run the suite with nextest
- *(openai-compatible)* read the whole request before answering

## [0.11.0](https://github.com/jfmoe/forager/compare/v0.10.0...v0.11.0) - 2026-10-11

### Added

- *(ssrn)* search through SSRN's own search API in the browser
- *(chatgpt)* start a Deep Research in a foreground window
- *(chatgpt)* read Deep Research results through OpenCLI

### Fixed

- *(gemini)* point a sign-in prompt after sending to the conversation
- *(gemini)* arm the capture before every navigation
- *(chatgpt)* find the composer and add-files menu on the live page
- *(chatgpt)* arm the capture before every navigation
- *(chatgpt)* weigh the usage receipt the same way in start and result
- *(chatgpt)* treat a null widget_state as a structure change
- *(chatgpt)* point a sign-in prompt after sending to the conversation
- *(chatgpt)* wait out the plan countdown instead of clicking Start
- *(chatgpt)* read a plan awaiting confirmation from its snapshot

### Other

- *(chatgpt)* share the conversation read between report and start
- *(spec)* list ChatGPT start among foreground commands
- *(chatgpt)* move the response cursor into a shared leaf module
- *(skill)* route explicit ChatGPT Deep Research requests
- *(adr)* accept the OpenAI terms risk under conditions
- *(cli)* share delegated research failure rendering
- *(cli)* share the Delegated Research Report writer
- *(research)* add ChatGPT Deep Research survey and live probe

## [0.10.0](https://github.com/jfmoe/forager/compare/v0.9.0...v0.10.0) - 2026-10-10

### Added

- *(gemini)* start Deep Research from the command line
- *(gemini)* read Deep Research results through OpenCLI

### Fixed

- *(gemini)* skip unrecognised progress items in a running read
- *(gemini)* wait longer for the plan card and say why confirm failed
- *(gemini)* attribute start failures only from real evidence

### Other

- format the Gemini start test
- *(gemini)* remove duplication from the Deep Research route
- add Gemini Deep Research route design and probe

## [0.9.0](https://github.com/jfmoe/forager/compare/v0.8.0...v0.9.0) - 2026-10-09

### Added

- *(xiaohongshu)* list note comments and expand their replies
- *(xiaohongshu)* date relative publish times from the local clock
- *(xiaohongshu)* fetch notes with a native full text
- *(xiaohongshu)* search notes through the user's logged-in Chrome

### Fixed

- *(smoke)* skip Xiaohongshu cases that cannot prove their coverage
- *(xiaohongshu)* treat only the login wall notice as logged out
- *(xiaohongshu)* read the capture only after the body is stored
- *(xiaohongshu)* count search responses in every page document

### Other

- *(smoke)* run the Xiaohongshu search case only on Unix
- *(smoke)* compile Xiaohongshu fixtures only on Unix
- *(doctor)* cover the Xiaohongshu route while no order enables it
- *(xiaohongshu)* classify unfinished reads in the route entry
- *(xiaohongshu)* share the read session and page helpers
- *(xiaohongshu)* design OpenCLI browser route from live probes

## [0.8.0](https://github.com/jfmoe/forager/compare/v0.7.0...v0.8.0) - 2026-10-08

### Added

- *(serpapi)* probe every key through the free Account API
- *(scholar)* list citing papers with platform scholar cited-by
- *(scholar)* add Google Scholar platform through SerpApi
- *(arxiv)* match double-quoted phrases in search queries

### Other

- *(scholar)* drop cited-by tests that repeat shared search rules
- *(design)* add Google Scholar platform via SerpApi
- *(research)* compare SSRN Crossref and browser search quality
- rename CONTEXT.md to GLOSSARY.md for agent skills

## [0.7.0](https://github.com/jfmoe/forager/compare/v0.6.0...v0.7.0) - 2026-09-28

### Added

- *(ssrn)* add native advanced search
- *(ssrn)* add Crossref advanced search
- *(platform)* deliver SSRN full text as Markdown from the browser download
- *(platform)* add the opt-in ssrn_browser route over an OpenCLI transport
- *(platform)* add the ssrn platform with the ssrn_crossref route

### Fixed

- *(cli)* box the large platform fetch output

### Other

- *(platform)* keep the fake opencli tests compiling on Windows
- *(platform)* prepare platform seams for a second platform

## [0.6.0](https://github.com/jfmoe/forager/compare/v0.5.3...v0.6.0) - 2026-09-26

### Added

- *(rate-limit)* enforce the connection limit across processes
- *(platform)* add arXiv full-text fetch and skill platform routing
- *(platform)* add the platform dimension and arXiv search
- *(fetch)* request Firecrawl PDF page markers
- *(fetch)* prefer Firecrawl in the default Web Fetch order

### Fixed

- *(platform)* correct arXiv ref parsing and query word splitting
- *(rate-limit)* bound the in-process state queue by the deadline

### Other

- *(spec)* sync pacing, arXiv, and attribution wording with the code
- *(catalog)* remove the speculative trait-operations registry
- *(research)* measure Firecrawl PDF page markers
- *(research)* compare Firecrawl and Tavily fetch quality
- prepare provider execution for platform routes
- *(research)* add scholarly access and arXiv full-text fetch research

## [0.5.3](https://github.com/jfmoe/forager/compare/v0.5.2...v0.5.3) - 2026-09-25

### Fixed

- *(search)* keep balanced parentheses in source URLs

## [0.5.2](https://github.com/jfmoe/forager/compare/v0.5.1...v0.5.2) - 2026-09-25

### Added

- *(docs-search)* prefer Exa over Context7 and share research discovery slots
- cut search token cost and run supplemental search alongside main search

### Other

- *(skills)* allow searches for new questions
- remove code-quality skill and reviews

## [0.5.1](https://github.com/jfmoe/forager/compare/v0.5.0...v0.5.1) - 2026-09-04

### Fixed

- preserve maintenance safeguards

### Other

- *(smoke)* decouple probes from deadline
- *(release)* satisfy Windows clippy
- *(time)* make timeout behavior deterministic
- *(release)* script artifact verification
- anchor AGENTS.md conventions to the architecture spec
- strengthen behavioral contracts
- lock source architecture boundaries
- separate CLI and capability boundaries
- group source modules by architecture
- *(skill)* add code quality guidance

## [0.5.0](https://github.com/jfmoe/forager/compare/v0.4.0...v0.5.0) - 2026-08-30

### Other

- close code quality reviews
- *(skill)* remove Kimi datasource
- *(chain)* isolate step preparation
- pin gate/SlicedEven incompatibility in the chain runner
- *(providers)* add capability catalogs
- *(net)* own provider HTTP reads
- share one typed chain runner across fallback loops
- merge research terminal state into one type
- own attempt-chain derived facts in attempt_trace

## [0.4.0](https://github.com/jfmoe/forager/compare/v0.3.0...v0.4.0) - 2026-08-29

### Fixed

- *(release)* clear expected doctor exit on Windows
- *(release)* align artifact gate with doctor exit contract

### Other

- simplify suite contracts
- [**breaking**] tighten audited contracts

## [0.3.0](https://github.com/jfmoe/forager/compare/v0.2.0...v0.3.0) - 2026-08-09

### Added

- *(research)* consume Context7 evidence locators
- *(search)* unify search candidate contract

### Fixed

- close search fallback and live smoke gaps
- *(map)* enforce Tavily API bounds ([#143](https://github.com/jfmoe/forager/pull/143))
- *(anysearch)* restore narrow markdown decoding ([#142](https://github.com/jfmoe/forager/pull/142))
- *(mcp)* preserve optional provider contracts ([#141](https://github.com/jfmoe/forager/pull/141))
- *(search)* restore streaming provider contracts
- *(search)* normalize completed main answers
- *(search)* restore supplemental fallback contract
- *(search)* remove validation and restore source targets
- *(research)* stabilize failure recovery manifest
- *(cli)* restore runtime observability contracts
- *(cli)* restore JSON preflight terminals
- *(net)* enforce protocol response byte caps
- *(net)* disable shared client redirects

### Other

- *(skill)* sync bundled CLI contract ([#146](https://github.com/jfmoe/forager/pull/146))
- streamline acceptance validation
- *(net)* stall after the first response byte
- restore project agent instructions
- sync agent skills and migration audit

### Removed

- Removed the `search --validation` option and `search.validation` configuration key without aliases or compatibility shims. Legacy uses now fail as unknown CLI or configuration input.

### Migration

- Upgrade the CLI to `forager >=0.3.0` before installing or using the bundled skill.
- Search JSON no longer returns `vertical_results`; read every non-primary Search Candidate, including Vertical Discovery results, from `extra_sources`.
- Before or after upgrading, use `forager config path` to locate the configuration file and manually delete any persisted `search.validation` key; `config path` does not load the schema. The obsolete `SMART_SEARCH_VALIDATION_LEVEL` migration mapping is deleted rather than treated as a runtime input.
- Callers that omit `--extra-sources` or pass `0` should account for the restored Supplemental Web Search request target of 3. Documentation Search and Vertical Search still default to 1, explicit `1..=20` targets are unchanged, and values above 20 are rejected before configuration loading or network access.

## [0.2.0](https://github.com/jfmoe/forager/compare/v0.1.2...v0.2.0) - 2026-08-05

### Added

- *(research)* deliver file-backed evidence index ([#104](https://github.com/jfmoe/forager/pull/104))
- *(research)* account for known URLs and terminal gaps ([#103](https://github.com/jfmoe/forager/pull/103))
- *(research)* budget evidence per subquestion
- *(search)* separate fetch and vertical results ([#101](https://github.com/jfmoe/forager/pull/101))
- *(search)* separate supplemental candidates ([#100](https://github.com/jfmoe/forager/pull/100))
- normalize web fetch content contract ([#99](https://github.com/jfmoe/forager/pull/99))

### Fixed

- preserve docs fallback after empty Context7 sources ([#95](https://github.com/jfmoe/forager/pull/95))

### Other

- document v0.2.0 output migration ([#106](https://github.com/jfmoe/forager/pull/106))
- *(skill)* rewrite forager orchestration ([#105](https://github.com/jfmoe/forager/pull/105))
- define consumable search and research outputs
- define inline citation binding
- decide skill search orchestration contract
- decide research engine evidence pipeline contract
- record Context7 MCP output contract
- retain current stdout contract
- decide provider-first fetch content contract
- require post-release local validation

### Changed

- Web Fetch now returns provider-independent Normalized Fetch Content and uses the shared `Tavily → Firecrawl → Jina` chain for URLs, PDFs, search-side fetches, and research evidence ([#99](https://github.com/jfmoe/forager/issues/99)).
- Search `sources` now contains only Primary Search Sources. `extra_sources` contains only Supplemental Search Candidates with provider-native summaries, while `vertical_results` contains only structured Vertical Discovery results ([#100](https://github.com/jfmoe/forager/issues/100), [#101](https://github.com/jfmoe/forager/issues/101)).
- Search-side Web Fetch results now appear in `extra_sources` with the successful provider and a 300-character body preview; the preview limit does not truncate provider-native candidate summaries ([#101](https://github.com/jfmoe/forager/issues/101)).
- Search source and candidate `title` fields are now optional in JSON; Markdown uses the URL as the link label when a title is absent. Invocation fields `query`, `model`, selected `provider`, and `capabilities` now live in the Search Result Journal rather than default stdout.
- Context7 docs JSON now exposes one readable payload with `provider`, `library_id`, `query`, and `content`; `provider_attempts` remains available only with `--verbose`. Automatic Documentation Search continues to the next provider when Context7 has no normalized source ([#95](https://github.com/jfmoe/forager/issues/95)).
- Research is now a deterministic, single-pass evidence pipeline with per-subquestion caps, first-class known URLs, terminal coverage gaps, and a file-backed Research Evidence Index ([#102](https://github.com/jfmoe/forager/issues/102), [#103](https://github.com/jfmoe/forager/issues/103), [#104](https://github.com/jfmoe/forager/issues/104)).
- The bundled skill now routes through `direct retrieval → ordinary search → research`, orients with ordinary search before writing a research plan, reads evidence from `evidence_items[].path`, and binds synthesized claims as `[eN](URL)` ([#105](https://github.com/jfmoe/forager/issues/105)).

### Removed

- Search no longer returns `validation_results`, duplicates Supplemental Search Candidates into `sources`, or copies URL-bearing Vertical Discovery records into `extra_sources`.
- Context7 docs no longer returns `code_snippets`, `info_snippets`, `results`, `total`, or synthesized `sources`.
- Research no longer returns `evidence_items[].content`, mechanical `final_answer`, duplicate top-level `content`, derived `citations`, inline `research_plan`, or invocation and diagnostic echoes in its default output. No aliases, compatibility shims, feature flags, or duplicate legacy arrays remain.

### Migration

- Upgrade the CLI to `forager >= 0.2.0` before installing or using the bundled skill.
- Search callers must treat `sources` as answer attribution, select and fetch any needed `extra_sources`, consume Vertical Discovery only from `vertical_results`, handle an omitted `title`, read `query`/`model`/selected `provider`/`capabilities` from the journal, and request `--verbose` when inline `provider_attempts` are required.
- Research callers must read selected evidence bodies from `evidence_items[].path`, synthesize the answer themselves, cite evidence with `[eN](URL)`, and treat unconsumed candidates as unverified fetch inputs rather than evidence.
- Context7 callers must reuse `library_id` as an identifier rather than pass it to `fetch`; use absolute URLs present in the returned content when source-page verification is required.

## [0.1.2](https://github.com/jfmoe/forager/compare/v0.1.1...v0.1.2) - 2026-08-02

### Added

- parallelize intra-stage fan-outs ([#84](https://github.com/jfmoe/forager/pull/84))
- cap response bodies and narrow redaction ([#81](https://github.com/jfmoe/forager/pull/81))
- prioritize primary search timeout budget ([#80](https://github.com/jfmoe/forager/pull/80))
- protect provider credentials with Secret type ([#78](https://github.com/jfmoe/forager/pull/78))

### Fixed

- *(smoke)* require statuspage outage JSON ([#75](https://github.com/jfmoe/forager/pull/75))
- redact provider error URLs
- *(openai)* reject truncated SSE responses
- unify private permission enforcement ([#63](https://github.com/jfmoe/forager/pull/63))
- *(config)* serialize configuration writes
- *(smoke)* avoid polling and journal pollution ([#61](https://github.com/jfmoe/forager/pull/61))
- *(config)* make endpoint URL defaults intrinsic
- *(anysearch)* cache verified domain manifest ([#57](https://github.com/jfmoe/forager/pull/57))
- implement Error for ResearchError ([#56](https://github.com/jfmoe/forager/pull/56))
- configure shared HTTP client ([#53](https://github.com/jfmoe/forager/pull/53))
- bound CI jobs and fixture accepts ([#52](https://github.com/jfmoe/forager/pull/52))
- isolate journal writes from cleanup failures ([#51](https://github.com/jfmoe/forager/pull/51))
- report research summary write failures ([#50](https://github.com/jfmoe/forager/pull/50))
- prevent retry duration overflow ([#49](https://github.com/jfmoe/forager/pull/49))
- *(net)* rotate credentials after HTTP 402

### Other

- clarify Windows CI verification
- complete public type documentation ([#70](https://github.com/jfmoe/forager/pull/70))
- *(providers)* unify execution loops ([#83](https://github.com/jfmoe/forager/pull/83))
- unify provider seam chains ([#82](https://github.com/jfmoe/forager/pull/82))
- stabilize response stall timeout ([#81](https://github.com/jfmoe/forager/pull/81))
- drive config leaves from schema ([#79](https://github.com/jfmoe/forager/pull/79))
- split configuration modules
- authenticate release-plz with GitHub App ([#76](https://github.com/jfmoe/forager/pull/76))
- consolidate HTTP fixtures ([#74](https://github.com/jfmoe/forager/pull/74))
- enforce clippy pedantic policy ([#73](https://github.com/jfmoe/forager/pull/73))
- consolidate pull request gates ([#72](https://github.com/jfmoe/forager/pull/72))
- cover journal persistence and cleanup ([#69](https://github.com/jfmoe/forager/pull/69))
- record intra-stage search concurrency decision ([#43](https://github.com/jfmoe/forager/pull/43)) ([#66](https://github.com/jfmoe/forager/pull/66))
- record primary-first timeout budget ADR and skill guidance ([#65](https://github.com/jfmoe/forager/pull/65))
- record output security architecture decision ([#41](https://github.com/jfmoe/forager/pull/41))
- validate acceptance tests against Cargo registry ([#60](https://github.com/jfmoe/forager/pull/60))
- remove duplicate transport fixture manifest ([#59](https://github.com/jfmoe/forager/pull/59))
- consolidate shared utility functions
- remove redundant classifier clones
- document installation and distribution
- require manual release dispatch
- record smartsearch archive
- record retirement cutover ([#23](https://github.com/jfmoe/forager/pull/23))
- open v0.1.1 switch gate
- record all L0 deep probes
- record v0.1.1 switch gate evidence

## [0.1.1](https://github.com/jfmoe/forager/compare/v0.1.0...v0.1.1) - 2026-07-27

### Fixed

- cover every research subquestion ([#26](https://github.com/jfmoe/forager/pull/26))
- resolve release assets outside a checkout
- make smoke test fixtures portable on Windows ([#26](https://github.com/jfmoe/forager/pull/26))

## [0.1.0](https://github.com/jfmoe/forager/releases/tag/v0.1.0) - 2026-07-27

### Added

- gate clean release assets ([#20](https://github.com/jfmoe/forager/pull/20))
- split forager skill workflows
- align model prompts and agent contracts
- deliver installable forager skill ([#21](https://github.com/jfmoe/forager/pull/21))
- add live smoke acceptance matrix ([#18](https://github.com/jfmoe/forager/pull/18))
- add offline smoke readiness check ([#17](https://github.com/jfmoe/forager/pull/17))
- add registry-backed provider doctor
- generate plans for bare research ([#15](https://github.com/jfmoe/forager/pull/15))
- execute caller research plans
- drive bare search with classifier
- honor caller capability declarations
- add OpenAI-compatible search fallback ([#11](https://github.com/jfmoe/forager/pull/11))
- complete xai default search invocation ([#10](https://github.com/jfmoe/forager/pull/10))
- add Tavily site mapping
- add web fetch capability
- deliver AnySearch acceptance surface
- add Context7 documentation search
- *(exa)* add similar page discovery
- add Exa search network runtime ([#4](https://github.com/jfmoe/forager/pull/4))
- add incremental setup wizard ([#3](https://github.com/jfmoe/forager/pull/3))
- add strict editable configuration ([#2](https://github.com/jfmoe/forager/pull/2))
- establish config path and release scaffolding ([#1](https://github.com/jfmoe/forager/pull/1))

### Fixed

- cap Tavily map provider timeout
- decode live AnySearch domain contracts
- make live classifier and research probes reliable
- match live provider protocol shapes

### Other

- stop freezing skill prompt prose
- seal tier zero and tier one regression gates ([#19](https://github.com/jfmoe/forager/pull/19))
- configure engineering agent skills
- Add Rust formatting and lint components
- Initial forager scaffold
# Changelog

All notable changes to forager will be recorded in this file.
