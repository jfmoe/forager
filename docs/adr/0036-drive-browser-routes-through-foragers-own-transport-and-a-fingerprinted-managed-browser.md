# Drive browser routes through forager's own transport and a fingerprinted Managed Browser

## Decision

A browser route reads a site in a browser that forager launches and drives itself, never in the
user's everyday browser and never through an external browser driver.

- **Browser transport.** Each browser command launches one browser process in its own process
  group and talks to it over a CDP pipe, with no debugging port and no resident process. The
  transport holds no site knowledge: which pages to open, which requests to read and how to judge
  the result belong to each route's page logic, written in Rust. Page logic changes page state
  only through CDP native input, and takes data only from passively captured requests and
  read-only page-state scripts. It never sends requests of its own to the site, and never rewrites
  the page's `fetch`, XHR or WebSocket.
- **Pacing per browser command.** A page load sends many requests that forager cannot see or pace
  one by one, so a browser route's access policy counts one browser command, across processes.
  The permit is held until the browser process is reaped, so a killed command still counts until
  it has stopped, and `forager browser login` counts against the same pace.
- **Managed Browser.** forager pins a release of fingerprint-chromium whose patch source is
  already public, downloads it from the upstream GitHub Release, verifies it against a SHA-256
  built into forager, and installs it with `forager browser install`. The pinned release ships an
  installer only for macOS arm64. The user may configure another local browser instead; forager
  does not check that it is a fingerprint build.
- **Browser Profile.** Each platform has its own profile in forager's state directory. Its
  fingerprint seed is generated once when the profile is created and never changes. The user logs
  in themselves in the headed window that `forager browser login <platform>` opens; later commands
  reuse the profile headless.
- **Fingerprint boundary.** forager accepts a browser fingerprint that the Managed Browser
  generates from the profile's fixed seed, on a fingerprint platform that matches the host. A
  profile always presents as the same device: forager never rotates fingerprints, never changes
  environment to get past a block, and injects no stealth script into the page.
- **Login state stays in the profile.** forager never reads, exports or injects cookies or tokens.
- **Read-only.** Routes perform no writes on the site: no likes, saves, comments or posts.
- **The route declares its browser environment.** The registration names the route's transport as
  a browser together with its page language; the route uses the logged-in Browser Profile of the
  platform it serves.
  Configuration checks, the preflight check, doctor and the platform checklist read that
  declaration and never branch on a route ID, so a new browser route writes only its page logic.

## Why

- Driving the user's own Chrome through OpenCLI opened an automation window on the user's
  desktop, and when another window covered it OpenCLI 2.x refused to send native input, so search
  and comments failed as Runtime. An agent calling forager in the background left that window
  covered most of the time.
- OpenCLI needed a global OpenCLI 2.x install, a manually updated Browser Bridge extension, JS
  adapters copied into OpenCLI's adapter directory, and Node; drift in any of them broke the
  route.
- A headless browser without a fingerprint is visible to the site: stock Chromium's new headless
  mode reports `HeadlessChrome` in its user agent. xiaohongshu-mcp ran headless by default, and in
  June and July 2026 its users reported accounts warned, muted or banned; on 2026-07-20 it moved to
  a bundled fingerprint browser whose macOS package is byte-identical to the release forager pins.
- On 2026-11-18 one real account completed a headed login and headless search, fetch and comments
  with the same release and launch arguments, with no risk-control signal. On 2026-11-19
  forager's own page logic ran 22 browser commands on that account across the main option
  combinations of the three operations, again with no signal.
- Pinning and verifying the browser makes the installed build known. The upstream publishes patch
  source late and releases installers unevenly across platforms, so a browser each user installs
  could be any build, complete or not.
- Rust page logic needs no Node at run or test time, and its judgements are pure functions that
  unit tests cover directly. The old JS adapters were bound to OpenCLI's interface and could not
  be reused as they were.

## Rejected alternatives

- **OpenCLI, an external browser CLI driving the user's Chrome.** It ran Xiaohongshu, SSRN and the
  Deep Research providers as process routes paced per OpenCLI command. It interfered with the
  desktop, failed when run in the background, and carried an external toolchain that drifted.
- **Keeping OpenCLI beside the browser transport.** A route's inputs, outputs and error classes do
  not depend on the transport, so a second path would keep OpenCLI, the Browser Bridge and Node,
  and two code paths, for no user who needs them. There is no compatibility layer.
- **A Node sidecar or Playwright for page logic.** It would reuse some adapter code but keep Node
  and a second browser runtime as dependencies, and tests would need Node too.
- **Letting users install the browser themselves.** It cannot guarantee which build is installed
  or that the package is complete; a user-chosen browser remains as a fallback.
- **A stock headless browser without a fingerprint.** Its headless traits are visible to the site,
  and accounts using one were banned.
- **A fake browser that speaks CDP in tests.** Simulating navigation, network events and
  evaluation would cost far more than the code it tests, and passing would only show agreement
  with the simulation. Offline tests cover the pure judgements and the transport without a real
  site, and the route's Live Checks prove the page logic on the site. The cost is that no offline
  test runs a browser route's success path end to end.

## Revisit when

- An account used through a browser route is warned, muted or banned. The evidence of
  feasibility is small: one account, a few sessions and tens of commands; long sessions, sustained
  use and other accounts are untested.
- Upstream stops publishing patch source or macOS installers, so the pinned release cannot move
  forward. The pin already lags the newest upstream release, and the older it gets the more
  Chromium security fixes it lacks.
- The site starts acting on the headless traits the build does not mask. Upstream changes only the
  `HeadlessChrome` user agent in headless mode, and forager corrects only the screen size; on
  2026-11-19 the headless window's outer height still exceeded its inner height and the page
  reported no focus.
- Browser routes are needed beyond macOS arm64. The transport runs only on Unix hosts, and on Unix
  hosts other than macOS arm64 only a user-chosen browser runs, where a non-fingerprint browser
  presents its own real traits.
