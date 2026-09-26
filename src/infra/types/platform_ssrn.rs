//! SSRN shapes: paper refs, search options, and item metadata.

use std::fmt;

use serde::{Deserialize, Serialize};

use super::platform::{Platform, PlatformRefError};

const SSRN_MAX_LIMIT: u16 = 100;
const ABSTRACT_PAGE_HOST: &str = "papers.ssrn.com";
const ABSTRACT_PAGE_PATH: &str = "sol3/papers.cfm";
const SHORT_URL_HOSTS: [&str; 2] = ["ssrn.com", "www.ssrn.com"];
const DOI_HOSTS: [&str; 2] = ["doi.org", "dx.doi.org"];
const DOI_PREFIX: &str = "10.2139/ssrn.";

#[derive(Clone, Debug, Eq, PartialEq)]
/// An SSRN paper identity: its abstract ID. SSRN keeps one DOI across revisions, so the ref
/// has no version.
pub struct SsrnRef {
    id: String,
}

impl SsrnRef {
    /// Parses `ssrn:<id>`, an SSRN abstract page URL, an `ssrn.com/abstract=<id>` URL, or the
    /// `10.2139/ssrn.<id>` DOI either bare or as a doi.org URL.
    ///
    /// # Errors
    ///
    /// Returns [`PlatformRefError`] for any other input, including SSRN PDF download links,
    /// whose file name numbers are not paper identities.
    pub fn parse(input: &str) -> Result<Self, PlatformRefError> {
        let trimmed = input.trim();
        let identifier = match trimmed.get(..5) {
            Some(prefix) if prefix.eq_ignore_ascii_case("ssrn:") => Some(&trimmed[5..]),
            _ => doi_identifier(trimmed).or_else(|| url_identifier(trimmed)),
        };
        identifier
            .and_then(Self::from_id)
            .ok_or_else(|| PlatformRefError {
                platform: Platform::Ssrn,
                input: input.to_owned(),
                hint: "pass an `ssrn:<id>` ref, an SSRN abstract page URL, or a 10.2139/ssrn.<id> DOI",
            })
    }

    /// Returns the ref of a Crossref DOI when it has the SSRN shape `10.2139/ssrn.<id>`.
    #[must_use]
    pub fn from_doi(doi: &str) -> Option<Self> {
        doi_identifier(doi).and_then(Self::from_id)
    }

    fn from_id(id: &str) -> Option<Self> {
        let is_id =
            !id.is_empty() && !id.starts_with('0') && id.bytes().all(|byte| byte.is_ascii_digit());
        is_id.then(|| Self { id: id.to_owned() })
    }

    /// Returns the abstract ID.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the SSRN DOI of the paper.
    #[must_use]
    pub fn doi(&self) -> String {
        format!("{DOI_PREFIX}{}", self.id)
    }

    /// Returns the abstract-page URL.
    #[must_use]
    pub fn canonical_url(&self) -> String {
        format!(
            "https://{ABSTRACT_PAGE_HOST}/{ABSTRACT_PAGE_PATH}?abstract_id={}",
            self.id
        )
    }
}

impl fmt::Display for SsrnRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.id)
    }
}

/// Returns the ID of a bare `10.2139/ssrn.<id>` DOI; DOIs compare case-insensitively.
fn doi_identifier(value: &str) -> Option<&str> {
    let prefix = value.get(..DOI_PREFIX.len())?;
    prefix
        .eq_ignore_ascii_case(DOI_PREFIX)
        .then(|| &value[DOI_PREFIX.len()..])
}

fn url_identifier(input: &str) -> Option<&str> {
    let rest = input
        .strip_prefix("https://")
        .or_else(|| input.strip_prefix("http://"))?;
    let (host, path) = rest.split_once('/')?;
    let host = host.to_ascii_lowercase();
    if host == ABSTRACT_PAGE_HOST {
        let (path, query) = path.split_once('?')?;
        if !path.eq_ignore_ascii_case(ABSTRACT_PAGE_PATH) {
            return None;
        }
        let query = query.split('#').next()?;
        return query
            .split('&')
            .find_map(|pair| pair.strip_prefix("abstract_id="));
    }
    let path = path.split(['?', '#']).next()?;
    if SHORT_URL_HOSTS.contains(&host.as_str()) {
        return path.strip_prefix("abstract=");
    }
    if DOI_HOSTS.contains(&host.as_str()) {
        return doi_identifier(path);
    }
    None
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
/// SSRN search options; the query is the only criterion.
pub struct SsrnSearchOptions {}

impl SsrnSearchOptions {
    pub(super) fn validate(query: &str, limit: u16) -> Result<(), String> {
        if !(1..=SSRN_MAX_LIMIT).contains(&limit) {
            return Err(format!("--limit must be between 1 and {SSRN_MAX_LIMIT}"));
        }
        if query.trim().is_empty() {
            return Err("ssrn search needs a query".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Serialize)]
/// SSRN metadata of a paper; a route fills only the fields it reads.
pub struct SsrnItemData {
    /// The complete author-written abstract.
    #[serde(rename = "abstract")]
    pub abstract_text: Option<String>,
    /// A search-result excerpt; never the abstract.
    pub snippet: Option<String>,
    /// The DOI as Crossref records it.
    pub doi: Option<String>,
    /// The Crossref work type, such as `posted-content` or `journal-article`.
    pub crossref_type: Option<String>,
    /// When Crossref registered the DOI; never an SSRN posting date.
    pub crossref_created: Option<String>,
    /// The posting date as the SSRN page shows it.
    pub posted: Option<String>,
    /// The last revision date as the SSRN page shows it.
    pub last_revised: Option<String>,
    /// The date written as the SSRN page shows it.
    pub date_written: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::SsrnRef;
    use crate::types::{Platform, PlatformRef, PlatformSearchOptions, PlatformSearchRequest};

    fn ssrn(input: &str) -> Option<String> {
        PlatformRef::parse(Platform::Ssrn, input)
            .ok()
            .map(|reference| reference.to_string())
    }

    #[test]
    fn every_supported_form_parses_to_the_same_ref() {
        let parsed = [
            "ssrn:2042750",
            "SSRN:2042750",
            " ssrn:2042750 ",
            "https://papers.ssrn.com/sol3/papers.cfm?abstract_id=2042750",
            "http://papers.ssrn.com/sol3/papers.cfm?abstract_id=2042750",
            "https://PAPERS.SSRN.COM/sol3/Papers.cfm?abstract_id=2042750",
            "https://papers.ssrn.com/sol3/papers.cfm?download=yes&abstract_id=2042750#body",
            "https://ssrn.com/abstract=2042750",
            "https://www.ssrn.com/abstract=2042750",
            "https://WWW.SSRN.com/abstract=2042750?utm=x",
            "10.2139/ssrn.2042750",
            "10.2139/SSRN.2042750",
            "https://doi.org/10.2139/ssrn.2042750",
            "https://dx.doi.org/10.2139/ssrn.2042750",
        ]
        .map(ssrn);

        assert_eq!(parsed, [(); 14].map(|()| Some("ssrn:2042750".to_owned())));
    }

    #[test]
    fn download_links_lookalike_hosts_and_short_links_are_rejected() {
        let parsed = [
            "https://papers.ssrn.com/sol3/Delivery.cfm/SSRN_ID2881657_code1556771.pdf?abstractid=2042750&mirid=1",
            "https://papers.ssrn.com/sol3/Delivery.cfm?abstractid=2042750",
            "https://papers.ssrn.com/sol3/papers.cfm?abstractid=2042750",
            "https://papers.ssrn.com/sol3/papers.cfm",
            "https://papers.ssrn.com.evil.example/sol3/papers.cfm?abstract_id=2042750",
            "https://ssrn.com.evil.example/abstract=2042750",
            "https://evilssrn.com/abstract=2042750",
            "https://ssrn.com/abstract=2042750x",
            "https://doi.org/10.2139/ssrn.2042750/extra",
            "https://doi.org/10.1000/ssrn.2042750",
            "https://bit.ly/abc",
            "2042750",
            "ssrn:",
            "ssrn:02042750",
            "ssrn:0",
            "ssrn:+2042750",
            "ssrn:2042750v2",
        ]
        .map(ssrn);

        assert_eq!(parsed, [const { None }; 17]);
    }

    #[test]
    fn canonical_urls_round_trip_to_the_ref() {
        let reference = PlatformRef::parse(Platform::Ssrn, "ssrn:2042750").expect("valid ref");

        let round_trip = PlatformRef::parse(Platform::Ssrn, &reference.canonical_url());

        assert_eq!(
            (reference.canonical_url(), round_trip.ok()),
            (
                "https://papers.ssrn.com/sol3/papers.cfm?abstract_id=2042750".to_owned(),
                Some(reference)
            )
        );
    }

    #[test]
    fn a_ref_names_its_ssrn_doi() {
        let reference = SsrnRef::parse("ssrn:2042750").expect("valid ref");

        assert_eq!(reference.doi(), "10.2139/ssrn.2042750");
    }

    #[test]
    fn only_ssrn_shaped_dois_yield_refs() {
        let refs = [
            "10.2139/ssrn.2042750",
            "10.2139/ssrn.2042750a",
            "10.2139/other.1",
            "10.1016/j.x.2020.1",
        ]
        .map(|doi| SsrnRef::from_doi(doi).map(|reference| reference.to_string()));

        assert_eq!(refs, [Some("2042750".to_owned()), None, None, None]);
    }

    #[test]
    fn unrecognized_refs_explain_the_accepted_forms() {
        let error = PlatformRef::parse(Platform::Ssrn, "https://bit.ly/abc").unwrap_err();

        assert_eq!(
            error.to_string(),
            "unrecognized ssrn reference `https://bit.ly/abc`; pass an `ssrn:<id>` ref, an SSRN abstract page URL, or a 10.2139/ssrn.<id> DOI"
        );
    }

    fn request(query: &str, limit: u16) -> PlatformSearchRequest {
        PlatformSearchRequest {
            query: query.into(),
            limit,
            options: PlatformSearchOptions::Ssrn(super::SsrnSearchOptions::default()),
            page: None,
        }
    }

    #[test]
    fn a_search_needs_a_query_with_a_word() {
        let results = ["", "  ", "momentum"].map(|query| request(query, 10).validate().is_ok());

        assert_eq!(results, [false, false, true]);
    }

    #[test]
    fn a_search_limits_page_size_to_one_through_one_hundred() {
        let results = [0, 1, 100, 101].map(|limit| request("x", limit).validate().is_ok());

        assert_eq!(results, [false, true, true, false]);
    }

    #[test]
    fn search_options_round_trip_through_json() {
        let original = PlatformSearchRequest {
            page: Some("40".into()),
            ..request("dual momentum", 20)
        };

        let encoded = serde_json::to_string(&original).expect("encode request");
        let decoded: PlatformSearchRequest =
            serde_json::from_str(&encoded).expect("decode request");

        assert_eq!(
            (encoded.contains(r#""platform":"ssrn""#), decoded),
            (true, original)
        );
    }
}
