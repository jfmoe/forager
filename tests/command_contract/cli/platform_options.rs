use std::collections::BTreeSet;

use clap::CommandFactory;
use forager::app::Cli;

use crate::support::{Fixture, Response, RunEnvironment};

/// Options every platform command shares; this test covers only route-specific options.
const SHARED: &[&str] = &[
    "--limit",
    "--cursor",
    "--format",
    "--timeout",
    "--output",
    "--receipt",
    "--verbose",
    "--help",
    "--depth",
    "--content-dir",
];

/// A non-default value of each route option, by platform command; an empty value is a flag.
const SAMPLES: &[(&str, &str, &str, &str)] = &[
    ("arxiv", "search", "--category", "q-fin.PM"),
    ("arxiv", "search", "--author", "Smith"),
    ("arxiv", "search", "--title", "Graph"),
    ("arxiv", "search", "--submitted-from", "2024-01-01"),
    ("arxiv", "search", "--submitted-to", "2024-02-01"),
    ("arxiv", "search", "--sort", "submitted"),
    ("ssrn", "search", "--scope", "title"),
    ("ssrn", "search", "--author", "Smith"),
    ("ssrn", "search", "--affiliation", "MIT"),
    ("ssrn", "search", "--published-from", "2020-01-01"),
    ("ssrn", "search", "--published-to", "2021-01-01"),
    ("ssrn", "search", "--created-from", "2020-01-01"),
    ("ssrn", "search", "--created-to", "2021-01-01"),
    ("ssrn", "search", "--updated-from", "2020-01-01"),
    ("ssrn", "search", "--updated-to", "2021-01-01"),
    ("ssrn", "search", "--has-abstract", ""),
    ("ssrn", "search", "--type", "journal-article"),
    ("ssrn", "search", "--orcid", "0000-0002-1825-0097"),
    ("ssrn", "search", "--funder", "10.13039/100000001"),
    ("ssrn", "search", "--sort", "citations"),
    ("ssrn", "search", "--order", "asc"),
    ("scholar", "search", "--year-from", "2020"),
    ("scholar", "search", "--year-to", "2021"),
    ("scholar", "search", "--review-only", ""),
    ("x", "search", "--handle", "forager"),
    ("x", "search", "--exclude-handle", "forager"),
    ("x", "search", "--from", "2024-01-01"),
    ("x", "search", "--to", "2024-02-01"),
    ("hn", "search", "--sort", "date"),
    ("hn", "search", "--type", "story"),
    ("hn", "search", "--author", "pg"),
    ("hn", "search", "--story", "8863"),
    ("hn", "search", "--from", "2024-01-01"),
    ("hn", "search", "--to", "2024-02-01"),
    ("hn", "search", "--min-points", "10"),
    ("hn", "search", "--min-comments", "5"),
    ("youtube", "search", "--sort", "date"),
    ("youtube", "search", "--from", "2024-01-01"),
    ("youtube", "search", "--to", "2024-02-01"),
    ("youtube", "search", "--duration", "short"),
    (
        "youtube",
        "search",
        "--channel",
        "youtube:channel/UCYO_jab_esuFRV4b17AJtAw",
    ),
    ("youtube", "search", "--has-captions", ""),
    ("youtube", "fetch", "--lang", "en"),
    ("polymarket", "search", "--include-closed", ""),
];

/// Options of browser routes: they become clicks and navigation in Page Logic, which no
/// ordinary test can drive, so only the live smoke proves them.
const LIVE_SMOKE_ONLY: &[(&str, &str, &str)] = &[
    ("xiaohongshu", "search", "--sort"),
    ("xiaohongshu", "search", "--note-type"),
    ("xiaohongshu", "search", "--publish-time"),
    ("reddit", "search", "--subreddit"),
    ("reddit", "search", "--sort"),
    ("reddit", "search", "--time"),
];

fn config(url: &str) -> String {
    format!(
        r#"
[providers.arxiv_api]
url = "{url}"
[providers.ssrn_crossref]
url = "{url}"
[providers.serpapi]
url = "{url}"
keys = ["k"]
[providers.xai]
url = "{url}"
keys = ["k"]
model = "grok-test"
tools = ["web_search", "x_search"]
[providers.hn_api]
url = "{url}"
firebase_url = "{url}"
[providers.youtube_api]
url = "{url}"
keys = ["k"]
[providers.polymarket_api]
url = "{url}"
[platforms.youtube]
order = ["youtube_api"]
[retry]
max_attempts = 1
[journal]
enabled = false
"#
    )
}

fn baseline(platform: &str, operation: &str) -> Vec<String> {
    let target: &[&str] = if operation == "fetch" {
        &["youtube:jNQXAC9IVRw", "--depth", "abstract"]
    } else {
        &["x"]
    };
    ["platform", platform, operation]
        .iter()
        .chain(target)
        .map(|argument| (*argument).to_owned())
        .collect()
}

/// The long options of every platform's search and fetch, as `platform:operation:option`.
fn listed_options() -> BTreeSet<String> {
    let command = Cli::command();
    let platform = command
        .find_subcommand("platform")
        .expect("platform command");
    platform
        .get_subcommands()
        .flat_map(|platform| {
            platform
                .get_subcommands()
                .filter(|operation| ["search", "fetch"].contains(&operation.get_name()))
                .flat_map(move |operation| {
                    operation.get_arguments().filter_map(move |argument| {
                        argument.get_long().map(|long| {
                            format!("{} {} --{long}", platform.get_name(), operation.get_name())
                        })
                    })
                })
        })
        .collect()
}

/// The status and the request lines and bodies a route sent for `arguments`.
fn requests(arguments: &[String]) -> (Option<i32>, Vec<String>) {
    let fixture = Fixture::start_repeating(Response::json(500, "{}"));
    let environment = RunEnvironment::new(&config(&fixture.url));
    let arguments = arguments.iter().map(String::as_str).collect::<Vec<_>>();
    let status = environment.run(&arguments).status.code();
    let sent = fixture
        .finish_all()
        .iter()
        .map(|request| {
            let line = request.lines().next().unwrap_or_default();
            let body = request.split_once("\r\n\r\n").map_or("", |(_, body)| body);
            format!("{line}\n{body}")
        })
        .collect();
    (status, sent)
}

#[test]
fn every_route_option_has_a_sample_value() {
    let known = SAMPLES
        .iter()
        .map(|(platform, operation, option, _)| (*platform, *operation, *option))
        .chain(LIVE_SMOKE_ONLY.iter().copied())
        .map(|(platform, operation, option)| format!("{platform} {operation} {option}"))
        .collect::<BTreeSet<_>>();
    let missing = listed_options()
        .into_iter()
        .filter(|option| {
            !SHARED
                .iter()
                .any(|shared| option.ends_with(&format!(" {shared}")))
        })
        .filter(|option| !known.contains(option))
        .collect::<Vec<_>>();

    assert!(
        missing.is_empty(),
        "add a non-default value for each of these options to SAMPLES, or to LIVE_SMOKE_ONLY when only a browser route reads it: {missing:#?}"
    );
}

#[test]
fn a_route_does_not_silently_ignore_an_explicitly_passed_option() {
    let mut ignored = Vec::new();
    for (platform, operation, option, value) in SAMPLES {
        let base = baseline(platform, operation);
        let (_, base_sent) = requests(&base);
        assert!(!base_sent.is_empty(), "{base:?} sends no request");
        let mut with_option = base.clone();
        with_option.push((*option).to_owned());
        if !value.is_empty() {
            with_option.push((*value).to_owned());
        }
        let (status, sent) = requests(&with_option);
        let refused = status == Some(2) && sent.is_empty();
        if !refused && sent == base_sent {
            ignored.push((*platform, *operation, *option));
        }
    }

    assert!(
        ignored.is_empty(),
        "these options neither change the request nor stop the command before it sends; make the route use them or refuse them in its support check: {ignored:#?}"
    );
}
