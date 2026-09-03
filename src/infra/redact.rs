use std::collections::HashSet;
use std::fmt;

use serde::{Deserialize, Serialize, Serializer};

pub(crate) const CREDENTIAL_MASK: &str = "********";

#[derive(Clone, Deserialize, Eq, Hash, PartialEq)]
#[serde(transparent)]
pub(crate) struct Secret(String);

impl Secret {
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }

    pub(crate) fn normalize(values: &mut Vec<Self>) {
        for value in values.iter_mut() {
            value.0 = value.0.trim().to_owned();
        }
        let mut seen = HashSet::new();
        values.retain(|value| !value.0.is_empty() && seen.insert(value.clone()));
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(CREDENTIAL_MASK)
    }
}

impl Serialize for Secret {
    fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        Err(serde::ser::Error::custom(
            "credentials cannot be serialized",
        ))
    }
}

impl From<&str> for Secret {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

pub(crate) fn redact_url(value: &str) -> String {
    let without_fragment = value.split_once('#').map_or(value, |(url, _)| url);
    let mut redacted = without_fragment.to_owned();

    if let Some(authority_start) = redacted.find("://").map(|index| index + 3) {
        let authority_end = redacted[authority_start..]
            .find(['/', '?'])
            .map_or(redacted.len(), |index| authority_start + index);
        if let Some(userinfo_end) = redacted[authority_start..authority_end].rfind('@') {
            redacted.replace_range(authority_start..=authority_start + userinfo_end, "");
        }
    }

    let Some(query_start) = redacted.find('?') else {
        return redacted;
    };
    let query = redacted[query_start + 1..]
        .split('&')
        .map(|pair| {
            let Some((name, value)) = pair.split_once('=') else {
                return pair.to_owned();
            };
            if is_sensitive_query_name(name) {
                format!("{name}={CREDENTIAL_MASK}")
            } else {
                format!("{name}={value}")
            }
        })
        .collect::<Vec<_>>()
        .join("&");
    redacted.truncate(query_start + 1);
    redacted.push_str(&query);
    redacted
}

pub(crate) fn redact_urls(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut remaining = value;
    while let Some(start) = match (remaining.find("https://"), remaining.find("http://")) {
        (Some(https), Some(http)) => Some(https.min(http)),
        (Some(https), None) => Some(https),
        (None, Some(http)) => Some(http),
        (None, None) => None,
    } {
        output.push_str(&remaining[..start]);
        remaining = &remaining[start..];
        let end = remaining
            .find(|character: char| {
                character.is_whitespace()
                    || matches!(character, '<' | '>' | '"' | '\'' | ')' | ']' | '}')
            })
            .unwrap_or(remaining.len());
        output.push_str(&redact_url(&remaining[..end]));
        remaining = &remaining[end..];
    }
    output.push_str(remaining);
    output
}

pub(crate) fn redact_credentials(value: &str, credentials: &[Secret]) -> String {
    credentials
        .iter()
        .fold(redact_urls(value), |redacted, credential| {
            let credential = credential.expose();
            if redacted.contains(credential) {
                redacted.replace(credential, CREDENTIAL_MASK)
            } else {
                redacted
            }
        })
}

fn is_sensitive_query_name(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    ["token", "key", "secret", "signature", "authorization"]
        .iter()
        .any(|sensitive| name.contains(sensitive))
}

#[cfg(test)]
mod tests {
    use super::{CREDENTIAL_MASK, Secret, redact_credentials, redact_url};

    #[test]
    fn secret_debug_is_masked_and_serde_only_accepts_input() {
        let secret = Secret::from("credential-canary-do-not-print");

        assert_eq!(format!("{secret:?}"), CREDENTIAL_MASK);
        assert!(serde_json::to_string(&secret).is_err());

        let deserialized: Secret =
            serde_json::from_str("\"credential-canary-do-not-print\"").expect("deserialize secret");
        assert_eq!(deserialized, Secret::from("credential-canary-do-not-print"));
    }

    #[test]
    fn secret_normalization_trims_drops_empty_and_preserves_first_occurrence() {
        let mut values = vec![
            Secret::from(" alpha "),
            Secret::from(""),
            Secret::from("beta"),
            Secret::from("alpha"),
            Secret::from("  "),
        ];

        Secret::normalize(&mut values);

        assert_eq!(values, [Secret::from("alpha"), Secret::from("beta")]);
    }

    #[test]
    fn credential_redaction_preserves_messages_without_matching_credentials() {
        let credentials = [Secret::from("credential-canary-do-not-print")];

        assert_eq!(
            redact_credentials("request failed without a credential", &credentials),
            "request failed without a credential"
        );
        assert_eq!(
            redact_credentials(
                "request failed for credential-canary-do-not-print",
                &credentials
            ),
            "request failed for ********"
        );
    }

    #[test]
    fn url_redaction_exhausts_credentials_fragments_and_safe_query_boundaries() {
        for (input, expected) in [
            (
                "https://user:password@example.test/path#private",
                "https://example.test/path",
            ),
            (
                "https://example.test/path?api_key=secret&safe=yes",
                "https://example.test/path?api_key=********&safe=yes",
            ),
            (
                "https://example.test/path?access_token=secret&signature=secret",
                "https://example.test/path?access_token=********&signature=********",
            ),
            (
                "https://example.test/path?monkey=value&author=alice",
                "https://example.test/path?monkey=********&author=alice",
            ),
            ("not a URL#fragment", "not a URL"),
        ] {
            assert_eq!(redact_url(input), expected, "input={input}");
        }
    }
}
