//! Hacker News configuration for the two endpoints of `hn_api`, shared by the `platform hn`
//! tests.

/// Points `hn_api` at the two fixture servers with one attempt per request, so a failure
/// response is terminal at once.
pub(crate) fn config(algolia: &str, firebase: &str) -> String {
    format!(
        "[providers.hn_api]\nurl = \"{algolia}/api/v1\"\nfirebase_url = \"{firebase}/v0\"\n\n[retry]\nmax_attempts = 1\n"
    )
}
