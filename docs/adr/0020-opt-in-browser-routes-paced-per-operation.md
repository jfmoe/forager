# Browser routes are opt-in and paced per operation

The `ssrn_browser` route reads SSRN in the user's own Chrome through a local OpenCLI adapter. It is the first **process route**: a Platform Route that runs a local command instead of sending HTTP requests. This record sets how such a route is enabled and paced.

## Users enable a browser route themselves

SSRN's Terms of Use forbid "automated queries of any sort" (read 2026-09-26; see `docs/research/2026-09-26-ssrn-integration.md`). The maintainer accepts this risk for personal use at a pace close to a person's, and only when the user opts in. Therefore:

- The platform catalog lists `ssrn_browser` as a valid route but never in `default_order`. It runs only after the user adds it to `platforms.ssrn.order`.
- No release adds a browser route to any default order.
- The route never solves or bypasses a site challenge. When SSRN shows a security check that does not clear by itself, the attempt fails as Auth, and the user passes the check in Chrome.

## A process route is paced per OpenCLI operation

For an HTTP route, the access policy paces every HTTP send. A browser page load sends many requests that forager cannot see or pace one by one. For a process route, the access-policy unit is one OpenCLI command: `ssrn_browser` allows one command every 5 seconds with a concurrency of 1, across all forager processes. The permit is held until the command's process is reaped, so a killed command still counts until it has stopped.

## Consequences

- Provider registrations declare their transport (HTTP, or an OpenCLI adapter with its site and contract version). Configuration checks, doctor, and the platform checklist branch on the transport, not on route IDs: an HTTP route has `url` and `timeout`, a process route has `command` and `timeout`.
- Doctor checks a process route, in shallow and deep mode, only when a platform order enables it, so an unused browser route never runs and never affects the health result.
- The route depends on the user's machine: OpenCLI, its browser bridge, a logged-in Chrome, and the forager adapter installed in OpenCLI's adapter directory. Doctor reports a missing or outdated adapter with the install steps.
- Process routes need Unix process groups to stop every process a command starts, so other hosts skip them.
