//! Live smoke of the Reddit cases: they run only once the user enables `reddit_web`.

mod support;

use serde_json::Value;
use support::Fixture;
use support::reddit::{
    Comment, EmbedPost, SearchResult, comments_page, embed_page, path, permalink_path, search_page,
};
use support::smoke::{DISABLED_PLATFORMS, SmokeEnvironment, case};

const CANARY: &str = "17snc7d";

// Debug path formatting supplies the quoted and escaped TOML string literal required by fixtures.
#[expect(clippy::unnecessary_debug_formatting)]
fn run(url: &str, order: &str) -> [Value; 2] {
    let environment = SmokeEnvironment::new(|journal_dir| {
        format!(
            "[providers.reddit_web]\nurl = \"{url}\"\nembed_url = \"{url}\"\n[platforms.reddit]\norder = {order}\n[journal]\ndir = {journal_dir:?}\n{DISABLED_PLATFORMS}"
        )
    });
    let output = environment.run(&["smoke", "--live", "--timeout", "20"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse live smoke JSON");
    ["C47", "C48"].map(|id| case(&payload, id)["status"].clone())
}

#[test]
fn live_smoke_runs_the_reddit_cases_only_once_the_route_is_enabled() {
    let reddit = Fixture::start_sequence(vec![
        search_page(
            &[SearchResult::new("1unx4wf", "Guidance on Async Rust")],
            None,
        ),
        comments_page(
            CANARY,
            &permalink_path(CANARY),
            1,
            &[Comment::new("k1", 0, "ferris", "<p>Hello.</p>")],
            false,
        ),
        embed_page(&EmbedPost::text(CANARY, "<p>The canary post.</p>")),
    ]);

    let disabled = run(&reddit.url, "[]");
    let enabled = run(&reddit.url, "[\"reddit_web\"]");
    let requests = reddit.finish_all();

    assert_eq!(
        (
            disabled,
            enabled,
            requests
                .iter()
                .map(|request| path(request))
                .collect::<Vec<_>>(),
        ),
        (
            ["unconfigured", "unconfigured"].map(|status| Value::String(status.into())),
            ["passed", "passed"].map(|status| Value::String(status.into())),
            vec![
                "/svc/shreddit/search/".to_owned(),
                format!("/svc/shreddit/comments/r/all/t3_{CANARY}"),
                permalink_path(CANARY),
            ],
        )
    );
}
