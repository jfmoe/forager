use std::time::Duration;

use serde_json::{Value, json};

use super::{GeminiApi, GenerateResponse, decode_answer, decode_generated, failure_message};
use crate::config::ModelRouteRuntimeConfig;
use crate::fixture_server::{Fixture, Response, block_on, header, ok_json, path, request_json};
use crate::net::{RetryPolicy, build_client};
use crate::providers::RouteContext;
use crate::providers::test_projection::failure;
// The stored bodies below read the sample store through this name.
use crate::samples;
use crate::types::{
    AttemptDisposition, AttemptErrorKind, AttemptTarget, ContentDepth, Deadline, ModelUsage,
    PlatformItem, PlatformItemData, PlatformRef, ProviderAttempt, TranscriptSource, YoutubeChapter,
    YoutubeItemData, YoutubeRef,
};

#[path = "../../../tests/support/gemini_api_samples.rs"]
mod stored;

use stored::{error, generated, stored};

const KEY: &str = "gemini-fixture-key-0123456789";
const MODEL: &str = "gemini-flash-latest";
/// The model version of the stored answer.
const MODEL_VERSION: &str = "gemini-3.8-flash-n";
const QUESTION: &str = "What animals stand behind the speaker?";
const ZOO_URL: &str = "https://www.youtube.com/watch?v=jNQXAC9IVRw";

/// The zoo video as the precheck hands it to the route, lasting `duration_seconds`, with a
/// description and one chapter.
pub(super) fn zoo(duration_seconds: Option<u64>) -> PlatformItem {
    let reference = YoutubeRef::video("jNQXAC9IVRw").expect("video ref");
    PlatformItem {
        url: reference.canonical_url(),
        reference: PlatformRef::Youtube(reference),
        depth: ContentDepth::Abstract,
        title: Some("Me at the zoo".into()),
        authors: vec!["Example Zoo Visitor".into()],
        published: Some("2005-04-24T03:31:52Z".into()),
        data: PlatformItemData::Youtube(YoutubeItemData {
            channel_ref: None,
            snippet: None,
            duration_seconds,
            live_status: None,
            views: None,
            likes: None,
            comments: None,
            language: None,
            has_captions: None,
            caption_tracks: None,
            description: Some("The first video.".into()),
            chapters: Some(vec![YoutubeChapter {
                start_seconds: 0,
                title: "Intro".into(),
            }]),
            transcript: None,
            position: None,
        }),
    }
}

/// The route against the fixture at `url`, under the path the real API uses, built as its
/// factory does with `keys` and one attempt per request.
fn route(url: &str, keys: &[&str]) -> GeminiApi {
    GeminiApi::build(
        ModelRouteRuntimeConfig {
            url: format!("{url}/v1beta"),
            keys: keys.iter().map(|key| (*key).into()).collect(),
            model: MODEL.to_owned(),
            timeout_seconds: 5,
        },
        RouteContext {
            client: build_client(true).expect("build client"),
            retry_policy: RetryPolicy::new(1, 1.0, Duration::ZERO),
            deadline: Deadline::new(Duration::from_secs(10)),
        },
    )
}

fn failed(status: u16, reason: &str, message: &str) -> Response {
    Response::json(status, &error(status, reason, message).to_string())
}

/// The JSON transcript a model writes as its answer text.
fn transcript(segments: &Value) -> String {
    json!({"language": "en", "segments": segments}).to_string()
}

fn response(body: &Value) -> GenerateResponse {
    serde_json::from_value(body.clone()).expect("generate response")
}

/// What an attempt records about the route: provider, target, disposition, status, model and
/// transport.
fn route_fields(
    attempt: &ProviderAttempt,
) -> (
    &'static str,
    AttemptTarget,
    AttemptDisposition,
    Option<u16>,
    Option<&str>,
    Option<&'static str>,
) {
    (
        attempt.provider,
        attempt.target,
        attempt.disposition,
        attempt.http_status,
        attempt.model.as_deref(),
        attempt.transport,
    )
}

/// The usage the stored answer reports.
fn stored_usage() -> ModelUsage {
    ModelUsage {
        prompt_tokens: Some(1747),
        output_tokens: Some(494),
        thoughts_tokens: Some(495),
        cached_tokens: None,
        total_tokens: Some(2736),
    }
}

// Decoding.

fn answer(finish_reason: &str) -> Result<String, AttemptErrorKind> {
    let mut body = generated("", finish_reason);
    body["candidates"][0]["content"]["parts"] = json!([
        {"text": "planning the answer", "thought": true},
        {"text": "Two elephants "},
        {"text": "at 00:05."}
    ]);
    response(&body).text().map_err(|failure| failure.kind)
}

#[test]
fn the_stored_answer_decodes_to_its_text_model_version_and_usage() {
    let decoded = decode_generated(&stored("generate-content.json").to_string(), decode_answer)
        .map_err(|refusal| refusal.message);

    assert_eq!(
        decoded,
        Ok((
            "Two elephants stand behind him (00:01).".to_owned(),
            Some(MODEL_VERSION.to_owned()),
            Some(stored_usage()),
        ))
    );
}

#[test]
fn the_answer_joins_the_text_parts_and_leaves_out_reasoning() {
    assert_eq!(answer("STOP"), Ok("Two elephants at 00:05.".to_owned()));
}

#[test]
fn a_truncated_withheld_or_blocked_answer_is_quality_and_an_unknown_stop_is_runtime() {
    let mut blocked = stored("generate-content.json");
    let fields = blocked.as_object_mut().expect("generate response");
    fields.remove("candidates");
    fields.insert(
        "promptFeedback".to_owned(),
        json!({"blockReason": "SAFETY"}),
    );
    let mut empty = stored("generate-content.json");
    empty["candidates"] = json!([]);
    let [blocked, empty] =
        [blocked, empty].map(|body| response(&body).text().map_err(|failure| failure.kind));

    assert_eq!(
        [
            answer("MAX_TOKENS"),
            answer("SAFETY"),
            answer("RECITATION"),
            blocked,
            answer("MALFORMED_FUNCTION_CALL"),
            empty,
        ],
        [
            Err(AttemptErrorKind::Quality),
            Err(AttemptErrorKind::Quality),
            Err(AttemptErrorKind::Quality),
            Err(AttemptErrorKind::Quality),
            Err(AttemptErrorKind::Runtime),
            Err(AttemptErrorKind::Runtime),
        ]
    );
}

#[test]
fn failures_name_the_status_reason_and_message_gemini_gives() {
    let body = stored("error.json").to_string();

    assert_eq!(
        [failure_message(&body, 403), failure_message("<html>", 502)],
        [
            "Gemini API returned HTTP 403 (PERMISSION_DENIED): The caller does not have permission",
            "Gemini API returned HTTP 502",
        ]
    );
}

// Ask.

#[test]
fn a_question_travels_with_the_video_url_and_the_key_only_in_its_header() {
    let fixture = Fixture::start_sequence(vec![ok_json(&stored("generate-content.json"))]);

    let outcome = block_on(route(&fixture.url, &[KEY]).ask(&zoo(Some(19)), QUESTION))
        .expect("the answer is relayed");
    let request = fixture.finish();
    let body = request_json(&request);
    let instruction = body["contents"][0]["parts"][1]["text"]
        .as_str()
        .unwrap_or_default();

    assert_eq!(
        (
            request.lines().next().unwrap_or_default(),
            header(&request, "x-goog-api-key"),
            &body["contents"][0]["parts"][0],
            instruction.ends_with(&format!("\n\nQuestion: {QUESTION}"))
                && instruction.contains("mm:ss"),
            body.get("generationConfig"),
            body["contents"][0]["parts"][0].get("videoMetadata"),
        ),
        (
            format!("POST /v1beta/models/{MODEL}:generateContent HTTP/1.1").as_str(),
            Some(KEY.to_owned()),
            &json!({"fileData": {"mimeType": "video/*", "fileUri": ZOO_URL}}),
            true,
            None,
            None,
        )
    );
    assert_eq!(
        (
            outcome.answer.as_str(),
            outcome.model.as_deref(),
            outcome.usage,
            outcome
                .attempts
                .iter()
                .map(route_fields)
                .collect::<Vec<_>>(),
        ),
        (
            "Two elephants stand behind him (00:01).",
            Some(MODEL_VERSION),
            Some(stored_usage()),
            vec![(
                "gemini_api",
                AttemptTarget::platform("youtube", "ask"),
                AttemptDisposition::Succeeded,
                Some(200),
                Some(MODEL),
                Some("http"),
            )],
        )
    );
}

#[test]
fn an_empty_answer_is_a_runtime_failure() {
    let fixture = Fixture::start_sequence(vec![ok_json(&generated("  ", "STOP"))]);

    let outcome = block_on(route(&fixture.url, &[KEY]).ask(&zoo(Some(19)), QUESTION));
    fixture.finish();

    assert_eq!(
        failure(outcome),
        (
            AttemptErrorKind::Runtime,
            "Gemini returned an empty answer".to_owned()
        )
    );
}

#[test]
fn a_rejected_key_is_an_auth_failure_naming_the_status_reason_and_message() {
    let fixture =
        Fixture::start_sequence(vec![Response::json(403, &stored("error.json").to_string())]);

    let outcome = block_on(route(&fixture.url, &[KEY]).ask(&zoo(Some(19)), QUESTION));
    fixture.finish();

    assert_eq!(
        failure(outcome),
        (
            AttemptErrorKind::Auth,
            "Gemini API returned HTTP 403 (PERMISSION_DENIED): The caller does not have permission"
                .to_owned()
        )
    );
}

#[test]
fn a_rate_limited_key_rotates_to_the_next_key_of_the_pool() {
    let keys = ["gemini-first-key", "gemini-second-key"];
    let fixture = Fixture::start_sequence(vec![
        failed(429, "RESOURCE_EXHAUSTED", "Resource has been exhausted."),
        ok_json(&stored("generate-content.json")),
    ]);

    let outcome = block_on(route(&fixture.url, &keys).ask(&zoo(Some(19)), QUESTION))
        .expect("the next key answers");
    let sent = fixture
        .finish_all()
        .iter()
        .map(|request| header(request, "x-goog-api-key").unwrap_or_default())
        .collect::<Vec<_>>();
    // The credential cursor persists across requests, so the first key may be either.
    let first = outcome.attempts[0].credential_index;
    let second = (first + 1) % keys.len();

    assert_eq!(
        (
            outcome
                .attempts
                .iter()
                .map(|attempt| (
                    attempt.error_kind,
                    attempt.credential_index,
                    attempt.rotation_count
                ))
                .collect::<Vec<_>>(),
            sent,
        ),
        (
            vec![
                (Some(AttemptErrorKind::RateLimited), first, 0),
                (None, second, 1),
            ],
            vec![keys[first].to_owned(), keys[second].to_owned()],
        )
    );
}

#[test]
fn the_key_never_reaches_a_failure_or_its_attempts() {
    const CANARY: &str = "gemini-canary-7f3c9e1d";
    let echoed = format!("API key not valid: {CANARY}");
    for (case, response) in [
        ("403", Some(failed(403, "PERMISSION_DENIED", &echoed))),
        ("429", Some(failed(429, "RESOURCE_EXHAUSTED", &echoed))),
        (
            "200 non-JSON",
            Some(Response::json(200, &format!("key={CANARY}"))),
        ),
        ("network", None),
    ] {
        let fixture = response.map(Fixture::start_repeating);
        let url = fixture.as_ref().map_or_else(
            || "http://127.0.0.1:9".to_owned(),
            |fixture| fixture.url.clone(),
        );

        let outcome = block_on(route(&url, &[CANARY]).ask(&zoo(Some(19)), QUESTION));
        let requests = fixture.map(|fixture| fixture.finish_all().len());

        let Err(error) = outcome else {
            panic!("{case} should fail");
        };
        assert_ne!(requests, Some(0), "{case} should reach the fixture");
        assert!(
            !format!("{error:?}").contains(CANARY),
            "{case} leaked the key: {error:?}"
        );
    }
}

// Transcribe.

#[test]
fn a_transcription_asks_for_the_transcript_schema_and_renders_the_model_transcript() {
    let fixture = Fixture::start_sequence(vec![ok_json(&generated(
        &transcript(&json!([
            {"start": 1.2, "end": 3.4, "text": "All right, so here we are, in front of the elephants."},
            {"start": 5.3, "end": 12.6, "text": "The cool thing about these guys is that they have really long trunks."},
            {"start": 16.9, "end": 18.9, "text": "And that's pretty much all there is to say."},
            {"start": 40.0, "end": 41.0, "text": "A segment past the end."}
        ])),
        "STOP",
    ))]);

    let outcome =
        block_on(route(&fixture.url, &[KEY]).transcribe(&zoo(Some(19)))).expect("a transcript");
    let request = fixture.finish();
    let body = request_json(&request);
    let schema = &body["generationConfig"]["responseSchema"];

    assert_eq!(
        (
            path(&request),
            header(&request, "x-goog-api-key"),
            &body["contents"][0]["parts"][0],
            &body["generationConfig"]["responseMimeType"],
            &schema["required"],
            &schema["properties"]["segments"]["items"]["required"],
        ),
        (
            format!("/v1beta/models/{MODEL}:generateContent"),
            Some(KEY.to_owned()),
            &json!({"fileData": {"mimeType": "video/*", "fileUri": ZOO_URL}}),
            &json!("application/json"),
            &json!(["language", "segments"]),
            &json!(["start", "end", "text"]),
        )
    );
    assert_eq!(
        (
            (
                outcome.transcript.language.as_str(),
                outcome.transcript.source,
                outcome.transcript.cues,
                outcome.transcript.paragraphs,
            ),
            outcome.diagnostic.as_deref(),
            outcome.model.as_deref(),
            outcome.usage,
            outcome
                .attempts
                .iter()
                .map(route_fields)
                .collect::<Vec<_>>(),
        ),
        (
            ("en", TranscriptSource::Model, 3, 2),
            Some(
                "dropped 1 model transcript segment(s) that end past the video's 19-second length"
            ),
            Some(MODEL_VERSION),
            Some(stored_usage()),
            vec![(
                "gemini_api",
                AttemptTarget::platform("youtube", "transcribe"),
                AttemptDisposition::Succeeded,
                Some(200),
                Some(MODEL),
                Some("http"),
            )],
        )
    );
    assert!(
        outcome.markdown.contains(
            "### Intro\n\n[00:01](https://www.youtube.com/watch?v=jNQXAC9IVRw&t=1s) All right, so here we are, in front of the elephants. The cool thing"
        ) && outcome.markdown.contains(
            "\n\n[00:16](https://www.youtube.com/watch?v=jNQXAC9IVRw&t=16s) And that's pretty much"
        ) && !outcome.markdown.contains("past the end"),
        "{}",
        outcome.markdown
    );
}

#[test]
fn a_truncated_or_invalid_model_transcript_is_a_quality_failure() {
    let outcomes = [
        generated(
            r#"{"language": "en", "segments": [{"start": 1.2, "end": 3.4, "text": "All ri"#,
            "MAX_TOKENS",
        ),
        generated("Here is the transcript: all right, so here we are.", "STOP"),
    ]
    .map(|body| {
        let fixture = Fixture::start_sequence(vec![ok_json(&body)]);
        let outcome = block_on(route(&fixture.url, &[KEY]).transcribe(&zoo(Some(19))));
        fixture.finish();
        failure(outcome)
    });

    assert_eq!(
        outcomes
            .iter()
            .map(|(kind, message)| (*kind, message.split(':').next().unwrap_or_default()))
            .collect::<Vec<_>>(),
        [
            (
                AttemptErrorKind::Quality,
                "Gemini stopped at its output token limit, so the answer is incomplete"
            ),
            (
                AttemptErrorKind::Quality,
                "the model's transcript is not valid JSON in the requested schema"
            ),
        ]
    );
}
