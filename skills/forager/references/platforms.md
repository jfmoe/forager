# Platform retrieval

A platform command retrieves platform-native items: a typed ref, the platform's metadata, and a
stated content depth. Check [`platform-vocabulary.json`](platform-vocabulary.json) first. A
platform absent from it has no platform command; follow
[`direct-retrieval.md`](direct-retrieval.md) or the cost ladder in `SKILL.md` for it instead.

## Choose the branch

- **Known item** (the user gave a platform URL, ref, or SSRN DOI, or forager returned one): run
  `forager platform <id> fetch 'REF_OR_URL'`. Prefer it over `forager fetch` for the same link; it
  returns the platform ref, metadata, and the content its routes can read.
- **Platform-only results, or platform options** (category, author, title, date range, sort): run
  `forager platform <id> search QUERY`. Set only options the request states or clearly implies.
- **Xiaohongshu (小红书) notes**: a note opens only with its access token, so fetch takes a search
  item's `access_url`, or a xiaohongshu.com note URL that carries `xsec_token`, exactly as given:
  `forager platform xiaohongshu fetch 'ACCESS_URL'`. For a link without the token (or a bare
  `xiaohongshu:` ref), search for the note first and fetch its `access_url`; `forager fetch` cannot
  read a note, because the page needs the user's login. For what readers say about a note, run
  `forager platform xiaohongshu comments 'ACCESS_URL'` with the same URL.
- **Papers citing a known paper** (follow-up work, replications, or rebuttals): run
  `forager platform scholar cited-by 'REF_OR_URL'`; see "Google Scholar cited-by".
- **X (Twitter)**: to hear what X says, or to answer from given accounts or days, run
  `forager platform x ask QUESTION`; for post items to list or cite, run
  `forager platform x search QUERY`; for a post URL or its replies, run
  `forager platform x fetch 'REF_OR_URL'`; for an X article (`/article/` in the URL), run
  `forager fetch 'URL'`. A general question that X may inform keeps using `forager search`,
  which already reads X. See "X posts".
- **Hacker News**: for what HN says about a topic, run `forager platform hn search QUERY`; for a
  discussion (an HN link the user gave, or a ref forager returned), run
  `forager platform hn fetch 'REF_OR_URL'`; for what is on the front page, run
  `forager platform hn stories`. See "Hacker News".
- **YouTube**: for videos on a topic, run `forager platform youtube search QUERY`; for one video's
  description, chapters, and counts (a video link the user gave, or a ref forager returned), run
  `forager platform youtube fetch 'REF_OR_URL' --depth abstract`; for what viewers say about it,
  run `forager platform youtube comments 'REF_OR_URL'`; for the videos of a course, playlist, or
  channel, run `forager platform youtube videos 'PLAYLIST_OR_CHANNEL'`. These need the user's
  YouTube Data API key. What a video says (its captions) needs the `youtube_web` route, which the
  user must enable: `forager platform youtube fetch 'REF_OR_URL'`. For what a video shows (slides,
  code, charts), run `forager platform youtube ask 'REF_OR_URL' 'QUESTION'`; for a transcript of a
  video without captions, run `forager platform youtube transcribe 'REF_OR_URL'`. These two need
  the user's Gemini key. For the video or audio file itself, see "Video and audio files". See
  "YouTube".
- **Polymarket**: for the markets on a topic, run `forager platform polymarket search QUERY`; for
  the live probability of each outcome of an event or market (a polymarket.com link the user
  gave, or a ref forager returned), run `forager platform polymarket fetch 'REF_OR_URL'`. See
  "Polymarket".

`forager platform <id> <op> --help` is the syntax authority; see the platform section of
[`cli.md`](cli.md) for output fields and exit codes.

## Content depth

Every item carries `depth`, the content it actually holds: `metadata` (bibliographic fields only,
`abstract: null`), `snippet` (a search excerpt, never the abstract), `abstract`, `full_text`, or
`thread` (a post with its conversation).

- **arXiv** `fetch` defaults to `--depth full_text`; `--depth abstract` skips the body.
- **SSRN** `fetch` defaults to `--depth metadata` and includes the abstract when a route has one.
  `--depth abstract` requires an abstract and fails with exit 5 when none is available. SSRN full
  text needs the `ssrn_browser` route: `--depth full_text` downloads the PDF in Chrome, converts
  it to Markdown through the Web Fetch chain, writes `ssrn-<id>.md`, and removes the PDF unless
  `--keep-pdf` keeps it next to the Markdown. Without the browser route it exits 2.
- **SSRN search** items from `ssrn_crossref` are `abstract` or `metadata`; items from
  `ssrn_browser` are `snippet` (`metadata` for a result card without an excerpt). A browser page never spans two SSRN result pages, so it can hold
  fewer items than `--limit`; follow `next_cursor` for more.
- **Xiaohongshu** search items are `metadata`: the card title, author, date, and counts, never the
  note body. `fetch` defaults to `--depth full_text`: it writes the note text, tags, and image links
  to `xiaohongshu-<id>.md` and needs no Web Fetch provider; `--depth metadata` returns the exact
  publish and edit times, tags, counts, images, video length and size, and IP region inline.
- **Scholar** search items are `snippet` (`metadata` for a result without an excerpt); `fetch` is
  `metadata` only and lists the paper's versions. Google Scholar holds no body; read it from the
  source, see "Read a Scholar paper".
- **X** search items are `snippet`: the post text as the model relayed it. `fetch` defaults to
  `--depth thread` and adds the posts it replies to, its replies, and the posts it quotes;
  `--depth metadata` returns the post alone.
- **Hacker News** search and `stories` items are `snippet` when the item has its own text (a
  comment, an Ask HN post) and `metadata` otherwise. `fetch` defaults to `--depth thread`, which
  writes the item and its whole discussion to `hn-<id>.md`; `--depth metadata` returns the item
  alone; `--depth full_text` reads the page a story links to through Web Fetch, or a text post's
  own body.
- **YouTube** search items are `snippet` (the description excerpt; `metadata` when it is empty).
  `fetch --depth metadata` returns the video's fields without its description; `--depth abstract`
  adds the full description and the chapters YouTube shows. `fetch` defaults to
  `--depth full_text`: through the `youtube_web` route it renders one caption track as a
  transcript in `youtube-<id>.md`, split by chapter into paragraphs that each open with a
  timestamp link. Without `youtube_web` in the order, `full_text` exits 2 with the command that
  enables it; pass `--depth metadata` or `--depth abstract` instead. `full_text` never falls back
  to a model's transcript; that comes only from `transcribe`.

## arXiv search queries

arXiv matches literal words, not meaning, and every query term must match. Use a few distinctive
keywords and quote multi-word concepts as phrases; scattered words match unrelated papers,
especially with `--sort submitted`. For an open-ended topic question, discover papers through
general search first and read them with `platform arxiv fetch`.

```console
forager platform arxiv search '"world model" robot manipulation' --category cs.RO --sort submitted
```

## SSRN advanced search

For title, author, affiliation, date, identifier, or ranking requirements, use typed SSRN search
flags; see [`cli.md`](cli.md#platform-ssrn-search). For example:

```console
forager platform ssrn search "dual momentum" --scope title --author Antonacci --has-abstract
```

The example uses Crossref because it requires an abstract field. Title queries do not require an exact phrase. Date filters
separate publication, Crossref registration, and metadata updates. ORCID and funder filters only
match deposited identifiers, so missing metadata can exclude relevant papers. If a configured
route cannot apply the criteria, preserve the request and report the support error. Continue a
result page with its cursor alone; do not repeat search criteria or fetch papers unless needed.

For native SSRN search, use an enabled browser route with `--scope title|all|full-text`,
`--mode fuzzy|boolean`, `--author`, `--date`, or `--sort posted|downloads|title` with `--order`.
Boolean supports AND, OR, NOT, and parentheses. Author text follows the site's matching rules;
try the surname when a full name has no matches, but report that changed condition.
`--scope full-text` still returns search cards only. The browser rejects affiliation, type,
abstract-presence, and arbitrary date-range filters; it does not approximate them locally.

```console
forager platform ssrn search "momentum AND portfolio" --scope title --mode boolean --date last-year --sort downloads
```

## Google Scholar search

Scholar runs on the user's own SerpApi key and its monthly search quota (250 on the free plan).
Every search page, every fetch, and every cited-by page costs one search, including a search with
no results and a fetch of a ref that does not exist. Repeating an identical request within one hour costs nothing.
`forager doctor --provider serpapi` costs nothing and reports each key's searches left and this
hour's usage; run it when the user asks how much Scholar quota remains.

- Search once with the default `--limit 20` and pick candidates from that page. Do not page with
  `--cursor` to collect more results; narrow the query or the years instead.
- Fetch a cluster only when you need its version links, for example to find a readable copy.

The query goes to Google Scholar unchanged. Quote phrases; `author:"Surname"` restricts the whole
query, including both sides of `OR`. These forms were checked against live Google Scholar:

```console
forager platform scholar search '"time series momentum" author:"Pedersen"'
forager platform scholar search 'trend OR reversal author:"Moskowitz"'
forager platform scholar search 'time series momentum' --year-from 2020
forager platform scholar search 'time series momentum' --review-only
```

`--year-from` and `--year-to` bound the publication year and can be used alone; `--review-only`
keeps review articles. Set them only when the request states or clearly implies them.

## Google Scholar cited-by

`cited-by` lists the papers Google Scholar counts as citing one paper, as search results with the
same fields. Use it to trace work forward from a known paper; arXiv and SSRN cannot. Pass the ref
or cluster URL exactly as forager or the user gave it.

- Narrow before paging: a well-cited paper has thousands of citing works, and each page costs one
  search. Use `--query` to match words within the citing papers, `--year-from`/`--year-to` for
  their publication years, or `--sort date` for the most recently indexed citations. Do not page
  with `--cursor` to collect more.
- `--sort date` cannot take years (exit 2): Google Scholar ignores the year range when it sorts by
  date. Its snippets start with Google Scholar's indexing age, such as `6 days ago - `; that is
  when Google Scholar indexed the citation, not the publication date.
- An empty list (exit 0) means either nobody cites the paper or Google Scholar does not know the
  ref; it cannot tell the two apart. Check the ref with `fetch` before reporting "no citations".
- A cited-by cursor works only with `cited-by`, and a search cursor only with `search`.

```console
forager platform scholar cited-by scholar:18208131694456651388 --query crash
forager platform scholar cited-by scholar:18208131694456651388 --year-from 2024
forager platform scholar cited-by scholar:18208131694456651388 --sort date
```

## X posts

X runs through the `xai` route with the xAI key of main search; no extra setup is needed. xAI's
`x_search` tool reads X for a model, and xAI returns only what the model writes, never the posts
themselves. So every X text is relayed content: `text` and `author_handle` may be reworded,
translated, or shortened. forager checks what it can: a search keeps only posts the response
cites as read, a fetch requires the root to be the requested post and a record that the tool
read it, and `published` comes from the post ID, not the model.

- Pass `--handle` (repeatable) for posts by given accounts, `--exclude-handle` to drop accounts,
  and `--from`/`--to` (inclusive UTC days) only when the request states or clearly implies them;
  xAI enforces them. There is no sort option: the model chooses Top or Latest itself.
- Search returns at most 20 posts and no cursor. For more, rewrite the query or narrow the
  filters.
- Each request lets the model read as many posts as it decides; through the official xAI API that
  is billed per post read. Prefer one `ask` over many searches when an answer is what the user
  wants.
- An X article (`/<handle>/article/<id>`, `/i/article/<id>`, `/status/<id>/article`) is not a
  post: `platform x fetch` rejects it with exit 2. Read it with `forager fetch 'URL'`, which
  returns the article's own text.
- To check a post's exact wording, run `forager fetch` on its status URL: Jina returns the post
  text verbatim, with its parent and some replies. Jina may temporarily refuse anonymous access
  to x.com (a `parameter` attempt naming the unblock time); a working Jina key is more reliable.

```console
forager platform x ask 'What are people saying about the Rust 2027 edition?'
forager platform x search 'Starship' --handle SpaceX --from 2026-10-01 --to 2026-10-14
forager platform x fetch 'https://x.com/SpaceX/status/2110126570233094527'
```

## Hacker News

Hacker News runs through the `hn_api` route and needs no key or setup: Algolia's HN Search API
searches, and the official Firebase API supplies the rank HN shows and live counts.

- Search queries take `"double quotes"` for a phrase and `-word` to exclude a word. Set `--type`,
  `--author`, `--story`, `--from`/`--to`, `--min-points`, and `--min-comments` only when the
  request implies them. To find comments inside one big discussion, search with `--story ID`
  instead of reading the whole file.
- A discussion can hold thousands of comments and a million characters. Read the file like a full
  text: the metadata on stdout first, then only the part you need. Top-level comments are in the
  rank HN shows (`order: ranked_top_level`), so the start of the file holds the most upvoted
  threads; replies are in time order. With `order: chronological` the rank was unavailable.
- For the HN discussion of a web page, search its title or URL words with `--type story`, then
  fetch the story.

```console
forager platform hn search 'rust async' --type story --min-points 100
forager platform hn fetch 'https://news.ycombinator.com/item?id=8863'
forager platform hn stories top --limit 10
```

## YouTube

YouTube runs through the `youtube_api` route: the official Data API v3 with the user's own key.
Search is the scarce resource: the default quota is 100 searches a day, and a failed search also
counts, so search once with the filters the request implies instead of probing with several
queries. Fetches, comments, and videos pages draw on a separate, larger budget (10,000 units a
day: one per fetch or comments read, one or two per videos page).

- Queries follow YouTube: `-word` excludes a word and `a|b` matches either. Set `--sort`,
  `--from`/`--to` (UTC days, both included), `--duration` (`short` under 4 minutes, `medium` 4 to
  20, `long` over 20), `--channel`, and `--has-captions` only when the request implies them. For a
  talk or lecture, `--duration long` filters out clips.
- `--channel` takes a `youtube:channel/<UC…>` ref (an item's `channel_ref`) or a
  `/channel/<UC…>` URL. An `@handle` URL is not accepted there; take the `channel_ref` of a video
  from that channel, or list the channel with `videos`.
- Fetch and comments read one video. A playlist or channel ref, or an `@handle` URL, exits 2.
- `comments` reads one page of a video's top-level comments (`--limit` up to 100,
  `--sort relevance|time`) with the few replies each carries; `replies_complete: false` means more
  replies exist that forager cannot read. Disabled comments exit 4 (`parameter`), not an empty
  list. Comments are viewers' opinions: report them as such, never as facts about the video.
- `videos` lists a playlist in its order, or a channel's uploads newest first. It takes a
  playlist or channel ref, a playlist URL, a watch URL with `list=`, a `/channel/<UC…>` URL, or an
  `@handle` URL. Items are search items plus `position`; continue with `--cursor`. Private and
  deleted videos are skipped, and stderr says where. Duration and counts are `null`; fetch the
  videos the request needs for them.
- Duration, counts, and `has_captions` come from a second request; when it fails the items stay,
  those fields are `null`, and stderr says so.

```console
forager platform youtube search 'retrieval augmented generation' --duration long --from 2026-09-01
forager platform youtube fetch 'https://www.youtube.com/watch?v=jNQXAC9IVRw' --depth abstract
forager platform youtube comments youtube:jNQXAC9IVRw --limit 10 --sort time
forager platform youtube videos 'https://www.youtube.com/@3blue1brown' --limit 10
```

### Captions: the `youtube_web` route

forager delivers text evidence about a video in layers that never stand in for each other: the
publisher's words (title, description, chapters) at `metadata` and `abstract`, and one platform
caption track at `full_text`. A transcript holds only the caption text: it is not evidence of what
the video shows (slides, code, charts) or of sounds other than speech.

- **What it is**: `youtube_web` reads YouTube's own watch page, player, and caption endpoints
  anonymously, without a key or an account. These endpoints are undocumented and YouTube's terms
  do not allow automated access, so the route is off by default and only the user may enable it.
  The risk falls on the user's network address, not an account: YouTube may block the egress IP
  for hours to weeks (datacenter IPs often at once), and waiting or switching networks recovers.
  With an official key also enabled, Google may suspend the user's Cloud project.
- **Enable**: only when the user asks for captions and accepts that risk, run
  `forager config set platforms.youtube.order '["youtube_api", "youtube_web", "gemini_api"]'`
  (keep every route already in the order; without a key, drop `youtube_api`, and then metadata and
  abstract also come from `youtube_web`, with `likes` and `comments` always `null`). The refusal
  that names this command builds it from the current order. The route sends one request every 4
  seconds and never retries; a full-text fetch takes three requests, so allow about 10 seconds a
  video and fetch only the videos the request needs.
- **Track**: by default, the publisher's captions in the video's original language, else its
  automatic (ASR) captions; when the original language is unknown, the video's only caption track
  if it has exactly one (then `language` stays `null`), otherwise none, and `--lang` must pick one. `--lang en --lang de` sets the languages in priority order; every
  publisher track in them comes before any ASR track. Machine translations are never used. The
  item's `caption_tracks` lists the tracks, and `transcript.source` says which kind was read:
  `manual` (the publisher's) or `asr` (YouTube's speech recognition, unpunctuated and error-prone).
- `--format content` prints the transcript instead of writing the file; a live stream has no
  transcript until it ends.

```console
forager platform youtube fetch 'https://www.youtube.com/watch?v=jNQXAC9IVRw'
forager platform youtube fetch youtube:jNQXAC9IVRw --lang de --format content
```

### Model perception: `ask` and `transcribe`

`ask` and `transcribe` hand the video to a Gemini model, which watches its pictures and listens to
its sound. They run through the `gemini_api` route with the user's own Gemini key
(`providers.gemini_api.keys`; `providers.gemini_api.url` may point at the user's own Gemini
gateway). Both first read the video's metadata through the Data API (one unit), and a video that
does not exist, is private, or is streaming live fails before the model is called. Each call
spends model tokens, about 91 per second of video (some 115,000 for a 21-minute video), and takes
from a few seconds to several minutes; `usage` reports what was spent.

- **`ask`** answers one question about one video, for what captions cannot say: what is on a
  slide, in the code, or on a chart, or who appears. The answer marks the moments it relies on as
  `mm:ss`. Ask once with everything the request needs rather than many narrow questions.
- **`transcribe`** writes a transcript in the same layout as a `full_text` fetch, to
  `youtube-<id>-transcribed.md`, with `transcript.source` `model`. Run it only when the user wants
  what is said and the video has no usable caption track (a `full_text` fetch exited 5, or the
  user cannot enable `youtube_web`), never in place of captions that exist. A model transcript is
  not verbatim, and its timestamps can be off by about a second.

```console
forager platform youtube ask youtube:jNQXAC9IVRw 'How many elephants stand behind the speaker, and what is he wearing?'
forager platform youtube transcribe 'https://www.youtube.com/watch?v=jNQXAC9IVRw' --format content
```

### Video and audio files

forager returns text evidence only and never downloads media. When the user explicitly wants the
file itself (to keep, watch offline, or pass to another tool), run `yt-dlp` directly; for what a
video says or shows, the commands above answer without a file. Downloading is against YouTube's
terms and can get the egress IP blocked, so download only the videos the user named, and never
pass cookies. yt-dlp needs deno and ffmpeg.

```console
yt-dlp --ignore-config --no-playlist -o 'DIR/%(id)s.%(ext)s' -- 'VIDEO_URL'
yt-dlp --ignore-config --no-playlist -x -o 'DIR/%(id)s.%(ext)s' -- 'VIDEO_URL'
```

The second command keeps only the audio. Report the path yt-dlp printed.

## Polymarket

Polymarket runs through the `polymarket_api` route and needs no key or setup: the Gamma API
searches and describes events and markets, and the CLOB API supplies the live order book. An
event (`polymarket:<event_slug>`) groups markets; a market (`polymarket:market/<market_slug>`)
has outcomes, usually Yes and No.

- An outcome's `price` is its market-implied probability: the midpoint of the best bid and best
  ask, so it is what traders pay now, not a calibrated forecast. Report it with its `basis`:
  - `midpoint`: from the live book; `midpoint_cached`: from Gamma's cache, which may lag up to 5
    minutes;
  - `no_two_sided_book`: one side has no orders, so there is no probability; never read it as 0
    or 50%;
  - `not_trading`: the market is closed or suspended;
  - `resolution`: the market resolved, and `price` is the payout (1 for the winner), not a
    probability.
- Search prices come from Gamma's cache (`price_source: gamma`). For a current figure, fetch the
  event and check `price_source: clob` and `prices_as_of`.
- A wide `spread` or a low `liquidity` makes the midpoint weak; quote `best_bid` and `best_ask`
  with it. The probabilities of a `neg_risk` event's markets need not sum to 1, and forager does
  not normalize them.
- `--depth abstract` adds the resolution rules, which decide what each outcome means; read them
  before saying what a market predicts.
- Slugs can change or be reused by a new event. To store or compare a market over time, key it by
  `condition_id` and the event by `event_id`; use the slug only for display and links. A slug that
  is no longer found means search again.
- Polymarket US (polymarket.us) is a separate product with its own markets; forager reads only
  polymarket.com.
- Polymarket's Terms of Use §4.2 forbid scraping the site with robots or crawlers, and forbid a
  Capital Market Client or market data distributor from using the data, including through the
  API, without written consent. forager calls only the public APIs; whether the second limit
  applies depends on who the user is.

```console
forager platform polymarket search 'fed rate cuts'
forager platform polymarket fetch 'https://polymarket.com/event/presidential-election-winner-2024'
forager platform polymarket fetch polymarket:presidential-election-winner-2024 --depth abstract --format content
```

## SSRN browser route

`ssrn_browser` searches and reads SSRN in the user's own Chrome through OpenCLI: search sends
SSRN's own search request from a blank tab, and fetch reads the paper page. It is off by default.
Enable it only when the user asks for it; it runs at most one browser command every 5 seconds.

- **Install**: OpenCLI 2.x (`npm install -g @jackwener/opencli@2`) with its Chrome Browser Bridge
  extension 1.0.25 or later connected (`opencli doctor`), then copy the `opencli/ssrn` directory
  next to this skill's `SKILL.md` to `~/.opencli/clis/ssrn` (replace the whole directory to
  update). Run the same copy after updating forager, then `forager doctor` to check the adapter. A
  forager that expects a newer adapter contract fails its commands with an install hint until the
  copy is updated. The required contract is `forager-ssrn/4`; replace the whole directory so that files the new contract dropped, such as
  `search-state.js`, are removed.
- **Enable**: `forager config set platforms.ssrn.order '["ssrn_crossref", "ssrn_browser"]'`. With
  this order, a `--depth abstract` fetch that Crossref cannot serve falls through to the browser.
  Set `providers.ssrn_browser.command` when `opencli` is not on `PATH`.

## Xiaohongshu browser route

`xiaohongshu_browser` searches and reads Xiaohongshu notes and their comments in the user's own Chrome through
OpenCLI, using the account the user is logged in with. It is off by default; enable it only when the user asks for
it. It runs at most one browser command every 10 seconds and never retries. Xiaohongshu's user
agreement forbids scraping and the site rate-limits accounts: tell the user that the searches,
fetches, and comment reads run under their account, suggest a secondary account rather than their main one, and keep
the number of commands small.

- **Install**: OpenCLI 2.x (`npm install -g @jackwener/opencli@2`) with its Chrome Browser Bridge
  extension 1.0.25 or later connected (`opencli doctor`), then copy the `opencli/forager-xhs`
  directory next to this skill's `SKILL.md` to `~/.opencli/clis/forager-xhs` (replace the whole
  directory to update), and run `forager doctor`. The required contract is `forager-xhs/2`. When
  a command or `forager doctor` reports contract `forager-xhs/1`, the installed adapter is the
  older one: replace the whole `~/.opencli/clis/forager-xhs` directory with the one from this
  skill and run `forager doctor` again.
- **Log in**: the user opens xiaohongshu.com in the Chrome that OpenCLI drives and logs in there.
  Never log in, solve a verification, or switch accounts for them.
- **Enable**: `forager config set platforms.xiaohongshu.order '["xiaohongshu_browser"]'`. Set
  `providers.xiaohongshu_browser.command` when `opencli` is not on `PATH`.
- **Search**: set `--sort`, `--note-type`, and `--publish-time` only when the request implies them.
  A search cannot continue in a later command: `next_cursor` is always `null`, which here means no
  continuation exists, not that the results are complete. When stderr says more results remain,
  rerun once with a larger `--limit` (at most 100) rather than paging.
- **Fetch**: pass the `access_url` of a search item unchanged. Its token works for that note only,
  in the logged-in session, and was still valid 22 minutes later; fetch soon after searching
  rather than storing access URLs. The output's `access_url` contains the token; the ref and `url`
  never do.
- **Comments**: pass the same `access_url`. Keep the default `--replies 0` unless the request is
  about the discussion under particular comments; each expanded comment is one more click in the
  user's account. The result never continues in a later command; when `has_more` is `true` and more
  comments matter, rerun once with a larger `--limit` (at most 50).

## Read full text

A full-text fetch writes the body to a Markdown file and stdout carries only metadata and
`content_url`, `content_provider`, `content_path`, `content_len`.

1. Read the stdout metadata first.
2. List the headings with `grep -n '^#' CONTENT_PATH`, then read only the line ranges you need.
3. For several papers, hand each `content_path` to a subagent that returns only conclusions and
   citations.

Use `--depth abstract` when the abstract answers the request. Use `--format content` only when the
body must enter context or a pipe directly.

For SSRN, `content_url` is the canonical abstract page (the signed download address is never
reported), the download and the conversion share the command deadline, so retry a timed-out
full-text fetch with a larger `--timeout` (for example 300), and pass `--keep-pdf` when the
original PDF must be checked against the Markdown.

## Read a Scholar paper

Take a `link`, `versions[].link`, or `resources[].url` exactly as forager returned it:

- an arxiv.org URL: `forager platform arxiv fetch 'URL'`;
- an SSRN URL: `forager platform ssrn fetch 'URL'`;
- any other URL, or an arXiv or SSRN URL that the platform command rejects with exit 2:
  `forager fetch 'URL'`.

Never build an arXiv ref, SSRN ref, or URL from a Scholar title, byline, or snippet.

## Consume results

- Search results are candidates at `abstract` or `metadata` depth, not evidence of the paper body.
- Pass a platform URL or ref exactly as the user gave it or as forager returned it. Cite and page
  with the ref, URL, and cursor that forager returned. Construct no ref or URL yourself.
- State the content depth behind each claim: `metadata`, `abstract`, or `full_text`, and for full
  text whether `content_url` is the HTML or the PDF. A `metadata` item supports claims about its
  title, authors, and date only.
- **arXiv evidence**: cite the versioned ref (`arxiv:2401.04088v1`); an abstract is not the full
  text; make no claim about authors, affiliations, or categories beyond the returned metadata.
- **SSRN evidence**: cite the ref (`ssrn:2042750`) and its abstract-page `url`; SSRN refs have no
  version. `published` is the date the route reports, at its own precision (often only a year);
  `crossref_created` is when Crossref registered the DOI, never the SSRN posting date. Use only the
  URLs forager returns; SSRN download links are never derived from the abstract ID.
- **Scholar evidence**: cite the ref (`scholar:18208131694456651388`) and the title. A `snippet` is
  a search excerpt, not the abstract, and supports no claim about the paper's findings. `authors`
  and `source` copy Google Scholar's byline, whose names are often initials and whose author lists
  may be cut short with `…`; never present them as a complete author list. `cited_by` measures
  attention, not quality. The first entry of a fetch's `versions` is not necessarily the published
  version; choose the source from `versions` by its `link` and `source`. `version_count` is the
  count a search reports, not the number of versions a fetch lists (at most 20, often fewer); never
  report one as the other.

- **Xiaohongshu evidence**: cite the ref (`xiaohongshu:<note_id>`) with the title, and give the
  user the `access_url` to open the note; the canonical `url` does not open without the access
  token. A search item is a card, not the note: it supports claims about the title, author, date,
  and counts only; fetch the note before making claims about what it says. A fetched note is one
  person's post: attribute its claims to the author, and note that images and videos were not
  read. A card's relative time (`3天前`, `昨天 21:38`) is converted to a `published` date from
  this machine's clock, so it is accurate to the day at best; quote `published_text` when the
  exact wording matters, and when `published` is `null`. Counts are Xiaohongshu's display text
  (`1.2万`), not exact numbers. Results are personalized and change between runs; do not present
  them as a complete or stable ranking. Comments are individual readers' reactions: report them
  as opinions with their counts, not as facts about the note's subject, and say how many of the
  note's comments you read (`has_more`, `replies_has_more`).

- **X evidence**: cite the ref (`x:<post_id>`) with the author handle, and give the user the
  `url` to open. Post text, handles, and ask answers are relayed by a model: when the exact wording
  matters (a quote, a number, a claim attributed to a person), check it with `forager fetch` on the
  post URL or tell the user to open the post, and never present relayed text as a verbatim
  quote. A post is one account's statement: attribute it.
  Thread replies are not checked against the response, so treat them with more caution than the
  root. An `ask` answer without `sources` is unverified. Results reflect what the model chose to
  read, not a complete or ranked view of X.

- **Hacker News evidence**: cite the ref (`hn:<id>`) and the `url`. A comment is one user's
  opinion: attribute it to its author and report it as an opinion, never as a fact about the
  subject. A discussion file is not a complete mirror of the HN page: deleted and dead comments
  are absent and comments from the last few minutes may be missing. Search and `stories` counts
  may lag the site by a few minutes; `metadata` and `thread` counts are live. Use only the refs
  forager returns; construct no item ID yourself.

- **YouTube evidence**: cite the ref (`youtube:<video_id>`) with the channel, and give the user the
  `url`. The description and chapters are what the publisher wrote about the video, not what is
  said or shown in it: never report them as the video's content. A transcript is what was said,
  as the caption track renders it: quote it with the paragraph's timestamp link, say whether it is
  the publisher's captions or automatic speech recognition (`transcript.source`), and never report
  it as what the video shows. Comments are what individual viewers wrote: attribute them, and
  never present them as what the video says. Counts are YouTube's at the time of the request.
  An `ask` answer and a `transcribe` transcript are a model's account of the video, relayed
  content: attribute them to the model (`model`), check a moment against the video before quoting
  it, never quote a model transcript as the speaker's exact words, and say when the answer rests
  on what the model saw rather than what was said. Use only the refs forager returns; construct no
  video ID yourself.

## Recover

- Exit 2 naming an unsupported option: retry once without that option and tell the user which
  condition you relaxed.
- A `--cursor` failure: drop the cursor and search again from the first page.
- Exit 3, or an `auth` error: go to Diagnose or configure in `SKILL.md`.
- Scholar `quota_exhausted` (exit 4): every configured SerpApi key has used up its searches. Tell
  the user the Scholar quota is exhausted and continue with `forager search` or the arXiv or SSRN
  platform. Do not retry Scholar.
- Scholar `rate_limited` (exit 4): SerpApi's hourly limit; tell the user and retry later, not in
  a loop.
- Scholar `auth` (exit 4): SerpApi rejected the key; ask the user to check
  `providers.serpapi.keys`.
- Scholar exit 3 naming `providers.serpapi.keys`: no SerpApi key is configured. Ask the user to
  run `forager config set providers.serpapi.keys -` and type `["THEIR_KEY"]` on stdin (or rerun
  `forager setup`), so the key stays out of shell history and out of this conversation.
- YouTube `quota_exhausted` (exit 4): the Data API key has used up today's quota (it resets at
  midnight Pacific time). Tell the user and continue with `forager search`; do not retry YouTube.
- YouTube `auth` (exit 4): the API rejected the key; ask the user to check
  `providers.youtube_api.keys`.
- YouTube exit 3 naming `providers.youtube_api.keys`: no key is configured, or more than one is
  (YouTube allows one). Ask the user to run `forager config set providers.youtube_api.keys -` and
  type `["THEIR_KEY"]` on stdin (or rerun `forager setup`).
- Exit 5 on a YouTube `--depth abstract` fetch: the video has no description. Fetch with
  `--depth metadata` and say the description is empty.
- Exit 2 on a YouTube full-text fetch naming `youtube_web`: captions need that route. Fall back to
  `--depth abstract`, and tell the user what enabling it means (see "Captions") rather than
  enabling it yourself.
- Exit 5 on a YouTube full-text fetch with no qualifying caption track: the message lists the
  languages that exist. Retry once with `--lang` for a listed language the user can use; when no
  listed language fits and the user needs what is said, run `transcribe` and say the transcript is
  a model's, not captions.
- YouTube `ask` or `transcribe` exit 3 naming `providers.gemini_api.keys`: no Gemini key is
  configured. Ask the user to run `forager config set providers.gemini_api.keys -` and type
  `["THEIR_KEY"]` on stdin.
- YouTube `ask` or `transcribe` `auth` (exit 4): Gemini rejected the key; ask the user to check
  `providers.gemini_api.keys`. `rate_limited` or `quota_exhausted`: every key is out of model
  quota; tell the user and do not retry in a loop.
- Exit 5 on `ask` or `transcribe`: the video is streaming live (retry after it ends), the output
  was cut off (for `transcribe`, the video is too long to transcribe in one answer), the model
  returned no valid transcript, or a safety filter blocked it. Report it; do not retry the same
  request.
- YouTube `auth` or `rate_limited` (exit 4) from `youtube_web`, or `runtime` saying the caption
  track was empty: YouTube is restricting the egress IP. Stop fetching YouTube captions, tell the
  user to wait or switch networks, and do not retry.
- Exit 5 on full-text fetch (both HTML and PDF too thin): report it; use `--depth abstract` only
  when the abstract still answers the request, and say so.
- Exit 5 on an SSRN `--depth abstract` fetch: no route had the abstract. Fetch at the default
  `metadata` depth and state that the abstract is unavailable.
- Exit 5 on an SSRN `--depth full_text` fetch: the download failed its checks or the converted
  body was too thin. Retry once with a larger `--timeout`; when it still fails, read the kept PDF
  whose path the error message names, or fall back to `--depth abstract` and say so.
- "SSRN paper not found in Crossref": the paper may still exist on SSRN; say that Crossref has no
  record of it rather than that the paper does not exist.
- An `ssrn_browser` `auth` error saying SSRN security verification did not clear: ask the user to
  open SSRN in Chrome and pass the check by hand, then retry. Never try to pass it for them.
- An `ssrn_browser`, `xiaohongshu_browser`, or Deep Research error saying it found OpenCLI 1.x
  output: forager needs OpenCLI 2.x and Browser Bridge 1.0.25 or later. Tell the user to upgrade
  both as in the install step, then retry; do not upgrade them yourself.
- An `ssrn_browser` error that names the forager OpenCLI adapter, or a doctor `message` with
  install steps: repeat the install step in "SSRN browser route".
- A `xiaohongshu_browser` `auth` error: the Chrome session is logged out, or Xiaohongshu wants a
  verification (461). Ask the user to open xiaohongshu.com in Chrome, log in or complete the check
  by hand, then retry once.
- A `xiaohongshu_browser` `parameter` error naming `300031`, `300017`, or a security restriction on
  a search: Xiaohongshu is limiting the account. Stop using Xiaohongshu for this task, tell the
  user, and do not retry; it may clear after a long pause.
- The same error on a fetch or a comments read (`Xiaohongshu note unavailable`): the token may be
  stale, the note restricted or removed, or the account rate-limited, and the site does not say
  which. Search once more for the note and retry with the new `access_url`; when that also fails with `300031` or
  `300017`, stop using Xiaohongshu, tell the user, and try again only after a long pause.
- Exit 5 on a Xiaohongshu fetch: the note has no title, text, or images; report it.
- A Xiaohongshu comments `runtime` error saying the adapter could not expand the replies of a
  comment: the page did not show the expected button. Retry once without `--replies`, and tell the
  user the replies were not read.
- A `xiaohongshu_browser` `timeout`: retry once with `--timeout 180`; when it times out again,
  report it.
- A `xiaohongshu_browser` `network` error saying Chrome could not load the page: the browser could
  not reach Xiaohongshu. Retry once after a short pause; when it fails again, tell the user to
  check the network in that Chrome.
- A `xiaohongshu_browser` error that names the forager OpenCLI adapter, or a doctor `message`
  with install steps: repeat the install step in "Xiaohongshu browser route".

- X exit 3 naming `providers.xai.keys`: no xAI key is configured; go to Diagnose or configure in
  `SKILL.md`. Exit 3 naming `platforms.x.order`: the user disabled X; do not re-enable it yourself.
- X `runtime` saying none of the posts could be verified, or that the posts are not the requested
  JSON: retry once; when it fails again, use `platform x ask` or `forager search` instead and say
  so.
- X `parameter` saying a post was not found or is not visible: the post may be deleted, private,
  or out of the model's reach; report it rather than retrying.
- A `t.co` link (exit 2): ask the user for the x.com URL it opens.

Platform retrieval is complete when the requested items or content are read at the depth the answer
needs, or a terminal failure is reported with its recovery.
