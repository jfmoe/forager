# Read Reddit in the logged-in Managed Browser

## Decision

The Reddit platform has one route, the browser route `reddit_browser`. It opens the site's real
search and post pages headless in Reddit's own Browser Profile, logged into an account the user
signed in to themselves with `forager browser login reddit`, and reads what the pages render. The
route is opt-in (ADR-0020), does not retry, and follows the browser transport's boundaries
(ADR-0036).

- **A read-only script projects, Rust judges.** Each operation navigates to a real page URL and
  evaluates one read-only script that selects and copies what is needed into a compact JSON
  projection: page facts (final URL, title, the raw login attribute, challenge markers, load
  errors) and data records (element attribute values and the HTML of body containers). The script
  makes no judgement, sends no request, changes nothing, and reads no component JS properties,
  stores or shadow DOM. Recognising challenges and logged-out pages, checking the post's identity,
  validating required fields, mapping output and converting body HTML to Markdown all happen in
  Rust.
- **The viewer's data stays in the page.** The projection carries no identifier of the logged-in
  account and no page token. The post page's screen-view record is parsed in the page and yields
  only the post ID and the adult flag; viewer attributes on the post element are not read.
  Passive capture records only the status of the site's requests and keeps no response body.
- **No requests of its own**, including the old `.json` endpoints and the page fragments.
- **Search loads more by letting the page scroll** and issues no continuation cursor; a short
  result says so on stderr.
- **Fetch reads the first batch** of comments the post page brings, without scrolling for more or
  expanding replies.
- **Stop at a challenge** (ADR-0020). A logged-out page counts as a challenge; a logged-in, loaded
  page without the post means only that this post is not visible to the account.

## Why

- On 2026-11-21, on the maintainer's only available egress (a datacenter IP behind a local proxy),
  every search and fetch of the anonymous route got HTTP 403 with Reddit's "blocked due to a
  network policy" page, and curl got 403 for the home page, the page fragments, the embed page and
  the anonymous `.json` endpoints. The whole site refused that egress, whatever the path.
- An anonymous Managed Browser got a reCAPTCHA "Prove your humanity" page on the search page in
  both samples. A logged-in Managed Browser opened search and post pages normally: 9 page opens,
  no challenge, and every reddit.com request 2xx.
- The official public Data API accepts no new applications and is closing on Reddit's announced
  schedule.
- A projection read from the DOM fits where captured responses do not: one 1,369-comment post's
  document was 4,066,580 bytes, 97% of the transport's per-response capture limit, while DOM
  evaluation is bound only by the much larger CDP message limit. Content the page loads on scroll
  enters the DOM too, so one read covers the first screen and later pages without attributing
  fragments to scrolls.
- Judgements written as pure functions over the projection are testable without a browser, and
  running the script on local pages against the same JSON samples binds script and judgement
  together.

## Rejected alternatives

- **Keeping the anonymous route `reddit_web`, alone or beside the browser route.** It read the
  site's own server-rendered search fragments, comment-tree fragments and embed pages over HTTP,
  with no account, on any host. It worked on the same egress at launch on 2026-10-28, but no test
  layer exercised it for 24 days, because it was opt-in and the release's live run ran on a
  configuration without it, and by 2026-11-21 the site refused that egress outright. Two routes
  would mean two page parsers and two cursor semantics, and a fallback would spend the egress
  IP's standing before touching the account. It is retired with no compatibility layer. The HTTP/2
  support it needed in the shared HTTP client (Reddit answered 403 when the TLS handshake's ALPN
  offered only HTTP/1.1) stays, as ordinary client configuration unrelated to any route. A client
  that imitates a browser's fingerprint was rejected then as detection evasion that would replace
  the whole HTTP stack, and switching to native-tls as inconsistent across platforms and
  irrelevant, since the cause was ALPN, not the TLS library.
- **An anonymous browser plus a one-time human challenge.** Passing the challenge probably sets a
  pass cookie, but its name, lifetime and binding to egress, fingerprint or headedness are
  unknown, every expiry needs a person, and `forager browser login` would have to serve a route
  that needs no login.
- **Querying endpoints from inside the page, including `.json`.** ADR-0036 forbids it and no
  required field needs it. Replaying a page's own continuation request returned the page after
  next, because the cursor is stateful on the server; `.json` returns rich fields when logged in,
  but the page never requests it, and the request would carry the account's cookies.
- **The Arctic Shift archive.** A third-party archive with lagging scores and counts; a keyword
  query got 422 "slow down" on 2026-10-17, and its deletion sync, licensing and availability are
  unclear.
- **ScrapeCreators.** A paid key, untested, and data passing through a vendor that leaves the
  source site's terms to its customers. It could be evaluated as another route without
  overturning this decision.
- **The official Data API.** No new applications are accepted, and it would need OAuth credential
  lifecycle management.

## Revisit when

- An account used through the route is warned, limited or banned, or logged-in pages begin to show
  challenges. Reddit's User Agreement does not allow automated access, and the risk now falls on
  the logged-in account instead of the egress IP. The evidence is thin: one account, one egress
  and one evening, with 9 prototype page opens, 13 runs of the route and 3 live test runs. Every
  logged-in page runs an invisible reCAPTCHA score whose threshold for a visible challenge is
  unknown, and the headless traits ADR-0036 lists remain.
- The page shape changes. Reading depends on attribute names of Reddit's custom elements, which
  have no public contract and already changed once between 2026-10 and 2026-11; only the route's
  Live Checks notice.
- Reddit offers official access suitable for a personal CLI.
- Reddit must be read reliably on hosts other than macOS arm64, where the browser transport has no
  Managed Browser package or does not run.

## Supersedes

- ADR-0033, Reddit through anonymous reads of the site's own pages: the route is retired, and its
  only surviving point, HTTP/2 in the shared client, is kept above as ordinary configuration.
