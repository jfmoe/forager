use crate::credentials::CredentialPool;
use crate::net::truncate_message;
use crate::redact::{redact_url, redact_urls as redact_urls_in_text};

pub(super) fn redact_message(
    message: &str,
    endpoint: &str,
    credentials: &CredentialPool,
) -> String {
    credentials
        .redact(message)
        .replace(endpoint, &redact_url(endpoint))
}

pub(super) fn redacted_message(
    message: &str,
    endpoint: &str,
    credentials: &CredentialPool,
) -> String {
    truncate_message(&redact_message(message, endpoint, credentials))
}

pub(crate) fn redacted_urls_message(message: &str, credentials: &CredentialPool) -> String {
    truncate_message(&redact_urls(message, credentials))
}

pub(super) fn redact_urls(message: &str, credentials: &CredentialPool) -> String {
    credentials.redact(&redact_urls_in_text(message))
}

#[cfg(test)]
mod tests {
    use crate::credentials::CredentialPool;

    use super::redacted_message;

    #[test]
    fn redacted_message_masks_credentials_and_endpoint_query_secrets() {
        let credentials = CredentialPool::new("test", vec!["credential-secret".into()]);
        let endpoint = "https://example.test/search?api_key=endpoint-secret";

        assert_eq!(
            redacted_message(
                &format!("request to {endpoint} failed with credential-secret"),
                endpoint,
                &credentials,
            ),
            "request to https://example.test/search?api_key=******** failed with ********"
        );
    }
}
