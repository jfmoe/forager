# Run Live Checks in tiers, with consent by selection

## Decision

Whether forager's understanding of each upstream still holds is checked by Live Checks, which
`forager doctor --live` runs against the real services. Each check is declared with its provider's
registration and carries one Live Tier:

- **`free`**: needs no credential and carries no account or egress risk.
- **`metered`**: needs a credential or spends quota, billed or not.
- **`attended`**: puts an account or the egress address at risk, so it runs only with a person
  present.

**The tier rule.** A check's tier is the highest tier of any route it can reach while it runs. A
route outside every default order (an opt-in route, ADR-0020) makes the check `attended`; a route
that sends a credential or spends quota makes it at least `metered`; otherwise it is `free`. An
unbilled account check that needs a key is `metered`. A check pins the routes it reaches, so the
user's configured order can never lead it to a route of a higher tier: the precheck of a model
operation is pinned to a route that is not opt-in. Each check's tier is declared with the check and
shown by the listing; this record holds only the rule.

Two flags sit beside the tier. An **expensive** check spends enough quota in one run to matter on
its own. A **manual-only** check runs only by a person, at release and after a change to related
code, because whether its use of the service complies with the provider's terms is unverified.

**Consent by selection.** A run that selects no tier runs `free` and `metered`; it reaches
`attended` only when it names a provider. Explicitly selecting the `attended` tier, or naming an
attended check's provider, is the user's consent to it; there is no separate consent option.
Selecting a platform is not consent, so a platform selection alone runs none of its attended
checks. A non-attended route that its platform's order leaves out still runs, because the check
pins it. An attended check still needs its prerequisites, such as an installed Managed Browser and
a logged-in Browser Profile, and is reported as unconfigured without them.

**The unattended defence.** `doctor --live` has an option that marks a run as unattended, and a run
in an environment that indicates CI behaves as if it were given. Such a run refuses the `attended`
tier, selected or reached by naming an attended provider, with an argument error before any
request, and it excludes every expensive and manual-only check. This is defence in depth: the main
guard remains that no run includes `attended` without an explicit selection.

**Where and how often.** The maintainer's machine runs the `free` tier daily and the `metered` tier
weekly, from job files kept in the repository and installed by hand; forager installs nothing.
The `attended` tier runs only with a person: after a release, and for a provider after a change
to its adapter, which is a completion condition of adapter work. GitHub Actions runs no Live
Check. Its two scheduled workflows only build forager on each release target's runner and check
dependency advisories, and each holds nothing beyond its own workflow token. Each scheduled job
gives its run a longer time limit than the default, so its failures have room to be confirmed.

**Confirmation.** An unattended run observes a failed check a second time, after one delay that
all its failed checks share, and only a second failure is confirmed. An authentication or
parameter failure is confirmed without a rerun, and a browser check is never rerun. A failure the
run could not observe twice is unconfirmed: the run still exits as failed, but the failure opens,
updates and closes nothing and is no baseline for later runs. A run that is not unattended is
recorded but is never a baseline either. There is no counting of failures across runs.

**Notification.** A pure decision over the scheduled run records keeps one monitoring issue per
provider in the public repository: opened when the provider's set of confirmed failing checks
becomes non-empty, updated when its members or their error kinds change, closed when those checks
pass in a later scheduled run. The job carries it out under the maintainer's own `gh` identity and
posts a macOS notification for every open and update. The issue text holds only check
identifiers, error kinds and HTTP statuses.

**Health.** The health summary that the weekly job runs flags a provider as failing when too few of
its calls in the period succeed, whatever the period before said, besides flagging a degradation
against the period before; and it flags a tier or a scheduled workflow whose last run is overdue
for its interval.

## Why

- Of 37 recorded incidents where an external service did not behave as forager expected, the
  offline tests caught none; 9 were found by the live run at release time. Contact with the real
  service is the only layer that notices an upstream change, so it has to run often, and how often
  it may run depends on what each check puts at stake.
- Opt-in routes were never run live until late November 2026: a route blocked by its site went
  unnoticed for 24 days while its 34 fixture tests stayed green. A single opt-in switch for the
  whole run made running them an all-or-nothing choice; a tier per check lets the free and metered
  checks run without a person and keeps the risky ones with one.
- A tier assigned per provider would put a provider's keyless check and its keyed check in one
  class: the anonymous Web Fetch path, which every user without a key takes, would share the
  `metered` tier of its provider's keyed fetch. A tier per check keeps that path in `free`.
- The risk of a check comes from the routes it reaches, not from the operation it names: a model
  operation reads its video through the platform's fetch chain before the model answers, and with
  an opt-in route first in the user's order it would reach that route. Pinning the routes makes the
  declared tier true whatever the configuration says.
- A live run by hand after each release leaves an upstream change unnoticed for the whole gap
  between releases, which has reached 20 days. A daily `free` run bounds that
  gap at a day for every keyless path, including the anonymous Web Fetch path most users take.
- The `metered` tier needs keys and a private gateway address, which a public repository could
  hold only as secrets, and it spends quota. Running it weekly on the maintainer's machine keeps
  both private. The estimated weekly spend, before the accounts are checked with the maintainer
  (November 2026): SerpApi three searches a run, about 13 a month against a free plan of 250;
  YouTube Data API three units a run against a daily quota of 10,000; one pipeline search and a
  handful of minimal model calls through the gateway; a few credits each on Tavily, Firecrawl and
  Exa; one keyed jina fetch. A confirmation rerun can double a failing check's spend.
- GitHub notifies nobody of their own actions, so an issue opened under the maintainer's identity
  alone would go unseen; the macOS notification is the signal, and the issue is the public record.
  A bot account or a token in the repository would be one more secret to keep.
- A single network blip opening a public issue would train the maintainer to ignore them. A second,
  independent observation in the same job filters it without a cross-run counter, which would need
  its own state and delay a real failure by a whole interval. A rejected key or request gives the
  same answer however long the run waits, so rerunning it only spends quota; a browser check opens
  pages of a logged-in site, so it is never repeated. An unconfirmed failure still fails the run,
  because nothing showed the check healthy, but it reaches no issue.
- A fallback provider failed all of its attempts for 30 days and was found only by reading the
  journal by hand. Against the period before, a provider that has been failing all along shows no
  change, so the failing rule does not depend on the comparison.
- A scheduled job that has stopped running looks the same as a healthy one. GitHub disables
  scheduled workflows after 60 days without activity and may drop runs, and a local job skips a
  run while the machine is off, so the health summary reports an overdue schedule instead of
  trusting silence.
- A schedule written wrong is the likeliest way for an attended check to run without a person, and
  CI is the likeliest place for it. Refusing the tier there, and failing the run instead of skipping
  the check, makes the mistake visible.

## Rejected alternatives

- **One opt-in option for the whole run**, as `forager smoke` had before Live Checks replaced it.
  The option covered every opt-in route at once, the command reported an unconfigured provider as
  a failure, and no recorded full run of it ever exited 0, so its exit code carried no signal.
- **A separate consent option for attended checks.** Selecting the tier or naming the provider
  already says the same thing; a second option would be one more thing to get out of step.
- **Treating a platform selection as consent.** A platform has routes of different tiers, and
  selecting one to check its keyed route must not open a logged-in site.
- **A table of checks and their tiers in this record.** It would go stale with every new check; the
  listing shows the declarations, which are the only copy.
- **Skipping the attended tier silently in an unattended run.** A wrong schedule would look healthy
  while it tried to run checks it must not.
- **Running the `free` tier on GitHub Actions.** It needs no key, but while forager has one
  maintainer the daily local run already covers it, and every Live Check kept off GitHub keeps one
  rule simple enough for a test to hold: no workflow runs a Live Check.
- **Counting failures across runs before opening an issue.** It needs state of its own and delays a
  real failure by a whole interval; the confirmation rerun gives a second observation within the
  job.
- **A notification on every failed run.** It repeats the same news daily; one issue per provider
  that changes only with its failing set tells the maintainer about changes.

## Revisit when

- forager has users beyond its maintainer, which would make the `free` tier on GitHub Actions
  worth running there.
- The metered accounts' real figures, checked with the maintainer, differ from the estimates above
  enough to change how often the tier can run.
- A site offers a sanctioned access path, which would move its route out of `attended`.
- A provider's terms are confirmed for the use a manual-only check makes, which would let that
  check drop the flag.
