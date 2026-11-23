mod support;

use support::hn::stderr;
use support::polymarket::{config, json as body, run};
use support::polymarket_samples::resolved;
use support::{Fixture, RunEnvironment};

const EVENT: &str = "how-many-fed-rate-cuts-in-2026";

#[test]
fn abstract_content_prints_the_resolution_rules() {
    let slug = "will-donald-trump-win-the-2024-us-presidential-election";
    let gamma = Fixture::start_sequence(vec![body(&resolved(slug, "Donald Trump", true))]);
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

#[test]
fn an_empty_order_is_a_configuration_error_before_any_request() {
    let gamma = Fixture::start_sequence(Vec::new());
    let clob = Fixture::start_sequence(Vec::new());
    let environment = RunEnvironment::new(&format!(
        "{}\n[platforms.polymarket]\norder = []\n",
        config(&gamma.url, &clob.url)
    ));

    let output = run(&environment, &["fetch", &format!("polymarket:{EVENT}")]);
    gamma.finish_all();
    clob.finish_all();

    assert_eq!(
        (output.status.code(), stderr(&output).trim().to_owned()),
        (
            Some(3),
            "config_error: platforms.polymarket.order has no configured route for polymarket fetch"
                .to_owned()
        )
    );
}
