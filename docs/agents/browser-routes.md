# Developing and live-testing browser routes

A browser route (`xiaohongshu_browser`, `reddit_browser`) goes through forager's own browser transport, launches the Managed Browser in its platform's Browser Profile and runs the route's own Page Logic. The split between transport and Page Logic is in `docs/architecture.md`; the decisions are ADR-0036 and, for Reddit, ADR-0037.

## What each layer proves

| Layer | How it runs | Proves |
| --- | --- | --- |
| Unit tests of the judgements (`*_tests.rs`) | The full test suite | The rules from an observed page to classification, checks, decoding and output |
| Binary tests against a stand-in browser | The full test suite | Launch arguments, the preflight check, setup hints, the deadline and process cleanup; the stand-in does not speak CDP |
| Real-browser tests (`browser_real_tests.rs`, the login wait in `ops/browser_setup.rs`, and the projections in `reddit_browser_search_tests.rs` and `reddit_browser_post_tests.rs`) | Ignored by default; see below | Native input, the single hittable target, wheel loading, request capture, the profile on disk, the browser exiting when forager is killed, and the values the Reddit projection scripts read on a page; they open only local pages |
| The platform's live smoke cases | `forager smoke --live` | Page driving and the success path on the real site; the only layer that notices a site redesign |

A change to page driving (navigation, input, waiting, the capture filter) is proven only by live smoke; a change to a judgement is proven by the unit tests.

## Real-browser tests

These tests are marked `#[ignore]`, so the full test suite lists them as skipped. After changing the browser transport, `browser login` or the Managed Browser, and before every release, run them on a host with the Managed Browser installed:

```sh
cargo nextest run --locked --profile real-browser --run-ignored only
```

- The Reddit projection tests (one for the search page, one for the post page) run the in-page projection script on local pages stored as samples (`tests/samples/reddit_browser/*.html`) and assert that its output equals the stored JSON projection the judgement tests read. After changing a projection script or a local page, run them with `FORAGER_WRITE_PROJECTIONS=1` to rewrite the JSON, then review the diff and update the samples' records.
- They need the installed Managed Browser (`forager browser install`), or another browser executable named by `FORAGER_REAL_BROWSER_PATH`. Without a browser the tests fail with the reason; they never pass silently.
- The tests open only static pages served on this machine, visit no real site, need no login, and each uses its own temporary Browser Profile.

## Before a live test

Run a command that opens site pages (a platform command, `doctor --provider <route>`, `smoke --live`) only when all of the following hold. Below, `<platform>` is `xiaohongshu` or `reddit`.

- The Managed Browser is installed: `forager browser install` exits 0.
- The user has logged in in person: the user runs `forager browser login <platform>` and logs in in the window it opens (a QR scan for Xiaohongshu, an account login for Reddit). Only after that command exits 0 does the Browser Profile carry a login marker. On Reddit the risk falls on the logged-in account, so the user chooses which account.
- `platforms.<platform>.order` contains the route. Enabling a route is the user's decision; do not edit the configuration file unless the user asks. To verify, enable it for one command through an environment variable, such as `FORAGER_PLATFORMS__REDDIT__ORDER='["reddit_browser"]'`. `smoke --live --opt-in` (the live step of a release) does not need this condition: with the browser installed and a login marker in the Browser Profile, it runs that platform's cases too, and the user's login is the consent.
- `forager doctor` shows the route as `reachable: true`. Shallow doctor runs only the preflight check, without starting the browser or visiting the site; when it fails, its `message` gives the fix.

To verify an unreleased change, run `./target/debug/forager`; the installed `forager` is the last release.

## Pacing and budget

- A route paces itself (one browser command at a time across processes) and does not retry. Do not start several commands in parallel to get around the pacing.
- **Page-open budget.** Every live platform command, deep doctor and smoke case opens real pages. Open at most about 10 on one site per investigation, and ask the user before going over. Questions that need repeated samples, such as how often a failure occurs, are bound by the same budget.
- **Save every open.** Write the full stdout and stderr of each live command to a file and analyse from the file; do not run the command again to look at another field.
- **Take Reddit post IDs only from search results**, never from memory. A misremembered ID once opened a post in an NSFW subreddit, and it stays in the account's browsing history. Do not open NSFW content.

## Stop rules

On Xiaohongshu, stop every live command and loop on the site at once when any of these appears:

- `auth`: not logged in (including an API `code` of −101 or −100), HTTP 461 or 471 asking for verification, or a page notice such as "请求太频繁" or "访问频次异常";
- `parameter`: 300031 or 300017 (from a page redirect or an API response), a security restriction, or an abnormal access link;
- a captcha or verification page (the route does not recognise these; it reports Runtime with the page title and URL);
- an API response with a non-zero `code` the route cannot classify (Runtime, the message starts with `Xiaohongshu answered` and carries the code);
- two `timeout` results in a row.

On Reddit, stop at once on `auth` (the verification page `Prove your humanity`, a document 403 or 429, or a logged-out page) and on two `timeout` results in a row.

After stopping, finish what can be done offline, then report the errors seen and how many times to the user. The user runs `forager browser login <platform>` and handles the login or verification in the window. A block does not lift quickly by waiting, and a run after a cool-down only triggers it again.

## Boundaries

Every live operation acts as the user's account. These are hard boundaries:

- Only the user logs in or passes a verification, by hand. The agent never scans a code, types an account or a verification code, or clicks through a verification page on the user's behalf.
- The login state stays in the Browser Profile. Do not read, copy or export its cookies, tokens or any of its files (the profile is in `$XDG_STATE_HOME/forager/browser/<platform>`, or `~/.local/state/forager/browser/<platform>` when that is unset), and do not copy the profile elsewhere to experiment.
- Read only the requests the page itself makes and the page state through read-only scripts. Page Logic sends no API request of its own and does not patch `fetch`, XHR or WebSocket; it changes page state only through native input.
- After a risk signal, do not retry, and do not try again with another profile, browser or fingerprint.
- Commits and live-test records never contain an Access Token (`xsec_token`), a cookie, an account identifier, or a real user's note or user ID; samples use fake values for these.

## Known behaviour

- **A search waits while the login window is open.** `browser login` and the platform commands share the route's pacing permit, and login holds it until the browser exits. While the login window is open, a command waits for the permit and times out if it does not get one by its deadline. A command that has the permit but finds the Browser Profile held by another forager process fails at once with Runtime.
- **The pinned version changed.** When an upgraded forager pins a different browser version, commands name the installed old version and ask for `forager browser install`. At run time only the pinned version is accepted.
- **A login marker does not mean a valid session.** When the session has expired, the command fails with Auth and suggests `forager browser login <platform>`, while the preflight check still passes.
- **Handshake failure.** When the browser exits before the CDP handshake or does not answer within the handshake timeout, the result is Runtime, and the message carries the last line of the browser's stderr, plus its exit status when it exited. Look at those first.
- **`success` does not tell an API failure on Xiaohongshu.** `search/onebox` can return `code: 0` with `success: false`, and the account request `user/me` returns `code: 0` with `guest: true` for a visitor.
- **Decoy copies.** Xiaohongshu stacks invisible copies with the same class name and text under its buttons, and renders every option of the filter panel twice. Every click goes through native input's single-hittable-target rule; never take the first element matching a class name or text.
