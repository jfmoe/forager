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

## Revisit when

- A site offers a sanctioned access path, which would move its route out of `attended`.
- A provider's terms are confirmed for the use a manual-only check makes, which would let that
  check drop the flag.
