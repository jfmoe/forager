# Browser routes are opt-in and paced per operation

The `ssrn_browser` route reads SSRN in the user's own Chrome through a local OpenCLI adapter. It is the first **process route**: a Platform Route that runs a local command instead of sending HTTP requests. This record sets how such a route is enabled and paced, and how it delivers full text.

## Users enable a browser route themselves

SSRN's Terms of Use forbid "automated queries of any sort" (read 2026-09-26; see `docs/research/2026-09-26-ssrn-integration.md`). The maintainer accepts this risk for personal use at a pace close to a person's, and only when the user opts in. Therefore:

- The platform catalog lists `ssrn_browser` as a valid route but never in `default_order`. It runs only after the user adds it to `platforms.ssrn.order`.
- No release adds a browser route to any default order.
- The route never solves or bypasses a site challenge. When SSRN shows a security check that does not clear by itself, the attempt fails as Auth, and the user passes the check in Chrome.

## A process route is paced per OpenCLI operation

For an HTTP route, the access policy paces every HTTP send. A browser page load sends many requests that forager cannot see or pace one by one. For a process route, the access-policy unit is one OpenCLI command: `ssrn_browser` allows one command every 5 seconds with a concurrency of 1, across all forager processes. The permit is held until the command's process is reaped, so a killed command still counts until it has stopped.

## Full text is converted from the local file and delivered as Markdown

A browser route cannot hand Web Fetch a URL: the SSRN download address is signed, expires after 300 seconds, and must never appear in output, attempts, or logs. Instead the route downloads the PDF inside the same OpenCLI operation that reads the paper page, verifies the file in the same attempt (complete download, file present, `%PDF-` magic, matching page id), and declares the local file as the full-text source. Web Fetch accepts a local file next to a URL as its input, so the same provider chain, thin-content gate, and error attribution convert it; Firecrawl reads it through `/v2/parse` with the same PDF parser options as `/scrape`. The conversion result is delivered as a local Markdown file, keeping stdout to metadata and paths; the PDF is removed after a successful delivery unless `--keep-pdf` moves it next to the Markdown, and it is always kept, with its path in the error message, when the conversion fails. The capability holds no platform knowledge: any platform that downloads a file reuses this path.

## Consequences

- Provider registrations declare their transport (HTTP, or an OpenCLI adapter with its site and contract version). Configuration checks, doctor, and the platform checklist branch on the transport, not on route IDs: an HTTP route has `url` and `timeout`, a process route has `command` and `timeout`.
- Doctor checks a process route, in shallow and deep mode, only when a platform order enables it, so an unused browser route never runs and never affects the health result.
- The route depends on the user's machine: OpenCLI, its browser bridge, a logged-in Chrome, and the forager adapter installed in OpenCLI's adapter directory. Doctor reports a missing or outdated adapter with the install steps.
- Process routes need Unix process groups to stop every process a command starts, so other hosts skip them.
- Web Fetch accepts a local file as its input next to a URL; a provider declares a pure support check, an unsupported source marks the provider Skipped, and a chain with no capable provider fails naming the source kind. The route's full-text source is a set of URLs or one verified local file, and the platform fetch orchestration runs the same Web Fetch chain for both. ADR 0022 adds a third source, a native body the route reads itself, which needs no Web Fetch; that supersedes this list as exhaustive.
- A killed command leaves no tab behind permanently: the OpenCLI daemon's command timer reclaims the ephemeral tab (verified 2026-09-26 with a SIGKILLed `ssrn paper` command: the tab was gone within the 25-second command timeout).
