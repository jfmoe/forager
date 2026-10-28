//! Reddit fixtures for `reddit_web`: the search results fragment, the comment-tree fragment, and
//! the embed page, trimmed from the anonymous responses of 2026-10-28 to the elements the route
//! reads, and request and output readers shared by the `platform reddit` tests.

use std::collections::BTreeMap;
use std::process::Output;

use serde_json::{Value, json};

use super::Response;

/// Enables `reddit_web` alone, with both of its origins at the fixture server.
pub(crate) fn config(url: &str) -> String {
    format!(
        "[providers.reddit_web]\nurl = \"{url}\"\nembed_url = \"{url}\"\n\n[platforms.reddit]\norder = [\"reddit_web\"]\n"
    )
}

pub(crate) fn html(body: &str) -> Response {
    Response::new(200, "text/html; charset=utf-8", body)
}

/// Escapes text for an HTML attribute value or element content.
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// One search result as the fragment lists it.
pub(crate) struct SearchResult<'a> {
    pub(crate) id: &'a str,
    pub(crate) title: &'a str,
    pub(crate) subreddit: &'a str,
    pub(crate) author: &'a str,
    pub(crate) published: &'a str,
    /// The excerpt and the fullname of the post or comment it comes from.
    pub(crate) snippet: Option<(&'a str, &'a str)>,
    pub(crate) votes: i64,
    pub(crate) comments: u64,
}

impl<'a> SearchResult<'a> {
    /// A result for post `id` in r/rust without an excerpt.
    pub(crate) fn new(id: &'a str, title: &'a str) -> Self {
        Self {
            id,
            title,
            subreddit: "rust",
            author: "ferris",
            published: "2026-07-05T08:48:25.306000+0000",
            snippet: None,
            votes: 2,
            comments: 10,
        }
    }

    fn render(&self) -> String {
        let fullname = format!("t3_{}", self.id);
        let mut search = json!({"sort": "RELEVANCE", "structure_type": "search"});
        if let Some((snippet, source)) = self.snippet {
            search["snippet"] = json!(snippet);
            search["snippet_id"] = json!(source);
        }
        let tracking = escape(
            &json!({
                "action_info": {"position": 0, "relative_position": 0},
                "post": {"id": fullname, "nsfw": false, "spoiler": false, "title": self.title},
                "profile": {"id": "t2_fixture", "name": self.author},
                "search": search,
                "subreddit": {"id": "t5_fixture", "name": self.subreddit, "nsfw": false}
            })
            .to_string(),
        );
        let path = format!(
            "/r/{}/comments/{}/{}/",
            self.subreddit,
            self.id,
            self.title.to_lowercase().replace(' ', "_")
        );
        let title = escape(self.title);
        let excerpt = self.snippet.map_or_else(String::new, |(snippet, _)| {
            format!(
                "<search-telemetry-tracker data-faceplate-tracking-context=\"{tracking}\" click-events=\"search/click/post\"><a href=\"{path}\">{}</a></search-telemetry-tracker>",
                escape(snippet)
            )
        });
        format!(
            "<search-telemetry-tracker data-faceplate-tracking-context=\"{tracking}\" view-events=\"search/view/post\" leave-events=\"post/leave/post\"><div data-testid=\"search-post-with-content-preview\"><search-telemetry-tracker data-faceplate-tracking-context=\"{tracking}\" click-events=\"search/click/post\"><h2><a data-testid=\"post-title\" href=\"{path}\" aria-label=\"{title}\"><faceplate-screen-reader-content>{title}</faceplate-screen-reader-content></a></h2></search-telemetry-tracker><div data-testid=\"sdui-post-unit\"><div class=\"post-credit-row\"><span><faceplate-hovercard label=\"r/{sub}\"><a href=\"/r/{sub}/\"><span class=\"truncate\">r/{sub}</span></a><div slot=\"content\"><faceplate-partial src=\"/svc/shreddit/community-hover-card/{sub}\" loading=\"programmatic\"></faceplate-partial></div></faceplate-hovercard><span>·</span><faceplate-timeago format=\"narrow\" ts=\"{published}\"></faceplate-timeago></span></div><a data-testid=\"post-title-text\" href=\"{path}\">{title}</a>{excerpt}<div data-testid=\"search-counter-row\"><span><faceplate-number number=\"{votes}\" pretty></faceplate-number> votes</span><span>·</span><span><faceplate-number number=\"{comments}\" pretty></faceplate-number> comments</span></div></div></div></search-telemetry-tracker><hr>",
            sub = self.subreddit,
            published = self.published,
            votes = self.votes,
            comments = self.comments,
        )
    }
}

/// A search results fragment listing `results`; `next_cursor` adds the lazily loaded next page.
pub(crate) fn search_page(results: &[SearchResult<'_>], next_cursor: Option<&str>) -> Response {
    let units = results.iter().map(SearchResult::render).collect::<String>();
    let next = next_cursor.map_or_else(String::new, |cursor| {
        format!(
            "<faceplate-partial loading=\"lazy\" src=\"/svc/shreddit/search/?q=rust&amp;type=posts&amp;sort=relevance&amp;cursor={cursor}\"></faceplate-partial>"
        )
    });
    html(&format!(
        "<!DOCTYPE html><html><body><faceplate-partial name=\"DataProtectionConsent\" src=\"/svc/shreddit/data-protection-consent\" loading=\"eager\"></faceplate-partial><div>{units}</div>{next}</body></html>"
    ))
}

/// The fragment a search without results returns.
pub(crate) fn no_results_page() -> Response {
    html(
        "<!DOCTYPE html><html><body><search-telemetry-tracker view-events=\"search/view/no_results\"><div>Hm... we couldn’t find any results</div></search-telemetry-tracker></body></html>",
    )
}

/// The verification page Reddit serves instead of a page when it wants a browser.
pub(crate) fn challenge_page() -> Response {
    html(
        "<!DOCTYPE html><html><body><form method=\"post\"><input type=\"hidden\" name=\"js_challenge\" value=\"1\"><noscript>Please enable JavaScript</noscript></form></body></html>",
    )
}

/// One comment of a comment-tree fragment.
pub(crate) struct Comment<'a> {
    pub(crate) id: &'a str,
    pub(crate) depth: usize,
    pub(crate) author: &'a str,
    pub(crate) score: i64,
    /// The rendered body; `None` for a deleted comment.
    pub(crate) body: Option<&'a str>,
    pub(crate) replies: Vec<Self>,
}

impl<'a> Comment<'a> {
    pub(crate) fn new(id: &'a str, depth: usize, author: &'a str, body: &'a str) -> Self {
        Self {
            id,
            depth,
            author,
            score: 10,
            body: Some(body),
            replies: Vec::new(),
        }
    }

    /// A deleted comment by a deleted author, which keeps its place among its replies.
    pub(crate) fn deleted(id: &'a str, depth: usize) -> Self {
        Self {
            id,
            depth,
            author: "[deleted]",
            score: 1,
            body: None,
            replies: Vec::new(),
        }
    }

    pub(crate) fn with_replies(mut self, replies: Vec<Self>) -> Self {
        self.replies = replies;
        self
    }

    fn render(&self, post: &str) -> String {
        let thing = format!("t1_{}", self.id);
        let flags = if self.body.is_none() {
            " is-comment-deleted is-author-deleted"
        } else {
            ""
        };
        let body = self.body.map_or_else(String::new, |body| {
            format!(
                "<div class=\"md\" id=\"{thing}-comment-rtjson-content\" slot=\"comment\"><div id=\"{thing}-post-rtjson-content\" dir=\"auto\">{body}</div></div>"
            )
        });
        let replies = self
            .replies
            .iter()
            .map(|reply| reply.render(post))
            .collect::<String>();
        format!(
            "<shreddit-comment created=\"2026-08-30T19:48:51.317000+0000\" author=\"{author}\" thingId=\"{thing}\" depth=\"{depth}\" permalink=\"/r/rust/comments/{post}/comment/{id}/\" score=\"{score}\" postId=\"t3_{post}\" content-type=\"text\"{flags}><details open><summary><div slot=\"commentMeta\"><a href=\"https://www.reddit.com/user/{author}/\">{author}</a><faceplate-timeago ts=\"2026-08-30T19:48:51.317Z\"><time>2mo ago</time></faceplate-timeago></div></summary><div class=\"comment-main-grid\">{body}</div>{replies}</details></shreddit-comment>",
            author = self.author,
            depth = self.depth,
            id = self.id,
            score = self.score,
        )
    }
}

/// The post page path of post `id` in r/rust.
pub(crate) fn permalink_path(id: &str) -> String {
    format!("/r/rust/comments/{id}/learning_async_rust/")
}

/// A comment-tree fragment of post `id`, whose page is `permalink`, holding `comments` of
/// `total`; `more` leaves a `more-comments` fragment to load.
pub(crate) fn comments_page(
    id: &str,
    permalink: &str,
    total: u64,
    comments: &[Comment<'_>],
    more: bool,
) -> Response {
    let tree = comments
        .iter()
        .map(|comment| comment.render(id))
        .collect::<String>();
    let more = if more {
        format!(
            "<faceplate-partial class=\"more-comments-partial\" loading=\"action\" src=\"/svc/shreddit/more-comments/rust/t3_{id}?sort=TOP&amp;startingDepth=2&amp;commentParentPositions=1,1\" method=\"post\" slot=\"children\"></faceplate-partial>"
        )
    } else {
        String::new()
    };
    html(&format!(
        "<!DOCTYPE html><html><body><shreddit-comment-tree-stats total-comments=\"{total}\" sort=\"TOP\"></shreddit-comment-tree-stats><shreddit-comment-tree id=\"comment-tree\" post-id=\"t3_{id}\" totalComments=\"{total}\" permalink=\"{permalink}\" ui=\"desktop\">{tree}{more}</shreddit-comment-tree></body></html>"
    ))
}

/// The comment-tree fragment of a post that does not exist.
pub(crate) fn missing_comments_page() -> Response {
    html(
        "<!DOCTYPE html><html><body><faceplate-tracker data-faceplate-tracking-context=\"{&quot;action_info&quot;:{}}\" routeName=\"top_comments\"></faceplate-tracker></body></html>",
    )
}

/// What an embed page shows about a post.
pub(crate) struct EmbedPost<'a> {
    pub(crate) id: &'a str,
    pub(crate) permalink: String,
    pub(crate) post_type: &'a str,
    /// The post's URL; its own page unless it links out or shows media.
    pub(crate) url: Option<String>,
    pub(crate) title: &'a str,
    pub(crate) author: &'a str,
    pub(crate) score: i64,
    pub(crate) body: Option<&'a str>,
}

impl<'a> EmbedPost<'a> {
    /// A text post in r/rust with `body`.
    pub(crate) fn text(id: &'a str, body: &'a str) -> Self {
        Self {
            id,
            permalink: format!("https://www.reddit.com{}", permalink_path(id)),
            post_type: "text",
            url: None,
            title: "Learning async rust",
            author: "gamer_redditor",
            score: 389,
            body: Some(body),
        }
    }
}

/// The embed page of `post`.
pub(crate) fn embed_page(post: &EmbedPost<'_>) -> Response {
    let fullname = format!("t3_{}", post.id);
    let url = post.url.clone().unwrap_or_else(|| post.permalink.clone());
    let record = escape(
        &json!({
            "post": {
                "id": fullname,
                "url": url,
                "translation_state": false,
                "nsfw": false,
                "created_timestamp": 1_788_114_674_504_i64,
                "type": post.post_type
            },
            "subreddit": {"name": "rust", "id": "t5_2s7lj"},
            "action_info": {"page_type": "embed"}
        })
        .to_string(),
    );
    let body = post.body.map_or_else(String::new, |body| {
        format!("<div id=\"{fullname}-post-rtjson-content\" class=\"md\" dir=\"auto\">{body}</div>")
    });
    html(&format!(
        "<!DOCTYPE html><html><body><shreddit-screenview-data data=\"{record}\"></shreddit-screenview-data><div id=\"{fullname}-embed-wrapper\"><div id=\"embed-container\"><div class=\"flex\"><div data-testid=\"embed-header\"><a href=\"https://www.reddit.com/r/rust/?utm_source=embedv2\" data-testid=\"subreddit-name\">rust</a><div><span>Posted by</span> <a href=\"https://www.reddit.com/user/{author}/?utm_source=embedv2\">{author}</a> · <faceplate-timeago ts=\"2026-08-30T18:31:14.504000+0000\" short></faceplate-timeago></div></div></div><a id=\"embed-title\" href=\"{permalink}?utm_source=embedv2\"><faceplate-tracker><shreddit-embed-title>{title}</shreddit-embed-title></faceplate-tracker></a>{body}<a data-testid=\"upvote\" href=\"{permalink}?utm_content=footer\"><span><span><faceplate-number number=\"{score}\" pretty></faceplate-number> upvotes</span></span></a></div></div><div id=\"canonical-url-updater\" value=\"{permalink}\"></div></body></html>",
        author = post.author,
        permalink = post.permalink,
        title = escape(post.title),
        score = post.score,
    ))
}

/// The embed page of a post Reddit will not show: the record stays, the post does not.
pub(crate) fn unavailable_embed_page(id: &str) -> Response {
    let record = escape(
        &json!({"post": {"id": format!("t3_{id}"), "type": "text"}, "action_info": {"page_type": "embed"}})
            .to_string(),
    );
    html(&format!(
        "<!DOCTYPE html><html><body><shreddit-screenview-data data=\"{record}\"></shreddit-screenview-data><div id=\"embed-container\"><p>Not supported post</p></div></body></html>"
    ))
}

pub(crate) fn payload(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "parse JSON stdout: {error}\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

/// Returns the path of a recorded request.
pub(crate) fn path(request: &str) -> String {
    let target = request.split_whitespace().nth(1).expect("request target");
    target.split('?').next().unwrap_or_default().to_owned()
}

/// Returns the decoded query parameters of a recorded request.
pub(crate) fn query(request: &str) -> BTreeMap<String, String> {
    let target = request.split_whitespace().nth(1).expect("request target");
    reqwest::Url::parse(&format!("http://fixture.test{target}"))
        .expect("request URL")
        .query_pairs()
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect()
}

pub(crate) fn pairs(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect()
}

pub(crate) fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Runs `forager platform reddit <arguments>`.
pub(crate) fn run(environment: &super::RunEnvironment, arguments: &[&str]) -> Output {
    let mut command = vec!["platform", "reddit"];
    command.extend_from_slice(arguments);
    environment.run(&command)
}
