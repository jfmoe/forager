use crate::support::youtube::config;
use crate::support::youtube_api_samples::{videos_page, zoo_video};
use crate::support::{Fixture, RunEnvironment, ok_json, stderr};

const CHAPTERED: &str =
    "The first video on YouTube.\n\n0:00 Elephants\n0:07 Trunks\n0:14 That's it";

fn fetch(environment: &RunEnvironment, arguments: &[&str]) -> std::process::Output {
    let mut command = vec!["platform", "youtube", "fetch"];
    command.extend_from_slice(arguments);
    environment.run(&command)
}

#[test]
fn content_format_prints_the_description() {
    let api = Fixture::start_sequence(vec![ok_json(&videos_page(&[zoo_video(CHAPTERED)]))]);
    let environment = RunEnvironment::new(&config(&api.url));

    let output = fetch(
        &environment,
        &[
            "youtube:jNQXAC9IVRw",
            "--depth",
            "abstract",
            "--format",
            "content",
        ],
    );
    api.finish();

    assert_eq!(
        (
            output.status.code(),
            String::from_utf8_lossy(&output.stdout).trim_end()
        ),
        (Some(0), CHAPTERED),
        "stderr: {}",
        stderr(&output)
    );
}
