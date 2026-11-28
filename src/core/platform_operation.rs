//! The one orchestration entry of every Platform Operation that one route serves.
//!
//! Planning follows every platform operation: the configured order ∩ the operation's route set
//! in the platform catalog ∩ configured routes, minus a route that cannot run the request. It
//! hands the operation the configuration of the route the operation's declaration names, read
//! from that route's own field, so the route's inherent method never receives another route's
//! configuration; a route that is absent from the order or unconfigured is a configuration
//! error. The route chain runs through the shared chain executor, and the answer carries every
//! attempt of the operation, the skipped routes and a precheck's read first.
//!
//! The no-wildcard rule covers every match in this module, so a new route of a paged operation
//! or a new operation outcome is a compile error at every site here.
#![deny(clippy::wildcard_enum_match_arm)]

use reqwest::Client;

use crate::catalog::{self, ProviderId};
use crate::chain::{
    self, BudgetPolicy, ChainOutcome, ChainSettings, ChainStep, DiagnosticMerge, LeadingTrace,
    StepIdentity, StepSuccess, StepVerdict, TerminalPolicy,
};
use crate::config::{
    BrowserRouteRuntimeConfig, HnRouteRuntimeConfig, KeyedHttpRouteRuntimeConfig,
    ModelRouteRuntimeConfig, PlatformRoutesRuntimeConfig, PlatformRuntimeConfig,
    PolymarketRouteRuntimeConfig, XaiRuntimeConfig,
};
use crate::net::RetryPolicy;
use crate::page_cursor::{self, PagedRequest};
use crate::platform_chain::{
    PlatformPreflightError, operation_target, plan_routes, route_candidates,
};
use crate::platform_precheck::{PrecheckDepth, PrecheckPlan, plan_precheck, run_precheck};
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
        let lead = LeadingTrace::new(self.skipped);
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
        let ChainOutcome {
            value: video,
            attempts,
            diagnostic,
        } = run_precheck(
            precheck,
            self.skipped,
            run.client.clone(),
            run.retry_policy,
            run.deadline,
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
        attempts: outcome.attempts,
        diagnostic: outcome.diagnostic,
    })
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
) -> Result<PlatformSearchPage, ProviderError> {
    let run = OperationRun {
        client,
        retry_policy,
        deadline,
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
) -> Result<PlatformSearchPage, ProviderError> {
    let run = OperationRun {
        client,
        retry_policy,
        deadline,
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
) -> Result<PlatformTranscribeResult, ProviderError> {
    let run = OperationRun {
        client,
        retry_policy,
        deadline,
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
) -> Result<PlatformCommentsPage, ProviderError> {
    let run = OperationRun {
        client,
        retry_policy,
        deadline,
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
) -> Result<PlatformAskResult, ProviderError> {
    let run = OperationRun {
        client,
        retry_policy,
        deadline,
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
) -> Result<HnStoriesResult, ProviderError> {
    let run = OperationRun {
        client,
        retry_policy,
        deadline,
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
) -> Result<PolymarketHistoryResult, ProviderError> {
    let run = OperationRun {
        client,
        retry_policy,
        deadline,
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
    /// The YouTube API route's videos, the only paged operation whose route resolves its
    /// collection.
    mod youtube_api {
        use std::time::Duration;

        use serde_json::json;

        use super::super::{plan_cursor_videos, plan_videos, run_videos};
        use crate::config::runtime_config_from_toml;
        use crate::fixture_server::{Fixture, Response, block_on, path, query};
        use crate::net::{RetryPolicy, build_client};
        use crate::samples::sample_json;
        use crate::types::{
            Deadline, Platform, PlatformSearchPage, ProviderError, YoutubeCollection,
            YoutubeVideosRequest,
        };

        fn run(plan: super::super::VideosPlan) -> Result<PlatformSearchPage, ProviderError> {
            block_on(run_videos(
                plan,
                build_client(true).expect("build client"),
                RetryPolicy::new(1, 1.0, Duration::ZERO),
                Deadline::new(Duration::from_secs(10)),
            ))
        }

        #[test]
        fn a_videos_cursor_continues_the_channel_a_handle_resolved_to_without_another_lookup() {
            let mut continued = sample_json("youtube_api/playlist-items.json");
            continued["nextPageToken"] = json!("EAAaBlBUOkNBRQ");
            let api = Fixture::start_sequence(
                [
                    sample_json("youtube_api/channels.json"),
                    continued,
                    sample_json("youtube_api/playlist-items.json"),
                ]
                .iter()
                .map(|body| Response::json(200, &body.to_string()))
                .collect(),
            );
            let runtime = runtime_config_from_toml(&format!(
                "[providers.youtube_api]\nurl = \"{}/youtube/v3\"\nkeys = [\"yt-key\"]\n",
                api.url
            ))
            .expect("valid configuration");
            let config = runtime.platforms.get(Platform::Youtube);
            let request = YoutubeVideosRequest {
                collection: YoutubeCollection::Handle("@examplemaths".to_owned()),
                limit: 1,
                page: None,
            };

            let first = run(plan_videos(config, &runtime.platform_routes, request).expect("plan"))
                .expect("the first page succeeds");
            let cursor = first.next_cursor.clone().expect("a later page");
            let second =
                run(plan_cursor_videos(config, &runtime.platform_routes, &cursor).expect("plan"))
                    .expect("the later page succeeds");
            let sent = api
                .finish_all()
                .iter()
                .map(|request| (path(request), query(request).remove("pageToken")))
                .collect::<Vec<_>>();
            let collection = |page: &PlatformSearchPage| {
                serde_json::to_value(&page.collection).expect("serialize")["ref"].clone()
            };

            assert_eq!(
                (
                    collection(&first),
                    collection(&second),
                    second.next_cursor,
                    sent
                ),
                (
                    json!("youtube:channel/UCExampleMathsChannel001"),
                    json!("youtube:channel/UCExampleMathsChannel001"),
                    None,
                    vec![
                        ("/youtube/v3/channels".to_owned(), None),
                        ("/youtube/v3/playlistItems".to_owned(), None),
                        (
                            "/youtube/v3/playlistItems".to_owned(),
                            Some("EAAaBlBUOkNBRQ".to_owned())
                        ),
                    ],
                )
            );
        }
    }

    /// The `gemini_api` route's ask and transcribe, which read the video through the precheck
    /// before any model request.
    mod gemini_api {
        use std::time::Duration;

        use serde_json::{Value, json};

        use super::super::{plan_ask, plan_transcribe, run_ask, run_transcribe};
        use crate::config::{RuntimeConfig, runtime_config_from_toml};
        use crate::fixture_server::{Fixture, Response, block_on, path};
        use crate::net::{RetryPolicy, build_client};
        use crate::samples::sample_json;
        use crate::types::{
            AttemptErrorKind, Deadline, Platform, PlatformAskRequest, ProviderError,
            YoutubeAskRequest, YoutubeRef, YoutubeTranscribeRequest,
        };

        /// The stored `videos.list` response holding `items`.
        fn videos(items: &[Value]) -> Response {
            let mut page = sample_json("youtube_api/video.json");
            page["items"] = json!(items);
            Response::json(200, &page.to_string())
        }

        fn zoo() -> Value {
            sample_json("youtube_api/video.json")["items"][0].clone()
        }

        /// The stored Gemini answer whose text is `text`.
        fn answer(text: &str) -> Response {
            Response::json(
                200,
                &crate::gemini_api_samples::generated(text, "STOP").to_string(),
            )
        }

        /// The default YouTube order over `youtube_api` at `api` and `gemini_api` at `gemini`.
        fn runtime(api: &Fixture, gemini: &Fixture) -> RuntimeConfig {
            runtime_config_from_toml(&format!(
                "[providers.youtube_api]\nurl = \"{}/youtube/v3\"\nkeys = [\"yt-key\"]\n\n[providers.gemini_api]\nurl = \"{}/v1beta\"\nkeys = [\"gemini-key\"]\nmodel = \"gemini-flash-latest\"\n",
                api.url, gemini.url
            ))
            .expect("valid configuration")
        }

        /// The outcome of one operation and the paths each fixture received.
        type Run = (Result<(), ProviderError>, Vec<String>, Vec<String>);

        /// An ask and a transcription of `video` with the precheck on `youtube_api` answering
        /// `precheck` and the model on `gemini_api` answering `model` to each.
        fn both(
            video: &str,
            precheck: &dyn Fn() -> Response,
            model: &dyn Fn() -> Vec<Response>,
        ) -> [Run; 2] {
            [true, false].map(|asks| {
                let api = Fixture::start_sequence(vec![precheck()]);
                let gemini = Fixture::start_sequence(model());
                let runtime = runtime(&api, &gemini);
                let config = runtime.platforms.get(Platform::Youtube);
                let video = YoutubeRef::video(video).expect("a video ID");
                let client = build_client(true).expect("build client");
                let retry = RetryPolicy::new(1, 1.0, Duration::ZERO);
                let deadline = Deadline::new(Duration::from_secs(10));
                let outcome = if asks {
                    let request = PlatformAskRequest::Youtube(YoutubeAskRequest {
                        video,
                        question: "What animals stand behind the speaker?".to_owned(),
                    });
                    let plan = plan_ask(config, &runtime.platform_routes, request).expect("plan");
                    block_on(run_ask(plan, client, retry, deadline)).map(drop)
                } else {
                    let request = YoutubeTranscribeRequest { video };
                    let plan =
                        plan_transcribe(config, &runtime.platform_routes, request).expect("plan");
                    block_on(run_transcribe(plan, client, retry, deadline)).map(drop)
                };
                let paths = |fixture: Fixture| {
                    fixture
                        .finish_all()
                        .iter()
                        .map(|request| path(request))
                        .collect()
                };
                (outcome, paths(api), paths(gemini))
            })
        }

        fn failure(outcome: &Result<(), ProviderError>) -> (AttemptErrorKind, &str, Vec<&str>) {
            let error = outcome.as_ref().expect_err("the operation fails");
            (
                error.kind,
                error.message.as_str(),
                error
                    .attempts
                    .iter()
                    .map(|attempt| attempt.provider)
                    .collect(),
            )
        }

        #[test]
        fn a_video_the_precheck_cannot_read_reaches_no_model() {
            let outcomes = both("aaaaaaaaaaa", &|| videos(&[]), &Vec::new);

            for (outcome, api, gemini) in &outcomes {
                assert_eq!(
                    (failure(outcome), api.as_slice(), gemini.len()),
                    (
                        (
                            AttemptErrorKind::Parameter,
                            "YouTube video not found or not public: youtube:aaaaaaaaaaa",
                            vec!["youtube_api"],
                        ),
                        ["/youtube/v3/videos".to_owned()].as_slice(),
                        0,
                    )
                );
            }
        }

        #[test]
        fn a_live_stream_is_a_quality_failure_that_reaches_no_model() {
            let mut live = zoo();
            live["snippet"]["liveBroadcastContent"] = json!("live");
            live["liveStreamingDetails"] = json!({"actualStartTime": "2026-10-18T01:00:00Z"});

            let outcomes = both("jNQXAC9IVRw", &|| videos(&[live.clone()]), &Vec::new);

            for (outcome, _, gemini) in &outcomes {
                assert_eq!(
                    (failure(outcome), gemini.len()),
                    (
                        (
                            AttemptErrorKind::Quality,
                            "youtube:jNQXAC9IVRw is streaming live; a model can read it only after the stream ends",
                            vec!["youtube_api"],
                        ),
                        0,
                    )
                );
            }
        }

        #[test]
        fn a_video_without_a_description_is_transcribed_with_an_empty_description() {
            let mut video = zoo();
            video["snippet"]["description"] = json!("");
            video["snippet"]["localized"]["description"] = json!("");
            let api = Fixture::start_sequence(vec![videos(&[video])]);
            let gemini = Fixture::start_sequence(vec![answer(
                &json!({"language": "en", "segments": [
                    {"start": 1.2, "end": 3.4, "text": "All right, so here we are."}
                ]})
                .to_string(),
            )]);
            let runtime = runtime(&api, &gemini);
            let request = YoutubeTranscribeRequest {
                video: YoutubeRef::video("jNQXAC9IVRw").expect("a video ID"),
            };
            let plan = plan_transcribe(
                runtime.platforms.get(Platform::Youtube),
                &runtime.platform_routes,
                request,
            )
            .expect("plan");

            let transcribed = block_on(run_transcribe(
                plan,
                build_client(true).expect("build client"),
                RetryPolicy::new(1, 1.0, Duration::ZERO),
                Deadline::new(Duration::from_secs(10)),
            ))
            .expect("a video without a description is transcribed");
            api.finish();
            gemini.finish();
            let document = transcribed
                .fetched
                .content
                .map(|content| content.text)
                .unwrap_or_default();

            assert!(
                document.starts_with("# Me at the zoo\n")
                    && document.contains("## Description\n\n(none)\n")
                    && document.contains(
                        "[00:01](https://www.youtube.com/watch?v=jNQXAC9IVRw&t=1s) All right, so here we are."
                    ),
                "{document}"
            );
        }
    }
}
