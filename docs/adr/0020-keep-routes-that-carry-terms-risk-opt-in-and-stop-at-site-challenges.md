# Keep routes that carry terms risk opt-in and stop at site challenges

## Decision

A Platform Route whose way of reading a site goes against that site's terms, or puts the user's
account or egress IP at risk, is **opt-in**: it is a valid entry of its platform's order but never
part of any default order, and no release adds it to one. It runs only after the user adds it.
Every browser route is opt-in; an HTTP route is opt-in when its access carries such a risk (today
`youtube_web`, which reads undocumented endpoints).

- **One registration of the risk.** Every opt-in route, HTTP or browser, registers one sentence in
  the platform catalog saying what enabling it risks and where the risk falls. The enable
  instructions that configuration errors print read that one place, with no branch on a route ID.
- **Unused routes stay idle.** Doctor checks an opt-in route only when an order enables it, so a
  route the user never chose never runs and never affects the health result.
- **Stop at a challenge.** A route never solves or bypasses a site's challenge, verification page
  or block. It fails as Auth, does not retry, and tells the user how to deal with it themselves; a
  blocked result is never reported as a legitimate empty one.

## Why

- Several sites forager reads forbid automated access in their terms: Xiaohongshu forbids
  crawlers, Reddit's User Agreement does not allow automated access, and YouTube's terms restrict
  automated access to its web endpoints. The maintainer accepts this risk only for personal use,
  at a pace close to a person's, and only when the user has chosen it knowing where the risk
  falls.
- The risk falls in different places: on the egress IP for an anonymous route, on the logged-in
  account for a browser route. The user can only weigh it if the instructions name it, and one
  registration keeps the instructions for HTTP and browser routes from drifting apart.
- Retrying after a challenge, or solving it, turns a temporary block into a longer one or into a
  lost account; stopping keeps the decision with the person who owns the account.

## Rejected alternatives

- **On by default with a warning.** Most users would carry a terms risk they never chose.
- **A separate enabled switch per route.** Adding the route to the order already expresses the
  choice; a second switch would be one more place to get out of step.
- **Risk text written per route in the error paths.** Each new opt-in route would need its own
  branch, and HTTP and browser routes would drift.
- **Solving or retrying past a challenge.** It is circumvention, and repeated requests extend the
  block.

## Revisit when

- A site offers a sanctioned access path for personal tools, which would make its route a
  candidate for a default order.
- An account or egress IP used through an opt-in route is warned, limited or banned, which would
  call the pace or the route itself into question.
