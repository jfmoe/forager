# Running Live Checks and their schedules

Live Checks are the only layer that reaches the real services. This procedure covers where each Live Tier runs, how the maintainer installs the scheduled jobs, how to read the issues they keep, and what an agent runs after changing an adapter, adding a check or fixing an upstream change. It also defines what a live run selects, reports and records; `forager doctor --help` gives the options, and ADR-0039 holds the reasons behind the tiers and the schedules.

## Where each tier runs

| Tier | Runs | Started by |
| --- | --- | --- |
| `free` | Daily, on the maintainer's machine | The daily job |
| `metered` | Weekly, on the maintainer's machine | The weekly job |
| `attended` | Only with the user present: after a release, as [`release.md`](release.md) says, and after a change to an attended route | A person |

GitHub runs no Live Check. Its two scheduled workflows, `scheduled-build.yml` and `scheduled-advisories.yml`, build forager on each release runner and check dependency advisories, and each keeps its own issue through the workflow token; a run cancelled by hand reports nothing. Every scheduled job passes `--unattended`, which refuses the `attended` tier and leaves out expensive and manual-only checks. A rule test checks that every `doctor --live` command in the job files passes it, that no job file names the `attended` tier or a provider with attended checks, and that each job's interval is its tier's.

## Installing the scheduled jobs

The job files are in `scripts/schedules/`; forager installs nothing. The jobs run under launchd in the maintainer's login session, so `gh` reads its token from the keychain and opens issues under the maintainer's own identity. GitHub does not notify a user of their own actions, so the jobs post a macOS notification as well.

| File | What it does |
| --- | --- |
| `forager-live-daily` | Runs the `free` tier unattended and hands the report to `forager-live-issues` |
| `forager-live-weekly` | Runs the `metered` tier the same way, then writes `github_workflows.json`, the record the health summary reads, from `gh`, then runs `doctor --health` and notifies when it exits with anything but 0 |
| `forager-live-issues` | Carries out the report's `notifications` with `gh`: finds the issue by label, opens it or comments on it when one is open, closes it, and notifies every open and update, every failed `gh` command, every run that exited with neither 0 nor 4, and every run that left a failure unconfirmed or a check unstarted at its time limit |
| `com.github.jfmoe.forager.live-daily.plist`, `com.github.jfmoe.forager.live-weekly.plist` | The launchd jobs that start the two scripts and give them a `PATH` |

Each script first checks that the programs it needs are on that `PATH`, and notifies the one it misses instead of running.

1. Make sure `forager`, `gh` and `jq` are installed, `gh auth status` shows the maintainer's account, and the keys of the `metered` providers are configured.
2. Copy the three scripts into `~/.local/bin`: `mkdir -p ~/.local/bin && install -m 755 scripts/schedules/forager-live-* ~/.local/bin/`. Copy them rather than linking, so a checked-out branch never changes what a job runs.
3. Create the log directory with `mkdir -p ~/Library/Logs/forager`. launchd opens the log file each plist names before the script starts, so the scripts' own `mkdir` comes too late for it.
4. Copy both plists into `~/Library/LaunchAgents/`, replace `USERNAME` with the account name, and load each with `launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/<file>`.
5. Start each job once with `launchctl kickstart gui/$(id -u)/<label>` and read its log in `~/Library/Logs/forager/`. That directory also keeps each tier's last report (`live-free.json`, `live-metered.json`) and the last health summary.

After a change to these files, copy them again; for a plist, `launchctl bootout` it before bootstrapping it again. launchd starts a job that a sleeping machine missed when the machine wakes, and skips it while the machine is off. Either way the health summary flags a tier or workflow whose last run is overdue.

The weekly job starts on Monday evening, local time, after the GitHub scheduled workflows have run early on Monday (UTC), so the record it writes holds this week's runs. A record written before them would hold last week's, and the health summary would report both workflows overdue within days. Keep that order when moving either schedule.

The jobs give each run a longer `--timeout` than the default: a failed check is confirmed only by a rerun after a delay, and only while the run has room for the check's own time limit, so a shorter run leaves more failures unconfirmed.

## What a live run selects

Each check has an `id` of the form `<provider>:<operation>`, with `:<variant>` for a second check of one operation; the pipeline check, `pipeline:search`, belongs to no provider. A check runs its route even when the platform's order leaves the route out.

| Tier | A check in it |
| --- | --- |
| `free` | needs no credential and carries no account or egress risk |
| `metered` | needs a credential or spends quota, billed or not |
| `attended` | puts an account or the egress address at risk, such as a browser route on a logged-in site |

- Without `--tier`, a run selects `free` and `metered`; naming a provider with `--provider` selects all three of its tiers. Selecting `attended`, or naming a provider that has attended checks, is the user's consent to run them; `--platform` alone is not consent and runs none of the platform's attended checks.
- `--provider` and `--platform` narrow the selection, and a check that reads another check's output (`depends_on`) brings that check along.
- `--unattended`, or a `CI` variable set to anything but empty or `false`, refuses the `attended` tier with exit 2 before any request and leaves out every `expensive` check (one that spends a notable share of a quota) and every `manual_only` check (one whose use of the service is not confirmed to comply with the provider's terms).
- `--list` prints each check with its tier, `depends_on`, time limit, `expensive`, `manual_only`, `hard_sample` (why its sample is hard for the route) and `selected`, and runs nothing.

## Reading a live run

A check's `status` is one of:

- `passed`: the command succeeded and its output met the check's contract.
- `failed`: the command failed, with its own `error_kind` and `message`, or its output broke the contract, with `error_kind: "contract"` and a `detail` that gives the `rule`, the `path` as the contract names it, `at` (the item or the object it could not step into) and `observed`, which holds only the type, length or count of what was there, never a value.
- `unconfigured`: a prerequisite this machine lacks (the Managed Browser, a logged-in Browser Profile, keys); its `message` names it and how to meet it. Neither a pass nor a failure.
- `not_started`, with `cause`:
  - `deadline`: the run's `--timeout` ended first; it counts as a failure.
  - `site_stop`: an earlier check on the same site met a stop signal, so the run opened no more pages there; [`browser-routes.md`](browser-routes.md) gives the signals and what to do.
  - `no_input`: the check it depends on did not pass or left nothing to read; a check whose dependency passed only on its rerun stays `no_input` in that run.

Each check runs forager as its own process, without that command's retries and without writing the Search Result Journal or Invocation Telemetry. A check on an HTTP route that fails with `network` or `timeout` runs once more at once. In an unattended run, every HTTP check still failing with anything but `auth` or `parameter` runs again after a shared one-minute delay, and its second result stands, with `rerun: true`. An unattended failure is `confirmed: true` when both runs failed or the first failure was final (`auth`, `parameter`, or any browser route's failure, which is never rerun), and `confirmed: false` when the run's time left no room for the rerun. A run that is not unattended reruns nothing and reports no `confirmed`.

A live run exits 0 when at least one check ran and every check that ran passed; 4 when a check failed, confirmed or not, or the time limit left one unstarted; 3 when none of the selected checks ran or the configuration cannot be loaded; 2 for the refusal of `attended` above.

## What a live run records

Every live run writes one private record in `live_runs/` under forager's state directory, kept 90 days: its time, version, tiers, whether it was `scheduled` (exactly when it was unattended), the summary, every declared check, and per check its outcome fields without any `message`, so no key, query, URL or content reaches it.

- **Transitions**: a check that reached a result (passed, or failed without `confirmed: false`) reports `transition` against the latest scheduled record in which it reached one: `newly_failed`, `still_failing` with `failing_since`, or `recovered`. An unconfirmed failure is never a baseline, and a run by hand reports transitions but never serves as one.
- **Notifications**: an unattended run's `notifications` lists, per provider whose failing set it changes, an `action` on the issue labelled `live-check:<provider>`: `open` when the set becomes non-empty, `update` when its checks or their error kinds change, `close` when every check in it passed or is no longer declared. The `body` lists each failing check with its error kind and HTTP status and the command that reruns it, and nothing else, because the issue is public. An error kind forager does not name shows as `unrecognised`.
- **Workflow record**: the weekly job writes `github_workflows.json` in the state directory from `gh`, listing each scheduled GitHub workflow with `workflow`, a positive whole `interval_days` and `last_success` (an RFC 3339 time, or `null` when it never succeeded). `doctor --health` reads only the workflows it lists, so it lists every scheduled one.

## Monitoring issues

Each monitoring issue carries one label, and the job finds the issue by that label:

- `live-check:<provider>`: one per provider, kept by the local jobs. The pipeline check's issue is `live-check:pipeline`.
- `workflow:scheduled-build` and `workflow:scheduled-advisories`: kept by the two GitHub workflows.

None is a triage label ([`triage-labels.md`](triage-labels.md)), so triage and the agent skills never take a monitoring issue as a ticket. Leave triage labels off these issues.

Reading a `live-check:` issue:

- The body lists each failing check of the provider with its error kind and HTTP status, and nothing else, because the repository is public. Only a confirmed failure reaches it: both observations failed, or the one observation was an `auth` or `parameter` failure.
- A comment means the set of failing checks or their error kinds changed, and lists the new set. Closing means every check in it passed in a later scheduled run or is no longer declared. A check that is unconfigured, unselected or not started changes nothing, and a run by hand never opens, updates or closes an issue.
- For the detail (the failed rule, the path, what was observed and the command's own message), read the tier's last report, or run the command the issue names: `forager doctor --live --provider <provider> --tier free,metered`, whose tier selection keeps a person's rerun out of the provider's attended checks. No provider declares the pipeline check, so its issue names `forager doctor --live --tier metered`, which runs every `metered` check.
- A failure left unconfirmed and a check the time limit left unstarted reach no issue. The job posts a macOS notification for a run that has either, and the report shows them with `confirmed: false` and `cause: "deadline"`.
- A decision whose `gh` command failed is not tried again: the next run compares against records that already hold this run. The notification names the issue and the action; carry it out by hand from the tier's last report.

Reading a `workflow:` issue: it links the failed run. A build failure is a retired runner image or a target that no longer builds; an advisory failure names the advisory and the crate.

## After changing an adapter

Adapter work is done only when the changed provider's Live Checks pass:

```sh
forager doctor --live --provider <provider>
```

Run only the changed provider's checks, by naming it, and save the report in the task's working directory under `.local/`; cite the status of each check when you report the work. Two kinds of check are started only by the user, present and agreeing: an `attended` check opens pages of a logged-in site or risks the egress address, as [`browser-routes.md`](browser-routes.md) says, and a `manual_only` check uses a service in a way not confirmed to comply with the provider's terms. `forager doctor --live --list --provider <provider>` shows both. Without the user, add `--tier free,metered` to leave out the attended checks; a provider with a manual-only check is not run at all, because no selection by hand leaves that check out. Report every check you did not run as not run. Do not run every provider's checks, such as `--tier free,metered` without `--provider`, because that includes other providers' manual-only checks. An `unconfigured` check proves nothing about its upstream; report it with its `message`. Give the tool that runs the command a timeout longer than the run's own.

## Adding a Live Check

A Live Check is declared with its provider's registration in the provider registry, beside the provider's other declarations; only the pipeline check lives in the Live Check runner.

1. Name it `<provider>:<operation>`, adding `:<variant>` when the provider has a second check of that operation.
2. Give it the tier that ADR-0039's tier rule assigns from the routes it reaches, and pin those routes in its command, so the user's configured order cannot lead it elsewhere.
3. Write a contract that names every field the route decodes from the upstream response, with its type and a lower bound, never an exact value.
4. Choose a sample that is hard for the route, such as a reply rather than a top-level post, and say in the declaration why it is hard.
5. Declare a dependency only on an earlier check of the same provider and tier.
6. Mark the check expensive when one run spends a notable share of a quota, and manual-only when its use of the service is not confirmed to comply with the provider's terms.
7. Run the in-crate tests: the registry's validation names the rule a declaration breaks. A test you add follows [`testing.md`](testing.md), so it waits on no real time.
8. Confirm with `forager doctor --live --list --provider <provider>` that the check is listed with its tier, then run it as [After changing an adapter](#after-changing-an-adapter) says.

## When an upstream changes shape

A contract failure gives the rule and the path that broke. Fix it where forager reads the upstream:

1. Capture the current response with a request a user of the route could make. Pass the key through an environment variable, so it reaches neither a file nor the shell history.
2. Store it by [Adding or refreshing a sample](testing.md#adding-or-refreshing-a-sample). Its steps replace every key, token, account identifier and private person's content before the sample is committed, and forbid committing a logged-in page as captured.
3. Correct the decoding against the new sample, and run the provider's offline tests and then its Live Checks.
