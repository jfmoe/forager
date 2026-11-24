//! X platform configuration shared by the `platform x` tests.

/// A configuration that sends xAI requests to `url` with `keys`; main search also enables web
/// search, which platform requests must never send.
pub(crate) fn config(url: &str, keys: &[&str]) -> String {
    let keys = keys
        .iter()
        .map(|key| format!("\"{key}\""))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "[providers.xai]\nurl = \"{url}\"\nkeys = [{keys}]\nmodel = \"grok-test\"\ntools = [\"web_search\", \"x_search\"]\n[retry]\nmax_wait = 0\n"
    )
}
