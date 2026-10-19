//! The flow a delegated research route runs in the user's own logged-in Chrome: `status` checks
//! the sign-in once in a background window, `start` asks the question once in a foreground
//! window, and `result` reads a conversation in a background window under the shared retry
//! policy. Each site supplies its adapter's route, its conversation identity, its page facts and
//! their decoding, and its wording.

use std::cell::OnceCell;
use std::marker::PhantomData;

use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::catalog::ProviderId;
use crate::config::ProcessRouteRuntimeConfig;
use crate::net::{AttemptFailure, RetryPolicy};
use crate::providers::execution::{ExecutionOutcome, execute_anonymous};
use crate::providers::opencli::{CommandFailure, Window};
use crate::providers::opencli_route::{OpenCliRoute, single_attempt};
use crate::rate_limit::RateLimiter;
use crate::types::{AttemptErrorKind, AttemptTarget, Deadline, ProviderError};

const STATUS_OPERATION: &str = "status";

/// The site-specific parts of a delegated research route.
pub(crate) trait ResearchSite {
    const ROUTE: ProviderId;
    /// The attempt-target name of `start`.
    const START_OPERATION: &'static str;
    /// The attempt-target name of `result`.
    const RESULT_OPERATION: &'static str;
    /// The failure of a `status` whose page did not finish loading before the read deadline.
    const STATUS_TIMEOUT_MESSAGE: &'static str;
    /// Appended to a failure that leaves unknown whether the site received the question.
    const UNKNOWN_OUTCOME: &'static str;
    /// The failure of a `start` attempt that outlived its deadline without returning page facts.
    const START_TIMEOUT_MESSAGE: &'static str;

    type Conversation: ResearchConversation;
    /// The page facts of the adapter's `report` command.
    type Report: DeserializeOwned;
    type State;
    /// The page facts of the adapter's `start` command.
    type StartFacts: DeserializeOwned;
    type Plan;

    /// The failure of a page that asks the user to sign in.
    fn signed_out() -> AttemptFailure;

    /// Classifies the page facts of a `report` command and decodes where the research stands.
    fn read_research(
        report: &Self::Report,
        conversation: &Self::Conversation,
    ) -> Result<Self::State, AttemptFailure>;

    /// Classifies the page facts of a `start` command. `created` receives the conversation as
    /// soon as the facts name it, so a failure can still point to it.
    fn read_start(
        facts: &Self::StartFacts,
        created: &OnceCell<Self::Conversation>,
    ) -> Result<(Self::Conversation, Self::Plan), AttemptFailure>;
}

/// The identity of a site's conversation.
pub(crate) trait ResearchConversation: Clone {
    /// The id the adapter's `report` command takes.
    fn id(&self) -> &str;
    /// The conversation's page on the site.
    fn url(&self) -> String;
}

/// A failed `start`, with the conversation it concerns when forager knows it.
pub(crate) struct StartFailure<C> {
    pub(crate) error: ProviderError,
    pub(crate) conversation: Option<C>,
}

pub(crate) struct DelegatedResearch<S> {
    opencli: OpenCliRoute,
    retry_policy: RetryPolicy,
    site: PhantomData<S>,
}

impl<S: ResearchSite> DelegatedResearch<S> {
    pub(crate) fn new(
        config: ProcessRouteRuntimeConfig,
        limiter: RateLimiter,
        retry_policy: RetryPolicy,
        deadline: Deadline,
    ) -> Self {
        Self {
            opencli: OpenCliRoute::new(S::ROUTE, config, limiter, deadline),
            retry_policy,
            site: PhantomData,
        }
    }

    /// Opens the conversation in a background window and reads where its newest research
    /// stands. Reading changes nothing, so failed attempts follow the shared retry policy.
    pub(crate) async fn result(
        &self,
        conversation: &S::Conversation,
    ) -> Result<ExecutionOutcome<S::State>, ProviderError> {
        let command = self.opencli.command(
            "report",
            vec![("conversation", conversation.id().to_owned())],
            Window::Background,
        );
        let command = &command;
        execute_anonymous(
            self.opencli.settings(
                AttemptTarget::operation(S::RESULT_OPERATION),
                self.retry_policy,
            ),
            move |deadline| async move {
                let report: S::Report = self.opencli.read(command, deadline).await?;
                S::read_research(&report, conversation).map(|state| (None, state))
            },
        )
        .await
    }

    /// Asks `query` in a foreground window, where the sites' tool menus render.
    ///
    /// Starting creates a conversation and spends the account's Deep Research quota, so it runs
    /// exactly one attempt whatever the retry configuration (ADR 0023). A failure carries the
    /// conversation once forager knows it.
    pub(crate) async fn start(
        &self,
        query: &str,
    ) -> Result<ExecutionOutcome<(S::Conversation, S::Plan)>, StartFailure<S::Conversation>> {
        let command = self.opencli.command(
            "start",
            vec![("query", query.to_owned())],
            Window::Foreground,
        );
        let command = &command;
        let created = OnceCell::new();
        let created_ref = &created;
        let mut settings = self.opencli.settings(
            AttemptTarget::operation(S::START_OPERATION),
            single_attempt(),
        );
        settings.timeout_message = S::START_TIMEOUT_MESSAGE;
        let execution = execute_anonymous(settings, move |deadline| async move {
            let facts: S::StartFacts = self
                .opencli
                .read(command, deadline)
                .await
                .map_err(start_command_failed::<S>)?;
            S::read_start(&facts, created_ref).map(|started| (None, started))
        })
        .await;
        execution.map_err(|mut error| {
            let conversation = created.into_inner();
            if let Some(url) = conversation.as_ref().map(ResearchConversation::url)
                && !error.message.contains(url.as_str())
            {
                error.message = format!("{}; see {url}", error.message);
            }
            StartFailure {
                error,
                conversation,
            }
        })
    }

    /// Opens the site in a background window and checks that the browser is signed in. The
    /// adapter contract and OpenCLI itself are checked on the way.
    pub(crate) async fn status(&self) -> Result<(), ProviderError> {
        let command = self
            .opencli
            .command(STATUS_OPERATION, Vec::new(), Window::Background);
        let command = &command;
        execute_anonymous(
            self.opencli
                .settings(AttemptTarget::operation(STATUS_OPERATION), single_attempt()),
            move |deadline| async move {
                let status: StatusData = self.opencli.read(command, deadline).await?;
                if status.page.signed_out {
                    return Err(S::signed_out());
                }
                if status.timed_out {
                    return Err(AttemptFailure {
                        kind: AttemptErrorKind::Timeout,
                        status: None,
                        message: S::STATUS_TIMEOUT_MESSAGE.into(),
                    });
                }
                Ok((None, ()))
            },
        )
        .await
        .map(|_| ())
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct StatusData {
    page: StatusPage,
    timed_out: bool,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct StatusPage {
    /// Whether the page asks the user to sign in.
    signed_out: bool,
}

/// Words a failure of the `start` command itself, which leaves no page facts. A command OpenCLI
/// never ran sent nothing, and neither did a browser that asks to sign in; any other failure
/// may have stopped after the site received the question.
fn start_command_failed<S: ResearchSite>(failure: CommandFailure) -> AttemptFailure {
    let CommandFailure {
        attempt,
        may_have_run,
    } = failure;
    if may_have_run && attempt.kind == AttemptErrorKind::Auth {
        return attempt;
    }
    let hint = if may_have_run {
        S::UNKNOWN_OUTCOME
    } else {
        "nothing was sent"
    };
    AttemptFailure {
        message: format!("{}; {hint}", attempt.message),
        ..attempt
    }
}
