//! The `ssrn_crossref` route: SSRN paper metadata from the anonymous Crossref REST API, limited
//! to the SSRN DOI prefix.

use std::fmt::Write as _;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use reqwest::Client;
use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::catalog::{PlatformOperation, ProviderId};
use crate::config::HttpRouteRuntimeConfig;
use crate::credentials::CredentialPool;
use crate::net::{AttemptFailure, RetryPolicy, read_complete_protocol, send_provider_request};
use crate::providers::execution::{ExecutionSettings, execute_anonymous};
use crate::providers::markup::{fold_whitespace, markup_paragraphs, markup_text};
use crate::providers::route_declaration::{
    PlatformRoute, RouteAdapter, RouteContext, RouteDeclaration,
};
use crate::providers::shared::{
    acquire_window, other_platform_message, parameter_error, redacted_urls_message,
};
use crate::providers::{PlatformFetch, PlatformSearch, route_limiter};
use crate::rate_limit::RateLimiter;
use crate::types::{
    AttemptErrorKind, AttemptTarget, ContentDepth, Deadline, FullTextSource, Platform,
    PlatformFetchOutcome, PlatformFetchRequest, PlatformItem, PlatformItemData, PlatformRef,
    PlatformSearchOutcome, PlatformSearchRequest, ProviderError, SsrnItemData, SsrnRef,
};

const ROUTE: ProviderId = ProviderId::SsrnCrossref;
#[path = "ssrn_crossref_search.rs"]
mod search;
use search::search_support;

/// The route's declaration: Search and Fetch of the catalog.
pub(crate) const DECLARATION: RouteDeclaration<HttpRouteRuntimeConfig> = RouteDeclaration {
    build: |config, context| Box::new(SsrnCrossref::build(config, context)),
    adapter: RouteAdapter {
        search_support: Some(search_support),
        fetch_support: Some(fetch_support),
        login_page: None,
    },
};

impl PlatformRoute for SsrnCrossref {
    fn into_search(self: Box<Self>) -> Option<Box<dyn PlatformSearch>> {
        Some(self)
    }

    fn into_fetch(self: Box<Self>) -> Option<Box<dyn PlatformFetch>> {
        Some(self)
    }
}

impl PlatformSearch for SsrnCrossref {
    fn search<'a>(
        &'a self,
        request: &'a PlatformSearchRequest,
    ) -> Pin<Box<dyn Future<Output = Result<PlatformSearchOutcome, ProviderError>> + Send + 'a>>
    {
        Box::pin(SsrnCrossref::search(self, request))
    }
}

impl PlatformFetch for SsrnCrossref {
    fn fetch<'a>(
        &'a self,
        request: &'a PlatformFetchRequest,
    ) -> Pin<Box<dyn Future<Output = Result<PlatformFetchOutcome, ProviderError>> + Send + 'a>>
    {
        Box::pin(SsrnCrossref::fetch(self, request))
    }
}

/// Returns whether the route can fetch at the requested depth; it never sends a request.
pub(crate) fn fetch_support(request: &PlatformFetchRequest) -> Result<(), String> {
    match request.depth {
        ContentDepth::Metadata | ContentDepth::Abstract => Ok(()),
        depth => Err(format!(
            "{} cannot fetch at depth `{}`",
            ROUTE.name(),
            depth.as_str()
        )),
    }
}

pub(crate) struct SsrnCrossref {
    config: HttpRouteRuntimeConfig,
    client: Client,
    // The route needs no credentials; the empty pool only drives shared redaction.
    credentials: CredentialPool,
    limiter: RateLimiter,
    retry_policy: RetryPolicy,
    deadline: Deadline,
}

impl SsrnCrossref {
    /// Builds the route under the access policy its registration declares.
    fn build(config: HttpRouteRuntimeConfig, context: RouteContext) -> Self {
        Self {
            config,
            client: context.client,
            credentials: CredentialPool::new(ROUTE.name(), Vec::new()),
            limiter: route_limiter(ROUTE)
                .expect("ssrn_crossref registration declares an access policy"),
            retry_policy: context.retry_policy,
            deadline: context.deadline,
        }
    }

    /// Reads the Crossref record of the paper's SSRN DOI. A missing abstract fails only when
    /// the request asks for one.
    pub(crate) async fn fetch(
        &self,
        request: &PlatformFetchRequest,
    ) -> Result<PlatformFetchOutcome, ProviderError> {
        let PlatformRef::Ssrn(requested) = &request.reference else {
            return Err(parameter_error(other_platform_message(
                ROUTE,
                request.reference.platform(),
            )));
        };
        let url = format!("{}/works/{}", self.base_url(), requested.doi());
        let url = &url;
        let execution = execute_anonymous(
            self.settings(PlatformOperation::Fetch),
            move |deadline| async move {
                let (status, work) = self
                    .send_once::<Work>(url, &[], "work", deadline)
                    .await
                    .map_err(|failure| match failure.status {
                        Some(404) => AttemptFailure {
                            message: format!("SSRN paper not found in Crossref: ssrn:{requested}"),
                            ..failure
                        },
                        _ => failure,
                    })?;
                let failure = |kind, message| AttemptFailure {
                    kind,
                    status,
                    message,
                };
                let item = select_paper(work, requested)
                    .map_err(|message| failure(AttemptErrorKind::Runtime, message))?;
                if request.depth == ContentDepth::Abstract && item.depth != ContentDepth::Abstract {
                    return Err(failure(
                        AttemptErrorKind::Quality,
                        format!("Crossref has no abstract for ssrn:{requested}"),
                    ));
                }
                Ok((status, item))
            },
        )
        .await?;
        Ok(PlatformFetchOutcome {
            item: execution.value,
            content_source: FullTextSource::Urls(Vec::new()),
            attempts: execution.attempts,
            diagnostic: execution.diagnostic,
        })
    }

    fn base_url(&self) -> &str {
        self.config.url.trim_end_matches('/')
    }

    fn settings(&self, operation: PlatformOperation) -> ExecutionSettings {
        ExecutionSettings {
            provider: ROUTE.name(),
            target: AttemptTarget::platform(Platform::Ssrn.as_str(), operation.as_str()),
            retry_policy: self.retry_policy,
            deadline: self.deadline,
            attempt_timeout: Duration::from_secs(self.config.timeout_seconds),
            verbose: false,
            timeout_message: "Crossref request timed out",
            model: None,
            transport: Some("http"),
            endpoint_host: None,
        }
    }

    /// Sends one paced GET and decodes the `message` of the expected Crossref message type.
    async fn send_once<T: DeserializeOwned>(
        &self,
        url: &str,
        query: &[(&str, String)],
        message_type: &str,
        deadline: Deadline,
    ) -> Result<(Option<u16>, T), AttemptFailure> {
        let _permit = acquire_window(&self.limiter, deadline).await?;
        let request = self.client.get(url).query(query);
        let response = send_provider_request(request, &self.credentials).await?;
        let body = read_complete_protocol(response, &self.credentials, failure_message).await?;
        decode_message(&body.text, message_type)
            .map(|message| (Some(body.status), message))
            .map_err(|message| AttemptFailure {
                kind: AttemptErrorKind::Runtime,
                status: Some(body.status),
                message: redacted_urls_message(&message, &self.credentials),
            })
    }
}

/// Returns the item of the Crossref record, which must carry the requested SSRN DOI.
fn select_paper(work: Work, requested: &SsrnRef) -> Result<PlatformItem, String> {
    let returned = work.doi.clone();
    match work.into_item() {
        Ok(item) if matches!(&item.reference, PlatformRef::Ssrn(reference) if reference == requested) => {
            Ok(item)
        }
        _ => Err(format!(
            "Crossref returned DOI {returned} for ssrn:{requested}"
        )),
    }
}

fn failure_message(body: &str, status: u16) -> String {
    serde_json::from_str::<ValidationFailure>(body)
        .ok()
        .and_then(|failure| failure.message.into_iter().next())
        .map_or_else(
            || format!("Crossref returned HTTP {status}"),
            |detail| format!("Crossref rejected the request: {}", detail.message),
        )
}

fn decode_message<T: DeserializeOwned>(body: &str, expected: &str) -> Result<T, String> {
    let envelope = serde_json::from_str::<Envelope>(body)
        .map_err(|error| format!("invalid Crossref response: {error}"))?;
    let message_type = envelope.message_type.as_deref().unwrap_or("none");
    if message_type != expected {
        return Err(format!(
            "Crossref returned message type `{message_type}`, not `{expected}`"
        ));
    }
    serde_json::from_value(envelope.message)
        .map_err(|error| format!("invalid Crossref {expected}: {error}"))
}

#[derive(Deserialize)]
struct Envelope {
    #[serde(rename = "message-type")]
    message_type: Option<String>,
    #[serde(default)]
    message: serde_json::Value,
}

#[derive(Deserialize)]
struct ValidationFailure {
    message: Vec<ValidationDetail>,
}

#[derive(Deserialize)]
struct ValidationDetail {
    message: String,
}

#[derive(Deserialize)]
struct WorkList {
    #[serde(rename = "total-results")]
    total_results: u64,
    #[serde(default)]
    items: Vec<Work>,
}

#[derive(Deserialize)]
struct Work {
    #[serde(rename = "DOI")]
    doi: String,
    #[serde(default)]
    title: Vec<String>,
    #[serde(default)]
    author: Vec<Author>,
    #[serde(rename = "abstract")]
    abstract_text: Option<String>,
    published: Option<DateParts>,
    #[serde(rename = "type")]
    kind: Option<String>,
    created: Option<Timestamp>,
}

#[derive(Deserialize)]
struct Author {
    given: Option<String>,
    family: Option<String>,
    /// The name of an organization author.
    name: Option<String>,
}

#[derive(Deserialize)]
struct DateParts {
    #[serde(rename = "date-parts", default)]
    date_parts: Vec<Vec<Option<i64>>>,
}

#[derive(Deserialize)]
struct Timestamp {
    #[serde(rename = "date-time")]
    date_time: Option<String>,
}

impl Work {
    /// Maps the record to an item, or returns its DOI when the DOI is not an SSRN DOI.
    fn into_item(self) -> Result<PlatformItem, String> {
        let Some(reference) = SsrnRef::from_doi(&self.doi) else {
            return Err(self.doi);
        };
        let abstract_text = self
            .abstract_text
            .as_deref()
            .map(|markup| markup_paragraphs(markup).join("\n\n"))
            .filter(|text| !text.is_empty());
        Ok(PlatformItem {
            url: reference.canonical_url(),
            reference: PlatformRef::Ssrn(reference),
            depth: if abstract_text.is_some() {
                ContentDepth::Abstract
            } else {
                ContentDepth::Metadata
            },
            title: Some(
                self.title
                    .iter()
                    .map(|title| markup_text(title))
                    .find(|title| !title.is_empty())
                    .unwrap_or_default(),
            ),
            authors: self
                .author
                .iter()
                .filter_map(Author::display_name)
                .collect(),
            published: self.published.as_ref().and_then(DateParts::to_date),
            data: PlatformItemData::Ssrn(SsrnItemData {
                abstract_text,
                doi: Some(self.doi),
                crossref_type: self.kind,
                crossref_created: self.created.and_then(|created| created.date_time),
            }),
        })
    }
}

impl Author {
    fn display_name(&self) -> Option<String> {
        let name = match (&self.given, &self.family) {
            (None, None) => self.name.clone()?,
            (given, family) => [given, family]
                .into_iter()
                .flatten()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(" "),
        };
        let name = fold_whitespace(&name);
        (!name.is_empty()).then_some(name)
    }
}

impl DateParts {
    /// Formats the first date at the precision Crossref reports: `2012`, `2012-04`, or
    /// `2012-04-19`. A year alone is never padded to a full date.
    fn to_date(&self) -> Option<String> {
        let mut parts = self.date_parts.first()?.iter().map_while(|part| *part);
        let mut date = format!("{:04}", parts.next()?);
        for part in parts.take(2) {
            let _ = write!(date, "-{part:02}");
        }
        Some(date)
    }
}

#[cfg(test)]
#[path = "ssrn_crossref_tests.rs"]
mod tests;
