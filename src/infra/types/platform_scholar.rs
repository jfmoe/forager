//! Google Scholar shapes: cluster refs, search options, the cited-by request, and item metadata.

use std::fmt;
use std::ops::RangeInclusive;

use serde::{Deserialize, Serialize};

use super::platform::{Platform, PlatformRefError};

pub(crate) const SCHOLAR_MAX_LIMIT: u16 = 20;
/// The attempt-target name of the cited-by operation.
pub(crate) const CITED_BY: &str = "cited_by";
const SCHOLAR_YEARS: RangeInclusive<u16> = 1000..=9999;
const SCHOLAR_HOST: &str = "scholar.google.com";
const REF_PREFIX: &str = "scholar:";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// A Google Scholar paper identity: the cluster ID that groups every version of one paper.
/// Clusters have no versions, so the ref has none.
pub struct ScholarRef {
    cluster_id: u64,
}

impl ScholarRef {
    /// Parses `scholar:<cluster_id>` or a `scholar.google.com/scholar` URL whose query names
    /// exactly one `cluster` and no `cites`.
    ///
    /// # Errors
    ///
    /// Returns [`PlatformRefError`] for any other input, including citing-article pages, author
    /// profiles, and cluster IDs that overflow a `u64`.
    pub fn parse(input: &str) -> Result<Self, PlatformRefError> {
        let trimmed = input.trim();
        let cluster = match trimmed.get(..REF_PREFIX.len()) {
            Some(prefix) if prefix.eq_ignore_ascii_case(REF_PREFIX) => {
                Some(&trimmed[REF_PREFIX.len()..])
            }
            _ => url_cluster(trimmed),
        };
        cluster
            .and_then(Self::from_cluster_id)
            .ok_or_else(|| PlatformRefError {
                platform: Platform::Scholar,
                input: Some(input.to_owned()),
                hint: "pass a `scholar:<cluster_id>` ref or a scholar.google.com/scholar?cluster=<id> URL",
            })
    }

    /// Returns the ref of a decimal cluster ID as Google Scholar prints it.
    #[must_use]
    pub fn from_cluster_id(value: &str) -> Option<Self> {
        let canonical = !value.starts_with('0') && value.bytes().all(|byte| byte.is_ascii_digit());
        canonical
            .then(|| value.parse().ok())
            .flatten()
            .map(|cluster_id| Self { cluster_id })
    }

    /// Returns the cluster ID.
    #[must_use]
    pub const fn cluster_id(self) -> u64 {
        self.cluster_id
    }

    /// Returns the cluster page that lists every version of the paper.
    #[must_use]
    pub fn canonical_url(self) -> String {
        format!("https://{SCHOLAR_HOST}/scholar?cluster={}", self.cluster_id)
    }
}

impl From<u64> for ScholarRef {
    fn from(cluster_id: u64) -> Self {
        Self { cluster_id }
    }
}

impl fmt::Display for ScholarRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.cluster_id)
    }
}

/// Google Scholar search options. The query itself carries Google Scholar's own operators.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ScholarSearchOptions {
    /// The first included publication year.
    pub year_from: Option<u16>,
    /// The last included publication year.
    pub year_to: Option<u16>,
    /// Only review articles.
    pub review_only: bool,
}

impl ScholarSearchOptions {
    pub(super) fn validate(&self, query: &str, limit: u16) -> Result<(), String> {
        validate_limit(limit)?;
        validate_years(self.year_from, self.year_to)?;
        if query.trim().is_empty() {
            return Err("scholar search needs a query".into());
        }
        Ok(())
    }
}

/// The order of the works that cite a paper.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ScholarCitedBySort {
    /// Google Scholar's relevance order.
    #[default]
    Relevance,
    /// The most recently indexed first.
    Date,
}

/// A request for one page of the works Google Scholar counts as citing a paper; a page cursor
/// encodes it completely.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ScholarCitedByRequest {
    /// The cited paper.
    #[serde(with = "cluster_id")]
    pub(crate) cited: ScholarRef,
    /// Words that the citing works must match, in Google Scholar's own query syntax.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) query: Option<String>,
    pub(crate) limit: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) year_from: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) year_to: Option<u16>,
    #[serde(default)]
    pub(crate) sort: ScholarCitedBySort,
    /// The route-owned position of the requested page; absent for the first page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) page: Option<String>,
}

impl ScholarCitedByRequest {
    /// Checks the cross-field rules that argument parsing cannot express.
    pub(crate) fn validate(&self) -> Result<(), String> {
        validate_limit(self.limit)?;
        validate_years(self.year_from, self.year_to)?;
        // Google Scholar ignores the year range when it sorts by date.
        if self.sort == ScholarCitedBySort::Date
            && (self.year_from.is_some() || self.year_to.is_some())
        {
            return Err("--sort date cannot be combined with --year-from or --year-to".into());
        }
        if self
            .query
            .as_deref()
            .is_some_and(|query| query.trim().is_empty())
        {
            return Err("--query needs a word".into());
        }
        Ok(())
    }
}

/// Serializes a ref as its bare cluster ID, keeping serde off the public ref type.
mod cluster_id {
    use serde::{Deserialize, Deserializer, Serializer};

    use super::ScholarRef;

    #[expect(
        clippy::trivially_copy_pass_by_ref,
        reason = "serde's `with` contract passes the field by reference"
    )]
    pub(super) fn serialize<S: Serializer>(
        reference: &ScholarRef,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_u64(reference.cluster_id())
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<ScholarRef, D::Error> {
        u64::deserialize(deserializer).map(ScholarRef::from)
    }
}

fn validate_limit(limit: u16) -> Result<(), String> {
    if (1..=SCHOLAR_MAX_LIMIT).contains(&limit) {
        Ok(())
    } else {
        Err(format!("--limit must be between 1 and {SCHOLAR_MAX_LIMIT}"))
    }
}

fn validate_years(from: Option<u16>, to: Option<u16>) -> Result<(), String> {
    for (flag, year) in [("--year-from", from), ("--year-to", to)] {
        if year.is_some_and(|year| !SCHOLAR_YEARS.contains(&year)) {
            return Err(format!(
                "{flag} must be between {} and {}",
                SCHOLAR_YEARS.start(),
                SCHOLAR_YEARS.end()
            ));
        }
    }
    if let (Some(from), Some(to)) = (from, to)
        && from > to
    {
        return Err("--year-from must not be later than --year-to".into());
    }
    Ok(())
}

/// Returns the `cluster` value of a Google Scholar results URL. The query must name exactly one
/// `cluster` and no `cites`; a page that lists citing works is not the paper itself.
fn url_cluster(input: &str) -> Option<&str> {
    let rest = input
        .strip_prefix("https://")
        .or_else(|| input.strip_prefix("http://"))?;
    let (host, rest) = rest.split_once('/')?;
    let (path, query) = rest.split_once('?')?;
    if !host.eq_ignore_ascii_case(SCHOLAR_HOST) || path.trim_end_matches('/') != "scholar" {
        return None;
    }
    let query = query.split('#').next()?;
    let mut clusters = Vec::new();
    for (name, value) in query
        .split('&')
        .map(|pair| pair.split_once('=').unwrap_or((pair, "")))
    {
        match name {
            "cluster" => clusters.push(value),
            "cites" => return None,
            _ => {}
        }
    }
    match clusters.as_slice() {
        [cluster] => Some(cluster),
        _ => None,
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(untagged)]
/// The Google Scholar fields of an item: a search result's own fields, or the versions a fetch
/// lists.
pub enum ScholarItemData {
    /// A search result.
    Result(ScholarResult),
    /// The versions of a cluster.
    Cluster(ScholarCluster),
}

#[derive(Clone, Debug, Serialize)]
/// The Google Scholar fields of one search result.
pub struct ScholarResult {
    /// The search-result excerpt; never an abstract.
    pub snippet: Option<String>,
    /// The page the result title links to.
    pub link: Option<String>,
    /// The byline exactly as Google Scholar shows it: authors, source, year, and domain.
    pub source: Option<String>,
    /// How many works Google Scholar counts as citing this one.
    pub cited_by: Option<u64>,
    /// How many versions Google Scholar reports for the cluster.
    pub version_count: Option<u64>,
    pub resources: Vec<ScholarResource>,
    /// The Google Scholar result type, such as `Pdf` or `Html`.
    pub result_type: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
/// The versions of a cluster in Google Scholar's order; none of them is marked canonical.
pub struct ScholarCluster {
    pub versions: Vec<ScholarVersion>,
}

#[derive(Clone, Debug, Serialize)]
/// One version of a paper in its cluster.
pub struct ScholarVersion {
    pub title: String,
    pub link: Option<String>,
    /// The byline exactly as Google Scholar shows it.
    pub source: Option<String>,
    pub resources: Vec<ScholarResource>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
/// A PDF or HTML copy Google Scholar links next to a result.
pub struct ScholarResource {
    /// The host label Google Scholar shows; it may be empty.
    pub title: String,
    pub file_format: Option<String>,
    pub url: String,
}

#[cfg(test)]
mod tests {
    use crate::types::{Platform, PlatformRef, PlatformSearchOptions, PlatformSearchRequest};

    use super::ScholarSearchOptions;

    fn scholar(input: &str) -> Option<String> {
        PlatformRef::parse(Platform::Scholar, input)
            .ok()
            .map(|reference| reference.to_string())
    }

    #[test]
    fn every_supported_form_parses_to_the_same_ref() {
        let parsed = [
            "scholar:18208131694456651388",
            "SCHOLAR:18208131694456651388",
            " scholar:18208131694456651388 ",
            "https://scholar.google.com/scholar?cluster=18208131694456651388",
            "http://scholar.google.com/scholar?cluster=18208131694456651388",
            "https://Scholar.Google.com/scholar?cluster=18208131694456651388",
            "https://scholar.google.com/scholar/?cluster=18208131694456651388",
            "https://scholar.google.com/scholar?cluster=18208131694456651388&hl=en&num=20&as_sdt=0,27",
            "https://scholar.google.com/scholar?hl=en&cluster=18208131694456651388#d=gs_qabs",
        ]
        .map(scholar);

        assert_eq!(
            parsed,
            [(); 9].map(|()| Some("scholar:18208131694456651388".to_owned()))
        );
    }

    #[test]
    fn citing_pages_profiles_duplicates_and_overflows_are_rejected() {
        let parsed = [
            "https://scholar.google.com/scholar?cites=18208131694456651388",
            "https://scholar.google.com/scholar?cluster=18208131694456651388&cites=18208131694456651388",
            "https://scholar.google.com/scholar?cluster=1&cluster=2",
            "https://scholar.google.com/scholar?cluster=18446744073709551616",
            "https://scholar.google.com/scholar?cluster=12x",
            "https://scholar.google.com/scholar?cluster=",
            "https://scholar.google.com/scholar?q=time+series+momentum",
            "https://scholar.google.com/citations?user=q9g8tuAAAAAJ&hl=en",
            "https://scholar.google.com/scholar_lookup?cluster=1",
            "https://scholar.google.com.evil.example/scholar?cluster=1",
            "https://scholar.google.co.uk/scholar?cluster=1",
            "18208131694456651388",
            "scholar:",
            "scholar:18446744073709551616",
            "scholar:0123",
            "scholar:+1",
            "scholar:fL6eJ05HsPwJ",
        ]
        .map(scholar);

        assert_eq!(parsed, [const { None }; 17]);
    }

    #[test]
    fn a_bare_cites_or_second_cluster_key_rejects_the_url() {
        let parsed = [
            "https://scholar.google.com/scholar?cluster=18208131694456651388&cites",
            "https://scholar.google.com/scholar?cites&cluster=18208131694456651388",
            "https://scholar.google.com/scholar?cluster=18208131694456651388&cluster",
        ]
        .map(scholar);

        assert_eq!(parsed, [const { None }; 3]);
    }

    #[test]
    fn the_largest_cluster_id_parses() {
        assert_eq!(
            scholar("scholar:18446744073709551615"),
            Some("scholar:18446744073709551615".to_owned())
        );
    }

    #[test]
    fn canonical_urls_round_trip_to_the_ref() {
        let reference =
            PlatformRef::parse(Platform::Scholar, "scholar:18208131694456651388").expect("ref");

        let round_trip = PlatformRef::parse(Platform::Scholar, &reference.canonical_url());

        assert_eq!(
            (reference.canonical_url(), reference.kind(), round_trip.ok()),
            (
                "https://scholar.google.com/scholar?cluster=18208131694456651388".to_owned(),
                "paper",
                Some(reference)
            )
        );
    }

    #[test]
    fn unrecognized_refs_explain_the_accepted_forms() {
        let error = PlatformRef::parse(Platform::Scholar, "https://bit.ly/abc").unwrap_err();

        assert_eq!(
            error.to_string(),
            "unrecognized scholar reference `https://bit.ly/abc`; pass a `scholar:<cluster_id>` ref or a scholar.google.com/scholar?cluster=<id> URL"
        );
    }

    fn request(query: &str, limit: u16) -> PlatformSearchRequest {
        PlatformSearchRequest {
            query: query.into(),
            limit,
            options: PlatformSearchOptions::Scholar(ScholarSearchOptions::default()),
            page: None,
        }
    }

    #[test]
    fn a_search_needs_a_query_with_a_word() {
        let results = ["", " \t ", "momentum"].map(|query| request(query, 20).validate());

        assert_eq!(
            results,
            [
                Err("scholar search needs a query".to_owned()),
                Err("scholar search needs a query".to_owned()),
                Ok(())
            ]
        );
    }

    #[test]
    fn a_search_limits_page_size_to_one_through_twenty() {
        let results = [0, 1, 20, 21].map(|limit| request("x", limit).validate().is_ok());

        assert_eq!(results, [false, true, true, false]);
    }

    fn with_years(from: Option<u16>, to: Option<u16>) -> PlatformSearchRequest {
        PlatformSearchRequest {
            options: PlatformSearchOptions::Scholar(ScholarSearchOptions {
                year_from: from,
                year_to: to,
                ..ScholarSearchOptions::default()
            }),
            ..request("momentum", 20)
        }
    }

    #[test]
    fn each_year_bound_lies_between_1000_and_9999() {
        let results = [
            (Some(999), None),
            (Some(1000), None),
            (None, Some(9999)),
            (None, Some(10000)),
            (Some(0), Some(2024)),
        ]
        .map(|(from, to)| with_years(from, to).validate());

        assert_eq!(
            results,
            [
                Err("--year-from must be between 1000 and 9999".to_owned()),
                Ok(()),
                Ok(()),
                Err("--year-to must be between 1000 and 9999".to_owned()),
                Err("--year-from must be between 1000 and 9999".to_owned()),
            ]
        );
    }

    #[test]
    fn the_first_year_may_equal_but_not_follow_the_last() {
        let results = [(2024, 2024), (2025, 2024)]
            .map(|(from, to)| with_years(Some(from), Some(to)).validate());

        assert_eq!(
            results,
            [
                Ok(()),
                Err("--year-from must not be later than --year-to".to_owned())
            ]
        );
    }

    #[test]
    fn search_options_round_trip_through_json() {
        let original = PlatformSearchRequest {
            options: PlatformSearchOptions::Scholar(ScholarSearchOptions {
                year_from: Some(2020),
                year_to: Some(2024),
                review_only: true,
            }),
            ..request("\"time series momentum\" author:\"Pedersen\"", 20)
        };

        let encoded = serde_json::to_string(&original).expect("encode request");
        let decoded: PlatformSearchRequest =
            serde_json::from_str(&encoded).expect("decode request");

        assert_eq!(
            (encoded.contains(r#""platform":"scholar""#), decoded),
            (true, original)
        );
    }
}
