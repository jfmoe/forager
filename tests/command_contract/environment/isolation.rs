use crate::support::{Fixture, forager_command_through, run_command};

#[test]
fn a_spawned_binary_sends_http_only_through_the_shared_proxy() {
    let proxy = Fixture::start(200, "text/plain", "proxied");
    let config = tempfile::tempdir().expect("config directory");
    std::fs::write(
        config.path().join("config.toml"),
        "[providers.jina]\nurl = \"http://jina.invalid\"\nkeys = []\n\
         [providers.firecrawl]\nkeys = []\n[providers.tavily]\nkeys = []\n\
         [retry]\nmax_attempts = 1\n[journal]\nenabled = false\n",
    )
    .expect("write config");

    let mut command = forager_command_through(&proxy.url);
    command
        .args(["fetch", "https://example.test/article"])
        .env("FORAGER_CONFIG_DIR", config.path())
        .env("HOME", config.path());
    run_command(&mut command, None);
    let request = proxy.finish();

    assert!(
        request
            .lines()
            .next()
            .is_some_and(|line| line.contains("http://jina.invalid/")),
        "{request}"
    );
}
