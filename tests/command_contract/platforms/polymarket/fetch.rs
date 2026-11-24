use crate::support::polymarket::{config, run};
use crate::support::polymarket_samples::resolved;
use crate::support::{Fixture, RunEnvironment, ok_json, stderr};

#[test]
fn abstract_content_prints_the_resolution_rules() {
    let slug = "will-donald-trump-win-the-2024-us-presidential-election";
    let gamma = Fixture::start_sequence(vec![ok_json(&resolved(slug, "Donald Trump", true))]);
    let clob = Fixture::start_sequence(Vec::new());
    let environment = RunEnvironment::new(&config(&gamma.url, &clob.url));

    let output = run(
        &environment,
        &[
            "fetch",
            &format!("polymarket:market/{slug}"),
            "--depth",
            "abstract",
            "--format",
            "content",
        ],
    );
    gamma.finish();
    clob.finish_all();

    assert_eq!(
        (
            output.status.code(),
            String::from_utf8_lossy(&output.stdout).trim().to_owned(),
        ),
        (Some(0), "Event rules.".to_owned()),
        "stderr: {}",
        stderr(&output)
    );
}
