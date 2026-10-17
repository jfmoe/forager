mod support;

use serde_json::json;

use support::gemini_api::{self, MODEL, MODEL_VERSION};
use support::youtube::{header, path, payload, stderr, videos_page, zoo_video};
use support::{Fixture, RunEnvironment, request_json};

const QUESTION: &str = "What animals stand behind the speaker?";

fn ask(environment: &RunEnvironment, arguments: &[&str]) -> std::process::Output {
    let mut command = vec!["platform", "youtube", "ask"];
    command.extend_from_slice(arguments);
    environment.run(&command)
}

#[test]
fn a_question_reaches_gemini_with_the_video_after_the_metadata_check_and_the_answer_is_relayed() {
    let api = Fixture::start_sequence(vec![videos_page(&[zoo_video("The first video.")])]);
    let gemini = Fixture::start_sequence(vec![gemini_api::answer(
        "Two elephants stand behind him (00:01).",
    )]);
    let environment = RunEnvironment::new(&gemini_api::config(&api.url, &gemini.url));

    let output = ask(
        &environment,
        &["https://youtu.be/jNQXAC9IVRw?t=3", QUESTION, "--verbose"],
    );
    let metadata_request = api.finish();
    let model_request = gemini.finish();
    let body = request_json(&model_request);
    let answer = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            path(&metadata_request),
            path(&model_request),
            header(&model_request, "x-goog-api-key"),
            model_request
                .lines()
                .next()
                .unwrap_or_default()
                .contains(gemini_api::KEY),
        ),
        (
            Some(0),
            "/youtube/v3/videos".to_owned(),
            format!("/v1beta/models/{MODEL}:generateContent"),
            Some(gemini_api::KEY.to_owned()),
            false,
        ),
        "stderr: {}",
        stderr(&output)
    );
    assert_eq!(
        (
            &body["contents"][0]["parts"][0],
            body["contents"][0]["parts"][1]["text"]
                .as_str()
                .is_some_and(|text| text.ends_with(&format!("Question: {QUESTION}"))
                    && text.contains("mm:ss")),
            body.get("generationConfig"),
            body["contents"][0]["parts"][0].get("videoMetadata"),
        ),
        (
            &json!({"fileData": {"mimeType": "video/*", "fileUri": "https://www.youtube.com/watch?v=jNQXAC9IVRw"}}),
            true,
            None,
            None,
        )
    );
    assert_eq!(
        (
            answer["provider_attempts"]
                .as_array()
                .map(|attempts| attempts
                    .iter()
                    .map(|attempt| format!(
                        "{} {}",
                        attempt["provider"].as_str().unwrap_or_default(),
                        attempt["operation"].as_str().unwrap_or_default()
                    ))
                    .collect::<Vec<_>>()),
            {
                let mut answer = answer.clone();
                answer
                    .as_object_mut()
                    .map(|fields| fields.remove("provider_attempts"));
                answer
            },
        ),
        (
            Some(vec![
                "youtube_api fetch".to_owned(),
                "gemini_api ask".to_owned(),
            ]),
            json!({
                "platform": "youtube",
                "provider": "gemini_api",
                "video": {
                    "ref": "youtube:jNQXAC9IVRw",
                    "url": "https://www.youtube.com/watch?v=jNQXAC9IVRw",
                    "title": "Me at the zoo"
                },
                "question": QUESTION,
                "answer": "Two elephants stand behind him (00:01).",
                "model": MODEL_VERSION,
                "usage": gemini_api::usage()
            }),
        )
    );
}

#[test]
fn a_video_the_metadata_check_cannot_read_fails_without_a_model_request() {
    let api = Fixture::start_sequence(vec![videos_page(&[])]);
    let gemini = Fixture::start_canary();
    let environment = RunEnvironment::new(&gemini_api::config(&api.url, &gemini.url));

    let output = ask(&environment, &["youtube:aaaaaaaaaaa", QUESTION]);
    api.finish();
    let failure = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            &failure["error_kind"],
            &failure["message"],
            gemini.finish_all().len(),
        ),
        (
            Some(4),
            &json!("parameter"),
            &json!("YouTube video not found or not public: youtube:aaaaaaaaaaa"),
            0,
        ),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn a_live_stream_fails_as_quality_without_a_model_request() {
    let mut live = zoo_video("Streaming now.");
    live["snippet"]["liveBroadcastContent"] = json!("live");
    live["liveStreamingDetails"] = json!({"actualStartTime": "2026-10-18T01:00:00Z"});
    let api = Fixture::start_sequence(vec![videos_page(&[live])]);
    let gemini = Fixture::start_canary();
    let environment = RunEnvironment::new(&gemini_api::config(&api.url, &gemini.url));

    let output = ask(&environment, &["youtube:jNQXAC9IVRw", QUESTION]);
    api.finish();
    let failure = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            &failure["error_kind"],
            gemini.finish_all().len(),
        ),
        (Some(5), &json!("quality"), 0),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn an_empty_answer_is_a_runtime_failure() {
    let api = Fixture::start_sequence(vec![videos_page(&[zoo_video("The first video.")])]);
    let gemini = Fixture::start_sequence(vec![gemini_api::answer("  ")]);
    let environment = RunEnvironment::new(&gemini_api::config(&api.url, &gemini.url));

    let output = ask(&environment, &["youtube:jNQXAC9IVRw", QUESTION]);
    api.finish();
    gemini.finish();
    let failure = payload(&output);

    assert_eq!(
        (output.status.code(), &failure["error_kind"]),
        (Some(4), &json!("runtime")),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn a_rate_limited_key_rotates_to_the_next_key_of_the_pool() {
    let api = Fixture::start_sequence(vec![videos_page(&[zoo_video("The first video.")])]);
    let gemini = Fixture::start_sequence(vec![
        gemini_api::error(429, "RESOURCE_EXHAUSTED", "Resource has been exhausted."),
        gemini_api::answer("Two elephants."),
    ]);
    let config = gemini_api::config(&api.url, &gemini.url).replace(
        &format!("keys = [\"{}\"]\nmodel", gemini_api::KEY),
        "keys = [\"gemini-first-key\", \"gemini-second-key\"]\nmodel",
    );
    let environment = RunEnvironment::new(&config);

    let output = ask(&environment, &["youtube:jNQXAC9IVRw", QUESTION]);
    api.finish();
    let keys = gemini
        .finish_all()
        .iter()
        .map(|request| header(request, "x-goog-api-key").unwrap_or_default())
        .collect::<Vec<_>>();

    assert_eq!(
        (
            output.status.code(),
            payload(&output)["answer"].clone(),
            keys
        ),
        (
            Some(0),
            json!("Two elephants."),
            vec![
                "gemini-first-key".to_owned(),
                "gemini-second-key".to_owned()
            ]
        ),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn a_rejected_key_is_an_auth_failure() {
    let api = Fixture::start_sequence(vec![videos_page(&[zoo_video("The first video.")])]);
    let gemini = Fixture::start_sequence(vec![gemini_api::error(
        403,
        "PERMISSION_DENIED",
        "The caller does not have permission",
    )]);
    let environment = RunEnvironment::new(&gemini_api::config(&api.url, &gemini.url));

    let output = ask(&environment, &["youtube:jNQXAC9IVRw", QUESTION]);
    api.finish();
    gemini.finish();
    let failure = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            &failure["error_kind"],
            &failure["message"]
        ),
        (
            Some(4),
            &json!("auth"),
            &json!(
                "Gemini API returned HTTP 403 (PERMISSION_DENIED): The caller does not have permission"
            )
        ),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn ask_without_a_gemini_key_is_a_config_failure_before_any_request() {
    let api = Fixture::start_canary();
    let environment = RunEnvironment::new(&support::youtube::config(&api.url));

    let output = ask(&environment, &["youtube:jNQXAC9IVRw", QUESTION]);

    assert_eq!(
        (
            output.status.code(),
            stderr(&output).contains("providers.gemini_api.keys"),
            api.finish_all().len()
        ),
        (Some(3), true, 0),
        "stderr: {}",
        stderr(&output)
    );
}
