//! The `polymarket_api` route: one anonymous route over Polymarket's official public read APIs
//! (ADR 0032).
//!
//! Gamma finds events and holds their metadata and cached quotes; the CLOB holds the live order
//! books; the Data API holds price histories. Search prices from Gamma alone in one request.
//! Fetch reads the event or market from Gamma and the live books of its open markets from the
//! CLOB; the CLOB only enriches the result, so when it fails the fetch prices from Gamma's cache
//! and one diagnostic says so. History reads the market from Gamma and its series from the Data
//! API, the only source of it. Every request to any endpoint follows the route's one access
//! policy.

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
    PlatformSearchOutcome, PlatformSearchRequest, PolymarketKind, PolymarketRef, ProviderError,
};

#[path = "polymarket_api_gamma.rs"]
mod gamma;
#[path = "polymarket_api_history.rs"]
mod history;
#[path = "polymarket_api_quotes.rs"]
mod quotes;

use gamma::{Entry, Event, GammaEvent, Market, Unreadable};

const ROUTE: ProviderId = ProviderId::PolymarketApi;
const PLATFORM: Platform = Platform::Polymarket;
/// Gamma search refuses a page past 100 with HTTP 422 (measured 2026-10-24).
const MAX_SEARCH_PAGE: u64 = 100;

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

/// The endpoints of the route.
#[derive(Clone, Copy)]
enum Endpoint {
    Gamma,
    Clob,
    Data,
}

impl Endpoint {
    /// The endpoint's base URL, without a trailing `/`.
    fn base_url(self, config: &PolymarketRouteRuntimeConfig) -> &str {
        match self {
            Self::Gamma => &config.url,
            Self::Clob => &config.clob_url,
            Self::Data => &config.data_url,
        }
        .trim_end_matches('/')
    }

    const fn timeout_message(self) -> &'static str {
        match self {
            Self::Gamma => "Polymarket Gamma API request timed out",
            Self::Clob => "Polymarket CLOB API request timed out",
            Self::Data => "Polymarket Data API request timed out",
        }
    }

    fn failure_message(self) -> fn(&str, u16) -> String {
        match self {
            Self::Gamma => |body, status| failure_message("Gamma", body, status),
            Self::Clob => |body, status| failure_message("CLOB", body, status),
            Self::Data => |body, status| failure_message("Data", body, status),
        }
    }

    fn reason_kind(self) -> fn(&str) -> Option<AttemptErrorKind> {
        match self {
            Self::Gamma => gamma_reason_kind,
            Self::Clob | Self::Data => |_| None,
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
        search_support(request).map_err(parameter_error)?;
        let PlatformSearchOptions::Polymarket(options) = &request.options else {
            unreachable!("the support check accepts only Polymarket options")
        };
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
        let entry = self.read_entry(reference, PlatformOperation::Fetch.as_str(), |entry| {
            if depth == ContentDepth::Abstract && entry.rules().is_empty() {
                return Err(no_rules(reference));
            }
            Ok(entry)
        });
        let item = match trail.required(entry.await)? {
            Entry::Event(event) => {
                let quotes = self.quote(&event.markets, &mut trail).await;
                event.into_item(depth, &quotes)
            }
            Entry::Market(market) => {
                let quotes = self.quote(std::slice::from_ref(&market), &mut trail).await;
                market.into_item(depth, &quotes)
            }
        };
        Ok(PlatformFetchOutcome {
            item,
            content_source: FullTextSource::Urls(Vec::new()),
            diagnostic: trail.diagnostic(),
            attempts: trail.attempts,
        })
    }

    /// Reads the event or market a ref names from Gamma's slug lookup; `then` turns the entry
    /// into the operation's value inside the same attempt, so its refusal is that attempt's
    /// failure. A 404 says the slug may have changed.
    async fn read_entry<T>(
        &self,
        reference: &PolymarketRef,
        operation: &'static str,
        then: impl Fn(Entry) -> Result<T, Refusal>,
    ) -> Result<ExecutionOutcome<T>, ProviderError> {
        let path = match reference.kind() {
            PolymarketKind::Event => format!("events/slug/{}", reference.slug()),
            PolymarketKind::Market => format!("markets/slug/{}", reference.slug()),
        };
        self.send_with(
            Endpoint::Gamma,
            Call::Get {
                path: &path,
                query: &[],
            },
            operation,
            |body| {
                let entry = match reference.kind() {
                    PolymarketKind::Event => Entry::Event(Event::decode(decode(body)?)?),
                    PolymarketKind::Market => Entry::Market(Market::decode(decode(body)?)?),
                };
                check_slug(reference, entry.slug())?;
                then(entry)
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
        let base = endpoint.base_url(&self.config);
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
    events: Option<Vec<GammaEvent>>,
    pagination: Option<Pagination>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Pagination {
    has_more: bool,
}

/// Decodes a search page into items, the count of results skipped, and whether more pages
/// follow. Only a result that names nothing forager can address, with an invalid slug or no
/// market list, is skipped; any other malformed result fails the page. A page whose every
/// result was skipped is not a legitimate empty result.
fn decode_search(body: &str) -> Result<(Vec<PlatformItem>, usize, bool), Refusal> {
    let response = decode::<SearchResponse>(body)?;
    let pagination = response.pagination.ok_or_else(|| {
        Refusal::runtime("invalid Polymarket Gamma API search response: no `pagination`")
    })?;
    let events = response.events.unwrap_or_default();
    let total = events.len();
    let mut items = Vec::with_capacity(total);
    for event in events {
        match Event::decode(event) {
            Ok(event) => items.push(event.into_search_item()),
            Err(Unreadable::Unaddressable(_)) => {}
            Err(malformed @ Unreadable::Malformed(_)) => return Err(malformed.into()),
        }
    }
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

/// An error body of any of the three APIs: `{"error": ...}`, which Gamma extends with
/// `"type": "validation error" | "not found error"`.
#[derive(Deserialize)]
struct ApiError {
    #[serde(rename = "type")]
    kind: Option<String>,
    error: Option<String>,
}

/// Words a failed response of the `api` API, with its `error` when the body has one.
fn failure_message(api: &str, body: &str, status: u16) -> String {
    match serde_json::from_str::<ApiError>(body)
        .ok()
        .and_then(|error| error.error)
    {
        Some(error) => format!("Polymarket {api} API returned HTTP {status}: {error}"),
        None => format!("Polymarket {api} API returned HTTP {status}"),
    }
}

/// forager builds every Gamma request, so a validation error is a forager defect, not a user
/// error.
fn gamma_reason_kind(body: &str) -> Option<AttemptErrorKind> {
    serde_json::from_str::<ApiError>(body)
        .ok()
        .and_then(|error| error.kind)
        .filter(|kind| kind == "validation error")
        .map(|_| AttemptErrorKind::Runtime)
}
