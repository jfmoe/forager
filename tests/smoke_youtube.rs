//! Live smoke of the YouTube cases: each runs only when its route is configured and enabled.

mod support;

use serde_json::{Value, json};
use support::Fixture;
use support::smoke::{DISABLED_PLATFORMS, SmokeEnvironment, case};
use support::youtube as yt;

#[test]
fn live_smoke_runs_the_youtube_cases_only_with_a_youtube_api_key() {
    let api = Fixture::start_sequence(vec![
        yt::search_page(
            &[yt::search_result("eMlx5fFNoYc", "Attention", "excerpt")],
            None,
        ),
        yt::videos_page(&[yt::video_stats("eMlx5fFNoYc", "PT26M10S", "1", None)]),
        yt::videos_page(&[yt::zoo_video("The first video.")]),
        yt::comment_threads_page(
            &[yt::comment_thread(
                &yt::comment("Ugz1", "fan", "Hi.", 1),
                0,
                &[],
            )],
            None,
        ),
        yt::playlist_items_page(
            &[yt::playlist_item(
                yt::LINEAR_ALGEBRA,
                0,
                "fNk_zzaMoSs",
                "Vectors",
                "",
            )],
            None,
        ),
    ]);
    let run = |keys: &str| {
        let environment = SmokeEnvironment::new(|journal_dir| {
            format!(
                "[providers.youtube_api]\nurl = \"{}\"\nkeys = {keys}\n[journal]\ndir = {journal_dir:?}\n{DISABLED_PLATFORMS}",
                api.url
            )
        });
        let output = environment.run(&["smoke", "--live", "--timeout", "20"]);
        let payload: Value = serde_json::from_slice(&output.stdout).expect("parse live smoke JSON");
        ["C36", "C37", "C39", "C40"].map(|id| case(&payload, id)["status"].clone())
    };

    let without_key = run("[]");
    let with_key = run("[\"test-youtube-key\"]");
    let requests = api.finish_all();

    assert_eq!(
        (
            without_key,
            with_key,
            requests[0].starts_with("GET /search?"),
            requests[0].contains("q=attention+transformer") && requests[0].contains("maxResults=5"),
            requests[2].starts_with("GET /videos?") && requests[2].contains("id=jNQXAC9IVRw"),
            requests[3].starts_with("GET /commentThreads?")
                && requests[3].contains("videoId=jNQXAC9IVRw")
                && requests[3].contains("maxResults=5"),
            requests[4].starts_with("GET /playlistItems?")
                && requests[4].contains("playlistId=PLZHQObOWTQDPD3MizzM2xVFitgF8hE_ab")
                && requests[4].contains("maxResults=5"),
        ),
        (
            [(); 4].map(|()| Value::String("unconfigured".into())),
            [(); 4].map(|()| Value::String("passed".into())),
            true,
            true,
            true,
            true,
            true,
        )
    );
}

#[test]
fn live_smoke_runs_the_youtube_model_cases_after_a_metadata_check_through_the_configured_order() {
    let zoo = || yt::videos_page(&[yt::zoo_video("The first video.")]);
    let api = Fixture::start_sequence(vec![
        yt::search_page(
            &[yt::search_result("eMlx5fFNoYc", "Attention", "excerpt")],
            None,
        ),
        yt::videos_page(&[yt::video_stats("eMlx5fFNoYc", "PT26M10S", "1", None)]),
        zoo(),
        yt::comment_threads_page(
            &[yt::comment_thread(
                &yt::comment("Ugz1", "fan", "Hi.", 1),
                0,
                &[],
            )],
            None,
        ),
        yt::playlist_items_page(
            &[yt::playlist_item(
                yt::LINEAR_ALGEBRA,
                0,
                "fNk_zzaMoSs",
                "Vectors",
                "",
            )],
            None,
        ),
        zoo(),
        zoo(),
    ]);
    let gemini = Fixture::start_sequence(vec![
        support::gemini_api::answer("Two elephants; a dark jacket."),
        support::gemini_api::answer(
            &json!({"language": "en", "segments": [{"start": 1.0, "end": 3.0, "text": "Here we are."}]})
                .to_string(),
        ),
    ]);
    let environment = SmokeEnvironment::new(|journal_dir| {
        format!(
            "[providers.youtube_api]\nurl = \"{}\"\nkeys = [\"test-youtube-key\"]\n[providers.gemini_api]\nurl = \"{}\"\nkeys = [\"test-gemini-key\"]\n[journal]\ndir = {journal_dir:?}\n{DISABLED_PLATFORMS}",
            api.url, gemini.url
        )
    });

    let output = environment.run(&["smoke", "--live", "--timeout", "30"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse live smoke JSON");
    let statuses = ["C41", "C42"].map(|id| case(&payload, id)["status"].clone());
    let api_requests = api.finish_all();
    let model_requests = gemini.finish_all();

    assert_eq!(
        (
            statuses,
            api_requests[5..]
                .iter()
                .all(|request| request.starts_with("GET /videos?")
                    && request.contains("id=jNQXAC9IVRw")),
            model_requests
                .iter()
                .map(|request| {
                    support::request_json(request)["contents"][0]["parts"][0]["fileData"]["fileUri"]
                        .clone()
                })
                .collect::<Vec<_>>(),
        ),
        (
            [(); 2].map(|()| Value::String("passed".into())),
            true,
            vec![json!("https://www.youtube.com/watch?v=jNQXAC9IVRw"); 2],
        ),
        "stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn live_smoke_reads_the_youtube_transcript_only_while_the_order_enables_youtube_web() {
    let youtube = Fixture::start_sequence_for(|origin| {
        vec![
            support::youtube_web::watch_page(&support::youtube_web::zoo_embedded("Zoo.")),
            support::youtube_web::json(&support::youtube_web::zoo_player(
                &[support::youtube_web::track(origin, "en", false)],
                0,
            )),
            support::youtube_web::srv1(support::youtube_web::ZOO_SRV1_EN),
        ]
    });
    let run = |order: &str| {
        let environment = SmokeEnvironment::new(|journal_dir| {
            format!(
                "[providers.youtube_web]\nurl = \"{}\"\n[journal]\ndir = {journal_dir:?}\n{DISABLED_PLATFORMS}[platforms.youtube]\norder = {order}\n",
                youtube.url
            )
        });
        let output = environment.run(&["smoke", "--live", "--timeout", "25"]);
        let payload: Value = serde_json::from_slice(&output.stdout).expect("parse live smoke JSON");
        ["C37", "C38"].map(|id| case(&payload, id)["status"].clone())
    };

    let disabled = run("[\"youtube_api\"]");
    let enabled = run("[\"youtube_web\"]");
    let requests = youtube.finish_all();

    assert_eq!(
        (
            disabled,
            enabled,
            requests[0].starts_with("GET /watch?v=jNQXAC9IVRw "),
        ),
        (
            [(); 2].map(|()| Value::String("unconfigured".into())),
            [
                Value::String("unconfigured".into()),
                Value::String("passed".into())
            ],
            true,
        )
    );
}
