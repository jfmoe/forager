mod support;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::{Value, json};

use support::youtube::{
    KEY, api_error, config, header, pairs, path, payload, query, search_page, search_result,
    stderr, video_stats, videos_page,
};
use support::{Fixture, Response, RunEnvironment};

fn search(environment: &RunEnvironment, arguments: &[&str]) -> std::process::Output {
    let mut command = vec!["platform", "youtube", "search"];
    command.extend_from_slice(arguments);
    environment.run(&command)
}

#[test]
fn options_map_to_data_api_parameters_and_the_key_travels_only_in_a_header() {
    let api = Fixture::start_sequence(vec![search_page(&[], None)]);
    let environment = RunEnvironment::new(&config(&api.url));

    let output = search(
        &environment,
        &[
            "attention -music",
            "--sort",
            "views",
            "--from",
            "2024-01-01",
            "--to",
            "2024-01-31",
            "--duration",
            "long",
            "--channel",
            "https://www.youtube.com/channel/UCYO_jab_esuFRV4b17AJtAw",
            "--has-captions",
            "--limit",
            "10",
        ],
    );
    let request = api.finish();

    assert_eq!(
        (
            output.status.code(),
            path(&request),
            query(&request),
            header(&request, "x-goog-api-key"),
            request.contains(&format!("key={KEY}")),
            payload(&output),
        ),
        (
            Some(0),
            "/youtube/v3/search".to_owned(),
            pairs(&[
                ("part", "snippet"),
                ("type", "video"),
                ("q", "attention -music"),
                ("maxResults", "10"),
                ("order", "viewCount"),
                ("publishedAfter", "2024-01-01T00:00:00Z"),
                ("publishedBefore", "2024-01-31T23:59:59Z"),
                ("videoDuration", "long"),
                ("channelId", "UCYO_jab_esuFRV4b17AJtAw"),
                ("videoCaption", "closedCaption"),
            ]),
            Some(KEY.to_owned()),
            false,
            json!({"platform": "youtube", "provider": "youtube_api", "items": [], "next_cursor": null}),
        ),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn results_carry_decoded_titles_and_the_batched_video_details() {
    let api = Fixture::start_sequence(vec![
        search_page(
            &[
                search_result(
                    "eMlx5fFNoYc",
                    "Attention in transformers, step-by-step | Deep Learning Chapter 6",
                    "Demystifying attention, the key mechanism inside transformers &amp; LLMs.",
                ),
                search_result(
                    "Ks-_Mh1QhMc",
                    "Your Body Language May Shape Who You Are &#39;TED&#39;",
                    "",
                ),
            ],
            Some("CAUQAA"),
        ),
        videos_page(&[
            video_stats("Ks-_Mh1QhMc", "PT21M3S", "29311923", None),
            video_stats("eMlx5fFNoYc", "PT26M10S", "4773774", Some("2478")),
        ]),
    ]);
    let environment = RunEnvironment::new(&config(&api.url));

    let output = search(&environment, &["attention"]);
    let requests = api.finish_all();
    let mut page = payload(&output);
    let cursor = page["next_cursor"].take();

    assert_eq!(
        (
            output.status.code(),
            query(&requests[0]).get("maxResults").cloned(),
            path(&requests[1]),
            query(&requests[1]),
            page,
            cursor
                .as_str()
                .is_some_and(|cursor| cursor.starts_with("v1.youtube_api.")),
        ),
        (
            Some(0),
            Some("20".to_owned()),
            "/youtube/v3/videos".to_owned(),
            pairs(&[
                ("part", "contentDetails,statistics"),
                ("id", "eMlx5fFNoYc,Ks-_Mh1QhMc")
            ]),
            json!({
                "platform": "youtube",
                "provider": "youtube_api",
                "items": [
                    {
                        "ref": "youtube:eMlx5fFNoYc",
                        "url": "https://www.youtube.com/watch?v=eMlx5fFNoYc",
                        "depth": "snippet",
                        "title": "Attention in transformers, step-by-step | Deep Learning Chapter 6",
                        "authors": ["3Blue1Brown"],
                        "published": "2024-04-07T12:53:54Z",
                        "channel_ref": "youtube:channel/UCYO_jab_esuFRV4b17AJtAw",
                        "snippet": "Demystifying attention, the key mechanism inside transformers & LLMs.",
                        "duration_seconds": 1570,
                        "live_status": "none",
                        "views": 4_773_774,
                        "likes": 91777,
                        "comments": 2478,
                        "language": null,
                        "has_captions": true,
                        "caption_tracks": null
                    },
                    {
                        "ref": "youtube:Ks-_Mh1QhMc",
                        "url": "https://www.youtube.com/watch?v=Ks-_Mh1QhMc",
                        "depth": "metadata",
                        "title": "Your Body Language May Shape Who You Are 'TED'",
                        "authors": ["3Blue1Brown"],
                        "published": "2024-04-07T12:53:54Z",
                        "channel_ref": "youtube:channel/UCYO_jab_esuFRV4b17AJtAw",
                        "snippet": null,
                        "duration_seconds": 1263,
                        "live_status": "none",
                        "views": 29_311_923,
                        "likes": 91777,
                        "comments": null,
                        "language": null,
                        "has_captions": true,
                        "caption_tracks": null
                    }
                ],
                "next_cursor": null
            }),
            true,
        ),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn a_failed_detail_batch_keeps_the_results_and_reports_what_is_missing() {
    let api = Fixture::start_sequence(vec![
        search_page(
            &[search_result("eMlx5fFNoYc", "Attention", "excerpt")],
            None,
        ),
        Response::json(
            503,
            r#"{"error": {"code": 503, "message": "Backend Error", "errors": [{"reason": "backendError"}]}}"#,
        ),
    ]);
    let environment = RunEnvironment::new(&config(&api.url));

    let output = search(&environment, &["attention"]);
    api.finish_all();
    let page = payload(&output);
    let item = &page["items"][0];

    assert_eq!(
        (
            output.status.code(),
            &item["ref"],
            &item["duration_seconds"],
            &item["views"],
            stderr(&output).contains("YouTube video details unavailable"),
        ),
        (
            Some(0),
            &json!("youtube:eMlx5fFNoYc"),
            &Value::Null,
            &Value::Null,
            true
        ),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn malformed_or_reversed_dates_fail_before_any_request() {
    let environment = RunEnvironment::new(&config("http://127.0.0.1:9"));

    let codes = [
        search(&environment, &["attention", "--from", "2024-02-30"]),
        search(&environment, &["attention", "--to", "2024-1-31"]),
        search(
            &environment,
            &["attention", "--from", "2024-02-01", "--to", "2024-01-31"],
        ),
    ]
    .map(|output| output.status.code());

    assert_eq!(codes, [Some(2); 3]);
}

#[test]
fn a_cursor_restores_the_query_and_options_with_the_page_token() {
    let api = Fixture::start_sequence(vec![search_page(&[], None)]);
    let environment = RunEnvironment::new(&config(&api.url));
    let request = json!({
        "query": "attention",
        "limit": 5,
        "options": {"platform": "youtube", "sort": "date", "has_captions": true},
        "page": "CAUQAA"
    });
    let cursor = format!(
        "v1.youtube_api.{}",
        URL_SAFE_NO_PAD.encode(request.to_string())
    );

    let output = search(&environment, &["--cursor", &cursor]);
    let request = api.finish();

    assert_eq!(
        (output.status.code(), query(&request)),
        (
            Some(0),
            pairs(&[
                ("part", "snippet"),
                ("type", "video"),
                ("q", "attention"),
                ("maxResults", "5"),
                ("order", "date"),
                ("videoCaption", "closedCaption"),
                ("pageToken", "CAUQAA"),
            ])
        ),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn error_reasons_decide_the_error_kind_and_the_key_never_appears() {
    let cases = [
        (
            api_error(
                400,
                "badRequest",
                Some("API_KEY_INVALID"),
                "API key not valid. Please pass a valid API key.",
            ),
            "auth",
        ),
        (
            api_error(
                403,
                "quotaExceeded",
                None,
                "The request cannot be completed because you have exceeded your quota.",
            ),
            "quota_exhausted",
        ),
        (
            api_error(400, "invalid", None, "Invalid value at 'published_after'"),
            "parameter",
        ),
    ];
    let expected = cases
        .iter()
        .map(|(_, kind)| (Some(4), Some((*kind).to_owned()), false))
        .collect::<Vec<_>>();
    let outcomes = cases
        .into_iter()
        .map(|(response, _)| {
            let api = Fixture::start_sequence(vec![response]);
            let environment = RunEnvironment::new(&config(&api.url));
            let output = search(&environment, &["attention", "--verbose"]);
            api.finish();
            let leaked = String::from_utf8_lossy(&output.stdout).contains(KEY)
                || stderr(&output).contains(KEY);
            (
                output.status.code(),
                payload(&output)["error_kind"].as_str().map(str::to_owned),
                leaked,
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(outcomes, expected);
}

#[test]
fn more_than_one_key_is_a_configuration_error() {
    let environment =
        RunEnvironment::new("[providers.youtube_api]\nkeys = [\"first-key\", \"second-key\"]\n");

    let output = search(&environment, &["attention"]);

    assert_eq!(
        (
            output.status.code(),
            stderr(&output).contains("providers.youtube_api.keys"),
        ),
        (Some(3), true),
        "stderr: {}",
        stderr(&output)
    );
}
