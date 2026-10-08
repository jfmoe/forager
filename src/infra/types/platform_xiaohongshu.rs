//! Xiaohongshu shapes: note refs, access tokens, search options, and item metadata.

use std::fmt;

use serde::{Deserialize, Serialize};

use super::platform::{Platform, PlatformRefError};

const XIAOHONGSHU_MAX_LIMIT: u16 = 100;
const REF_PREFIX: &str = "xiaohongshu:";
const NOTE_HOSTS: [&str; 2] = ["www.xiaohongshu.com", "xiaohongshu.com"];
const NOTE_PAGE: &str = "https://www.xiaohongshu.com/explore/";
const NOTE_ID_LENGTH: usize = 24;
const MAX_TOKEN_LENGTH: usize = 128;
const TOKEN_PARAMETER: &str = "xsec_token";
// Xiaohongshu opens a note with any `xsec_source`; the access link always names search.
const ACCESS_SOURCE: &str = "pc_search";

const REF_HINT: &str = "pass a `xiaohongshu:<note_id>` ref or a xiaohongshu.com note URL such as an `access_url` from search";
const TOKEN_HINT: &str = "the xsec_token of the note URL must be 1 to 128 URL-safe base64 characters (A-Z, a-z, 0-9, `_`, `=`, `-`)";
const SHORT_LINK_HINT: &str = "xhslink.com short links need a network request to expand: open the link in a browser and pass the full xiaohongshu.com note URL";
const REDNOTE_HINT: &str = "rednote.com links are not supported: pass a xiaohongshu.com note URL";

#[derive(Clone, Debug, Eq, PartialEq)]
/// A Xiaohongshu note identity: its 24-digit hexadecimal note ID, in lowercase. Notes have no
/// versions, and the access token that opens a note is not part of its identity.
pub struct XiaohongshuRef {
    note_id: String,
}

impl XiaohongshuRef {
    /// Parses `xiaohongshu:<note_id>` or a xiaohongshu.com note URL, ignoring its access token.
    ///
    /// # Errors
    ///
    /// Returns [`PlatformRefError`] for any other input, including xhslink.com short links,
    /// rednote.com links, and note URLs whose `xsec_token` is malformed. The error never echoes
    /// the input, which may carry an access token.
    pub fn parse(input: &str) -> Result<Self, PlatformRefError> {
        Self::parse_with_access(input).map(|(reference, _)| reference)
    }

    /// Parses a ref or a note URL into the note identity and the access token the URL carries.
    pub(crate) fn parse_with_access(
        input: &str,
    ) -> Result<(Self, Option<AccessToken>), PlatformRefError> {
        let trimmed = input.trim();
        let parsed = match trimmed.get(..REF_PREFIX.len()) {
            Some(prefix) if prefix.eq_ignore_ascii_case(REF_PREFIX) => {
                Self::from_note_id(&trimmed[REF_PREFIX.len()..])
                    .map(|reference| (reference, None))
                    .ok_or(REF_HINT)
            }
            _ => parse_url(trimmed),
        };
        parsed.map_err(|hint| PlatformRefError {
            platform: Platform::Xiaohongshu,
            input: None,
            hint,
        })
    }

    /// Returns the ref of a note ID as Xiaohongshu prints it.
    #[must_use]
    pub fn from_note_id(value: &str) -> Option<Self> {
        (value.len() == NOTE_ID_LENGTH && value.bytes().all(|byte| byte.is_ascii_hexdigit())).then(
            || Self {
                note_id: value.to_ascii_lowercase(),
            },
        )
    }

    /// Returns the note ID.
    #[must_use]
    pub fn note_id(&self) -> &str {
        &self.note_id
    }

    /// Returns the note page without any access token; it identifies the note but does not
    /// open it.
    #[must_use]
    pub fn canonical_url(&self) -> String {
        format!("{NOTE_PAGE}{}", self.note_id)
    }

    /// Returns the link that opens the note in a logged-in browser.
    pub(crate) fn access_url(&self, token: &AccessToken) -> String {
        format!(
            "{NOTE_PAGE}{}?{TOKEN_PARAMETER}={}&xsec_source={ACCESS_SOURCE}",
            self.note_id,
            token.as_str()
        )
    }
}

impl fmt::Display for XiaohongshuRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.note_id)
    }
}

/// The `xsec_token` that opens a Xiaohongshu note: a parameter the note needs, not part of its
/// identity. Its debug form hides the value and it never serializes, so it reaches only the
/// route and the `access_url` that a successful result prints.
#[derive(Clone, Eq, PartialEq)]
pub(crate) struct AccessToken(String);

impl AccessToken {
    /// Accepts 1 to 128 URL-safe base64 characters.
    pub(crate) fn new(value: &str) -> Option<Self> {
        let valid = (1..=MAX_TOKEN_LENGTH).contains(&value.len())
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'=' | b'-'));
        valid.then(|| Self(value.to_owned()))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for AccessToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AccessToken(********)")
    }
}

/// The result order of a Xiaohongshu search.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum XiaohongshuSort {
    /// Xiaohongshu's own blend.
    #[default]
    Comprehensive,
    /// The newest first.
    Latest,
    MostLiked,
    MostCommented,
    MostCollected,
}

/// The kind of note a Xiaohongshu search lists.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum XiaohongshuNoteType {
    #[default]
    All,
    /// Image notes.
    Image,
    /// Video notes.
    Video,
}

/// How recently the notes of a Xiaohongshu search were published.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum XiaohongshuPublishTime {
    #[default]
    Any,
    Day,
    Week,
    HalfYear,
}

/// Xiaohongshu search options; the query itself goes to the site unchanged.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct XiaohongshuSearchOptions {
    pub sort: XiaohongshuSort,
    pub note_type: XiaohongshuNoteType,
    pub publish_time: XiaohongshuPublishTime,
}

impl XiaohongshuSearchOptions {
    pub(super) fn validate(query: &str, limit: u16) -> Result<(), String> {
        if !(1..=XIAOHONGSHU_MAX_LIMIT).contains(&limit) {
            return Err(format!(
                "--limit must be between 1 and {XIAOHONGSHU_MAX_LIMIT}"
            ));
        }
        if query.trim().is_empty() {
            return Err("xiaohongshu search needs a query".into());
        }
        Ok(())
    }
}

/// Splits a note URL into the note and the access token of its query. The hint names the part
/// that is wrong and never repeats the input.
fn parse_url(input: &str) -> Result<(XiaohongshuRef, Option<AccessToken>), &'static str> {
    let scheme_end = input.find("://").ok_or(REF_HINT)?;
    if !["http", "https"]
        .iter()
        .any(|scheme| input[..scheme_end].eq_ignore_ascii_case(scheme))
    {
        return Err(REF_HINT);
    }
    let rest = &input[scheme_end + 3..];
    let rest = rest.split('#').next().unwrap_or_default();
    let (address, query) = rest.split_once('?').unwrap_or((rest, ""));
    let (host, path) = address.split_once('/').unwrap_or((address, ""));
    let host = host.to_ascii_lowercase();
    if host == "xhslink.com" || host.ends_with(".xhslink.com") {
        return Err(SHORT_LINK_HINT);
    }
    if host == "rednote.com" || host.ends_with(".rednote.com") {
        return Err(REDNOTE_HINT);
    }
    if !NOTE_HOSTS.contains(&host.as_str()) {
        return Err(REF_HINT);
    }
    let segments = path.trim_end_matches('/').split('/').collect::<Vec<_>>();
    let note_id = match segments.as_slice() {
        ["explore" | "search_result", id]
        | ["discovery", "item", id]
        | ["user", "profile", _, id] => *id,
        _ => return Err(REF_HINT),
    };
    let reference = XiaohongshuRef::from_note_id(note_id).ok_or(REF_HINT)?;
    let mut tokens = query
        .split('&')
        .map(|pair| pair.split_once('=').unwrap_or((pair, "")))
        .filter(|(name, _)| *name == TOKEN_PARAMETER)
        .map(|(_, value)| value);
    let token = match (tokens.next(), tokens.next()) {
        (None, _) => None,
        (Some(value), None) => Some(
            percent_decode(value)
                .as_deref()
                .and_then(AccessToken::new)
                .ok_or(TOKEN_HINT)?,
        ),
        (Some(_), Some(_)) => return Err(TOKEN_HINT),
    };
    Ok((reference, token))
}

/// Decodes `%XX` escapes into ASCII; any other escape, or a `+`, leaves no valid token.
fn percent_decode(value: &str) -> Option<String> {
    let mut decoded = String::with_capacity(value.len());
    let mut bytes = value.bytes();
    while let Some(byte) = bytes.next() {
        match byte {
            b'%' => {
                let high = char::from(bytes.next()?).to_digit(16)?;
                let low = char::from(bytes.next()?).to_digit(16)?;
                decoded.push(char::from(u8::try_from(high * 16 + low).ok()?));
            }
            b'+' => return None,
            _ => decoded.push(char::from(byte)),
        }
    }
    Some(decoded)
}

#[derive(Clone, Debug, Serialize)]
/// The Xiaohongshu fields of a search result, read from the note card.
pub struct XiaohongshuItemData {
    /// `image` or `video`, or Xiaohongshu's own value for another kind.
    pub note_type: Option<String>,
    pub author_id: Option<String>,
    /// The counts exactly as Xiaohongshu shows them, such as `1.2万`.
    pub likes: Option<String>,
    pub collects: Option<String>,
    pub comments: Option<String>,
    pub shares: Option<String>,
    /// The publication date text exactly as the card shows it.
    pub published_text: Option<String>,
    /// The link that opens the note in a logged-in browser; it carries the access token.
    pub access_url: String,
}

#[cfg(test)]
mod tests {
    use super::{AccessToken, XiaohongshuRef, XiaohongshuSearchOptions};
    use crate::types::{Platform, PlatformRef, PlatformSearchOptions, PlatformSearchRequest};

    const NOTE: &str = "64a1b2c3d4e5f60718293a4b";

    fn parsed(input: &str) -> Option<(String, Option<String>)> {
        XiaohongshuRef::parse_with_access(input)
            .ok()
            .map(|(reference, token)| {
                (
                    reference.to_string(),
                    token.map(|token| token.as_str().to_owned()),
                )
            })
    }

    #[test]
    fn every_note_url_form_parses_to_the_same_ref() {
        let parsed = [
            "xiaohongshu:64a1b2c3d4e5f60718293a4b",
            " XIAOHONGSHU:64A1B2C3D4E5F60718293A4B ",
            "https://www.xiaohongshu.com/explore/64a1b2c3d4e5f60718293a4b",
            "http://xiaohongshu.com/explore/64a1b2c3d4e5f60718293a4b/",
            "https://WWW.Xiaohongshu.com/discovery/item/64a1b2c3d4e5f60718293a4b",
            "https://www.xiaohongshu.com/search_result/64a1b2c3d4e5f60718293a4b#comments",
            "https://www.xiaohongshu.com/user/profile/5ff0e6410000000001008400/64a1b2c3d4e5f60718293a4b",
            "https://www.xiaohongshu.com/explore/64a1b2c3d4e5f60718293a4b?source=webshare&xhsshare=pc_web",
        ]
        .map(|input| parsed(input).map(|(reference, _)| reference));

        assert_eq!(parsed, [(); 8].map(|()| Some(NOTE.to_owned())));
    }

    #[test]
    fn a_note_url_yields_its_decoded_access_token() {
        let parsed = [
            "https://www.xiaohongshu.com/explore/64a1b2c3d4e5f60718293a4b?xsec_token=ABcd-_09xyz%3D&xsec_source=pc_search",
            "https://www.xiaohongshu.com/discovery/item/64a1b2c3d4e5f60718293a4b?source=webshare&xsec_token=AB12=#top",
            "https://www.xiaohongshu.com/explore/64a1b2c3d4e5f60718293a4b",
            "xiaohongshu:64a1b2c3d4e5f60718293a4b",
        ]
        .map(|input| parsed(input).and_then(|(_, token)| token));

        assert_eq!(
            parsed,
            [
                Some("ABcd-_09xyz=".to_owned()),
                Some("AB12=".to_owned()),
                None,
                None
            ]
        );
    }

    #[test]
    fn a_token_must_be_one_to_128_url_safe_base64_characters() {
        let long = "A".repeat(128);
        let too_long = "A".repeat(129);
        let accepted = [
            "",
            "A",
            long.as_str(),
            too_long.as_str(),
            "AB+cd",
            "AB/cd",
            "AB cd",
            "AB.cd",
            "令牌",
        ]
        .map(|value| AccessToken::new(value).is_some());

        assert_eq!(
            accepted,
            [false, true, true, false, false, false, false, false, false]
        );
    }

    #[test]
    fn a_url_with_a_malformed_token_is_rejected_without_echoing_it() {
        let inputs = [
            "https://www.xiaohongshu.com/explore/64a1b2c3d4e5f60718293a4b?xsec_token=",
            "https://www.xiaohongshu.com/explore/64a1b2c3d4e5f60718293a4b?xsec_token=SECRET%2Bplus",
            "https://www.xiaohongshu.com/explore/64a1b2c3d4e5f60718293a4b?xsec_token=SECRET%zz",
            "https://www.xiaohongshu.com/explore/64a1b2c3d4e5f60718293a4b?xsec_token=SECRETa&xsec_token=SECRETb",
        ];
        let messages = inputs.map(|input| {
            XiaohongshuRef::parse_with_access(input)
                .map(|_| ())
                .unwrap_err()
                .to_string()
        });

        assert_eq!(
            messages,
            [(); 4].map(|()| "unrecognized xiaohongshu reference; the xsec_token of the note URL must be 1 to 128 URL-safe base64 characters (A-Z, a-z, 0-9, `_`, `=`, `-`)".to_owned())
        );
    }

    #[test]
    fn short_links_and_rednote_links_are_rejected_with_their_own_hints() {
        let messages = [
            "http://xhslink.com/a/AbCdEfG",
            "https://www.xhslink.com/m/AbCdEfG",
            "https://www.rednote.com/explore/64a1b2c3d4e5f60718293a4b",
            "https://rednote.com/explore/64a1b2c3d4e5f60718293a4b?xsec_token=SECRET",
        ]
        .map(|input| XiaohongshuRef::parse(input).unwrap_err().to_string());

        assert_eq!(
            messages,
            [
                "unrecognized xiaohongshu reference; xhslink.com short links need a network request to expand: open the link in a browser and pass the full xiaohongshu.com note URL".to_owned(),
                "unrecognized xiaohongshu reference; xhslink.com short links need a network request to expand: open the link in a browser and pass the full xiaohongshu.com note URL".to_owned(),
                "unrecognized xiaohongshu reference; rednote.com links are not supported: pass a xiaohongshu.com note URL".to_owned(),
                "unrecognized xiaohongshu reference; rednote.com links are not supported: pass a xiaohongshu.com note URL".to_owned(),
            ]
        );
    }

    #[test]
    fn malformed_ids_and_other_pages_are_rejected_without_echoing_the_input() {
        let inputs = [
            "xiaohongshu:64a1b2c3d4e5f60718293a4",
            "xiaohongshu:64a1b2c3d4e5f60718293a4bc",
            "xiaohongshu:64a1b2c3d4e5f60718293a4g",
            "xiaohongshu:",
            "64a1b2c3d4e5f60718293a4b",
            "https://www.xiaohongshu.com/explore",
            "https://www.xiaohongshu.com/explore/64a1b2c3d4e5f60718293a4b/extra",
            "https://www.xiaohongshu.com/user/profile/5ff0e6410000000001008400",
            "https://www.xiaohongshu.com/notes/64a1b2c3d4e5f60718293a4b",
            "https://edith.xiaohongshu.com/explore/64a1b2c3d4e5f60718293a4b",
            "https://www.xiaohongshu.com.evil.example/explore/64a1b2c3d4e5f60718293a4b",
            "ftp://www.xiaohongshu.com/explore/64a1b2c3d4e5f60718293a4b",
        ];
        let messages = inputs.map(|input| XiaohongshuRef::parse(input).unwrap_err().to_string());

        assert_eq!(
            messages,
            [(); 12].map(|()| "unrecognized xiaohongshu reference; pass a `xiaohongshu:<note_id>` ref or a xiaohongshu.com note URL such as an `access_url` from search".to_owned())
        );
    }

    #[test]
    fn canonical_urls_round_trip_to_the_ref() {
        let reference =
            PlatformRef::parse(Platform::Xiaohongshu, &format!("xiaohongshu:{NOTE}")).expect("ref");

        let round_trip = PlatformRef::parse(Platform::Xiaohongshu, &reference.canonical_url());

        assert_eq!(
            (reference.canonical_url(), reference.kind(), round_trip.ok()),
            (
                format!("https://www.xiaohongshu.com/explore/{NOTE}"),
                "note",
                Some(reference)
            )
        );
    }

    #[test]
    fn an_access_url_carries_the_token_and_parses_back_to_both() {
        let reference = XiaohongshuRef::from_note_id(NOTE).expect("note ID");
        let token = AccessToken::new("ABcd-_09=").expect("token");

        let access_url = reference.access_url(&token);

        assert_eq!(
            (access_url.clone(), parsed(&access_url)),
            (
                format!(
                    "https://www.xiaohongshu.com/explore/{NOTE}?xsec_token=ABcd-_09=&xsec_source=pc_search"
                ),
                Some((NOTE.to_owned(), Some("ABcd-_09=".to_owned())))
            )
        );
    }

    #[test]
    fn an_access_token_debugs_without_its_value() {
        let token = AccessToken::new("SECRETtoken").expect("token");

        assert_eq!(format!("{token:?}"), "AccessToken(********)");
    }

    fn request(query: &str, limit: u16) -> PlatformSearchRequest {
        PlatformSearchRequest {
            query: query.into(),
            limit,
            options: PlatformSearchOptions::Xiaohongshu(XiaohongshuSearchOptions::default()),
            page: None,
        }
    }

    #[test]
    fn a_search_needs_a_query_with_a_word_and_a_limit_of_one_to_one_hundred() {
        let results = [
            ("", 20),
            (" \t ", 20),
            ("咖啡", 0),
            ("咖啡", 1),
            ("咖啡", 100),
            ("咖啡", 101),
        ]
        .map(|(query, limit)| request(query, limit).validate());

        assert_eq!(
            results,
            [
                Err("xiaohongshu search needs a query".to_owned()),
                Err("xiaohongshu search needs a query".to_owned()),
                Err("--limit must be between 1 and 100".to_owned()),
                Ok(()),
                Ok(()),
                Err("--limit must be between 1 and 100".to_owned()),
            ]
        );
    }
}
