# Deliver YouTube's three evidence layers through three routes

## Decision

The `youtube` platform delivers only text evidence, split by source into three layers that never
stand in for each other, each delivered by its own route over the HTTP transport:

| Layer | Content | Where in the contract | Route |
|---|---|---|---|
| Publisher text | Title, description, chapters | Metadata and abstract | `youtube_api` (YouTube Data API v3, user's key); `youtube_web` can also deliver it |
| Platform captions | One caption track already on the platform, uploaded by the publisher or recognised automatically | Full text, written as a Transcript file | `youtube_web` (anonymous reads of the web page and the undocumented player and caption endpoints) |
| Model perception | A model's answer or transcript after it has watched and listened | The Platform Operations Ask and Transcribe, as Relayed Content | `gemini_api` (Gemini's native API, user's key, configurable endpoint) |

- **No fallback between layers.** Full text delivers only captions that exist on the platform; a
  video without captions fails full text instead of falling back to a model transcript. A model
  transcript comes only from Transcribe, which the caller asks for explicitly.
- **`youtube_web` is opt-in** (ADR-0020). Its enable instructions warn that using it together with
  an official key may get the user's Google project suspended under the Developer Policies' ban on
  obtaining scraped data.
- **`youtube_api` takes exactly one key.** Rotating keys when quota runs out would amount to using
  several projects to get around the quota, so configuration rejects more than one key, and quota
  errors are QuotaExhausted with no rotation.
- **The key travels in a request header**, not the URL's query, so it never reaches the URL that
  HTTP client errors, attempts and logs carry, and no URL redaction is needed.
- **Errors are attributed by the upstream reason; status mapping stays in the network layer.** An
  invalid key is HTTP 400 while quota and other permission errors are all HTTP 403, so status alone
  cannot tell them apart. The route supplies a reason classifier that the network layer's read
  boundary applies, and a reason it does not recognise falls back to the shared status mapping.
- **Search enriches results with one batched video lookup** for duration, counts and the caption
  flag. It costs one quota unit; when it fails the search results stay and a diagnostic is
  reported, following ADR-0027's enhancement rule.

## Why

Measured and read on 2026-10-17:

- The official Data API searches, reads metadata and comments, and lists playlists and channels,
  but cannot read other people's captions: caption download needs OAuth and edit rights on the
  video. The default quota is 100 searches and 10,000 units of other reads a day, and failed
  requests are billed too.
- The undocumented web path (watch page, then the player endpoint, then the caption track URL)
  returns uploaded and automatic captions anonymously, and each track says whether it is automatic.
  The risk falls on the egress IP and is temporary; no case of an account banned for anonymous
  access was found. It does breach the Developer Policies and the terms' limits on automated
  access.
- Gemini understands the picture and sound of a YouTube video URL directly and bills by token.
- The Developer Policies require one project per client and forbid getting around the quota.
- Captions, descriptions and model output differ in trustworthiness, so a caller needs to know who
  wrote each piece of text.

## Rejected alternatives

- **A local-tool transport running yt-dlp.** It reads captions, but a new transport kind, an
  external dependency and install steps for one platform do not pay off when the HTTP transport
  already works. Its automatic captions also repeat as they scroll (one track was 252 KB as VTT
  against 56 KB as XML).
- **SerpApi's transcript engine.** It silently ignores the request for automatic captions, returns
  uploaded ones while echoing the automatic type, and does not say which track it returned, so the
  caption source cannot be labelled reliably; one video costs two billed calls.
- **A model transcript as the full-text fallback.** Callers would cite the model's dictation as
  the platform's captions, mixing the layers.
- **A credential pool for `youtube_api`.** See the quota rule above.
- **The official API only.** No captions, so the video's content would be only its description.
- **A browser route.** It would need a login and a browser, while anonymous HTTP already returns
  the captions.

## Revisit when

- `youtube_web` breaks often. YouTube changes its player endpoints from time to time, and the route
  is maintained against youtube-transcript-api's fixes; datacenter IPs are often blocked on the
  first request, so it does not suit CI. Frequent breakage would reopen yt-dlp.
- The search quota becomes the bottleneck. Day to day, 100 searches a day limit `youtube_api`, and
  deep doctor and live smoke each spend one; fetch, comments and each page of videos cost one unit.
- Whether using subscription quota through a third-party Gemini-compatible gateway complies with
  Google's terms is checked. It has not been, so `gemini_api` points at the official API by
  default.
- forager adds a cache across calls. The API terms require API data to be refreshed or deleted
  within 30 days; today forager keeps nothing between calls and the files it writes are the
  user's to manage.
- Without any key, only the opt-in `youtube_web` works, and it offers no search; a keyless search
  path would change that.
