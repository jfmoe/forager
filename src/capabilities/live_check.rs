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

/// What the command's output must show for the check to pass.
#[derive(Clone, Copy, Debug)]
pub(crate) enum LiveContract {
    /// A non-empty `answer`, such as a main search or a platform ask.
    Answer,
    /// An answer with supplemental sources.
    PipelineSearch,
    /// Non-empty `results`.
    Results,
    /// Non-empty `content`, read with keys or, when `anonymous`, without any.
    Fetch { anonymous: bool },
    /// Non-empty `items`.
    PlatformItems,
    /// A `ref` and a `title` or `text`.
    PlatformItem,
    /// A `ref` and a body written to the content directory.
    PlatformFullText,
    /// A non-empty `comments` page.
    Comments,
    /// A Xiaohongshu note whose comments span a second page and include an expanded reply
    /// thread.
    XiaohongshuComments,
    /// A resolved event priced by its payouts without the CLOB: `winner` resolved Yes at 1.
    PolymarketResolved { winner: &'static str },
    /// An event priced live by the CLOB, unless none of its markets is open.
    PolymarketLive,
    /// A price history that ends in a settlement at 1, kept out of the series.
    PolymarketSettlement,
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
