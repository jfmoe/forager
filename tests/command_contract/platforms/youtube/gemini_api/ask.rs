use serde_json::json;

use crate::support::gemini_api::{self, MODEL_VERSION};
use crate::support::youtube_api_samples::{videos_page, zoo_video};
use crate::support::{Fixture, RunEnvironment, header, ok_json, payload, stderr};

const QUESTION: &str = "What animals stand behind the speaker?";

fn ask(environment: &RunEnvironment, arguments: &[&str]) -> std::process::Output {
    let mut command = vec!["platform", "youtube", "ask"];
    command.extend_from_slice(arguments);
    environment.run(&command)
}

#[test]
fn a_question_reaches_gemini_with_the_video_after_the_metadata_check_and_the_answer_is_relayed() {
    let api = Fixture::start_sequence(vec![ok_json(&videos_page(&[zoo_video(
        "The first video.",
    )]))]);
    let gemini = Fixture::start_sequence(vec![gemini_api::answer(
        "Two elephants stand behind him (00:01).",
    )]);
    let environment = RunEnvironment::new(&gemini_api::config(&api.url, &gemini.url));

    let output = ask(
        &environment,
        &["https://youtu.be/jNQXAC9IVRw?t=3", QUESTION, "--verbose"],
    );
    api.finish();
    let model_request = gemini.finish();
    let answer = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            header(&model_request, "x-goog-api-key"),
            model_request
                .lines()
                .next()
                .unwrap_or_default()
                .contains(gemini_api::KEY),
        ),
        (Some(0), Some(gemini_api::KEY.to_owned()), false),
        "stderr: {}",
        stderr(&output)
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
