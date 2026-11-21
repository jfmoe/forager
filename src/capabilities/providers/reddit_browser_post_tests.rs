use serde_json::Value;

use super::super::projection_script;
use super::{POST_PROJECTION, PostObservation, PostProjection, PostTarget, post_outcome};
use crate::types::{AttemptErrorKind, ContentDepth, FullTextSource, RedditRef};

const POST_PAGE: &str = "reddit_browser/post.json";
const BIG_POST_PAGE: &str = "reddit_browser/big-post.json";
const MISSING_POST_PAGE: &str = "reddit_browser/missing-post.json";

fn fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("read {path}: {error}"))
}

fn projection(name: &str) -> PostProjection {
    serde_json::from_str(&fixture(name)).unwrap_or_else(|error| panic!("decode {name}: {error}"))
}

fn reference(id: &str) -> RedditRef {
    RedditRef::from_id(id).expect("a post ID")
}

fn observed(projection: PostProjection) -> PostObservation {
    PostObservation {
        projection,
        document_status: Some(200),
        timed_out: false,
    }
}

/// The fetched item as the command prints it, with the content source beside it.
fn fetched(
    observation: &PostObservation,
    id: &str,
    depth: ContentDepth,
) -> Result<(Value, FullTextSource), (AttemptErrorKind, String)> {
    post_outcome(observation, &reference(id), depth)
        .map(|(item, source)| {
            (
                serde_json::to_value(item).expect("an item serializes"),
                source,
            )
        })
        .map_err(|failure| (failure.kind, failure.message))
}

fn failure(observation: &PostObservation, id: &str) -> (AttemptErrorKind, String) {
    fetched(observation, id, ContentDepth::Thread).expect_err("a failed fetch")
}

const POST_TEXT: &str = "I have written C for years and the example runtime still surprises me. The [FAQ](https://www.reddit.com/r/rust/wiki/faq) says `spawn` needs **Send**.\n\n- Pinning\n- Cancellation\n\n```\nasync fn main() {}\n```";

#[test]
fn metadata_reads_the_post_element_and_its_text() {
    let (item, source) = fetched(
        &observed(projection(POST_PAGE)),
        "1q2w3e4",
        ContentDepth::Metadata,
    )
    .expect("a post page");

    assert_eq!(
        (item, source),
        (
            serde_json::json!({
                "ref": "reddit:1q2w3e4",
                "url": "https://www.reddit.com/comments/1q2w3e4/",
                "depth": "metadata",
                "title": "Learning the example runtime feels slow",
                "authors": ["example_poster"],
                "published": "2026-08-30T18:31:14Z",
                "subreddit": "rust",
                "permalink": "https://www.reddit.com/r/rust/comments/1q2w3e4/learning_the_example_runtime/",
                "post_type": "text",
                "link_url": null,
                "score": 389,
                "comments": 94,
                "nsfw": false,
                "spoiler": null,
                "snippet": null,
                "snippet_source": null,
                "text": POST_TEXT,
            }),
            FullTextSource::Urls(Vec::new())
        )
    );
}

const POST_THREAD: &str = "# Learning the example runtime feels slow

- Ref: `reddit:1q2w3e4` <https://www.reddit.com/comments/1q2w3e4/>
- Permalink: <https://www.reddit.com/r/rust/comments/1q2w3e4/learning_the_example_runtime/>
- Subreddit: r/rust
- Author: u/example_poster
- Score: 389
- Comments: 94
- Published: 2026-08-30T18:31:14Z

I have written C for years and the example runtime still surprises me. The [FAQ](https://www.reddit.com/r/rust/wiki/faq) says `spawn` needs **Send**.

- Pinning
- Cancellation

```
async fn main() {}
```

## Comments

The first page of comments in Reddit's best order: 6 of 94 comments.

- **u/example_one** · 156 points · 2026-08-30T19:48:51Z · [t1_c1top01](https://www.reddit.com/r/rust/comments/1q2w3e4/comment/c1top01/)

  Did you try the [tutorial](https://example.org/tutorial)?

  - **u/example_two** · 89 points · 2026-08-31T01:55:59Z · [t1_c1rep02](https://www.reddit.com/r/rust/comments/1q2w3e4/comment/c1rep02/)

    Seconding this, it *helped*.

    - **u/example_three** · 22 points · 2026-08-31T11:34:01Z · [t1_c1deep3](https://www.reddit.com/r/rust/comments/1q2w3e4/comment/c1deep3/)

      > helped

      Same here.

- **u/example_four** · 70 points · 2026-08-30T18:54:04Z · [t1_c1top04](https://www.reddit.com/r/rust/comments/1q2w3e4/comment/c1top04/)

  Give it time.

- **[deleted]** · 2026-08-31T00:02:17Z · `t1_c1gone5`

  *[deleted or removed]*

  - **u/example_six** · 2 points · 2026-08-31T14:01:27Z · [t1_c1orph6](https://www.reddit.com/r/rust/comments/1q2w3e4/comment/c1orph6/)

    What did they say?
";

#[test]
fn thread_writes_the_post_and_the_page_comments_nested_by_depth_in_the_page_order() {
    let (item, source) = fetched(
        &observed(projection(POST_PAGE)),
        "1q2w3e4",
        ContentDepth::Thread,
    )
    .expect("a post page");

    assert_eq!(
        (
            [
                &item["depth"],
                &item["comments_delivered"],
                &item["has_more_comments"],
                &item["order"],
            ],
            source,
        ),
        (
            [
                &serde_json::json!("thread"),
                &serde_json::json!(6),
                &serde_json::json!(false),
                &serde_json::json!("best"),
            ],
            FullTextSource::Native(POST_THREAD.to_owned()),
        )
    );
}

#[test]
fn a_page_that_marks_more_comments_says_the_thread_holds_only_its_first_batch() {
    let (item, source) = fetched(
        &observed(projection(BIG_POST_PAGE)),
        "1z9y8x7",
        ContentDepth::Thread,
    )
    .expect("a post page");

    let FullTextSource::Native(markdown) = source else {
        panic!("a discussion file")
    };
    assert_eq!(
        (
            &item["has_more_comments"],
            &item["order"],
            markdown
                .lines()
                .find(|line| line.starts_with("The first page")),
        ),
        (
            &serde_json::json!(true),
            &serde_json::json!("top"),
            Some(
                "The first page of comments in Reddit's top order: 1 of 1369 comments; Reddit has more that this file does not hold."
            ),
        )
    );
}

#[test]
fn each_comment_sort_the_page_reports_maps_to_its_order() {
    let orders = ["TOP", "CONFIDENCE", "NEW", "CONTROVERSIAL", "OLD", "QA"].map(|sort| {
        let mut page = projection(POST_PAGE);
        page.sort = Some(sort.to_owned());
        fetched(&observed(page), "1q2w3e4", ContentDepth::Thread)
            .map(|(item, _)| item["order"].clone())
            .expect("a post page")
    });

    assert_eq!(
        orders,
        ["top", "best", "new", "controversial", "old", "qa"].map(Value::from)
    );
}

#[test]
fn a_comment_sort_forager_does_not_know_is_a_format_change() {
    let mut page = projection(POST_PAGE);
    page.sort = Some("RANDOM".to_owned());

    assert_eq!(
        failure(&observed(page), "1q2w3e4"),
        (
            AttemptErrorKind::Runtime,
            "the Reddit post page orders its comments by `RANDOM`, which forager does not know; the page format may have changed".to_owned()
        )
    );
}

#[test]
fn the_comment_sort_is_read_only_for_the_thread() {
    let mut page = projection(POST_PAGE);
    page.sort = Some("RANDOM".to_owned());

    let item = fetched(&observed(page), "1q2w3e4", ContentDepth::Metadata).map(|(item, _)| item);

    assert_eq!(item.map(|item| item.get("order").cloned()), Ok(None));
}

#[test]
fn full_text_of_a_post_with_text_is_its_title_and_text() {
    let (_, source) = fetched(
        &observed(projection(POST_PAGE)),
        "1q2w3e4",
        ContentDepth::FullText,
    )
    .expect("a post page");

    assert_eq!(
        source,
        FullTextSource::Native(format!(
            "# Learning the example runtime feels slow\n\n{POST_TEXT}"
        ))
    );
}

#[test]
fn full_text_of_a_link_post_is_the_page_it_links() {
    let mut page = projection(POST_PAGE);
    let post = page.post.as_mut().expect("a post");
    post.post_type = Some("link".to_owned());
    post.content_href = Some("https://example.org/article".to_owned());
    post.body = None;

    let (item, source) =
        fetched(&observed(page), "1q2w3e4", ContentDepth::FullText).expect("a post page");

    assert_eq!(
        (&item["link_url"], source),
        (
            &serde_json::json!("https://example.org/article"),
            FullTextSource::Urls(vec!["https://example.org/article".to_owned()])
        )
    );
}

#[test]
fn full_text_of_a_gallery_without_text_is_a_quality_failure_naming_its_media() {
    let failed = fetched(
        &observed(projection(BIG_POST_PAGE)),
        "1z9y8x7",
        ContentDepth::FullText,
    )
    .map(|_| ());

    assert_eq!(
        failed,
        Err((
            AttemptErrorKind::Quality,
            "reddit:1z9y8x7 is a post of type `gallery` without text of its own; its media is at https://www.reddit.com/gallery/1z9y8x7; use --depth thread to read its discussion".to_owned()
        ))
    );
}

#[test]
fn full_text_of_a_post_type_forager_does_not_know_is_a_quality_failure_even_with_text() {
    let mut page = projection(POST_PAGE);
    page.post.as_mut().expect("a post").post_type = Some("poll".to_owned());

    let failed = fetched(&observed(page), "1q2w3e4", ContentDepth::FullText).map(|_| ());

    assert_eq!(
        failed.map_err(|(kind, _)| kind),
        Err(AttemptErrorKind::Quality)
    );
}

#[test]
fn the_adult_flag_comes_from_the_screen_view_record() {
    let mut page = projection(POST_PAGE);
    page.screenview.as_mut().expect("a record").nsfw = Some(true);

    let item = fetched(&observed(page), "1q2w3e4", ContentDepth::Metadata).map(|(item, _)| item);

    assert_eq!(item.map(|item| item["nsfw"].clone()), Ok(Value::Bool(true)));
}

#[test]
fn a_logged_in_loaded_page_without_a_post_is_a_missing_post() {
    let failed = failure(&observed(projection(MISSING_POST_PAGE)), "1m5n6b7");

    assert_eq!(
        failed,
        (
            AttemptErrorKind::Parameter,
            "Reddit post not found: reddit:1m5n6b7; it may be deleted, private, or restricted, or hidden by the account's adult-content setting".to_owned()
        )
    );
}

#[test]
fn a_page_that_shows_another_post_delivers_nothing() {
    let failed = failure(&observed(projection(POST_PAGE)), "1a1a1a1");

    assert_eq!(
        failed,
        (
            AttemptErrorKind::Runtime,
            "the Reddit post page shows t3_1q2w3e4 instead of t3_1a1a1a1".to_owned()
        )
    );
}

#[test]
fn a_screen_view_record_of_another_post_delivers_nothing() {
    let mut page = projection(POST_PAGE);
    page.screenview.as_mut().expect("a record").id = Some("t3_1a1a1a1".to_owned());

    assert_eq!(
        failure(&observed(page), "1q2w3e4"),
        (
            AttemptErrorKind::Runtime,
            "the Reddit post page shows t3_1a1a1a1 instead of t3_1q2w3e4".to_owned()
        )
    );
}

#[test]
fn a_post_element_without_a_title_is_a_format_change() {
    let mut page = projection(POST_PAGE);
    page.post.as_mut().expect("a post").title = None;

    assert_eq!(
        failure(&observed(page), "1q2w3e4"),
        (
            AttemptErrorKind::Runtime,
            "the Reddit post page gives no title for t3_1q2w3e4; the page format may have changed"
                .to_owned()
        )
    );
}

#[test]
fn a_refused_document_is_auth_even_with_the_post_shown() {
    let observation = PostObservation {
        document_status: Some(429),
        ..observed(projection(POST_PAGE))
    };

    assert_eq!(failure(&observation, "1q2w3e4").0, AttemptErrorKind::Auth);
}

/// The kind of a read whose deadline passed while the page at `url` showed no post.
fn unfinished_at(url: &str) -> AttemptErrorKind {
    let mut page = projection(MISSING_POST_PAGE);
    page.page.url = url.to_owned();
    page.page.ready_state = "interactive".to_owned();
    let observation = PostObservation {
        timed_out: true,
        ..observed(page)
    };
    failure(&observation, "1m5n6b7").0
}

#[test]
fn a_read_deadline_on_the_page_the_post_redirects_to_is_a_timeout() {
    assert_eq!(
        unfinished_at("https://www.reddit.com/r/Example_Adult/comments/1m5n6b7/a_slug/"),
        AttemptErrorKind::Timeout
    );
}

#[test]
fn a_read_deadline_on_another_page_is_an_unexpected_page() {
    assert_eq!(
        unfinished_at("https://www.reddit.com/"),
        AttemptErrorKind::Runtime
    );
}

#[test]
fn the_post_page_url_asks_for_the_top_comments() {
    assert_eq!(
        PostTarget::new(&reference("1q2w3e4")).url,
        "https://www.reddit.com/comments/1q2w3e4/?sort=top"
    );
}

/// Runs the post projection on local pages trimmed from those Reddit served on 2026-11-21, with
/// fake post, author, and account values, and compares it with the projections the judgement
/// tests read; with `FORAGER_WRITE_PROJECTIONS` set, it writes them instead.
#[cfg(unix)]
#[test]
#[ignore = "launches a real browser"]
fn the_projection_of_each_local_post_page_is_the_fixture_the_judgement_reads() {
    const POST_PATH: &str = "/r/rust/comments/1q2w3e4/learning_the_example_runtime/";
    const BIG_POST_PATH: &str = "/r/example_sub/comments/1z9y8x7/an_example_gallery/";
    const MISSING_POST_PATH: &str = "/r/example_adult/comments/1m5n6b7/a_slug/";
    super::super::tests::check_projections(
        &[
            (POST_PATH, "reddit_browser/post.html"),
            (BIG_POST_PATH, "reddit_browser/big-post.html"),
            (MISSING_POST_PATH, "reddit_browser/missing-post.html"),
        ],
        &[
            (POST_PATH, "reddit_browser/post.html", POST_PAGE),
            (BIG_POST_PATH, "reddit_browser/big-post.html", BIG_POST_PAGE),
            (
                MISSING_POST_PATH,
                "reddit_browser/missing-post.html",
                MISSING_POST_PAGE,
            ),
        ],
        &projection_script(POST_PROJECTION),
    );
}

#[test]
fn a_page_without_a_post_whose_document_failed_on_the_server_is_no_missing_post() {
    let observation = PostObservation {
        document_status: Some(503),
        ..observed(projection(MISSING_POST_PAGE))
    };

    let failed = post_outcome(&observation, &reference("1m5n6b7"), ContentDepth::Thread)
        .map(|_| ())
        .map_err(|failure| (failure.kind, failure.status, failure.message));

    assert_eq!(
        failed,
        Err((
            AttemptErrorKind::Runtime,
            Some(503),
            "Reddit answered the post page with HTTP 503 and showed no post; the site may be failing, so retry later".to_owned()
        ))
    );
}

#[test]
fn a_loaded_error_page_whose_document_failed_on_the_server_ends_the_read() {
    let mut page = projection(MISSING_POST_PAGE);
    page.page.app = false;
    page.page.user_logged_in = None;
    page.screenview = None;

    assert!(super::post_ended(&page, Some(502)));
}
