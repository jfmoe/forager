//! The one orchestration entry of every Platform Operation that one route serves.
//!
//! Planning follows every platform operation: the configured order ∩ the operation's route set
//! in the platform catalog ∩ configured routes, minus a route that cannot run the request. It
//! hands the operation the configuration of the route the operation's declaration names, read
//! from that route's own field, so the route's inherent method never receives another route's
//! configuration; a route that is absent from the order or unconfigured is a configuration
//! error. The route chain runs through the shared chain executor. The attempts of a successful
//! operation without `--verbose` are dropped here, once for every operation.
//!
//! The no-wildcard rule covers every match in this module, so a new route of a paged operation
//! or a new operation outcome is a compile error at every site here.
#![deny(clippy::wildcard_enum_match_arm)]

use reqwest::Client;

use crate::catalog::{self, ProviderId};
use crate::chain::{
    self, BudgetPolicy, ChainOutcome, ChainSettings, ChainStep, DiagnosticMerge, StepIdentity,
    StepSuccess, StepVerdict, TerminalPolicy,
};
use crate::config::{
    BrowserRouteRuntimeConfig, HnRouteRuntimeConfig, KeyedHttpRouteRuntimeConfig,
    ModelRouteRuntimeConfig, PlatformRoutesRuntimeConfig, PlatformRuntimeConfig,
    PolymarketRouteRuntimeConfig, XaiRuntimeConfig,
};
use crate::net::{RetryPolicy, combine_diagnostics};
use crate::page_cursor::{self, PagedRequest};
use crate::platform_chain::{
    PlatformPreflightError, operation_target, plan_routes, route_candidates,
};
use crate::platform_precheck::{
    PrecheckDepth, PrecheckPlan, Prechecked, plan_precheck, run_precheck,
};
use crate::providers::{self, OperationRoute, RouteContext};
use crate::types::{
    ASK, AskedVideo, CITED_BY, COMMENTS, ContentDepth, Deadline, HISTORY, HnStoriesOutcome,
    HnStoriesRequest, HnStoriesResult, ModelAnswerOutcome, ModelTranscriptOutcome, Platform,
    PlatformAskRequest, PlatformAskResult, PlatformCollection, PlatformCommentsPage,
    PlatformCommentsRequest, PlatformContent, PlatformFetchResult, PlatformItem, PlatformItemData,
    PlatformRef, PlatformSearchOutcome, PlatformSearchPage, PlatformTranscribeResult,
    PolymarketHistoryOutcome, PolymarketHistoryRequest, PolymarketHistoryResult, ProviderAttempt,
    ProviderError, STORIES, ScholarCitedByRequest, TRANSCRIBE, VIDEOS, XAskOutcome, XAskRequest,
    XAskResult, XiaohongshuCommentsOutcome, XiaohongshuCommentsPage, XiaohongshuCommentsRequest,
    YoutubeAskRequest, YoutubeAskResult, YoutubeCollection, YoutubeCommentsOutcome,
    YoutubeCommentsPage, YoutubeCommentsRequest, YoutubeTranscribeRequest, YoutubeVideosOutcome,
    YoutubeVideosRequest,
};

/// A Platform Operation of one platform that one route serves: its name in the platform
/// catalog, and the declaration of the route that answers it.
struct Operation<C: 'static, Q: 'static, I: 'static, T: 'static> {
    platform: Platform,
    name: &'static str,
    exhausted_message: &'static str,
    route: &'static OperationRoute<C, Q, I, T>,
}

const SCHOLAR_CITED_BY: Operation<
    KeyedHttpRouteRuntimeConfig,
    ScholarCitedByRequest,
    ScholarCitedByRequest,
    PlatformSearchOutcome,
> = Operation {
    platform: Platform::Scholar,
    name: CITED_BY,
    exhausted_message: "platform cited-by deadline elapsed",
    route: &providers::SCHOLAR_CITED_BY,
};

const XIAOHONGSHU_COMMENTS: Operation<
    BrowserRouteRuntimeConfig,
    XiaohongshuCommentsRequest,
    XiaohongshuCommentsRequest,
    XiaohongshuCommentsOutcome,
> = Operation {
    platform: Platform::Xiaohongshu,
    name: COMMENTS,
    exhausted_message: "platform comments deadline elapsed",
    route: &providers::XIAOHONGSHU_COMMENTS,
};

const X_ASK: Operation<XaiRuntimeConfig, XAskRequest, XAskRequest, XAskOutcome> = Operation {
    platform: Platform::X,
    name: ASK,
    exhausted_message: "platform ask deadline elapsed",
    route: &providers::X_ASK,
};

const HN_STORIES: Operation<
    HnRouteRuntimeConfig,
    HnStoriesRequest,
    HnStoriesRequest,
    HnStoriesOutcome,
> = Operation {
    platform: Platform::Hn,
    name: STORIES,
    exhausted_message: "platform stories deadline elapsed",
    route: &providers::HN_STORIES,
};

const POLYMARKET_HISTORY: Operation<
    PolymarketRouteRuntimeConfig,
    PolymarketHistoryRequest,
    PolymarketHistoryRequest,
    PolymarketHistoryOutcome,
> = Operation {
    platform: Platform::Polymarket,
    name: HISTORY,
    exhausted_message: "platform history deadline elapsed",
    route: &providers::POLYMARKET_HISTORY,
};

const YOUTUBE_COMMENTS: Operation<
    KeyedHttpRouteRuntimeConfig,
    YoutubeCommentsRequest,
    YoutubeCommentsRequest,
    YoutubeCommentsOutcome,
> = Operation {
    platform: Platform::Youtube,
    name: COMMENTS,
    exhausted_message: "platform comments deadline elapsed",
    route: &providers::YOUTUBE_COMMENTS,
};

const YOUTUBE_VIDEOS: Operation<
    KeyedHttpRouteRuntimeConfig,
    YoutubeVideosRequest,
    YoutubeVideosRequest,
    YoutubeVideosOutcome,
> = Operation {
    platform: Platform::Youtube,
    name: VIDEOS,
    exhausted_message: "platform videos deadline elapsed",
    route: &providers::YOUTUBE_VIDEOS,
};

const YOUTUBE_TRANSCRIBE: Operation<
    ModelRouteRuntimeConfig,
    YoutubeTranscribeRequest,
    PlatformItem,
    ModelTranscriptOutcome,
> = Operation {
    platform: Platform::Youtube,
    name: TRANSCRIBE,
    exhausted_message: "platform transcribe deadline elapsed",
    route: &providers::YOUTUBE_TRANSCRIBE,
};

const YOUTUBE_ASK: Operation<
    ModelRouteRuntimeConfig,
    YoutubeAskRequest,
    (PlatformItem, String),
    ModelAnswerOutcome,
> = Operation {
    platform: Platform::Youtube,
    name: ASK,
    exhausted_message: "platform ask deadline elapsed",
    route: &providers::YOUTUBE_ASK,
};

/// What planning hands an operation: the configuration of its route, when the order lists the
/// route and it can run the request, and the routes skipped before it runs.
struct OperationPlan<C: 'static, Q: 'static, I: 'static, T: 'static> {
    operation: &'static Operation<C, Q, I, T>,
    configs: Vec<C>,
    skipped: Vec<ProviderAttempt>,
}

/// Plans `operation` over the order of `config`, the operation's platform; a `pinned` route, from
/// a cursor, is the only route that may run.
fn plan<C: Clone, Q, I, T>(
    operation: &'static Operation<C, Q, I, T>,
    config: &PlatformRuntimeConfig,
    routes: &PlatformRoutesRuntimeConfig,
    request: &Q,
    pinned: Option<ProviderId>,
) -> Result<OperationPlan<C, Q, I, T>, PlatformPreflightError> {
    let route = operation.route;
    let plan = plan_routes(
        operation.name,
        operation.platform,
        &config.order_key(),
        catalog::platform(operation.platform).operation_routes(operation.name),
        &route_candidates(config),
        pinned,
        |id| (id == route.route).then(|| providers::operation_support(route, request)),
    )?;
    Ok(OperationPlan {
        operation,
        configs: plan
            .routes
            .iter()
            .map(|_| (route.config)(routes).clone())
            .collect(),
        skipped: plan.skipped,
    })
}

/// What every route of an operation runs with.
struct OperationRun {
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
    verbose: bool,
}

/// The answer of an operation's route, with the attempts the command reports and the diagnostic.
struct Answered<T> {
    route: ProviderId,
    value: T,
    attempts: Vec<ProviderAttempt>,
    diagnostic: Option<String>,
}

impl<C: Send + 'static, Q, I: Sync, T: Traced + Send> OperationPlan<C, Q, I, T> {
    /// Runs the route chain on `input`, led by the skipped routes.
    async fn run(self, input: &I, run: &OperationRun) -> Result<Answered<T>, ProviderError> {
        let lead = LeadingTrace {
            attempts: self.skipped,
            diagnostic: None,
        };
        answer(self.operation, self.configs, input, lead, run).await
    }

    /// Reads the video through `precheck`, then runs the route chain on the input `input` makes
    /// of the video read; the skipped routes lead the read, and the read leads the chain.
    async fn run_prechecked(
        self,
        precheck: PrecheckPlan,
        run: &OperationRun,
        input: impl FnOnce(&PlatformItem) -> I,
    ) -> Result<(PlatformItem, Answered<T>), ProviderError> {
        let Prechecked {
            video,
            attempts,
            diagnostic,
        } = run_precheck(
            precheck,
            self.skipped,
            run.client.clone(),
            run.retry_policy,
            run.deadline,
            run.verbose,
        )
        .await?;
        let input = input(&video);
        let lead = LeadingTrace {
            attempts,
            diagnostic,
        };
        let answered = answer(self.operation, self.configs, &input, lead, run).await?;
        Ok((video, answered))
    }
}

/// Runs the route chain of `operation` over `configs`, led by `lead`.
async fn answer<C: Send + 'static, Q, I: Sync, T: Traced + Send>(
    operation: &'static Operation<C, Q, I, T>,
    configs: Vec<C>,
    input: &I,
    lead: LeadingTrace,
    run: &OperationRun,
) -> Result<Answered<T>, ProviderError> {
    let route = operation.route;
    let identity = |_: &C| StepIdentity {
        provider: route.route.name(),
        model: None,
        endpoint_host: None,
    };
    let steps = configs
        .into_iter()
        .map(|context| ChainStep {
            context,
            configured: true,
        })
        .collect();
    let result = chain::run_chain(
        steps,
        ChainSettings {
            target: operation_target(operation.platform, operation.name),
            budget_policy: BudgetPolicy::SlicedEven {
                skipped_message: "skipped to preserve fallback deadline budget",
            },
            fallback_off: false,
            diagnostic_merge: DiagnosticMerge::Join,
            terminal: TerminalPolicy::ChainWide {
                verbose: run.verbose,
                exhausted_message: operation.exhausted_message,
            },
            identity: &identity,
        },
        run.deadline,
        |config, step_deadline| {
            let context = RouteContext {
                client: run.client.clone(),
                retry_policy: run.retry_policy,
                deadline: step_deadline,
            };
            let answered = (route.run)(config, context, input);
            async move {
                match answered.await {
                    Ok(mut value) => {
                        let (attempts, diagnostic) = value.take_trace();
                        StepVerdict::Accepted(StepSuccess {
                            value,
                            attempts,
                            diagnostic,
                        })
                    }
                    Err(error) => StepVerdict::Failed(error),
                }
            }
        },
    )
    .await;
    let outcome = lead.lead(result)?;
    Ok(Answered {
        route: route.route,
        value: outcome.value,
        attempts: if run.verbose {
            outcome.attempts
        } else {
            Vec::new()
        },
        diagnostic: outcome.diagnostic,
    })
}

/// The attempts recorded before an operation's route chain, such as the skipped routes and a
/// precheck's read, and the diagnostic of that read.
struct LeadingTrace {
    attempts: Vec<ProviderAttempt>,
    diagnostic: Option<String>,
}

impl LeadingTrace {
    /// Leads the result of the route chain: these attempts come first and this diagnostic joins
    /// the chain's, on success and on failure.
    fn lead<T>(
        self,
        result: Result<ChainOutcome<T>, ProviderError>,
    ) -> Result<ChainOutcome<T>, ProviderError> {
        let Self {
            mut attempts,
            diagnostic,
        } = self;
        match result {
            Ok(mut outcome) => {
                attempts.append(&mut outcome.attempts);
                outcome.attempts = attempts;
                outcome.diagnostic = joined(diagnostic, outcome.diagnostic);
                Ok(outcome)
            }
            Err(mut error) => {
                attempts.append(&mut error.attempts);
                error.attempts = attempts;
                error.diagnostic = joined(diagnostic, error.diagnostic);
                Err(error)
            }
        }
    }
}

fn joined(first: Option<String>, second: Option<String>) -> Option<String> {
    combine_diagnostics([first, second].into_iter().flatten())
}

/// The answer of a route's inherent method, which carries the route's attempts and diagnostic.
trait Traced {
    fn take_trace(&mut self) -> (Vec<ProviderAttempt>, Option<String>);
}

macro_rules! traced {
    ($($outcome:ty),+ $(,)?) => {
        $(impl Traced for $outcome {
            fn take_trace(&mut self) -> (Vec<ProviderAttempt>, Option<String>) {
                (std::mem::take(&mut self.attempts), self.diagnostic.take())
            }
        })+
    };
}

traced!(
    PlatformSearchOutcome,
    XiaohongshuCommentsOutcome,
    XAskOutcome,
    HnStoriesOutcome,
    PolymarketHistoryOutcome,
    YoutubeCommentsOutcome,
    YoutubeVideosOutcome,
    ModelAnswerOutcome,
    ModelTranscriptOutcome,
);

/// The planned page of the works citing a Google Scholar paper.
pub(crate) struct CitedByPlan {
    request: ScholarCitedByRequest,
    route: OperationPlan<
        KeyedHttpRouteRuntimeConfig,
        ScholarCitedByRequest,
        ScholarCitedByRequest,
        PlatformSearchOutcome,
    >,
}

/// Plans the first page of the works citing a Google Scholar paper; `config` is Google
/// Scholar's.
pub(crate) fn plan_cited_by(
    config: &PlatformRuntimeConfig,
    routes: &PlatformRoutesRuntimeConfig,
    request: ScholarCitedByRequest,
) -> Result<CitedByPlan, PlatformPreflightError> {
    let route = plan(&SCHOLAR_CITED_BY, config, routes, &request, None)?;
    Ok(CitedByPlan { request, route })
}

/// Plans the cited-by page a cursor names; only the route that produced the cursor may run it.
pub(crate) fn plan_cursor_cited_by(
    config: &PlatformRuntimeConfig,
    routes: &PlatformRoutesRuntimeConfig,
    cursor: &str,
) -> Result<CitedByPlan, PlatformPreflightError> {
    let (pinned, request) = page_cursor::decode(cursor, config.platform(), CITED_BY)
        .map_err(PlatformPreflightError::Argument)?;
    let route = plan(&SCHOLAR_CITED_BY, config, routes, &request, Some(pinned))?;
    Ok(CitedByPlan { request, route })
}

/// Runs a planned cited-by page and encodes the next-page cursor.
pub(crate) async fn run_cited_by(
    plan: CitedByPlan,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
    verbose: bool,
) -> Result<PlatformSearchPage, ProviderError> {
    let run = OperationRun {
        client,
        retry_policy,
        deadline,
        verbose,
    };
    let CitedByPlan { request, route } = plan;
    let answered = route.run(&request, &run).await?;
    let route = answered.route;
    Ok(PlatformSearchPage {
        platform: Platform::Scholar,
        provider: route.name(),
        collection: None,
        items: answered.value.items,
        next_cursor: answered
            .value
            .next_page
            .map(|page| page_cursor::encode(route, request.at_page(page))),
        attempts: answered.attempts,
        diagnostic: answered.diagnostic,
    })
}

/// The planned page of the videos of a YouTube playlist or channel.
pub(crate) struct VideosPlan {
    request: YoutubeVideosRequest,
    route: OperationPlan<
        KeyedHttpRouteRuntimeConfig,
        YoutubeVideosRequest,
        YoutubeVideosRequest,
        YoutubeVideosOutcome,
    >,
}

/// Plans the first page of the videos of a YouTube playlist or channel; `config` is YouTube's.
pub(crate) fn plan_videos(
    config: &PlatformRuntimeConfig,
    routes: &PlatformRoutesRuntimeConfig,
    request: YoutubeVideosRequest,
) -> Result<VideosPlan, PlatformPreflightError> {
    let route = plan(&YOUTUBE_VIDEOS, config, routes, &request, None)?;
    Ok(VideosPlan { request, route })
}

/// Plans the videos page a cursor names; only the route that produced the cursor may run it.
pub(crate) fn plan_cursor_videos(
    config: &PlatformRuntimeConfig,
    routes: &PlatformRoutesRuntimeConfig,
    cursor: &str,
) -> Result<VideosPlan, PlatformPreflightError> {
    let (pinned, request) = page_cursor::decode(cursor, config.platform(), VIDEOS)
        .map_err(PlatformPreflightError::Argument)?;
    let route = plan(&YOUTUBE_VIDEOS, config, routes, &request, Some(pinned))?;
    Ok(VideosPlan { request, route })
}

/// Runs a planned videos page. The page and its cursor name the collection the route read, such
/// as the channel a handle resolved to.
pub(crate) async fn run_videos(
    plan: VideosPlan,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
    verbose: bool,
) -> Result<PlatformSearchPage, ProviderError> {
    let run = OperationRun {
        client,
        retry_policy,
        deadline,
        verbose,
    };
    let VideosPlan { request, route } = plan;
    let answered = route.run(&request, &run).await?;
    let route = answered.route;
    let outcome = answered.value;
    let collection = PlatformCollection::new(PlatformRef::Youtube(outcome.collection.clone()));
    let resolved = YoutubeVideosRequest {
        collection: YoutubeCollection::Ref(outcome.collection),
        ..request
    };
    Ok(PlatformSearchPage {
        platform: Platform::Youtube,
        provider: route.name(),
        collection: Some(collection),
        items: outcome.items,
        next_cursor: outcome
            .next_page
            .map(|page| page_cursor::encode(route, resolved.at_page(page))),
        attempts: answered.attempts,
        diagnostic: answered.diagnostic,
    })
}

/// The planned transcription of a YouTube video: the precheck, then the route.
pub(crate) struct TranscribePlan {
    precheck: PrecheckPlan,
    route: OperationPlan<
        ModelRouteRuntimeConfig,
        YoutubeTranscribeRequest,
        PlatformItem,
        ModelTranscriptOutcome,
    >,
}

/// Plans a transcription; `config` is YouTube's.
pub(crate) fn plan_transcribe(
    config: &PlatformRuntimeConfig,
    routes: &PlatformRoutesRuntimeConfig,
    request: YoutubeTranscribeRequest,
) -> Result<TranscribePlan, PlatformPreflightError> {
    let route = plan(&YOUTUBE_TRANSCRIBE, config, routes, &request, None)?;
    let precheck = plan_precheck(config, request.video, PrecheckDepth::Description)?;
    Ok(TranscribePlan { precheck, route })
}

/// Runs a planned transcription: the precheck, then the route chain. The transcript document is
/// the result's content, not yet written.
pub(crate) async fn run_transcribe(
    plan: TranscribePlan,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
    verbose: bool,
) -> Result<PlatformTranscribeResult, ProviderError> {
    let run = OperationRun {
        client,
        retry_policy,
        deadline,
        verbose,
    };
    let (item, answered) = plan
        .route
        .run_prechecked(plan.precheck, &run, Clone::clone)
        .await?;
    Ok(transcribed(item, answered))
}

/// The result of a transcription: the prechecked item at full-text depth, carrying the
/// transcript instead of its description and chapters, which the document holds.
fn transcribed(
    mut item: PlatformItem,
    answered: Answered<ModelTranscriptOutcome>,
) -> PlatformTranscribeResult {
    let Answered {
        route,
        value: outcome,
        attempts,
        diagnostic,
    } = answered;
    item.depth = ContentDepth::FullText;
    if let PlatformItemData::Youtube(data) = &mut item.data {
        data.description = None;
        data.chapters = None;
        data.transcript = Some(outcome.transcript);
    }
    let content = PlatformContent::new(item.url.clone(), route.name(), outcome.markdown);
    PlatformTranscribeResult {
        fetched: PlatformFetchResult {
            platform: Platform::Youtube,
            provider: route.name(),
            item,
            content: Some(content),
            attempts,
            diagnostic,
        },
        model: outcome.model,
        usage: outcome.usage,
    }
}

/// The planned read of the comments of a Xiaohongshu note or a YouTube video.
pub(crate) struct CommentsPlan(PlannedComments);

enum PlannedComments {
    Xiaohongshu {
        request: XiaohongshuCommentsRequest,
        route: OperationPlan<
            BrowserRouteRuntimeConfig,
            XiaohongshuCommentsRequest,
            XiaohongshuCommentsRequest,
            XiaohongshuCommentsOutcome,
        >,
    },
    Youtube {
        request: YoutubeCommentsRequest,
        route: OperationPlan<
            KeyedHttpRouteRuntimeConfig,
            YoutubeCommentsRequest,
            YoutubeCommentsRequest,
            YoutubeCommentsOutcome,
        >,
    },
}

/// Plans a comments read; `config` is the request platform's.
pub(crate) fn plan_comments(
    config: &PlatformRuntimeConfig,
    routes: &PlatformRoutesRuntimeConfig,
    request: PlatformCommentsRequest,
) -> Result<CommentsPlan, PlatformPreflightError> {
    Ok(CommentsPlan(match request {
        PlatformCommentsRequest::Xiaohongshu(request) => PlannedComments::Xiaohongshu {
            route: plan(&XIAOHONGSHU_COMMENTS, config, routes, &request, None)?,
            request,
        },
        PlatformCommentsRequest::Youtube(request) => PlannedComments::Youtube {
            route: plan(&YOUTUBE_COMMENTS, config, routes, &request, None)?,
            request,
        },
    }))
}

/// Runs a planned comments read.
pub(crate) async fn run_comments(
    plan: CommentsPlan,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
    verbose: bool,
) -> Result<PlatformCommentsPage, ProviderError> {
    let run = OperationRun {
        client,
        retry_policy,
        deadline,
        verbose,
    };
    Ok(match plan.0 {
        PlannedComments::Xiaohongshu { request, route } => {
            let answered = route.run(&request, &run).await?;
            PlatformCommentsPage::Xiaohongshu(XiaohongshuCommentsPage {
                platform: Platform::Xiaohongshu,
                provider: answered.route.name(),
                note: PlatformRef::Xiaohongshu(request.note),
                comments: answered.value.comments,
                has_more: answered.value.has_more,
                attempts: answered.attempts,
                diagnostic: answered.diagnostic,
            })
        }
        PlannedComments::Youtube { request, route } => {
            let answered = route.run(&request, &run).await?;
            PlatformCommentsPage::Youtube(YoutubeCommentsPage {
                platform: Platform::Youtube,
                provider: answered.route.name(),
                video: PlatformRef::Youtube(request.video),
                comments: answered.value.comments,
                has_more: answered.value.has_more,
                attempts: answered.attempts,
                diagnostic: answered.diagnostic,
            })
        }
    })
}

/// The planned ask: an X ask needs nothing before its route; a YouTube ask is about one video,
/// which its precheck reads first.
pub(crate) struct AskPlan(PlannedAsk);

enum PlannedAsk {
    X {
        request: XAskRequest,
        route: OperationPlan<XaiRuntimeConfig, XAskRequest, XAskRequest, XAskOutcome>,
    },
    Youtube {
        request: YoutubeAskRequest,
        precheck: PrecheckPlan,
        route: OperationPlan<
            ModelRouteRuntimeConfig,
            YoutubeAskRequest,
            (PlatformItem, String),
            ModelAnswerOutcome,
        >,
    },
}

/// Plans an ask; `config` is the request platform's.
pub(crate) fn plan_ask(
    config: &PlatformRuntimeConfig,
    routes: &PlatformRoutesRuntimeConfig,
    request: PlatformAskRequest,
) -> Result<AskPlan, PlatformPreflightError> {
    Ok(AskPlan(match request {
        PlatformAskRequest::X(request) => PlannedAsk::X {
            route: plan(&X_ASK, config, routes, &request, None)?,
            request,
        },
        PlatformAskRequest::Youtube(request) => {
            let route = plan(&YOUTUBE_ASK, config, routes, &request, None)?;
            let precheck = plan_precheck(config, request.video.clone(), PrecheckDepth::Metadata)?;
            PlannedAsk::Youtube {
                request,
                precheck,
                route,
            }
        }
    }))
}

/// Runs a planned ask: the precheck, when the platform has one, then the route chain.
pub(crate) async fn run_ask(
    plan: AskPlan,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
    verbose: bool,
) -> Result<PlatformAskResult, ProviderError> {
    let run = OperationRun {
        client,
        retry_policy,
        deadline,
        verbose,
    };
    Ok(match plan.0 {
        PlannedAsk::X { request, route } => {
            let answered = route.run(&request, &run).await?;
            PlatformAskResult::X(XAskResult {
                platform: Platform::X,
                provider: answered.route.name(),
                query: request.query,
                answer: answered.value.answer,
                sources: answered.value.sources,
                attempts: answered.attempts,
                diagnostic: answered.diagnostic,
            })
        }
        PlannedAsk::Youtube {
            request,
            precheck,
            route,
        } => {
            let (video, answered) = route
                .run_prechecked(precheck, &run, |video| {
                    (video.clone(), request.question.clone())
                })
                .await?;
            PlatformAskResult::Youtube(YoutubeAskResult {
                platform: Platform::Youtube,
                provider: answered.route.name(),
                video: AskedVideo {
                    reference: video.reference,
                    url: video.url,
                    title: video.title,
                },
                question: request.question,
                answer: answered.value.answer,
                model: answered.value.model,
                usage: answered.value.usage,
                attempts: answered.attempts,
                diagnostic: answered.diagnostic,
            })
        }
    })
}

/// The planned listing of a Hacker News story list.
pub(crate) struct StoriesPlan {
    request: HnStoriesRequest,
    route:
        OperationPlan<HnRouteRuntimeConfig, HnStoriesRequest, HnStoriesRequest, HnStoriesOutcome>,
}

/// Plans a stories listing; `config` is Hacker News's.
pub(crate) fn plan_stories(
    config: &PlatformRuntimeConfig,
    routes: &PlatformRoutesRuntimeConfig,
    request: HnStoriesRequest,
) -> Result<StoriesPlan, PlatformPreflightError> {
    let route = plan(&HN_STORIES, config, routes, &request, None)?;
    Ok(StoriesPlan { request, route })
}

/// Runs a planned stories listing.
pub(crate) async fn run_stories(
    plan: StoriesPlan,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
    verbose: bool,
) -> Result<HnStoriesResult, ProviderError> {
    let run = OperationRun {
        client,
        retry_policy,
        deadline,
        verbose,
    };
    let StoriesPlan { request, route } = plan;
    let answered = route.run(&request, &run).await?;
    Ok(HnStoriesResult {
        platform: Platform::Hn,
        provider: answered.route.name(),
        list: request.list,
        items: answered.value.items,
        attempts: answered.attempts,
        diagnostic: answered.diagnostic,
    })
}

/// The planned Price History of one outcome of a Polymarket market.
pub(crate) struct HistoryPlan {
    request: PolymarketHistoryRequest,
    route: OperationPlan<
        PolymarketRouteRuntimeConfig,
        PolymarketHistoryRequest,
        PolymarketHistoryRequest,
        PolymarketHistoryOutcome,
    >,
}

/// Plans a price history; `config` is Polymarket's.
pub(crate) fn plan_history(
    config: &PlatformRuntimeConfig,
    routes: &PlatformRoutesRuntimeConfig,
    request: PolymarketHistoryRequest,
) -> Result<HistoryPlan, PlatformPreflightError> {
    let route = plan(&POLYMARKET_HISTORY, config, routes, &request, None)?;
    Ok(HistoryPlan { request, route })
}

/// Runs a planned price history.
pub(crate) async fn run_history(
    plan: HistoryPlan,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
    verbose: bool,
) -> Result<PolymarketHistoryResult, ProviderError> {
    let run = OperationRun {
        client,
        retry_policy,
        deadline,
        verbose,
    };
    let HistoryPlan { request, route } = plan;
    let answered = route.run(&request, &run).await?;
    let history = answered.value;
    Ok(PolymarketHistoryResult {
        platform: Platform::Polymarket,
        provider: answered.route.name(),
        market: history.market,
        outcome: history.outcome,
        window: request.window,
        bucket_seconds: request.bucket.seconds(),
        points: history.points,
        settlement: history.settlement,
        attempts: answered.attempts,
        diagnostic: answered.diagnostic,
    })
}

#[cfg(test)]
mod tests {
    use super::LeadingTrace;
    use crate::chain::{ChainOutcome, skipped_attempt};
    use crate::types::{AttemptErrorKind, AttemptTarget, ProviderAttempt, ProviderError};

    fn attempt(provider: &'static str) -> ProviderAttempt {
        skipped_attempt(AttemptTarget::platform("youtube", "ask"), provider, "test")
    }

    fn trace() -> LeadingTrace {
        LeadingTrace {
            attempts: vec![attempt("youtube_api")],
            diagnostic: Some("read".into()),
        }
    }

    fn providers(attempts: &[ProviderAttempt]) -> Vec<&str> {
        attempts.iter().map(|attempt| attempt.provider).collect()
    }

    #[test]
    fn the_leading_trace_leads_a_successful_chain() {
        let outcome = trace()
            .lead(Ok(ChainOutcome {
                value: (),
                attempts: vec![attempt("gemini_api")],
                diagnostic: Some("answered".into()),
            }))
            .unwrap_or_else(|error| panic!("lead failed: {}", error.message));

        assert_eq!(
            (providers(&outcome.attempts), outcome.diagnostic.as_deref()),
            (vec!["youtube_api", "gemini_api"], Some("read\nanswered"))
        );
    }

    #[test]
    fn the_leading_trace_leads_a_failed_chain() {
        let error = trace()
            .lead::<()>(Err(ProviderError {
                kind: AttemptErrorKind::Quality,
                message: "refused".into(),
                attempts: vec![attempt("gemini_api")],
                verbose: false,
                diagnostic: None,
            }))
            .err()
            .map(|error| (providers(&error.attempts).join(","), error.diagnostic));

        assert_eq!(
            error,
            Some(("youtube_api,gemini_api".to_owned(), Some("read".to_owned())))
        );
    }
}
