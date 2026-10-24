//! Hacker News listings: Algolia search pages and the official story lists.

use std::collections::HashMap;

use serde::Deserialize;

use super::hn_markup::hn_markdown;
use super::{Endpoint, FirebaseItem, HnApi, ROUTE, comment_ref, non_empty, published};
use crate::catalog::PlatformOperation;
use crate::providers::shared::{Trail, other_platform_message, parameter_error};
use crate::types::{
    AttemptErrorKind, ContentDepth, HnItemData, HnItemType, HnRankedItem, HnRef, HnSearchOptions,
    HnSort, HnStoriesOutcome, HnStoriesRequest, PlatformItem, PlatformItemData, PlatformRef,
    PlatformSearchOptions, PlatformSearchOutcome, PlatformSearchRequest, ProviderError, STORIES,
};

/// Algolia returns at most the first 1000 hits of any query.
const MAX_REACHABLE_HITS: u64 = 1000;

/// Returns whether the route can run the search; it never sends a request. A page past the
/// first 1000 hits would come back empty, so it fails here.
pub(crate) fn search_support(request: &PlatformSearchRequest) -> Result<(), String> {
    if !matches!(request.options, PlatformSearchOptions::Hn(_)) {
        return Err(other_platform_message(ROUTE, request.options.platform()));
    }
    let page = page_number(request.page.as_deref())?;
    if (page + 1) * u64::from(request.limit) > MAX_REACHABLE_HITS {
        return Err(format!(
            "{} cannot read past the first {MAX_REACHABLE_HITS} results of a search; narrow the query or filters",
            ROUTE.name()
        ));
    }
    Ok(())
}

fn page_number(page: Option<&str>) -> Result<u64, String> {
    page.map_or(Ok(0), |page| {
        page.parse()
            .map_err(|_| format!("invalid HN search page position `{page}`"))
    })
}

#[derive(Deserialize)]
pub(super) struct SearchResponse {
    pub(super) hits: Vec<Hit>,
    #[serde(rename = "nbPages")]
    nb_pages: u64,
}

/// One Algolia search hit; which text field is set depends on the item type.
#[derive(Deserialize)]
pub(super) struct Hit {
    #[serde(rename = "objectID")]
    pub(super) object_id: String,
    #[serde(rename = "_tags", default)]
    tags: Vec<String>,
    title: Option<String>,
    url: Option<String>,
    author: Option<String>,
    points: Option<u64>,
    num_comments: Option<u64>,
    story_text: Option<String>,
    comment_text: Option<String>,
    job_text: Option<String>,
    story_id: Option<u64>,
    story_title: Option<String>,
    parent_id: Option<u64>,
    created_at_i: Option<i64>,
}

impl Hit {
    pub(super) fn into_item(self, depth: ContentDepth) -> Result<PlatformItem, String> {
        let reference = HnRef::from_id(&self.object_id).ok_or_else(|| {
            format!(
                "HN Search API returned invalid item ID `{}`",
                self.object_id
            )
        })?;
        let item_type = ["comment", "pollopt", "poll", "job", "story"]
            .into_iter()
            .find(|tag| self.tags.iter().any(|known| known == tag))
            .and_then(HnItemType::parse)
            .ok_or_else(|| format!("HN Search API returned hn:{reference} without an item type"))?;
        let text = [self.comment_text, self.story_text, self.job_text]
            .into_iter()
            .flatten()
            .find_map(|html| hn_markdown(&html));
        let is_comment = item_type == HnItemType::Comment;
        Ok(PlatformItem {
            url: reference.canonical_url(),
            reference: PlatformRef::Hn(reference),
            depth,
            title: (!is_comment).then(|| non_empty(self.title)).flatten(),
            authors: self.author.into_iter().collect(),
            published: published(self.created_at_i),
            data: PlatformItemData::Hn(HnItemData {
                item_type,
                link_url: non_empty(self.url),
                points: self.points,
                comments: self.num_comments,
                text,
                story_ref: comment_ref(item_type, self.story_id),
                story_title: is_comment.then_some(self.story_title).flatten(),
                parent_ref: comment_ref(item_type, self.parent_id),
                comments_delivered: None,
                order: None,
            }),
        })
    }

    /// Decodes a listing hit: an item with its own body is a snippet, one without is
    /// metadata.
    fn into_listed_item(self) -> Result<PlatformItem, String> {
        let has_text = [&self.comment_text, &self.story_text, &self.job_text]
            .into_iter()
            .flatten()
            .any(|html| hn_markdown(html).is_some());
        self.into_item(if has_text {
            ContentDepth::Snippet
        } else {
            ContentDepth::Metadata
        })
    }
}

impl HnApi {
    pub(crate) async fn search(
        &self,
        request: &PlatformSearchRequest,
    ) -> Result<PlatformSearchOutcome, ProviderError> {
        let PlatformSearchOptions::Hn(options) = &request.options else {
            return Err(parameter_error(other_platform_message(
                ROUTE,
                request.options.platform(),
            )));
        };
        search_support(request).map_err(parameter_error)?;
        let page = page_number(request.page.as_deref()).map_err(parameter_error)?;
        let path = match options.sort {
            HnSort::Relevance => "search",
            HnSort::Date => "search_by_date",
        };
        let mut query = vec![("query", request.query.clone())];
        query.extend(search_filters(options));
        query.push(("hitsPerPage", request.limit.to_string()));
        if request.page.is_some() {
            query.push(("page", page.to_string()));
        }
        let mut trail = Trail::default();
        let response = trail.required(
            self.get::<SearchResponse>(
                Endpoint::Algolia,
                path,
                &query,
                PlatformOperation::Search.as_str(),
            )
            .await,
        )?;
        let items = response
            .hits
            .into_iter()
            .map(Hit::into_listed_item)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|message| trail.error(AttemptErrorKind::Runtime, message))?;
        let limit = u64::from(request.limit);
        let next_page = (page + 1 < response.nb_pages && (page + 2) * limit <= MAX_REACHABLE_HITS)
            .then(|| (page + 1).to_string());
        Ok(PlatformSearchOutcome {
            items,
            next_page,
            diagnostic: trail.diagnostic(),
            attempts: trail.attempts,
        })
    }

    /// Lists the current items of one official story list in list order: the IDs from
    /// Firebase, the items from one Algolia request, and each item Algolia lacks from Firebase.
    /// Deleted, dead, missing, and unreadable items are skipped with a diagnostic; only a failed
    /// list fails the operation.
    pub(crate) async fn stories(
        &self,
        request: &HnStoriesRequest,
    ) -> Result<HnStoriesOutcome, ProviderError> {
        let list = request.list.as_str();
        let mut trail = Trail::default();
        let ids = trail
            .required(
                self.get::<Option<Vec<u64>>>(
                    Endpoint::Firebase,
                    &format!("{list}stories.json"),
                    &[],
                    STORIES,
                )
                .await,
            )?
            .ok_or_else(|| {
                trail.error(
                    AttemptErrorKind::Runtime,
                    format!("HN Firebase API returned no {list} story list"),
                )
            })?;
        let ids = ids
            .into_iter()
            .take(usize::from(request.limit))
            .collect::<Vec<_>>();
        if ids.is_empty() {
            return Ok(HnStoriesOutcome {
                items: Vec::new(),
                diagnostic: trail.diagnostic(),
                attempts: trail.attempts,
            });
        }
        let mut indexed = trail
            .required(self.search_objects(&ids, STORIES).await)?
            .into_iter()
            .map(|hit| (hit.object_id.clone(), hit))
            .collect::<HashMap<_, _>>();
        let mut items = Vec::with_capacity(ids.len());
        let (mut skipped, mut unread) = (0, 0);
        for (index, id) in ids.into_iter().enumerate() {
            let item = if let Some(hit) = indexed.remove(&id.to_string()) {
                Some(hit.into_listed_item())
            } else {
                let Ok(live) = trail.optional(self.firebase_item(id, STORIES).await) else {
                    unread += 1;
                    continue;
                };
                live.filter(|live| !live.deleted && !live.dead)
                    .map(listed_firebase_item)
            };
            match item {
                Some(item) => items.push(HnRankedItem {
                    item: item
                        .map_err(|message| trail.error(AttemptErrorKind::Runtime, message))?,
                    rank: index + 1,
                }),
                None => skipped += 1,
            }
        }
        if skipped > 0 {
            trail.note(format!(
                "skipped {skipped} deleted, dead, or missing items of the HN {list} list"
            ));
        }
        if unread > 0 {
            let noun = if unread == 1 { "item" } else { "items" };
            trail.note(format!(
                "could not read {unread} {noun} of the HN {list} list from the HN Firebase API"
            ));
        }
        Ok(HnStoriesOutcome {
            items,
            diagnostic: trail.diagnostic(),
            attempts: trail.attempts,
        })
    }
}

fn listed_firebase_item(live: FirebaseItem) -> Result<PlatformItem, String> {
    let depth = if live.text.as_deref().and_then(hn_markdown).is_some() {
        ContentDepth::Snippet
    } else {
        ContentDepth::Metadata
    };
    live.into_item(depth)
}

/// Maps the options to Algolia parameters: tag conditions all match, the types inside one
/// group match any; numeric conditions all match. An option not given sends nothing.
fn search_filters(options: &HnSearchOptions) -> Vec<(&'static str, String)> {
    let mut tags = Vec::new();
    if !options.types.is_empty() {
        let types = options
            .types
            .iter()
            .map(|item_type| item_type.as_str())
            .collect::<Vec<_>>();
        tags.push(format!("({})", types.join(",")));
    }
    if let Some(author) = &options.author {
        tags.push(format!("author_{author}"));
    }
    if let Some(story) = options.story {
        tags.push(format!("story_{story}"));
    }
    let mut numeric = Vec::new();
    if let Some(from) = options.from {
        let start = from.and_hms_opt(0, 0, 0).expect("midnight exists");
        numeric.push(format!("created_at_i>={}", start.and_utc().timestamp()));
    }
    if let Some(to) = options.to {
        let end = to.and_hms_opt(23, 59, 59).expect("the last second exists");
        numeric.push(format!("created_at_i<={}", end.and_utc().timestamp()));
    }
    if let Some(points) = options.min_points {
        numeric.push(format!("points>={points}"));
    }
    if let Some(comments) = options.min_comments {
        numeric.push(format!("num_comments>={comments}"));
    }
    let mut filters = Vec::new();
    if !tags.is_empty() {
        filters.push(("tags", tags.join(",")));
    }
    if !numeric.is_empty() {
        filters.push(("numericFilters", numeric.join(",")));
    }
    filters
}
