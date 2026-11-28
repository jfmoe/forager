//! The `arxiv_api` route: arXiv Query API requests, Atom feed decoding, and the HTML
//! availability probe that orders full-text URLs.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use reqwest::{Client, StatusCode, Url};
use serde::Deserialize;

use crate::catalog::{PlatformOperation, ProviderId};
use crate::config::HttpRouteRuntimeConfig;
use crate::credentials::CredentialPool;
use crate::net::{
    AttemptFailure, RetryPolicy, error_kind_for_status, read_complete_protocol,
    send_provider_request,
};
use crate::providers::execution::{ExecutionOutcome, ExecutionSettings, execute_anonymous};
use crate::providers::route_declaration::{
    PlatformRoute, RouteAdapter, RouteContext, RouteDeclaration,
};
use crate::providers::shared::{
    acquire_window, other_platform_message, parameter_error, redacted_urls_message,
};
use crate::providers::{PlatformFetch, PlatformSearch, route_limiter};
use crate::rate_limit::RateLimiter;
use crate::types::{
    ArxivItemData, ArxivRef, ArxivSearchOptions, ArxivSort, AttemptErrorKind, AttemptTarget,
    ContentDepth, Deadline, FullTextSource, Platform, PlatformFetchOutcome, PlatformFetchRequest,
    PlatformItem, PlatformItemData, PlatformRef, PlatformSearchOptions, PlatformSearchOutcome,
    PlatformSearchRequest, ProviderAttempt, ProviderError,
};

const ROUTE: ProviderId = ProviderId::ArxivApi;
const ERROR_ENTRY_PATH: &str = "arxiv.org/api/errors";
// arXiv submission dates start in 1991; the upper bound only closes an open range.
const EARLIEST_SUBMISSION: &str = "199101010000";
const LATEST_SUBMISSION: &str = "999912312359";

/// The route's declaration: Search and Fetch of the catalog.
pub(crate) const DECLARATION: RouteDeclaration<HttpRouteRuntimeConfig> = RouteDeclaration {
    build: |config, context| Box::new(ArxivApi::build(config, context)),
    adapter: RouteAdapter {
        search_support: Some(search_support),
        fetch_support: Some(fetch_support),
        login_page: None,
    },
};

impl PlatformRoute for ArxivApi {
    fn into_search(self: Box<Self>) -> Option<Box<dyn PlatformSearch>> {
        Some(self)
    }

    fn into_fetch(self: Box<Self>) -> Option<Box<dyn PlatformFetch>> {
        Some(self)
    }
}

impl PlatformSearch for ArxivApi {
    fn search<'a>(
        &'a self,
        request: &'a PlatformSearchRequest,
    ) -> Pin<Box<dyn Future<Output = Result<PlatformSearchOutcome, ProviderError>> + Send + 'a>>
    {
        Box::pin(ArxivApi::search(self, request))
    }
}

impl PlatformFetch for ArxivApi {
    fn fetch<'a>(
        &'a self,
        request: &'a PlatformFetchRequest,
    ) -> Pin<Box<dyn Future<Output = Result<PlatformFetchOutcome, ProviderError>> + Send + 'a>>
    {
        Box::pin(ArxivApi::fetch(self, request))
    }
}

/// Returns whether the route can run the request with every explicit option; it never sends
/// a request. The route applies every arXiv option; only a page position it did not issue fails.
pub(crate) fn search_support(request: &PlatformSearchRequest) -> Result<(), String> {
    match request.options {
        PlatformSearchOptions::Arxiv(_) => page_start(request.page.as_deref()).map(|_| ()),
        PlatformSearchOptions::Ssrn(_)
        | PlatformSearchOptions::Scholar(_)
        | PlatformSearchOptions::Xiaohongshu(_)
        | PlatformSearchOptions::X(_)
        | PlatformSearchOptions::Hn(_)
        | PlatformSearchOptions::Polymarket(_)
        | PlatformSearchOptions::Reddit(_)
        | PlatformSearchOptions::Youtube(_) => {
            Err(other_platform_message(ROUTE, request.options.platform()))
        }
    }
}

/// Returns whether the route can fetch at the requested depth; it never sends a request.
pub(crate) fn fetch_support(request: &PlatformFetchRequest) -> Result<(), String> {
    match request.depth {
        ContentDepth::Abstract | ContentDepth::FullText => Ok(()),
        depth => Err(format!(
            "{} cannot fetch at depth `{}`",
            ROUTE.name(),
            depth.as_str()
        )),
    }
}

pub(crate) struct ArxivApi {
    config: HttpRouteRuntimeConfig,
    client: Client,
    // The route needs no credentials; the empty pool only drives shared redaction.
    credentials: CredentialPool,
    limiter: RateLimiter,
    retry_policy: RetryPolicy,
    deadline: Deadline,
}

struct FeedPage {
    items: Vec<PlatformItem>,
    total_results: u64,
}

impl ArxivApi {
    /// Builds the route under the access policy its registration declares.
    fn build(config: HttpRouteRuntimeConfig, context: RouteContext) -> Self {
        Self {
            config,
            client: context.client,
            credentials: CredentialPool::new(ROUTE.name(), Vec::new()),
            limiter: route_limiter(ROUTE)
                .expect("arxiv_api registration declares an access policy"),
            retry_policy: context.retry_policy,
            deadline: context.deadline,
        }
    }

    pub(crate) async fn search(
        &self,
        request: &PlatformSearchRequest,
    ) -> Result<PlatformSearchOutcome, ProviderError> {
        let PlatformSearchOptions::Arxiv(options) = &request.options else {
            return Err(parameter_error(other_platform_message(
                ROUTE,
                request.options.platform(),
            )));
        };
        let start = page_start(request.page.as_deref()).map_err(parameter_error)?;
        let query = [
            (
                "search_query",
                search_query(&request.query, options).map_err(parameter_error)?,
            ),
            ("start", start.to_string()),
            ("max_results", request.limit.to_string()),
            ("sortBy", sort_by(options.sort).to_owned()),
            ("sortOrder", "descending".to_owned()),
        ];
        let query = &query;
        let execution = execute_anonymous(
            self.settings(PlatformOperation::Search, self.retry_policy, self.deadline),
            move |deadline| async move { self.send_once(query, deadline).await },
        )
        .await?;
        let FeedPage {
            items,
            total_results,
        } = execution.value;
        let next_start = start.saturating_add(items.len() as u64);
        let has_next_page = items.len() == usize::from(request.limit) && next_start < total_results;
        Ok(PlatformSearchOutcome {
            items,
            next_page: has_next_page.then(|| next_start.to_string()),
            attempts: execution.attempts,
            diagnostic: execution.diagnostic,
        })
    }

    /// Reads the metadata of the requested paper version and, at full-text depth, orders the
    /// full-text URLs of the version arXiv returned.
    pub(crate) async fn fetch(
        &self,
        request: &PlatformFetchRequest,
    ) -> Result<PlatformFetchOutcome, ProviderError> {
        let PlatformRef::Arxiv(requested) = &request.reference else {
            return Err(parameter_error(other_platform_message(
                ROUTE,
                request.reference.platform(),
            )));
        };
        let query = [
            ("id_list", requested.to_string()),
            ("max_results", "1".to_owned()),
        ];
        let query = &query;
        let ExecutionOutcome {
            value: (version, item),
            mut attempts,
            diagnostic,
        } = execute_anonymous(
            self.settings(PlatformOperation::Fetch, self.retry_policy, self.deadline),
            move |deadline| async move {
                let (status, page) = self.send_once(query, deadline).await?;
                select_paper(page.items, requested)
                    .map(|paper| (status, paper))
                    .map_err(|(kind, message)| AttemptFailure {
                        kind,
                        status,
                        message,
                    })
            },
        )
        .await?;
        let content_source = if request.depth == ContentDepth::FullText {
            match self.html_availability(&version).await {
                Ok((availability, mut probe_attempts)) => {
                    attempts.append(&mut probe_attempts);
                    FullTextSource::Urls(content_urls(&version, availability))
                }
                Err(mut error) => {
                    attempts.append(&mut error.attempts);
                    error.attempts = attempts;
                    return Err(error);
                }
            }
        } else {
            FullTextSource::Urls(Vec::new())
        };
        Ok(PlatformFetchOutcome {
            item,
            content_source,
            attempts,
            diagnostic,
        })
    }

    /// Asks the Query API host whether the version has an official HTML rendering. The probe
    /// never retries: an unknown answer already falls back to trying HTML first. A pacing
    /// failure is terminal, because the probe must not send outside the access policy.
    async fn html_availability(
        &self,
        version: &ArxivRef,
    ) -> Result<(HtmlAvailability, Vec<ProviderAttempt>), ProviderError> {
        let Some(url) = html_probe_url(&self.config.url, version) else {
            return Ok((HtmlAvailability::Unknown, Vec::new()));
        };
        let url = &url;
        let pacing_failed = &AtomicBool::new(false);
        let single_attempt = RetryPolicy::new(1, 1.0, Duration::ZERO);
        let result = execute_anonymous(
            self.settings(PlatformOperation::Fetch, single_attempt, self.deadline),
            move |deadline| async move { self.probe_once(url, deadline, pacing_failed).await },
        )
        .await;
        match result {
            Ok(outcome) => Ok((outcome.value, outcome.attempts)),
            Err(error) if pacing_failed.load(Ordering::Relaxed) => Err(error),
            Err(error) => Ok((HtmlAvailability::Unknown, error.attempts)),
        }
    }

    async fn probe_once(
        &self,
        url: &Url,
        deadline: Deadline,
        pacing_failed: &AtomicBool,
    ) -> Result<(Option<u16>, HtmlAvailability), AttemptFailure> {
        let _permit = acquire_window(&self.limiter, deadline)
            .await
            .inspect_err(|_| pacing_failed.store(true, Ordering::Relaxed))?;
        let response =
            send_provider_request(self.client.head(url.clone()), &self.credentials).await?;
        let status = response.status();
        match status {
            StatusCode::NOT_FOUND => Ok((Some(status.as_u16()), HtmlAvailability::Absent)),
            status if status.is_success() => Ok((Some(status.as_u16()), HtmlAvailability::Present)),
            status => Err(AttemptFailure {
                kind: error_kind_for_status(status, ""),
                status: Some(status.as_u16()),
                message: format!("arXiv HTML probe returned HTTP {}", status.as_u16()),
            }),
        }
    }

    fn settings(
        &self,
        operation: PlatformOperation,
        retry_policy: RetryPolicy,
        deadline: Deadline,
    ) -> ExecutionSettings {
        ExecutionSettings {
            provider: ROUTE.name(),
            target: AttemptTarget::platform(Platform::Arxiv.as_str(), operation.as_str()),
            retry_policy,
            deadline,
            attempt_timeout: Duration::from_secs(self.config.timeout_seconds),
            timeout_message: "arXiv API request timed out",
            model: None,
            transport: Some("http"),
            endpoint_host: None,
        }
    }

    async fn send_once(
        &self,
        query: &[(&str, String)],
        deadline: Deadline,
    ) -> Result<(Option<u16>, FeedPage), AttemptFailure> {
        let _permit = acquire_window(&self.limiter, deadline).await?;
        let request = self.client.get(&self.config.url).query(query);
        let response = send_provider_request(request, &self.credentials).await?;
        let body = read_complete_protocol(response, &self.credentials, failure_message).await?;
        let status = body.status;
        let page = read_feed(&body.text).map_err(|(kind, message)| AttemptFailure {
            kind,
            status: Some(status),
            message: redacted_urls_message(&message, &self.credentials),
        })?;
        Ok((Some(status), page))
    }
}

/// Reads a successful Query API body: an error entry is the request's fault, and a body that
/// is not an arXiv feed, or an entry that cannot be mapped, is a runtime failure.
fn read_feed(text: &str) -> Result<FeedPage, (AttemptErrorKind, String)> {
    let feed = decode_feed(text).map_err(|error| (AttemptErrorKind::Runtime, error))?;
    if let Some(message) = feed.error_message() {
        return Err((AttemptErrorKind::Parameter, message));
    }
    let total_results = feed.total_results;
    let items = feed
        .entries
        .into_iter()
        .map(Entry::into_item)
        .collect::<Result<_, _>>()
        .map_err(|error| (AttemptErrorKind::Runtime, error))?;
    Ok(FeedPage {
        items,
        total_results,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HtmlAvailability {
    Present,
    Absent,
    Unknown,
}

/// Returns the paper the Query API returned for `requested`, with the version it carries. An
/// empty feed means arXiv has no such paper or version.
fn select_paper(
    items: Vec<PlatformItem>,
    requested: &ArxivRef,
) -> Result<(ArxivRef, PlatformItem), (AttemptErrorKind, String)> {
    let Some(item) = items.into_iter().next() else {
        return Err((
            AttemptErrorKind::Parameter,
            format!("arXiv item not found: arxiv:{requested}"),
        ));
    };
    match &item.reference {
        PlatformRef::Arxiv(returned)
            if returned.id() == requested.id()
                && requested
                    .version()
                    .is_none_or(|version| returned.version() == Some(version)) =>
        {
            Ok((returned.clone(), item))
        }
        returned => Err((
            AttemptErrorKind::Runtime,
            format!("arXiv returned {returned} for arxiv:{requested}"),
        )),
    }
}

/// Returns the HTML probe URL on the Query API host; the export mirror serves the same pages.
fn html_probe_url(api_url: &str, version: &ArxivRef) -> Option<Url> {
    Url::parse(api_url)
        .ok()?
        .join(&format!("/html/{version}"))
        .ok()
}

/// Orders the full-text URLs: the abstract page is never full text, and a version without
/// HTML goes straight to its PDF.
fn content_urls(version: &ArxivRef, availability: HtmlAvailability) -> Vec<String> {
    match availability {
        HtmlAvailability::Absent => vec![version.pdf_url()],
        HtmlAvailability::Present | HtmlAvailability::Unknown => {
            vec![version.html_url(), version.pdf_url()]
        }
    }
}

fn page_start(page: Option<&str>) -> Result<u64, String> {
    page.map_or(Ok(0), |page| {
        page.parse()
            .map_err(|_| format!("invalid arXiv page position `{page}`"))
    })
}

/// Builds the `search_query` value: every query term, category group, phrase, and date range
/// must match. Each term is quoted, so arXiv query syntax in it stays literal.
fn search_query(query: &str, options: &ArxivSearchOptions) -> Result<String, String> {
    let mut clauses = ArxivSearchOptions::query_terms(query)?
        .into_iter()
        .map(|term| format!("all:\"{term}\""))
        .collect::<Vec<_>>();
    match options.categories.as_slice() {
        [] => {}
        [category] => clauses.push(format!("cat:{category}")),
        categories => clauses.push(format!(
            "({})",
            categories
                .iter()
                .map(|category| format!("cat:{category}"))
                .collect::<Vec<_>>()
                .join(" OR ")
        )),
    }
    for (field, phrase) in [("au", &options.author), ("ti", &options.title)] {
        if let Some(phrase) = phrase {
            let words = ArxivSearchOptions::words(phrase).collect::<Vec<_>>();
            clauses.push(format!("{field}:\"{}\"", words.join(" ")));
        }
    }
    if options.submitted_from.is_some() || options.submitted_to.is_some() {
        let from = options.submitted_from.map_or_else(
            || EARLIEST_SUBMISSION.to_owned(),
            |date| format!("{}0000", date.format("%Y%m%d")),
        );
        let to = options.submitted_to.map_or_else(
            || LATEST_SUBMISSION.to_owned(),
            |date| format!("{}2359", date.format("%Y%m%d")),
        );
        clauses.push(format!("submittedDate:[{from} TO {to}]"));
    }
    Ok(clauses.join(" AND "))
}

fn sort_by(sort: ArxivSort) -> &'static str {
    match sort {
        ArxivSort::Relevance => "relevance",
        ArxivSort::Submitted => "submittedDate",
        ArxivSort::Updated => "lastUpdatedDate",
    }
}

fn failure_message(body: &str, status: u16) -> String {
    decode_feed(body)
        .ok()
        .and_then(|feed| feed.error_message())
        .unwrap_or_else(|| format!("arXiv returned HTTP {status}"))
}

fn decode_feed(body: &str) -> Result<Feed, String> {
    quick_xml::de::from_str(body).map_err(|error| format!("invalid arXiv Atom feed: {error}"))
}

// quick-xml matches element local names, so the `opensearch:` and `arxiv:` extension elements
// are named without their prefixes. Every arXiv feed carries `totalResults`, so requiring it
// rejects a successful response that is not an arXiv feed.
#[derive(Deserialize)]
struct Feed {
    #[serde(rename = "totalResults")]
    total_results: u64,
    #[serde(rename = "entry", default)]
    entries: Vec<Entry>,
}

impl Feed {
    fn error_message(&self) -> Option<String> {
        self.entries
            .iter()
            .find(|entry| entry.id.contains(ERROR_ENTRY_PATH))
            .map(|entry| {
                let message = entry.summary.as_deref().map(fold_whitespace);
                format!(
                    "arXiv rejected the request: {}",
                    message.as_deref().unwrap_or("unknown error")
                )
            })
    }
}

#[derive(Deserialize)]
struct Entry {
    id: String,
    title: Option<String>,
    summary: Option<String>,
    published: Option<String>,
    updated: Option<String>,
    #[serde(rename = "author", default)]
    authors: Vec<Author>,
    #[serde(rename = "link", default)]
    links: Vec<Link>,
    #[serde(rename = "category", default)]
    categories: Vec<Category>,
    primary_category: Option<Category>,
    doi: Option<String>,
    journal_ref: Option<String>,
    comment: Option<String>,
}

#[derive(Deserialize)]
struct Author {
    name: String,
}

#[derive(Deserialize)]
struct Link {
    #[serde(rename = "@href")]
    href: String,
    #[serde(rename = "@title")]
    title: Option<String>,
}

#[derive(Deserialize)]
struct Category {
    #[serde(rename = "@term")]
    term: String,
}

impl Entry {
    fn into_item(self) -> Result<PlatformItem, String> {
        let reference = ArxivRef::parse(&self.id)
            .map(PlatformRef::Arxiv)
            .map_err(|_| format!("arXiv entry has an unrecognized id `{}`", self.id))?;
        let pdf_url = self
            .links
            .into_iter()
            .find(|link| link.title.as_deref() == Some("pdf"))
            .map(|link| link.href);
        Ok(PlatformItem {
            url: reference.canonical_url(),
            reference,
            depth: ContentDepth::Abstract,
            title: Some(
                self.title
                    .as_deref()
                    .map(fold_whitespace)
                    .unwrap_or_default(),
            ),
            authors: self
                .authors
                .iter()
                .map(|author| fold_whitespace(&author.name))
                .filter(|name| !name.is_empty())
                .collect(),
            published: non_empty(self.published),
            data: PlatformItemData::Arxiv(ArxivItemData {
                abstract_text: self
                    .summary
                    .as_deref()
                    .map(str::trim)
                    .unwrap_or_default()
                    .to_owned(),
                updated: non_empty(self.updated),
                primary_category: self.primary_category.map(|category| category.term),
                categories: self
                    .categories
                    .into_iter()
                    .map(|category| category.term)
                    .collect(),
                doi: non_empty(self.doi),
                journal_ref: self.journal_ref.as_deref().map(fold_whitespace),
                comment: self.comment.as_deref().map(fold_whitespace),
                pdf_url,
            }),
        })
    }
}

fn non_empty(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn fold_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
#[path = "arxiv_api_tests.rs"]
mod tests;
