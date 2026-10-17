//! The X platform route through xAI's `x_search` tool (ADR 0026).
//!
//! xAI never returns what the tool read; the model relays the posts. Search and fetch ask for
//! structured output and keep only posts whose IDs appear among the response's citations, which
//! then list every post the tool read, and date each post by its snowflake ID. Ask keeps the
//! model's inline citations instead. Every request sends `x_search` alone, whatever main search
//! enables.

use std::collections::HashSet;

use chrono::NaiveDate;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use super::Xai;
use super::responses::{ResponsesRequest, output_text};
use crate::catalog::{PlatformOperation, ProviderId};
use crate::net::{AttemptFailure, combine_diagnostics};
use crate::providers::execution::{ExecutionOutcome, execute_v2};
use crate::providers::shared::{normalize_main_search, other_platform_message, parameter_error};
use crate::providers::types::with_time_context;
use crate::types::{
    ASK, AttemptErrorKind, AttemptTarget, ContentDepth, FullTextSource, Platform,
    PlatformFetchOutcome, PlatformFetchRequest, PlatformItem, PlatformItemData, PlatformRef,
    PlatformSearchOptions, PlatformSearchOutcome, PlatformSearchRequest, ProviderError, Source,
    XAskOutcome, XAskRequest, XPostData, XPostRole, XRef, XSearchOptions, XSource, XThreadPost,
};

const ROUTE: ProviderId = ProviderId::Xai;
const PLATFORM: Platform = Platform::X;

const POST_RULES: &str = "List only posts that your x_search tool calls returned in this response; never list a post from memory, and never invent or alter a URL. For each post give its URL in the form https://x.com/<handle>/status/<post_id>, the author's handle without @, and the full post text exactly as the tool returned it: do not translate, summarize, or shorten it.";
const THREAD_INSTRUCTIONS: &str = "You read X (formerly Twitter) with the x_search tool. Fetch the thread of the requested post with the tool's thread fetch, then list the posts it returned in conversation order: the requested post with role root, the posts it replies to with role parent, the replies to it with role reply, and the posts it quotes with role quoted.";
const METADATA_INSTRUCTIONS: &str = "You read X (formerly Twitter) with the x_search tool. Fetch the requested post with the tool's thread fetch, then list only that post, with role root.";
const MISSING_POST_RULE: &str =
    "If the requested post does not exist or is not visible, return an empty posts array.";
const ASK_INSTRUCTIONS: &str = "You answer questions from posts on X (formerly Twitter) alone, found with the x_search tool. Another AI agent reads your answer and checks key claims against the cited posts.\n\nGuidelines:\n- Search X before answering. Match search breadth to the question.\n- Cite the X post behind every claim. Keep names, numbers, and dates exactly as the posts state them.\n- Say when posts disagree, and say plainly when no post addresses the question.\n- Lead with the direct answer, then the supporting details.\n- Write compact Markdown. Do not restate the question, add a closing summary, or offer further help.\n";

/// Returns whether the route can run the search; it never sends a request. X search issues no
/// cursor, so only a first page can run.
pub(crate) fn search_support(request: &PlatformSearchRequest) -> Result<(), String> {
    if !matches!(request.options, PlatformSearchOptions::X(_)) {
        return Err(other_platform_message(ROUTE, request.options.platform()));
    }
    if request.page.is_some() {
        return Err(format!(
            "{} cannot continue a search from a page position; X search has no cursor",
            ROUTE.name()
        ));
    }
    Ok(())
}

/// Returns whether the route can fetch the post at the requested depth; it never sends a
/// request.
pub(crate) fn fetch_support(request: &PlatformFetchRequest) -> Result<(), String> {
    fetch_target(request).map(|_| ())
}

fn fetch_target(request: &PlatformFetchRequest) -> Result<XRef, String> {
    let PlatformRef::X(requested) = request.reference else {
        return Err(other_platform_message(ROUTE, request.reference.platform()));
    };
    match request.depth {
        ContentDepth::Thread | ContentDepth::Metadata => Ok(requested),
        depth => Err(format!(
            "{} cannot fetch at depth `{}`; X posts are read at `thread` or `metadata` depth",
            ROUTE.name(),
            depth.as_str()
        )),
    }
}

impl Xai {
    /// Searches X posts and keeps only those the search verifiably read.
    pub(crate) async fn search_posts(
        &self,
        request: &PlatformSearchRequest,
    ) -> Result<PlatformSearchOutcome, ProviderError> {
        let PlatformSearchOptions::X(options) = &request.options else {
            return Err(parameter_error(other_platform_message(
                ROUTE,
                request.options.platform(),
            )));
        };
        let instructions = format!(
            "You search X (formerly Twitter) with the x_search tool and list the posts it finds. {POST_RULES} List the posts most relevant to the request first, at most {}. If no post matches, return an empty posts array.",
            request.limit
        );
        let body = ResponsesRequest::new(
            &self.config.model,
            Some(&instructions),
            &request.query,
            vec![x_search_tool(options)],
        )
        .with_format(posts_format("x_posts", false));
        let execution = self
            .run_x(PlatformOperation::Search.as_str(), &body, |response| {
                decode_search(response, options, request.limit)
            })
            .await?;
        let (items, dropped) = execution.value;
        Ok(PlatformSearchOutcome {
            items,
            next_page: None,
            attempts: execution.attempts,
            diagnostic: combine_diagnostics(execution.diagnostic.into_iter().chain(dropped)),
        })
    }

    /// Fetches one post, and at `thread` depth its conversation.
    pub(crate) async fn fetch_post(
        &self,
        request: &PlatformFetchRequest,
    ) -> Result<PlatformFetchOutcome, ProviderError> {
        let requested = fetch_target(request).map_err(parameter_error)?;
        let task = if request.depth == ContentDepth::Thread {
            THREAD_INSTRUCTIONS
        } else {
            METADATA_INSTRUCTIONS
        };
        let instructions = format!("{task} {POST_RULES} {MISSING_POST_RULE}");
        let input = format!(
            "The requested post: ID {}, {}",
            requested.post_id(),
            requested.canonical_url()
        );
        let body = ResponsesRequest::new(
            &self.config.model,
            Some(&instructions),
            &input,
            vec![x_search_tool(&XSearchOptions::default())],
        )
        .with_format(posts_format("x_thread", true));
        let execution = self
            .run_x(PlatformOperation::Fetch.as_str(), &body, |response| {
                decode_thread(response, requested, request.depth)
            })
            .await?;
        let (item, skipped) = execution.value;
        Ok(PlatformFetchOutcome {
            item,
            content_source: FullTextSource::Urls(Vec::new()),
            attempts: execution.attempts,
            diagnostic: combine_diagnostics(execution.diagnostic.into_iter().chain(skipped)),
        })
    }

    /// Answers a question from X posts alone, with the posts and pages it cites.
    pub(crate) async fn ask(&self, request: &XAskRequest) -> Result<XAskOutcome, ProviderError> {
        let input = with_time_context(&request.query);
        let body = ResponsesRequest::new(
            &self.config.model,
            Some(ASK_INSTRUCTIONS),
            &input,
            vec![x_search_tool(&request.options)],
        );
        let execution = self
            .run_x(ASK, &body, |response| {
                let (answer, sources) = output_text(response)?;
                let (answer, sources) = normalize_main_search(&answer, sources, &self.credentials)?;
                Ok((answer, x_sources(sources)))
            })
            .await?;
        let (answer, sources) = execution.value;
        Ok(XAskOutcome {
            answer,
            sources,
            attempts: execution.attempts,
            diagnostic: execution.diagnostic,
        })
    }

    /// Runs one request of an X operation with credential claim, rotation, and retry. `decode`
    /// turns the completed response into the operation's value inside the attempt, so a
    /// refusal is that attempt's failure.
    async fn run_x<T>(
        &self,
        operation: &'static str,
        body: &ResponsesRequest<'_>,
        decode: impl Fn(&Value) -> Result<T, AttemptFailure>,
    ) -> Result<ExecutionOutcome<T>, ProviderError> {
        let decode = &decode;
        execute_v2(
            &self.credentials,
            self.settings(
                AttemptTarget::platform(PLATFORM.as_str(), operation),
                self.config.model.clone(),
            ),
            |credential, _| async move {
                let (status, response) = self.complete(body, &credential).await?;
                decode(&response).map(|value| (Some(status), value))
            },
        )
        .await
    }
}

/// Returns the `x_search` tool with the filters xAI enforces on its server; an absent filter
/// sends no field.
fn x_search_tool(options: &XSearchOptions) -> Value {
    let mut tool = json!({ "type": "x_search" });
    if !options.handles.is_empty() {
        tool["allowed_x_handles"] = json!(options.handles);
    }
    if !options.excluded_handles.is_empty() {
        tool["excluded_x_handles"] = json!(options.excluded_handles);
    }
    if let Some(from) = options.from {
        tool["from_date"] = json!(day(from));
    }
    // xAI's `to_date` excludes its day, while `--to` includes it.
    if let Some(to) = options.to {
        tool["to_date"] = json!(day(to.succ_opt().unwrap_or(to)));
    }
    tool
}

fn day(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

/// Returns the strict structured output of a post list; fetch also asks for each post's role.
fn posts_format(name: &str, with_role: bool) -> Value {
    let mut properties = json!({
        "url": {"type": "string"},
        "author_handle": {"type": "string"},
        "text": {"type": "string"},
    });
    let mut required = vec!["url", "author_handle", "text"];
    if with_role {
        properties["role"] =
            json!({"type": "string", "enum": ["parent", "root", "reply", "quoted"]});
        required.push("role");
    }
    json!({
        "type": "json_schema",
        "name": name,
        "strict": true,
        "schema": {
            "type": "object",
            "additionalProperties": false,
            "required": ["posts"],
            "properties": {
                "posts": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "additionalProperties": false,
                        "required": required,
                        "properties": properties,
                    }
                }
            }
        }
    })
}

#[derive(Deserialize)]
struct Posts<P> {
    posts: Vec<P>,
}

#[derive(Deserialize)]
struct RelayedPost {
    url: String,
    author_handle: String,
    text: String,
}

#[derive(Deserialize)]
struct RelayedThreadPost {
    url: String,
    author_handle: String,
    text: String,
    role: XPostRole,
}

fn runtime(message: String) -> AttemptFailure {
    AttemptFailure {
        kind: AttemptErrorKind::Runtime,
        status: Some(200),
        message,
    }
}

/// Decodes the structured output and the IDs of every post the response cites as read.
fn structured<T: DeserializeOwned>(response: &Value) -> Result<(T, HashSet<u64>), AttemptFailure> {
    let (text, sources) = output_text(response)?;
    let decoded = serde_json::from_str(&text).map_err(|error| {
        runtime(format!(
            "xAI returned X posts that are not the requested JSON: {error}"
        ))
    })?;
    let read = sources
        .iter()
        .filter_map(|source| XRef::from_status_url(&source.url))
        .map(XRef::post_id)
        .collect();
    Ok((decoded, read))
}

/// Decodes the search posts into items and the diagnostic of the posts dropped as unverified.
/// A non-empty list whose every post is dropped is Runtime, never a legitimately empty page.
fn decode_search(
    response: &Value,
    options: &XSearchOptions,
    limit: u16,
) -> Result<(Vec<PlatformItem>, Option<String>), AttemptFailure> {
    let (Posts { posts }, read) = structured::<Posts<RelayedPost>>(response)?;
    let count = posts.len();
    let mut dropped = Vec::new();
    let mut items: Vec<PlatformItem> = Vec::new();
    for post in posts {
        match verify(&post.url, &read, options) {
            Ok(reference)
                if items
                    .iter()
                    .any(|item| item.reference == PlatformRef::X(reference)) => {}
            Ok(reference) => items.push(post_item(
                reference,
                &post.author_handle,
                post.text,
                ContentDepth::Snippet,
                None,
            )),
            Err(reason) => dropped.push(format!("{} ({reason})", post.url)),
        }
    }
    let dropped = (!dropped.is_empty()).then(|| {
        format!(
            "{} dropped {} of {count} X posts it could not verify: {}",
            ROUTE.name(),
            dropped.len(),
            dropped.join("; ")
        )
    });
    if items.is_empty() && count > 0 {
        return Err(runtime(format!(
            "xAI returned {count} X posts but none could be verified; {}",
            dropped.unwrap_or_default()
        )));
    }
    items.truncate(usize::from(limit));
    Ok((items, dropped))
}

/// Returns the post a relayed URL names when the search read it and its ID dates it inside the
/// requested days.
fn verify(url: &str, read: &HashSet<u64>, options: &XSearchOptions) -> Result<XRef, &'static str> {
    let reference = XRef::from_status_url(url).ok_or("not an X post URL")?;
    if !read.contains(&reference.post_id()) {
        return Err("not among the posts the search read");
    }
    if options.from.is_some() || options.to.is_some() {
        let published = reference
            .published_on()
            .ok_or("its ID carries no publication time")?;
        if options.from.is_some_and(|from| published < from)
            || options.to.is_some_and(|to| published > to)
        {
            return Err("published outside the requested dates");
        }
    }
    Ok(reference)
}

/// Decodes the fetched post and, at `thread` depth, its conversation. The root must be the
/// requested post and cited as read; the other posts cannot be checked, because the citations
/// name only the root.
fn decode_thread(
    response: &Value,
    requested: XRef,
    depth: ContentDepth,
) -> Result<(PlatformItem, Option<String>), AttemptFailure> {
    let (Posts { posts }, read) = structured::<Posts<RelayedThreadPost>>(response)?;
    if posts.is_empty() {
        return Err(AttemptFailure {
            kind: AttemptErrorKind::Parameter,
            status: Some(200),
            message: format!("X post x:{requested} was not found or is not visible"),
        });
    }
    let roots = posts
        .iter()
        .filter(|post| post.role == XPostRole::Root)
        .collect::<Vec<_>>();
    let [root] = roots.as_slice() else {
        return Err(runtime(format!(
            "xAI returned {} root posts for x:{requested}; expected exactly one",
            roots.len()
        )));
    };
    if XRef::from_status_url(&root.url) != Some(requested) {
        return Err(runtime(format!(
            "xAI returned {} as the root post instead of x:{requested}",
            root.url
        )));
    }
    if !read.contains(&requested.post_id()) {
        return Err(runtime(format!(
            "xAI did not cite x:{requested} among the posts it read"
        )));
    }
    let mut skipped = Vec::new();
    let thread = (depth == ContentDepth::Thread).then(|| {
        posts
            .iter()
            .filter_map(|post| {
                let Some(reference) = XRef::from_status_url(&post.url) else {
                    skipped.push(post.url.clone());
                    return None;
                };
                Some(XThreadPost {
                    reference: PlatformRef::X(reference),
                    url: reference.canonical_url(),
                    author_handle: handle(&post.author_handle),
                    published: reference.published(),
                    text: post.text.clone(),
                    role: post.role,
                })
            })
            .collect()
    });
    let skipped = (!skipped.is_empty()).then(|| {
        format!(
            "{} skipped {} thread posts without an X post URL: {}",
            ROUTE.name(),
            skipped.len(),
            skipped.join("; ")
        )
    });
    let item = post_item(
        requested,
        &root.author_handle,
        root.text.clone(),
        depth,
        thread,
    );
    Ok((item, skipped))
}

fn post_item(
    reference: XRef,
    author_handle: &str,
    text: String,
    depth: ContentDepth,
    thread: Option<Vec<XThreadPost>>,
) -> PlatformItem {
    let author_handle = handle(author_handle);
    PlatformItem {
        reference: PlatformRef::X(reference),
        url: reference.canonical_url(),
        depth,
        title: None,
        authors: (!author_handle.is_empty())
            .then(|| author_handle.clone())
            .into_iter()
            .collect(),
        published: reference.published(),
        data: PlatformItemData::X(XPostData {
            author_handle,
            text,
            thread,
        }),
    }
}

fn handle(value: &str) -> String {
    let value = value.trim();
    value.strip_prefix('@').unwrap_or(value).to_owned()
}

/// Maps the cited pages to sources: a post gets its ref and canonical URL, and any other page,
/// such as a profile, keeps its URL without a ref.
fn x_sources(sources: Vec<Source>) -> Vec<XSource> {
    let mut mapped: Vec<XSource> = Vec::new();
    for source in sources {
        let (reference, url) = match XRef::from_status_url(&source.url) {
            Some(reference) => (Some(PlatformRef::X(reference)), reference.canonical_url()),
            None => (None, source.url),
        };
        if !mapped.iter().any(|known| known.url == url) {
            mapped.push(XSource {
                reference,
                url,
                title: source.title,
            });
        }
    }
    mapped
}
