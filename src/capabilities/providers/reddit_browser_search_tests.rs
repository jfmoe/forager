use serde_json::Value;

use super::super::projection_script;
use super::{SEARCH_PROJECTION, SearchObservation, SearchProjection, SearchTarget, search_outcome};
use crate::types::{AttemptErrorKind, RedditSearchOptions};

const SEARCH_PAGE: &str = "reddit_browser/search.json";
const CAPTCHA_PAGE: &str = "reddit_browser/captcha.json";

fn fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("read {path}: {error}"))
}

fn projection(name: &str) -> SearchProjection {
    serde_json::from_str(&fixture(name)).unwrap_or_else(|error| panic!("decode {name}: {error}"))
}

fn global_target() -> SearchTarget {
    SearchTarget::new("example runtime", &RedditSearchOptions::default())
}

fn observed(projection: SearchProjection) -> SearchObservation {
    SearchObservation {
        projection,
        document_status: Some(200),
        timed_out: false,
    }
}

fn outcome(
    observation: &SearchObservation,
    limit: usize,
) -> Result<Value, (AttemptErrorKind, Option<u16>, String)> {
    search_outcome(observation, &global_target(), limit)
        .map(|decoded| {
            serde_json::json!({
                "items": decoded.items,
                "skipped": decoded.skipped,
                "incomplete": decoded.incomplete,
            })
        })
        .map_err(|failure| (failure.kind, failure.status, failure.message))
}

#[test]
fn the_results_page_decodes_into_post_items() {
    let decoded = outcome(&observed(projection(SEARCH_PAGE)), 10).expect("a results page");

    assert_eq!(
        decoded["items"],
        serde_json::json!([
            {
                "ref": "reddit:1a2b3c4",
                "url": "https://www.reddit.com/comments/1a2b3c4/",
                "depth": "snippet",
                "title": "Why is the example runtime so big?",
                "authors": ["example_author_one"],
                "published": "2026-06-13T07:09:42Z",
                "subreddit": "rust",
                "permalink": "https://www.reddit.com/r/rust/comments/1a2b3c4/why_is_the_example_runtime_so_big/",
                "post_type": null,
                "link_url": null,
                "score": 124,
                "comments": 53,
                "nsfw": false,
                "spoiler": false,
                "snippet": "A fake comment excerpt about a minimal runtime.",
                "snippet_source": "comment"
            },
            {
                "ref": "reddit:1d5e6f7",
                "url": "https://www.reddit.com/comments/1d5e6f7/",
                "depth": "snippet",
                "title": "An example FFI bridge for Rust <-> C# interop",
                "authors": ["example_author_two"],
                "published": "2026-08-31T15:25:23Z",
                "subreddit": "rust",
                "permalink": "https://www.reddit.com/r/rust/comments/1d5e6f7/an_example_ffi_bridge_rust_c_interop/",
                "post_type": null,
                "link_url": null,
                "score": 59,
                "comments": 10,
                "nsfw": false,
                "spoiler": false,
                "snippet": "The post itself says how the two runtimes talk “across” the C ABI.",
                "snippet_source": "post"
            },
            {
                "ref": "reddit:f9g8h7",
                "url": "https://www.reddit.com/comments/f9g8h7/",
                "depth": "metadata",
                "title": "An older example post about async",
                "authors": ["example_author_three"],
                "published": "2020-03-23T12:20:15Z",
                "subreddit": "rust",
                "permalink": "https://www.reddit.com/r/rust/comments/f9g8h7/an_older_example_post_about_async/",
                "post_type": null,
                "link_url": null,
                "score": 281,
                "comments": 44,
                "nsfw": false,
                "spoiler": false,
                "snippet": null,
                "snippet_source": null
            },
            {
                "ref": "reddit:1j2k3l4",
                "url": "https://www.reddit.com/comments/1j2k3l4/",
                "depth": "snippet",
                "title": "Looking for an example runtime",
                "authors": [],
                "published": "2024-04-04T07:54:28Z",
                "subreddit": "rust",
                "permalink": "https://www.reddit.com/r/rust/comments/1j2k3l4/looking_for_an_example_runtime/",
                "post_type": null,
                "link_url": null,
                "score": 6,
                "comments": 8,
                "nsfw": false,
                "spoiler": false,
                "snippet": "A fake excerpt of a post whose author deleted the account.",
                "snippet_source": "post"
            }
        ])
    );
}

/// The projection pages with what the browser served each from.
#[cfg(unix)]
const PROJECTED_PAGES: [(&str, &str, &str); 2] = [
    (
        "/search/?q=example+runtime&type=posts&sort=relevance",
        "reddit_browser/search.html",
        SEARCH_PAGE,
    ),
    (
        "/r/rust/search/?q=example+runtime&restrict_sr=1&sort=relevance",
        "reddit_browser/captcha.html",
        CAPTCHA_PAGE,
    ),
];

/// Runs the search projection on the local copies of the pages Reddit served on 2026-11-21 and
/// compares it with the projections the judgement tests read; with
/// `FORAGER_WRITE_PROJECTIONS` set, it writes them instead.
#[cfg(unix)]
#[test]
#[ignore = "launches a real browser"]
fn the_projection_of_each_local_page_is_the_fixture_the_judgement_reads() {
    use crate::providers::real_browser::{Pages, Sandbox, executable};

    const SERVED: crate::providers::real_browser::FixturePages = &[
        ("/search/", "reddit_browser/search.html"),
        ("/r/rust/search/", "reddit_browser/captcha.html"),
    ];
    let pages = Pages::serving(SERVED);
    let executable = executable();
    let script = projection_script(SEARCH_PROJECTION);
    for (path, served, expected) in PROJECTED_PAGES {
        let url = pages.url(path);
        let projected: Value = Sandbox::new().run(&executable, async |page| {
            page.navigate(&url).await?;
            page.evaluate(&script).await
        });
        let projected = serde_json::to_string_pretty(&projected)
            .expect("a JSON value serializes")
            .replace(pages.origin(), super::SITE_ORIGIN);
        let file = format!("{}/tests/fixtures/{expected}", env!("CARGO_MANIFEST_DIR"));
        if std::env::var_os("FORAGER_WRITE_PROJECTIONS").is_some() {
            std::fs::write(&file, format!("{projected}\n")).expect("write the projection");
            continue;
        }
        assert_eq!(
            projected.trim_end(),
            fixture(expected).trim_end(),
            "{served}"
        );
    }
}

fn ids(decoded: &Value) -> Vec<String> {
    decoded["items"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|item| item["ref"].as_str().unwrap_or_default().to_owned())
                .collect()
        })
        .unwrap_or_default()
}

fn failure(observation: &SearchObservation) -> (AttemptErrorKind, Option<u16>, String) {
    outcome(observation, 10).expect_err("a failed read")
}

/// A logged-in search page that has finished loading and shows no results yet.
fn bare_page() -> SearchProjection {
    SearchProjection {
        results: Vec::new(),
        next_page: false,
        ..projection(SEARCH_PAGE)
    }
}

#[test]
fn a_post_that_a_later_page_repeats_is_delivered_once_and_the_items_stop_at_the_limit() {
    let mut merged = projection(SEARCH_PAGE);
    let first = merged.results[..2].to_vec();
    merged.results.truncate(2);
    let mut later = projection(SEARCH_PAGE);
    // The repeat carries another position in its tracking record, so the page shows it as a
    // new result.
    let mut repeat = first[0].clone();
    repeat.tracking = repeat
        .tracking
        .map(|tracking| tracking.replace("\"position\":0", "\"position\":7"));
    later.results = vec![repeat, later.results[2].clone(), later.results[3].clone()];
    merged.absorb(later);

    let decoded = outcome(&observed(merged), 3).expect("a results page");

    assert_eq!(
        (ids(&decoded), decoded["incomplete"].clone()),
        (
            vec![
                "reddit:1a2b3c4".to_owned(),
                "reddit:1d5e6f7".to_owned(),
                "reddit:f9g8h7".to_owned()
            ],
            Value::Null
        )
    );
}

#[test]
fn fewer_results_than_the_limit_with_a_next_page_left_say_the_list_is_incomplete() {
    let with_next_page = outcome(&observed(projection(SEARCH_PAGE)), 10).expect("a results page");
    let last_page = outcome(
        &observed(SearchProjection {
            next_page: false,
            ..projection(SEARCH_PAGE)
        }),
        10,
    )
    .expect("a results page");

    assert_eq!(
        (
            with_next_page["incomplete"].clone(),
            last_page["incomplete"].clone()
        ),
        (
            Value::from(
                "Reddit has more results than the 4 this command read; Reddit search issues no cursor to continue from, so narrow the query, the subreddit, or the time window to reach them"
            ),
            Value::Null
        )
    );
}

#[test]
fn results_that_name_no_post_are_skipped_and_counted() {
    let mut page = projection(SEARCH_PAGE);
    page.results[1].tracking = Some(r#"{"post":{"id":"t5_2s7lj"}}"#.into());
    page.results[2].tracking = None;

    let decoded = outcome(&observed(page), 10).expect("a results page");

    assert_eq!(
        (ids(&decoded).len(), decoded["skipped"].clone()),
        (
            2,
            Value::from("skipped 2 Reddit search results that could not be read")
        )
    );
}

#[test]
fn a_page_whose_results_all_name_no_post_is_a_format_change() {
    let mut page = projection(SEARCH_PAGE);
    for result in &mut page.results {
        result.tracking = Some("{}".into());
    }

    assert_eq!(
        failure(&observed(page)),
        (
            AttemptErrorKind::Runtime,
            None,
            "none of the 4 Reddit search results could be read; the page format may have changed"
                .into()
        )
    );
}

#[test]
fn a_page_marked_as_having_no_results_is_an_empty_search() {
    let decoded = outcome(
        &observed(SearchProjection {
            no_results: true,
            ..bare_page()
        }),
        10,
    )
    .expect("an empty search");

    assert_eq!(decoded["items"], serde_json::json!([]));
}

#[test]
fn the_verification_page_is_auth() {
    let (kind, status, message) = failure(&observed(projection(CAPTCHA_PAGE)));

    assert_eq!((kind, status), (AttemptErrorKind::Auth, None));
    assert!(
        message.starts_with("Reddit showed a verification page instead of `Reddit - Prove your humanity` (https://www.reddit.com/r/rust/search/")
            && message.contains("run `forager browser login reddit`"),
        "{message}"
    );
}

#[test]
fn a_refused_document_is_auth_with_its_status_even_with_results_shown() {
    let refused = [403, 429].map(|status| {
        failure(&SearchObservation {
            document_status: Some(status),
            ..observed(projection(SEARCH_PAGE))
        })
    });

    assert_eq!(
        refused.map(|(kind, status, _)| (kind, status)),
        [
            (AttemptErrorKind::Auth, Some(403)),
            (AttemptErrorKind::Auth, Some(429))
        ]
    );
}

#[test]
fn a_page_that_is_not_logged_in_is_auth() {
    let mut page = projection(SEARCH_PAGE);
    page.page.user_logged_in = Some("false".into());
    let mut unmarked = projection(SEARCH_PAGE);
    unmarked.page.user_logged_in = None;

    let failures = [page, unmarked].map(|page| failure(&observed(page)));

    assert_eq!(
        failures,
        [(), ()].map(|()| (
            AttemptErrorKind::Auth,
            None,
            "Reddit treats the browser session as logged out; run `forager browser login reddit`, then retry".to_owned()
        ))
    );
}

#[test]
fn a_loaded_search_page_without_results_or_a_no_results_notice_is_a_format_change() {
    let mut elsewhere = bare_page();
    elsewhere.page.url = "https://www.reddit.com/".into();
    elsewhere.page.title = "Reddit - The heart of the internet".into();

    let failures = [bare_page(), elsewhere].map(|page| failure(&observed(page)));

    assert_eq!(
        failures,
        [
            (
                AttemptErrorKind::Runtime,
                None,
                "the Reddit search page shows neither results nor a no-results notice; the page format may have changed".to_owned()
            ),
            (
                AttemptErrorKind::Runtime,
                None,
                "the Reddit page logic stopped at an unexpected page: `Reddit - The heart of the internet` (https://www.reddit.com/)".to_owned()
            ),
        ]
    );
}

#[test]
fn a_read_deadline_without_a_final_page_is_network_timeout_or_runtime_by_where_it_stopped() {
    let mut not_loaded = bare_page();
    not_loaded.page.load_error = Some("ERR_CONNECTION_CLOSED".into());
    let mut elsewhere = bare_page();
    elsewhere.page.url = "https://www.reddit.com/".into();
    elsewhere.page.title = "Reddit".into();

    let failures = [not_loaded, bare_page(), elsewhere].map(|page| {
        let (kind, _, message) = failure(&SearchObservation {
            timed_out: true,
            ..observed(page)
        });
        (kind, message)
    });

    assert_eq!(
        failures,
        [
            (
                AttemptErrorKind::Network,
                "the browser could not load the Reddit page (ERR_CONNECTION_CLOSED) and had not loaded it again by the read deadline; check the network and retry".to_owned()
            ),
            (
                AttemptErrorKind::Timeout,
                "the Reddit search page showed no results before the read deadline".to_owned()
            ),
            (
                AttemptErrorKind::Runtime,
                "the Reddit page logic stopped at an unexpected page: `Reddit` (https://www.reddit.com/)".to_owned()
            ),
        ]
    );
}

#[test]
fn the_search_url_carries_the_query_scope_order_and_window() {
    use crate::types::{RedditSort, RedditTime};

    let urls = [
        RedditSearchOptions {
            sort: RedditSort::Top,
            time: Some(RedditTime::Year),
            ..RedditSearchOptions::default()
        },
        RedditSearchOptions {
            subreddit: Some("rust".into()),
            sort: RedditSort::New,
            time: None,
        },
    ]
    .map(|options| SearchTarget::new("async runtime", &options));

    assert_eq!(
        urls.map(|target| (target.url, target.path)),
        [
            (
                "https://www.reddit.com/search/?q=async+runtime&type=posts&sort=top&t=year"
                    .to_owned(),
                "/search/".to_owned()
            ),
            (
                "https://www.reddit.com/r/rust/search/?q=async+runtime&restrict_sr=1&sort=new"
                    .to_owned(),
                "/r/rust/search/".to_owned()
            ),
        ]
    );
}
