//! `forager doctor --provider gemini_api`: the deep probe lists the models with every key of the
//! pool instead of running a billed model request.

mod support;

use serde_json::{Value, json};

use support::gemini_api;
use support::youtube::{header, path, payload};
use support::{Fixture, Response, RunEnvironment};

fn config(url: &str, keys: &[&str]) -> String {
    format!(
        "[providers.gemini_api]\nurl = \"{url}/v1beta\"\nkeys = {keys:?}\n\n[retry]\nmax_attempts = 1\n"
    )
}

fn models() -> Response {
    Response::json(
        200,
        &json!({"models": [{"name": "models/gemini-flash-latest"}]}).to_string(),
    )
}

#[test]
fn deep_doctor_lists_the_models_with_every_key_and_reports_each_key() {
    let fixture = Fixture::start_sequence(vec![
        gemini_api::error(
            403,
            "PERMISSION_DENIED",
            "Method doesn't allow unregistered callers.",
        ),
        models(),
    ]);
    let environment = RunEnvironment::new(&config(&fixture.url, &["revoked-key", "good-key"]));

    let output = environment.run(&["doctor", "--provider", "gemini_api"]);
    let requests = fixture.finish_all();
    let payload = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            &payload["ok"],
            &payload["checks"],
            &payload["error_kind"],
            &payload["message"],
            &payload["keys"],
        ),
        (
            Some(4),
            &Value::Bool(false),
            &json!([{"name": "models", "transport": "http", "ok": false}]),
            &json!("auth"),
            &json!(
                "providers.gemini_api.keys[0]: Gemini API returned HTTP 403 (PERMISSION_DENIED): Method doesn't allow unregistered callers."
            ),
            &json!([
                {"key_index": 0, "ok": false, "error_kind": "auth"},
                {"key_index": 1, "ok": true}
            ]),
        )
    );
    assert_eq!(
        requests
            .iter()
            .map(|request| (path(request), header(request, "x-goog-api-key")))
            .collect::<Vec<_>>(),
        ["revoked-key", "good-key"].map(|key| ("/v1beta/models".to_owned(), Some(key.to_owned())))
    );
}
