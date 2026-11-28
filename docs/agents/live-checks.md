# Running Live Checks and their schedules

Live Checks are the only layer that reaches the real services. This procedure covers where each Live Tier runs, how the maintainer installs the scheduled jobs, how to read the issues they keep, and what an agent runs after changing an adapter, adding a check or fixing an upstream change. The product skill defines the command, its statuses and its exit codes; ADR-0039 holds the reasons behind the tiers and the schedules.

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
3. Copy both plists into `~/Library/LaunchAgents/`, replace `USERNAME` with the account name, and load each with `launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/<file>`.
4. Start each job once with `launchctl kickstart gui/$(id -u)/<label>` and read its log in `~/Library/Logs/forager/`. That directory also keeps each tier's last report (`live-free.json`, `live-metered.json`) and the last health summary.

After a change to these files, copy them again; for a plist, `launchctl bootout` it before bootstrapping it again. launchd starts a job that a sleeping machine missed when the machine wakes, and skips it while the machine is off. Either way the health summary flags a tier or workflow whose last run is overdue.

The weekly job starts on Monday evening, local time, after the GitHub scheduled workflows have run early on Monday (UTC), so the record it writes holds this week's runs. A record written before them would hold last week's, and the health summary would report both workflows overdue within days. Keep that order when moving either schedule.

The jobs give each run a longer `--timeout` than the default: a failed check is confirmed only by a rerun after a delay, and only while the run has room for the check's own time limit, so a shorter run leaves more failures unconfirmed.

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

Save the report in the task's working directory under `.local/` and cite the status of each check when you report the work. A provider with checks in the `attended` tier opens pages of a logged-in site or risks the egress address, and naming it selects those checks, so run it only with the user present and agreeing, as [`browser-routes.md`](browser-routes.md) says. Without the user, run `--tier free,metered` and report the attended checks as not run. An `unconfigured` check proves nothing about its upstream; report it with its `message`. Give the tool that runs the command a timeout longer than the run's own.

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
