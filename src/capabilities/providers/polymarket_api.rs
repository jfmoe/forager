//! The `polymarket_api` route: one anonymous route over Polymarket's official public read APIs
//! (ADR 0032).
//!
//! Gamma finds events and holds their metadata and cached quotes; the CLOB holds the live order
//! books. Search prices from Gamma alone in one request. Fetch reads the event or market from
//! Gamma and the live books of its open markets from the CLOB; the CLOB only enriches the
//! result, so when it fails the fetch prices from Gamma's cache and one diagnostic says so.
//! Every request to either endpoint follows the route's one access policy.

use std::collections::HashMap;
use std::time::Duration;

use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;

use crate::catalog::{PlatformOperation, ProviderId};
use crate::config::PolymarketRouteRuntimeConfig;
use crate::credentials::CredentialPool;
use crate::net::{
    AttemptFailure, RetryPolicy, read_complete_protocol_with_reasons, send_provider_request,
};
use crate::providers::execution::{ExecutionOutcome, ExecutionSettings, execute_anonymous};
use crate::providers::shared::{
    Refusal, Trail, acquire_window, other_platform_message, parameter_error,
};
use crate::rate_limit::RateLimiter;
use crate::types::{
    AttemptErrorKind, AttemptTarget, ContentDepth, Deadline, FullTextSource, Platform,
    PlatformFetchOutcome, PlatformFetchRequest, PlatformItem, PlatformRef, PlatformSearchOptions,
    PlatformSearchOutcome, PlatformSearchRequest, PolymarketKind, PolymarketPriceSource,
    PolymarketRef, PolymarketStatus, ProviderError,
};

#[path = "polymarket_api_gamma.rs"]
mod gamma;
#[path = "polymarket_api_quotes.rs"]
mod quotes;

use gamma::{Event, GammaEvent, GammaMarket, Market, MarketQuotes};

const ROUTE: ProviderId = ProviderId::PolymarketApi;
const PLATFORM: Platform = Platform::Polymarket;
/// Gamma search refuses a page past 100 with HTTP 422 (measured 2026-10-24).
const MAX_SEARCH_PAGE: u64 = 100;
const STALE_QUOTES: &str = "prices are Gamma's cached quotes, which may lag up to 5 minutes";

/// Returns whether the route can run the search; it never sends a request. Gamma refuses a
/// page past the last one it serves, so such a cursor fails here.
pub(crate) fn search_support(request: &PlatformSearchRequest) -> Result<(), String> {
    if !matches!(request.options, PlatformSearchOptions::Polymarket(_)) {
        return Err(other_platform_message(ROUTE, request.options.platform()));
    }
    let page = page_number(request.page.as_deref())?;
    if page > MAX_SEARCH_PAGE {
        return Err(format!(
            "{} cannot read past page {MAX_SEARCH_PAGE} of a search; narrow the query",
            ROUTE.name()
        ));
    }
    Ok(())
}

/// Returns whether the route can fetch at the requested depth; it never sends a request.
pub(crate) fn fetch_support(request: &PlatformFetchRequest) -> Result<(), String> {
    if !matches!(request.reference, PlatformRef::Polymarket(_)) {
        return Err(other_platform_message(ROUTE, request.reference.platform()));
    }
    match request.depth {
        ContentDepth::Metadata | ContentDepth::Abstract => Ok(()),
        depth => Err(format!(
            "{} cannot fetch at depth `{}`; it reads metadata and the resolution rules (abstract)",
            ROUTE.name(),
            depth.as_str()
        )),
    }
}

/// The 1-based search page a request names; the first page has no position.
fn page_number(page: Option<&str>) -> Result<u64, String> {
    page.map_or(Ok(1), |page| {
        page.parse()
            .ok()
            .filter(|page| *page >= 1)
            .ok_or_else(|| format!("invalid Polymarket search page position `{page}`"))
    })
}

/// The two endpoints of the route.
#[derive(Clone, Copy)]
enum Endpoint {
    Gamma,
    Clob,
}

impl Endpoint {
    const fn timeout_message(self) -> &'static str {
        match self {
            Self::Gamma => "Polymarket Gamma API request timed out",
            Self::Clob => "Polymarket CLOB API request timed out",
        }
    }

    fn failure_message(self) -> fn(&str, u16) -> String {
        match self {
            Self::Gamma => gamma_failure_message,
            Self::Clob => clob_failure_message,
        }
    }

    fn reason_kind(self) -> fn(&str) -> Option<AttemptErrorKind> {
        match self {
            Self::Gamma => gamma_reason_kind,
            Self::Clob => |_| None,
        }
    }
}

/// One request: a GET with query parameters or a POST with a JSON body.
enum Call<'a> {
    Get {
        path: &'a str,
        query: &'a [(&'a str, String)],
    },
    Post {
        path: &'a str,
        body: &'a Value,
    },
}

pub(crate) struct PolymarketApi {
    config: PolymarketRouteRuntimeConfig,
    client: Client,
    // The route needs no credentials; the empty pool only drives shared redaction.
    credentials: CredentialPool,
    limiter: RateLimiter,
    retry_policy: RetryPolicy,
    deadline: Deadline,
}

impl PolymarketApi {
    pub(crate) fn new(
        config: PolymarketRouteRuntimeConfig,
        client: Client,
        limiter: RateLimiter,
        retry_policy: RetryPolicy,
        deadline: Deadline,
    ) -> Self {
        Self {
            config,
            client,
            credentials: CredentialPool::new(ROUTE.name(), Vec::new()),
            limiter,
            retry_policy,
            deadline,
        }
    }

    /// Searches events with Gamma's public search, one result per event, priced from Gamma
    /// alone. A result that cannot become an item is skipped with one diagnostic.
    pub(crate) async fn search(
        &self,
        request: &PlatformSearchRequest,
    ) -> Result<PlatformSearchOutcome, ProviderError> {
        let PlatformSearchOptions::Polymarket(options) = &request.options else {
            return Err(parameter_error(other_platform_message(
                ROUTE,
                request.options.platform(),
            )));
        };
        search_support(request).map_err(parameter_error)?;
        let page = page_number(request.page.as_deref()).map_err(parameter_error)?;
        let mut query = vec![
            ("q", request.query.clone()),
            ("limit_per_type", request.limit.to_string()),
            ("page", page.to_string()),
            ("search_tags", "false".to_owned()),
            ("search_profiles", "false".to_owned()),
        ];
        if !options.include_closed {
            query.push(("events_status", "active".to_owned()));
        }
        let mut trail = Trail::default();
        let (items, skipped, has_more) = trail.required(
            self.send(
                Endpoint::Gamma,
                Call::Get {
                    path: "public-search",
                    query: &query,
                },
                PlatformOperation::Search.as_str(),
                decode_search,
            )
            .await,
        )?;
        if skipped > 0 {
            trail.note(format!(
                "skipped {skipped} Polymarket search {} that could not be read",
                if skipped == 1 { "result" } else { "results" }
            ));
        }
        Ok(PlatformSearchOutcome {
            items,
            next_page: (has_more && page < MAX_SEARCH_PAGE).then(|| (page + 1).to_string()),
            diagnostic: trail.diagnostic(),
            attempts: trail.attempts,
        })
    }

    /// Reads one event or market from Gamma, then the live books of its open markets from the
    /// CLOB. Resolved, closed, and suspended markets never reach the CLOB.
    pub(crate) async fn fetch(
        &self,
        request: &PlatformFetchRequest,
    ) -> Result<PlatformFetchOutcome, ProviderError> {
        fetch_support(request).map_err(parameter_error)?;
        let PlatformRef::Polymarket(reference) = &request.reference else {
            unreachable!("the support check accepts only Polymarket refs")
        };
        let depth = request.depth;
        let mut trail = Trail::default();
        let item = match reference.kind() {
            PolymarketKind::Event => {
                let event = trail.required(self.read_event(reference, depth).await)?;
                let open = event
                    .markets
                    .iter()
                    .filter(|market| market.status == PolymarketStatus::Open)
                    .collect::<Vec<_>>();
                let (quotes, source, as_of) = self.quote(&open, &mut trail).await;
                event.into_item(depth, &quotes, source, as_of)
            }
            PolymarketKind::Market => {
                let market = trail.required(self.read_market(reference, depth).await)?;
                let open = if market.status == PolymarketStatus::Open {
                    vec![&market]
                } else {
                    Vec::new()
                };
                let (mut quotes, source, as_of) = self.quote(&open, &mut trail).await;
                let quotes = quotes
                    .remove(market.reference.slug())
                    .unwrap_or(MarketQuotes::Cached);
                market.into_item(depth, &quotes, source, as_of)
            }
        };
        Ok(PlatformFetchOutcome {
            item,
            content_source: FullTextSource::Urls(Vec::new()),
            diagnostic: trail.diagnostic(),
            attempts: trail.attempts,
        })
    }

    /// Reads the live quotes of the open markets. Returns each market's quotes by slug, the
    /// price source, and when the live prices were read; with no open market, or when the CLOB
    /// fails, every market prices from Gamma.
    async fn quote(
        &self,
        open: &[&Market],
        trail: &mut Trail,
    ) -> (
        HashMap<String, MarketQuotes>,
        PolymarketPriceSource,
        Option<String>,
    ) {
        if open.is_empty() {
            return (HashMap::new(), PolymarketPriceSource::Gamma, None);
        }
        let tokens = open
            .iter()
            .flat_map(|market| market.tokens.iter().cloned())
            .collect::<Vec<_>>();
        let live = match self.live_quotes(&tokens, trail).await {
            Ok(live) => live,
            Err(error) => {
                trail.note(format!(
                    "Polymarket CLOB API unavailable ({}); {STALE_QUOTES}",
                    error.message
                ));
                return (HashMap::new(), PolymarketPriceSource::Gamma, None);
            }
        };
        let missing = tokens
            .iter()
            .filter(|token| !live.tops.contains_key(*token))
            .count();
        if missing > 0 {
            trail.note(format!(
                "Polymarket CLOB API returned no quote for {missing} {}; {} no price",
                if missing == 1 { "outcome" } else { "outcomes" },
                if missing == 1 { "it has" } else { "they have" }
            ));
        }
        let quotes = open
            .iter()
            .map(|market| {
                let quotes = MarketQuotes::Live {
                    tops: market
                        .tokens
                        .iter()
                        .map(|token| live.tops.get(token).copied())
                        .collect(),
                    last_trade: live
                        .last_trades
                        .as_ref()
                        .and_then(|trades| {
                            market.tokens.first().and_then(|token| trades.get(token))
                        })
                        .copied(),
                };
                (market.reference.slug().to_owned(), quotes)
            })
            .collect();
        (quotes, PolymarketPriceSource::Clob, Some(live.as_of))
    }

    async fn read_event(
        &self,
        reference: &PolymarketRef,
        depth: ContentDepth,
    ) -> Result<ExecutionOutcome<Event>, ProviderError> {
        let path = format!("events/slug/{}", reference.slug());
        self.send_with(
            Endpoint::Gamma,
            Call::Get {
                path: &path,
                query: &[],
            },
            PlatformOperation::Fetch.as_str(),
            |body| {
                let event = Event::decode(decode::<GammaEvent>(body)?).map_err(Refusal::runtime)?;
                check_slug(reference, event.reference.slug())?;
                if depth == ContentDepth::Abstract && event.rules().is_empty() {
                    return Err(no_rules(reference));
                }
                Ok(event)
            },
            |failure| not_found(failure, reference),
        )
        .await
    }

    async fn read_market(
        &self,
        reference: &PolymarketRef,
        depth: ContentDepth,
    ) -> Result<ExecutionOutcome<Market>, ProviderError> {
        let path = format!("markets/slug/{}", reference.slug());
        self.send_with(
            Endpoint::Gamma,
            Call::Get {
                path: &path,
                query: &[],
            },
            PlatformOperation::Fetch.as_str(),
            |body| {
                let market =
                    Market::decode(decode::<GammaMarket>(body)?).map_err(Refusal::runtime)?;
                check_slug(reference, market.reference.slug())?;
                if depth == ContentDepth::Abstract && market.rules().is_empty() {
                    return Err(no_rules(reference));
                }
                Ok(market)
            },
            |failure| not_found(failure, reference),
        )
        .await
    }

    /// Sends one request under the route's access policy; `decode` turns the body into the
    /// operation's value inside the attempt, so its refusal is that attempt's failure.
    async fn send<T>(
        &self,
        endpoint: Endpoint,
        call: Call<'_>,
        operation: &'static str,
        decode: impl Fn(&str) -> Result<T, Refusal>,
    ) -> Result<ExecutionOutcome<T>, ProviderError> {
        self.send_with(endpoint, call, operation, decode, |failure| failure)
            .await
    }

    /// Like [`Self::send`]; `refine` rewords a failed response the operation can describe
    /// better.
    async fn send_with<T>(
        &self,
        endpoint: Endpoint,
        call: Call<'_>,
        operation: &'static str,
        decode: impl Fn(&str) -> Result<T, Refusal>,
        refine: impl Fn(AttemptFailure) -> AttemptFailure,
    ) -> Result<ExecutionOutcome<T>, ProviderError> {
        let base = match endpoint {
            Endpoint::Gamma => &self.config.url,
            Endpoint::Clob => &self.config.clob_url,
        };
        let base = base.trim_end_matches('/');
        let (failure_message, reason_kind) = (endpoint.failure_message(), endpoint.reason_kind());
        let (call, decode, refine) = (&call, &decode, &refine);
        execute_anonymous(
            self.settings(endpoint, operation),
            move |deadline| async move {
                let _permit = acquire_window(&self.limiter, deadline).await?;
                let request = match call {
                    Call::Get { path, query } => {
                        self.client.get(format!("{base}/{path}")).query(query)
                    }
                    Call::Post { path, body } => {
                        self.client.post(format!("{base}/{path}")).json(body)
                    }
                };
                let response = send_provider_request(request, &self.credentials).await?;
                let body = read_complete_protocol_with_reasons(
                    response,
                    &self.credentials,
                    failure_message,
                    reason_kind,
                )
                .await
                .map_err(refine)?;
                let value = decode(&body.text).map_err(|refusal| refusal.at(body.status))?;
                Ok((Some(body.status), value))
            },
        )
        .await
    }

    fn settings(&self, endpoint: Endpoint, operation: &'static str) -> ExecutionSettings {
        ExecutionSettings {
            provider: ROUTE.name(),
            target: AttemptTarget::platform(PLATFORM.as_str(), operation),
            retry_policy: self.retry_policy,
            deadline: self.deadline,
            attempt_timeout: Duration::from_secs(self.config.timeout_seconds),
            verbose: false,
            timeout_message: endpoint.timeout_message(),
            model: None,
            transport: Some("http"),
            endpoint_host: None,
        }
    }
}

#[derive(Deserialize)]
struct SearchResponse {
    /// Gamma leaves the key out when nothing matches.
    events: Option<Vec<Value>>,
    pagination: Option<Pagination>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Pagination {
    has_more: bool,
}

/// Decodes a search page into items, the count of results skipped, and whether more pages
/// follow. A page whose every result was skipped is not a legitimate empty result.
fn decode_search(body: &str) -> Result<(Vec<PlatformItem>, usize, bool), Refusal> {
    let response = decode::<SearchResponse>(body)?;
    let pagination = response.pagination.ok_or_else(|| {
        Refusal::runtime("invalid Polymarket Gamma API search response: no `pagination`")
    })?;
    let events = response.events.unwrap_or_default();
    let total = events.len();
    let items = events
        .into_iter()
        .filter_map(|event| {
            let event = serde_json::from_value::<GammaEvent>(event).ok()?;
            Event::decode(event).ok().map(Event::into_search_item)
        })
        .collect::<Vec<_>>();
    if total > 0 && items.is_empty() {
        return Err(Refusal::runtime(format!(
            "Polymarket Gamma API returned {total} search results and none could be read"
        )));
    }
    let skipped = total - items.len();
    Ok((items, skipped, pagination.has_more))
}

fn decode<T: serde::de::DeserializeOwned>(body: &str) -> Result<T, Refusal> {
    serde_json::from_str(body).map_err(|error| {
        Refusal::runtime(format!("invalid Polymarket Gamma API response: {error}"))
    })
}

fn check_slug(reference: &PolymarketRef, returned: &str) -> Result<(), Refusal> {
    if returned == reference.slug() {
        return Ok(());
    }
    Err(Refusal::runtime(format!(
        "Polymarket Gamma API answered polymarket:{reference} with `{returned}`"
    )))
}

fn no_rules(reference: &PolymarketRef) -> Refusal {
    Refusal::new(
        AttemptErrorKind::Quality,
        format!("polymarket:{reference} has no resolution rules"),
    )
}

/// Words Gamma's 404 as the entry the user named being gone, since slugs can change.
fn not_found(mut failure: AttemptFailure, reference: &PolymarketRef) -> AttemptFailure {
    if failure.status == Some(404) {
        failure.message = format!(
            "Polymarket {} not found: polymarket:{reference}; slugs can change, search again",
            reference.kind().as_str()
        );
    }
    failure
}

/// Gamma's error body: `{"type": "validation error" | "not found error", "error": ...}`.
#[derive(Deserialize)]
struct GammaError {
    #[serde(rename = "type")]
    kind: Option<String>,
    error: Option<String>,
}

fn gamma_failure_message(body: &str, status: u16) -> String {
    match serde_json::from_str::<GammaError>(body)
        .ok()
        .and_then(|error| error.error)
    {
        Some(error) => format!("Polymarket Gamma API returned HTTP {status}: {error}"),
        None => format!("Polymarket Gamma API returned HTTP {status}"),
    }
}

/// forager builds every Gamma request, so a validation error is a forager defect, not a user
/// error.
fn gamma_reason_kind(body: &str) -> Option<AttemptErrorKind> {
    serde_json::from_str::<GammaError>(body)
        .ok()
        .and_then(|error| error.kind)
        .filter(|kind| kind == "validation error")
        .map(|_| AttemptErrorKind::Runtime)
}

#[derive(Deserialize)]
struct ClobError {
    error: String,
}

fn clob_failure_message(body: &str, status: u16) -> String {
    serde_json::from_str::<ClobError>(body).map_or_else(
        |_| format!("Polymarket CLOB API returned HTTP {status}"),
        |error| {
            format!(
                "Polymarket CLOB API returned HTTP {status}: {}",
                error.error
            )
        },
    )
}
