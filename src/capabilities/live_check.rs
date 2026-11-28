//! The Live Check declaration: what one check runs against the real service, in which Live
//! Tier, and what its output must show. Declarations are static data, so the catalog depends on
//! neither configuration nor adapters; `doctor --live` runs them.

use std::time::Duration;

use super::ProviderId;
use crate::types::Platform;

/// How much a Live Check puts at stake each time it runs.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) enum LiveTier {
    /// Needs no credential and carries no account or egress risk.
    Free,
    /// Needs a credential or spends quota, billed or not.
    Metered,
    /// Puts an account or the egress address at risk, so it runs only with a person present.
    Attended,
}

impl LiveTier {
    pub(crate) const ALL: [Self; 3] = [Self::Free, Self::Metered, Self::Attended];

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Free => "free",
            Self::Metered => "metered",
            Self::Attended => "attended",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tier| tier.as_str() == value)
    }

    /// How many days apart the maintainer's schedule runs the tier; `None` for a tier that no
    /// schedule may run.
    pub(crate) const fn schedule_days(self) -> Option<u32> {
        match self {
            Self::Free => Some(1),
            Self::Metered => Some(7),
            Self::Attended => None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct LiveCheck {
    /// `<provider>:<operation>[:<variant>]`.
    pub(crate) id: &'static str,
    pub(crate) tier: LiveTier,
    /// A check of the same provider and tier, declared earlier, whose output this check reads.
    pub(crate) depends_on: Option<&'static str>,
    /// How long the check's command may run before it is killed.
    pub(crate) time_limit: Duration,
    /// Spends enough quota in one run that no unattended run includes it.
    pub(crate) expensive: bool,
    /// Runs only by a person, at release and after a change to related code.
    pub(crate) manual_only: bool,
    pub(crate) command: LiveCommand,
    pub(crate) contract: LiveContract,
    /// Why the sample is hard for the route: what an easy sample would let a broken read pass.
    pub(crate) hard_sample: &'static str,
}

/// The time limit of a check whose command makes a few plain HTTP requests.
pub(crate) const HTTP_LIMIT: Duration = Duration::from_mins(1);
/// The time limit of a check that waits for a model to answer.
pub(crate) const MODEL_LIMIT: Duration = Duration::from_mins(3);
/// The time limit of a check that drives the Managed Browser, launch and pacing included.
pub(crate) const BROWSER_LIMIT: Duration = Duration::from_secs(150);

impl LiveCheck {
    pub(crate) const fn new(
        id: &'static str,
        tier: LiveTier,
        command: LiveCommand,
        contract: LiveContract,
        hard_sample: &'static str,
    ) -> Self {
        Self {
            id,
            tier,
            depends_on: None,
            time_limit: HTTP_LIMIT,
            expensive: false,
            manual_only: false,
            command,
            contract,
            hard_sample,
        }
    }

    pub(crate) const fn depends_on(self, id: &'static str) -> Self {
        Self {
            depends_on: Some(id),
            ..self
        }
    }

    pub(crate) const fn time_limit(self, time_limit: Duration) -> Self {
        Self { time_limit, ..self }
    }

    pub(crate) const fn expensive(self) -> Self {
        Self {
            expensive: true,
            ..self
        }
    }

    pub(crate) const fn manual_only(self) -> Self {
        Self {
            manual_only: true,
            ..self
        }
    }

    /// The operation the identifier names.
    pub(crate) fn operation(&self) -> &'static str {
        self.id.split(':').nth(1).unwrap_or_default()
    }

    /// The platform whose operation the check runs, when it runs one.
    pub(crate) const fn platform(&self) -> Option<Platform> {
        match self.command.routes {
            RoutePin::Platform(platform, _) => Some(platform),
            _ => None,
        }
    }
}

/// One forager command and the routes it is held to.
#[derive(Clone, Copy, Debug)]
pub(crate) struct LiveCommand {
    pub(crate) arguments: &'static [LiveArgument],
    pub(crate) routes: RoutePin,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum LiveArgument {
    Text(&'static str),
    /// The check's time limit in whole seconds.
    Timeout,
    /// A temporary directory that the command writes into and the run removes.
    ContentDir,
    /// What the check reads from the output of the check it depends on.
    Input(LiveInput),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LiveInput {
    /// A string field of the first item.
    FirstItem(&'static str),
    /// The access URL of the item with the most comments, the first on a tie.
    MostCommentedAccessUrl,
    /// The ref of the first item marked as not NSFW; an item without the flag is never chosen.
    FirstSafeRef,
}

/// Which routes the command may reach, so the check proves one route whatever the user's
/// configuration orders.
#[derive(Clone, Copy, Debug)]
pub(crate) enum RoutePin {
    /// The platform's order lists only these routes, the checked one last; a model operation
    /// also lists the route that reads its item first.
    Platform(Platform, &'static [ProviderId]),
    /// Web Fetch runs only this provider, with its keys.
    WebFetch(ProviderId),
    /// Web Fetch runs only this provider, with its keys removed.
    AnonymousWebFetch(ProviderId),
    /// Main search runs only this backend, without fallback.
    MainSearch(ProviderId),
    /// The command names its provider itself, or has only one.
    Configured,
}

impl RoutePin {
    /// Every route the command can reach.
    pub(crate) const fn routes(&self) -> &[ProviderId] {
        match self {
            Self::Platform(_, routes) => routes,
            Self::WebFetch(provider)
            | Self::AnonymousWebFetch(provider)
            | Self::MainSearch(provider) => std::slice::from_ref(provider),
            Self::Configured => &[],
        }
    }
}

/// What the command's output must show for the check to pass: every field the route decodes from
/// the upstream response, and what the check's sample must show beyond them.
#[derive(Clone, Copy, Debug)]
pub(crate) struct LiveContract {
    /// In the order they are judged; a list comes before the fields of its items.
    pub(crate) fields: &'static [Field],
    pub(crate) sample: &'static [SampleRule],
}

impl LiveContract {
    pub(crate) const fn fields(fields: &'static [Field]) -> Self {
        Self {
            fields,
            sample: &[],
        }
    }

    pub(crate) const fn with_sample(self, sample: &'static [SampleRule]) -> Self {
        Self { sample, ..self }
    }
}

/// One field of the output: where it is, the shape its value must have and, inside a list,
/// whether every item or only some must have it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Field {
    /// Dot-separated keys from the top of the output; `name[]` steps into each item of a list.
    pub(crate) path: &'static str,
    pub(crate) shape: Shape,
    /// Only some items carry the field, because the upstream leaves it out of others; at least
    /// one must.
    pub(crate) in_some_items: bool,
}

impl Field {
    const fn new(path: &'static str, shape: Shape) -> Self {
        Self {
            path,
            shape,
            in_some_items: false,
        }
    }

    /// A string with a character other than whitespace.
    pub(crate) const fn text(path: &'static str) -> Self {
        Self::new(path, Shape::Text)
    }

    /// A whole number of at least `min`.
    pub(crate) const fn count(path: &'static str, min: u64) -> Self {
        Self::new(path, Shape::Count { min })
    }

    /// A number of at least `min`.
    pub(crate) const fn number(path: &'static str, min: f64) -> Self {
        Self::new(path, Shape::Number { min })
    }

    pub(crate) const fn boolean(path: &'static str) -> Self {
        Self::new(path, Shape::Bool)
    }

    /// A list of at least `min` items.
    pub(crate) const fn list(path: &'static str, min: usize) -> Self {
        Self::new(path, Shape::List { min })
    }

    pub(crate) const fn in_some_items(self) -> Self {
        Self {
            in_some_items: true,
            ..self
        }
    }
}

/// The type and lower bound a field's value must meet; a contract never names an exact value.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Shape {
    Text,
    Count { min: u64 },
    Number { min: f64 },
    Bool,
    List { min: usize },
}

/// What a check's sample was chosen to show, beyond the fields its route decodes.
#[derive(Clone, Copy, Debug)]
pub(crate) enum SampleRule {
    /// Every provider attempt ran with keys or, when `anonymous`, without any: a read after the
    /// keys failed proves the Anonymous Fallback, not the keys.
    Credentials { anonymous: bool },
    /// The market `winner` of a resolved event is priced by its payout, so the fetch reads it in
    /// one attempt without the CLOB.
    ResolvedByPayout { winner: &'static str },
    /// An event with an open market is priced by live CLOB quotes, not Gamma's cached prices.
    LiveQuotes,
    /// A comment carries one reply until its thread is expanded, so when any comment declares
    /// two or more replies, some comment carries two or more. A note without such a comment has
    /// no thread to expand.
    RepliesExpanded,
}

/// A result on a browser route that ends every remaining check on its site in the run, because
/// more page loads would push the account further into the site's risk control.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StopSignal {
    /// A login wall, a verification page or a refused page.
    Auth,
    /// A block page or a security restriction.
    Parameter,
    /// A page or an answer the route cannot classify, such as a captcha.
    Runtime,
    /// A second timeout right after another.
    TwoTimeoutsInARow,
}

impl StopSignal {
    pub(crate) const fn describe(self) -> &'static str {
        match self {
            Self::Auth => "an authentication failure",
            Self::Parameter => "a parameter failure",
            Self::Runtime => "a page or an answer the route cannot classify",
            Self::TwoTimeoutsInARow => "a second timeout in a row",
        }
    }
}
