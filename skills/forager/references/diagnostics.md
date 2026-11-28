# Configure and diagnose

Exit codes: `2` input forager refused before sending anything, `3` configuration (including a
missing key or an empty order), `4` a provider or transport failure (`auth`, `rate_limited`,
`quota_exhausted`, `parameter`, `network`, `timeout`, `runtime`), `5` content too thin or
missing. The message names the key, provider or fix; start from it.

## Keys stay out of the conversation

Never put a key on a command line or ask the user to paste one into the chat. Ask the user to run
`forager config set providers.<provider>.keys -` and type `["THEIR_KEY"]` on stdin, or to run
`forager setup`, which prompts for the main model's and the providers' keys (Enter keeps the
current value).

## Find and repair configuration

1. `forager config path` prints the configuration file's path, even when the file is broken.
2. `forager config list` prints every effective value with its source, keys masked. When the file
   cannot be parsed, it names the path, the bad key and the location.
3. Repair a parseable file with `forager config set KEY VALUE` (plain text for strings, numbers
   and booleans; a TOML literal such as `'["a","b"]'` for an array) or `forager config unset KEY`.
   Edit the file at the printed path only when its TOML syntax is broken.

- A `FORAGER_*` environment variable overrides the file; `config list` shows it as the source and
  `config unset` warns when one still applies. An unknown key in the file, or an unknown
  `FORAGER_*` variable, exits 3 naming it.
- `capabilities.<capability>.order` lists the providers for `web_search`, `web_fetch` and
  `docs_search`; a provider left out is never used. An empty `web_search` or `docs_search` order
  shows up as a capability gap.
- `platforms.<platform>.order` lists a platform's routes; empty turns the platform off. Opt-in
  routes are added by the user, as the entry says.

The step is done when `forager config list` loads and shows the value the failed command needed.

## Check providers

- `forager doctor`: configuration, permissions, state directories and whether every configured
  provider answers. Exit 4 when one does not; its entry says which.
- `forager doctor --offline`: the same without any request.
- `forager doctor --provider PROVIDER`: one provider's configuration, prerequisites and, where
  the provider offers a free account check, each key's state (SerpApi reports each key's
  searches left). It sends no billed request and opens no page.
- `forager doctor --health`: from local records alone, how each provider behaved in the user's
  real use over recent days, including failures a fallback hid. A `failing` or `degraded` finding
  names a provider to check with `--provider`. With `telemetry_enabled: false` no new records
  were kept, so an empty summary does not mean a healthy one. `schedules` describe the
  maintainer's scheduled runs: on any other machine they are `not_recorded`, which is expected.
- `forager doctor --live --provider PROVIDER` sends real requests to check that a provider's
  operations still work. Run it only when the user asks: it can spend quota, and naming a provider
  with attended checks, such as a browser route, is the user's consent to open pages of a
  logged-in site or risk the network address.

The step is done when the report names the cause and the command or user action that repairs it.

## Browser routes

`forager browser install` downloads and verifies the browser that opt-in routes use; a package
exists only for macOS on Apple silicon, and elsewhere `browser.path` names a local Chromium-based
browser. `forager browser login <platform>` opens a window where the user logs in; the login and
any verification there are the user's. `forager doctor` then reports whether the route can run.
