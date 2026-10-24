//! The `hn_api` route: one route over two official Hacker News endpoints (ADR 0027).
//!
//! Algolia's HN Search API searches items and returns a whole discussion tree in one request;
//! the official Firebase API knows the rank Hacker News shows and the live counts. Firebase only
//! enriches what Algolia returns: when an enrichment request fails, the Algolia result stands
//! and one diagnostic says what is missing. The story lists exist only on Firebase, so a list
//! request failure fails `stories`. Every request to either endpoint follows the route's one
//! access policy.

use std::time::Duration;

use chrono::DateTime;
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::catalog::{PlatformOperation, ProviderId};
use crate::config::HnRouteRuntimeConfig;
use crate::credentials::CredentialPool;
use crate::net::{AttemptFailure, RetryPolicy, read_complete_protocol, send_provider_request};
use crate::providers::execution::{ExecutionOutcome, ExecutionSettings, execute_anonymous};
use crate::providers::shared::{
    Trail, acquire_window, other_platform_message, parameter_error, utc_second,
};
use crate::rate_limit::RateLimiter;
use crate::types::{
    AttemptErrorKind, AttemptTarget, ContentDepth, Deadline, FullTextSource, HnItemData,
    HnItemType, HnRef, Platform, PlatformFetchOutcome, PlatformFetchRequest, PlatformItem,
    PlatformItemData, PlatformRef, ProviderError,
};

#[path = "hn_markup.rs"]
mod hn_markup;
#[path = "hn_api_search.rs"]
mod search;
#[path = "hn_api_thread.rs"]
mod thread;

pub(crate) use search::search_support;

use hn_markup::hn_markdown;
use thread::{ThreadNode, ThreadRoot};

const ROUTE: ProviderId = ProviderId::HnApi;
const PLATFORM: Platform = Platform::Hn;

/// Returns whether the route can fetch at the requested depth; it never sends a request. The
/// item type is unknown until the item is read, so a comment asked for its full text fails
/// later.
pub(crate) fn fetch_support(request: &PlatformFetchRequest) -> Result<(), String> {
    if !matches!(request.reference, PlatformRef::Hn(_)) {
        return Err(other_platform_message(ROUTE, request.reference.platform()));
    }
    match request.depth {
        ContentDepth::Thread | ContentDepth::Metadata | ContentDepth::FullText => Ok(()),
        depth => Err(format!(
            "{} cannot fetch at depth `{}`",
            ROUTE.name(),
            depth.as_str()
        )),
    }
}

/// The two endpoints of the route.
#[derive(Clone, Copy)]
enum Endpoint {
    Algolia,
    Firebase,
}

impl Endpoint {
    const fn label(self) -> &'static str {
        match self {
            Self::Algolia => "HN Search API",
            Self::Firebase => "HN Firebase API",
        }
    }

    const fn timeout_message(self) -> &'static str {
        match self {
            Self::Algolia => "HN Search API request timed out",
            Self::Firebase => "HN Firebase API request timed out",
        }
    }
}

pub(crate) struct HnApi {
    config: HnRouteRuntimeConfig,
    client: Client,
    // The route needs no credentials; the empty pool only drives shared redaction.
    credentials: CredentialPool,
    limiter: RateLimiter,
    retry_policy: RetryPolicy,
    deadline: Deadline,
}

impl HnApi {
    pub(crate) fn new(
        config: HnRouteRuntimeConfig,
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

    /// Reads one item at the requested depth.
    pub(crate) async fn fetch(
        &self,
        request: &PlatformFetchRequest,
    ) -> Result<PlatformFetchOutcome, ProviderError> {
        let PlatformRef::Hn(reference) = request.reference else {
            return Err(parameter_error(other_platform_message(
                ROUTE,
                request.reference.platform(),
            )));
        };
        fetch_support(request).map_err(parameter_error)?;
        let mut trail = Trail::default();
        let (item, content_source) = match request.depth {
            ContentDepth::Thread => self.read_thread(reference, &mut trail).await?,
            ContentDepth::FullText => {
                let item = self.read_metadata(reference, &mut trail).await?;
                let source = full_text_source(reference, &item)
                    .map_err(|(kind, message)| trail.error(kind, message))?;
                (item, source)
            }
            _ => (
                self.read_metadata(reference, &mut trail).await?,
                FullTextSource::Urls(Vec::new()),
            ),
        };
        Ok(PlatformFetchOutcome {
            item,
            content_source,
            diagnostic: trail.diagnostic(),
            attempts: trail.attempts,
        })
    }

    /// Asks the Firebase API for its newest item ID, which proves the endpoint answers.
    pub(crate) async fn probe_firebase(&self) -> Result<(), ProviderError> {
        self.get::<u64>(
            Endpoint::Firebase,
            "maxitem.json",
            &[],
            PlatformOperation::Search.as_str(),
        )
        .await
        .map(|_| ())
    }

    /// Reads the item metadata: the Algolia record, with the live counts and the deleted or
    /// dead flags of the Firebase item. An item too new for Algolia is built from Firebase.
    async fn read_metadata(
        &self,
        reference: HnRef,
        trail: &mut Trail,
    ) -> Result<PlatformItem, ProviderError> {
        let operation = PlatformOperation::Fetch.as_str();
        let hits = trail.required(self.search_objects(&[reference.id()], operation).await)?;
        let indexed = hits
            .into_iter()
            .find(|hit| hit.object_id == reference.to_string())
            .map(|hit| hit.into_item(ContentDepth::Metadata))
            .transpose()
            .map_err(|message| trail.error(AttemptErrorKind::Runtime, message))?;
        let live = trail.optional(self.firebase_item(reference.id(), operation).await);
        match (indexed, live) {
            (_, Ok(Some(live))) if live.deleted || live.dead => Err(trail.error(
                AttemptErrorKind::Parameter,
                format!("HN item hn:{reference} is deleted or dead"),
            )),
            (Some(mut item), Ok(Some(live))) => {
                live.correct_counts(&mut item);
                Ok(item)
            }
            (Some(item), Ok(None)) => Ok(item),
            (Some(item), Err(error)) => {
                trail.note(format!(
                    "HN Firebase API unavailable ({}); points and comment counts may lag",
                    error.message
                ));
                Ok(item)
            }
            (None, Ok(Some(live))) => live
                .into_item(ContentDepth::Metadata)
                .map_err(|message| trail.error(AttemptErrorKind::Runtime, message)),
            (None, Ok(None)) => Err(trail.error(
                AttemptErrorKind::Parameter,
                format!("HN item not found: hn:{reference}"),
            )),
            (None, Err(error)) => Err(trail.error(error.kind, error.message)),
        }
    }

    /// Reads the discussion rooted at the item and renders it as Markdown. Top-level comments
    /// follow the Firebase rank; without it, every comment is in time order.
    async fn read_thread(
        &self,
        reference: HnRef,
        trail: &mut Trail,
    ) -> Result<(PlatformItem, FullTextSource), ProviderError> {
        let operation = PlatformOperation::Fetch.as_str();
        let tree = trail.required(self.items_tree(reference).await)?;
        let live = match trail.optional(self.firebase_item(reference.id(), operation).await) {
            Ok(Some(live)) if live.deleted || live.dead => {
                return Err(trail.error(
                    AttemptErrorKind::Parameter,
                    format!("HN item hn:{reference} is deleted or dead"),
                ));
            }
            Ok(Some(live)) => Some(live),
            Ok(None) => {
                trail.note(format!(
                    "HN Firebase API has no item hn:{reference}; comments are in chronological order"
                ));
                None
            }
            Err(error) => {
                trail.note(format!(
                    "HN Firebase API unavailable ({}); comments are in chronological order and counts may lag",
                    error.message
                ));
                None
            }
        };
        let item_type = HnItemType::parse(&tree.item_type).ok_or_else(|| {
            trail.error(
                AttemptErrorKind::Runtime,
                format!(
                    "HN item hn:{reference} has an unknown type `{}`",
                    tree.item_type
                ),
            )
        })?;
        let story_id = tree.story_id.filter(|id| *id != reference.id());
        let mut story_title = None;
        if item_type == HnItemType::Comment
            && let Some(story_id) = story_id
        {
            match trail.optional(self.firebase_item(story_id, operation).await) {
                Ok(story) => story_title = story.and_then(|story| story.title),
                Err(error) => trail.note(format!(
                    "HN Firebase API unavailable ({}); the story title is missing",
                    error.message
                )),
            }
        }
        let (root, order) = ThreadRoot::new(tree, live.as_ref().map(|live| live.kids.as_slice()));
        let delivered = root.comment_count();
        let comments = live.as_ref().and_then(|live| live.descendants).or_else(|| {
            matches!(item_type, HnItemType::Story | HnItemType::Poll).then_some(delivered as u64)
        });
        let item = PlatformItem {
            url: reference.canonical_url(),
            reference: PlatformRef::Hn(reference),
            depth: ContentDepth::Thread,
            title: (item_type != HnItemType::Comment)
                .then(|| root.node().title.clone())
                .flatten(),
            authors: root.node().author.clone().into_iter().collect(),
            published: published(root.node().created_at_i),
            data: PlatformItemData::Hn(HnItemData {
                item_type,
                link_url: non_empty(root.node().url.clone()),
                points: live
                    .as_ref()
                    .and_then(|live| live.score)
                    .or(root.node().points),
                comments,
                text: root.node().text.as_deref().and_then(hn_markdown),
                story_ref: comment_ref(item_type, story_id),
                story_title: (item_type == HnItemType::Comment)
                    .then_some(story_title)
                    .flatten(),
                parent_ref: comment_ref(item_type, root.node().parent_id),
                comments_delivered: Some(delivered),
                order: Some(order),
            }),
        };
        let markdown = root.render(&item);
        Ok((item, FullTextSource::Native(markdown)))
    }

    /// Searches Algolia for the items with these IDs, of any type, in one request.
    async fn search_objects(
        &self,
        ids: &[u64],
        operation: &'static str,
    ) -> Result<ExecutionOutcome<Vec<search::Hit>>, ProviderError> {
        let filters = ids
            .iter()
            .map(|id| format!("objectID:{id}"))
            .collect::<Vec<_>>()
            .join(" OR ");
        let query = [("filters", filters), ("hitsPerPage", ids.len().to_string())];
        let outcome = self
            .get::<search::SearchResponse>(Endpoint::Algolia, "search", &query, operation)
            .await?;
        Ok(ExecutionOutcome {
            value: outcome.value.hits,
            attempts: outcome.attempts,
            diagnostic: outcome.diagnostic,
        })
    }

    /// Reads the whole Algolia tree rooted at the item.
    async fn items_tree(
        &self,
        reference: HnRef,
    ) -> Result<ExecutionOutcome<ThreadNode>, ProviderError> {
        let path = format!("items/{reference}");
        self.get_with(
            Endpoint::Algolia,
            &path,
            &[],
            PlatformOperation::Fetch.as_str(),
            thread::decode_tree,
        )
        .await
        .map_err(|mut error| {
            if let Some(attempt) = error
                .attempts
                .last_mut()
                .filter(|attempt| attempt.http_status == Some(404))
            {
                attempt.message = format!("HN item not found: hn:{reference}");
                error.message.clone_from(&attempt.message);
            }
            error
        })
    }

    /// Reads one Firebase item; `None` when Firebase has no such item.
    async fn firebase_item(
        &self,
        id: u64,
        operation: &'static str,
    ) -> Result<ExecutionOutcome<Option<FirebaseItem>>, ProviderError> {
        self.get(
            Endpoint::Firebase,
            &format!("item/{id}.json"),
            &[],
            operation,
        )
        .await
    }

    /// Sends one GET to an endpoint under the route's access policy and decodes the JSON body.
    async fn get<T: DeserializeOwned>(
        &self,
        endpoint: Endpoint,
        path: &str,
        query: &[(&str, String)],
        operation: &'static str,
    ) -> Result<ExecutionOutcome<T>, ProviderError> {
        self.get_with(endpoint, path, query, operation, decode_json)
            .await
    }

    /// Sends one GET to an endpoint under the route's access policy; a body `decode` rejects
    /// fails the attempt as Runtime.
    async fn get_with<T>(
        &self,
        endpoint: Endpoint,
        path: &str,
        query: &[(&str, String)],
        operation: &'static str,
        decode: fn(&str) -> Result<T, String>,
    ) -> Result<ExecutionOutcome<T>, ProviderError> {
        let base = match endpoint {
            Endpoint::Algolia => &self.config.url,
            Endpoint::Firebase => &self.config.firebase_url,
        };
        let url = &format!("{}/{path}", base.trim_end_matches('/'));
        let failure_message = match endpoint {
            Endpoint::Algolia => algolia_failure_message,
            Endpoint::Firebase => firebase_failure_message,
        };
        execute_anonymous(
            self.settings(endpoint, operation),
            move |deadline| async move {
                let _permit = acquire_window(&self.limiter, deadline).await?;
                let request = self.client.get(url).query(query);
                let response = send_provider_request(request, &self.credentials).await?;
                let body =
                    read_complete_protocol(response, &self.credentials, failure_message).await?;
                let value = decode(&body.text).map_err(|error| AttemptFailure {
                    kind: AttemptErrorKind::Runtime,
                    status: Some(body.status),
                    message: format!("invalid {} response: {error}", endpoint.label()),
                })?;
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

/// Says where the full text of an item lives: the page a link item points to, or the item's
/// own title and body. A comment or poll option is part of a discussion, not a document.
fn full_text_source(
    reference: HnRef,
    item: &PlatformItem,
) -> Result<FullTextSource, (AttemptErrorKind, String)> {
    let PlatformItemData::Hn(data) = &item.data else {
        unreachable!("the hn route builds hn items")
    };
    let kind = match data.item_type {
        HnItemType::Comment => Some("a comment"),
        HnItemType::Pollopt => Some("a poll option"),
        HnItemType::Story | HnItemType::Poll | HnItemType::Job => None,
    };
    if let Some(kind) = kind {
        return Err((
            AttemptErrorKind::Parameter,
            format!(
                "hn:{reference} is {kind}; full_text applies to stories, polls, and jobs; use --depth thread"
            ),
        ));
    }
    if let Some(url) = &data.link_url {
        return Ok(FullTextSource::Urls(vec![url.clone()]));
    }
    let body = [
        item.title.as_ref().map(|title| format!("# {title}")),
        data.text.clone(),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join("\n\n");
    if body.is_empty() {
        return Err((
            AttemptErrorKind::Quality,
            format!("hn:{reference} has no title or text to read"),
        ));
    }
    Ok(FullTextSource::Native(body))
}

/// One Firebase item. Firebase omits every absent field, so each one is optional.
#[derive(Deserialize)]
struct FirebaseItem {
    id: u64,
    #[serde(rename = "type")]
    item_type: Option<String>,
    by: Option<String>,
    time: Option<i64>,
    text: Option<String>,
    title: Option<String>,
    url: Option<String>,
    score: Option<u64>,
    descendants: Option<u64>,
    /// The direct replies, in the rank Hacker News shows.
    #[serde(default)]
    kids: Vec<u64>,
    parent: Option<u64>,
    #[serde(default)]
    deleted: bool,
    #[serde(default)]
    dead: bool,
}

impl FirebaseItem {
    /// Replaces an indexed item's counts with the live ones, where Firebase has them.
    fn correct_counts(&self, item: &mut PlatformItem) {
        if let PlatformItemData::Hn(data) = &mut item.data {
            data.points = self.score.or(data.points);
            data.comments = self.descendants.or(data.comments);
        }
    }

    /// Builds an item from Firebase alone; a comment's story is unknown here.
    fn into_item(self, depth: ContentDepth) -> Result<PlatformItem, String> {
        let item_type = self
            .item_type
            .as_deref()
            .and_then(HnItemType::parse)
            .ok_or_else(|| format!("HN item hn:{} has an unknown type", self.id))?;
        let reference = HnRef::new(self.id)
            .ok_or_else(|| format!("HN Firebase API returned invalid item ID {}", self.id))?;
        Ok(PlatformItem {
            url: reference.canonical_url(),
            reference: PlatformRef::Hn(reference),
            depth,
            title: (item_type != HnItemType::Comment)
                .then_some(self.title)
                .flatten(),
            authors: self.by.into_iter().collect(),
            published: published(self.time),
            data: PlatformItemData::Hn(HnItemData {
                item_type,
                link_url: non_empty(self.url),
                points: self.score,
                comments: self.descendants,
                text: self.text.as_deref().and_then(hn_markdown),
                story_ref: None,
                story_title: None,
                parent_ref: comment_ref(item_type, self.parent),
                comments_delivered: None,
                order: None,
            }),
        })
    }
}

fn decode_json<T: DeserializeOwned>(body: &str) -> Result<T, String> {
    serde_json::from_str(body).map_err(|error| error.to_string())
}

/// Returns the ref of a comment's story or parent; `None` for other item types.
fn comment_ref(item_type: HnItemType, id: Option<u64>) -> Option<PlatformRef> {
    (item_type == HnItemType::Comment)
        .then_some(id)
        .flatten()
        .and_then(HnRef::new)
        .map(PlatformRef::Hn)
}

/// Formats a Unix time as a platform item timestamp.
fn published(seconds: Option<i64>) -> Option<String> {
    seconds
        .and_then(|seconds| DateTime::from_timestamp(seconds, 0))
        .map(utc_second)
}

fn non_empty(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

#[derive(Deserialize)]
struct AlgoliaError {
    message: String,
}

fn algolia_failure_message(body: &str, status: u16) -> String {
    serde_json::from_str::<AlgoliaError>(body).map_or_else(
        |_| format!("HN Search API returned HTTP {status}"),
        |error| format!("HN Search API rejected the request: {}", error.message),
    )
}

fn firebase_failure_message(_body: &str, status: u16) -> String {
    let reason = StatusCode::from_u16(status)
        .ok()
        .and_then(|status| status.canonical_reason())
        .map(|reason| format!(" ({reason})"))
        .unwrap_or_default();
    format!("HN Firebase API returned HTTP {status}{reason}")
}
