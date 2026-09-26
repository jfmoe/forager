//! arXiv shapes: paper refs, search options, and item metadata.

use std::fmt;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use super::platform::{Platform, PlatformRefError};

const ARXIV_MAX_LIMIT: u16 = 100;
const ARXIV_HOSTS: [&str; 3] = ["arxiv.org", "www.arxiv.org", "export.arxiv.org"];

#[derive(Clone, Debug, Eq, PartialEq)]
/// An arXiv paper identity in the new (`2401.01234`) or old (`hep-th/9901001`) scheme, with an
/// optional version.
pub struct ArxivRef {
    id: String,
    version: Option<u32>,
}

impl ArxivRef {
    /// Parses `arxiv:<id>[v<n>]` or an arXiv abstract, PDF, or HTML page URL on arxiv.org or its
    /// export mirror.
    ///
    /// # Errors
    ///
    /// Returns [`PlatformRefError`] for any other input.
    pub fn parse(input: &str) -> Result<Self, PlatformRefError> {
        let trimmed = input.trim();
        let identifier = match trimmed.get(..6) {
            Some(prefix) if prefix.eq_ignore_ascii_case("arxiv:") => Some(&trimmed[6..]),
            _ => page_identifier(trimmed),
        };
        identifier
            .and_then(Self::from_identifier)
            .ok_or_else(|| PlatformRefError {
                platform: Platform::Arxiv,
                input: input.to_owned(),
                hint: "pass an `arxiv:<id>[v<n>]` ref or an original arxiv.org URL",
            })
    }

    fn from_identifier(value: &str) -> Option<Self> {
        let (id, version) = split_version(value);
        let id = if is_new_style_id(id) {
            id.to_owned()
        } else {
            old_style_id(id)?
        };
        Some(Self { id, version })
    }

    /// Returns the identifier without its version.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the version, when the ref names one.
    #[must_use]
    pub const fn version(&self) -> Option<u32> {
        self.version
    }

    /// Returns the abstract-page URL.
    #[must_use]
    pub fn canonical_url(&self) -> String {
        format!("https://arxiv.org/abs/{self}")
    }

    /// Returns the official HTML full-text URL; it carries a version only when the ref does.
    #[must_use]
    pub fn html_url(&self) -> String {
        format!("https://arxiv.org/html/{self}")
    }

    /// Returns the PDF full-text URL; it carries a version only when the ref does.
    #[must_use]
    pub fn pdf_url(&self) -> String {
        format!("https://arxiv.org/pdf/{self}")
    }
}

impl fmt::Display for ArxivRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.version {
            Some(version) => write!(formatter, "{}v{version}", self.id),
            None => formatter.write_str(&self.id),
        }
    }
}

fn page_identifier(input: &str) -> Option<&str> {
    let rest = input
        .strip_prefix("https://")
        .or_else(|| input.strip_prefix("http://"))?;
    let (host, path) = rest.split_once('/')?;
    if !ARXIV_HOSTS.contains(&host.to_ascii_lowercase().as_str()) {
        return None;
    }
    let path = path.split(['?', '#']).next()?;
    let (page, identifier) = path.split_once('/')?;
    let identifier = identifier.strip_suffix('/').unwrap_or(identifier);
    match page {
        "abs" | "html" => Some(identifier),
        "pdf" => Some(identifier.strip_suffix(".pdf").unwrap_or(identifier)),
        _ => None,
    }
}

fn split_version(value: &str) -> (&str, Option<u32>) {
    let Some(position) = value.rfind('v') else {
        return (value, None);
    };
    let digits = &value[position + 1..];
    let is_version = !digits.is_empty()
        && !digits.starts_with('0')
        && digits.bytes().all(|byte| byte.is_ascii_digit());
    match digits.parse().ok().filter(|_| is_version) {
        Some(version) => (&value[..position], Some(version)),
        None => (value, None),
    }
}

// New-style IDs are YYMM.NNNN (2007–2014) or YYMM.NNNNN (2015 onward).
fn is_new_style_id(id: &str) -> bool {
    let Some((prefix, number)) = id.split_once('.') else {
        return false;
    };
    prefix.len() == 4
        && all_digits(prefix)
        && (1..=12).contains(&prefix[2..].parse::<u8>().unwrap_or(0))
        && matches!(number.len(), 4 | 5)
        && all_digits(number)
}

// Old-style IDs are archive[.SUBJECT]/YYMMNNN, for example hep-th/9901001 or math.GT/0309136.
// The subject class is not part of the identity: arXiv resolves math.GT/0309136 to
// math/0309136, and the Query API finds only the latter.
fn old_style_id(id: &str) -> Option<String> {
    let (archive, number) = id.split_once('/')?;
    let (archive, subject) = archive
        .split_once('.')
        .map_or((archive, None), |(archive, subject)| {
            (archive, Some(subject))
        });
    (is_hyphenated_lowercase(archive)
        && subject.is_none_or(|subject| {
            subject.len() == 2 && subject.bytes().all(|byte| byte.is_ascii_uppercase())
        })
        && number.len() == 7
        && all_digits(number))
    .then(|| format!("{archive}/{number}"))
}

fn all_digits(value: &str) -> bool {
    value.bytes().all(|byte| byte.is_ascii_digit())
}

fn is_hyphenated_lowercase(value: &str) -> bool {
    value
        .split('-')
        .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_lowercase()))
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
/// The arXiv result order; every order is descending, newest or best first.
pub enum ArxivSort {
    /// Best match first.
    #[default]
    Relevance,
    /// Most recently submitted first.
    Submitted,
    /// Most recently updated first.
    Updated,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
/// Optional arXiv search filters and order.
pub struct ArxivSearchOptions {
    /// Category codes such as `q-fin.PM`; any category matches.
    #[serde(default)]
    pub categories: Vec<String>,
    /// An author-name phrase.
    #[serde(default)]
    pub author: Option<String>,
    /// A title phrase.
    #[serde(default)]
    pub title: Option<String>,
    /// The first included submission date (UTC).
    #[serde(default)]
    pub submitted_from: Option<NaiveDate>,
    /// The last included submission date (UTC); the whole day is included.
    #[serde(default)]
    pub submitted_to: Option<NaiveDate>,
    /// The result order.
    #[serde(default)]
    pub sort: ArxivSort,
}

impl ArxivSearchOptions {
    /// Splits plain text into the literal words a search must match.
    ///
    /// A double quote cannot appear inside a quoted arXiv term and a backslash escapes the
    /// closing quote, so both separate words.
    pub fn words(text: &str) -> impl Iterator<Item = &str> {
        text.split(|character: char| character.is_whitespace() || matches!(character, '"' | '\\'))
            .filter(|word| !word.is_empty())
    }

    pub(super) fn validate(&self, query: &str, limit: u16) -> Result<(), String> {
        if !(1..=ARXIV_MAX_LIMIT).contains(&limit) {
            return Err(format!("--limit must be between 1 and {ARXIV_MAX_LIMIT}"));
        }
        if let Some(category) = self
            .categories
            .iter()
            .find(|category| !is_arxiv_category(category))
        {
            return Err(format!(
                "invalid arXiv category `{category}`; pass a code such as cs.AI or q-fin.PM"
            ));
        }
        for (flag, value) in [("--author", &self.author), ("--title", &self.title)] {
            if value
                .as_deref()
                .is_some_and(|value| Self::words(value).next().is_none())
            {
                return Err(format!("{flag} must contain at least one word"));
            }
        }
        if let (Some(from), Some(to)) = (self.submitted_from, self.submitted_to)
            && from > to
        {
            return Err("--submitted-from must not be later than --submitted-to".into());
        }
        let has_criterion = Self::words(query).next().is_some()
            || !self.categories.is_empty()
            || self.author.is_some()
            || self.title.is_some();
        if !has_criterion {
            return Err(
                "arxiv search needs a query or at least one of --category, --author, --title"
                    .into(),
            );
        }
        Ok(())
    }
}

fn is_arxiv_category(value: &str) -> bool {
    let (archive, subject) = value
        .split_once('.')
        .map_or((value, None), |(archive, subject)| (archive, Some(subject)));
    is_hyphenated_lowercase(archive)
        && subject.is_none_or(|subject| {
            subject
                .split('-')
                .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_alphabetic()))
        })
}

#[derive(Clone, Debug, Serialize)]
/// arXiv metadata of a paper version.
pub struct ArxivItemData {
    #[serde(rename = "abstract")]
    pub abstract_text: String,
    pub updated: Option<String>,
    pub primary_category: Option<String>,
    pub categories: Vec<String>,
    pub doi: Option<String>,
    pub journal_ref: Option<String>,
    pub comment: Option<String>,
    pub pdf_url: Option<String>,
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::{ArxivRef, ArxivSearchOptions, ArxivSort};
    use crate::types::{Platform, PlatformRef, PlatformSearchOptions, PlatformSearchRequest};

    fn arxiv(input: &str) -> Option<String> {
        PlatformRef::parse(Platform::Arxiv, input)
            .ok()
            .map(|reference| reference.to_string())
    }

    #[test]
    fn arxiv_ref_strings_parse_new_and_old_ids_with_and_without_versions() {
        let parsed = [
            "arxiv:2401.01234v2",
            "arxiv:2401.01234",
            "arXiv:0704.0001",
            "arxiv:hep-th/9901001v1",
            "arxiv:math.GT/0309136",
            "arxiv:solv-int/9901001",
        ]
        .map(arxiv);

        assert_eq!(
            parsed,
            [
                Some("arxiv:2401.01234v2".into()),
                Some("arxiv:2401.01234".into()),
                Some("arxiv:0704.0001".into()),
                Some("arxiv:hep-th/9901001v1".into()),
                Some("arxiv:math/0309136".into()),
                Some("arxiv:solv-int/9901001".into()),
            ]
        );
    }

    #[test]
    fn arxiv_abstract_page_urls_parse_to_refs() {
        let parsed = [
            "https://arxiv.org/abs/2401.01234v2",
            "http://arxiv.org/abs/hep-th/9901001v1",
            "https://export.arxiv.org/abs/2401.01234?context=cs#top",
            "https://www.arxiv.org/abs/2401.01234/",
        ]
        .map(arxiv);

        assert_eq!(
            parsed,
            [
                Some("arxiv:2401.01234v2".into()),
                Some("arxiv:hep-th/9901001v1".into()),
                Some("arxiv:2401.01234".into()),
                Some("arxiv:2401.01234".into()),
            ]
        );
    }

    #[test]
    fn arxiv_pdf_html_and_mirror_urls_parse_to_refs() {
        let parsed = [
            "https://arxiv.org/pdf/2401.01234v2",
            "https://arxiv.org/pdf/2401.01234v2.pdf",
            "https://arxiv.org/pdf/hep-th/9901001.pdf",
            "https://arxiv.org/html/2401.01234v2",
            "https://arxiv.org/html/2401.01234v2/",
            "https://arxiv.org/html/2401.01234v2#S3",
            "https://export.arxiv.org/pdf/2401.01234",
            "https://export.arxiv.org/abs/math.GT/0309136v1",
        ]
        .map(arxiv);

        assert_eq!(
            parsed,
            [
                Some("arxiv:2401.01234v2".into()),
                Some("arxiv:2401.01234v2".into()),
                Some("arxiv:hep-th/9901001".into()),
                Some("arxiv:2401.01234v2".into()),
                Some("arxiv:2401.01234v2".into()),
                Some("arxiv:2401.01234v2".into()),
                Some("arxiv:2401.01234".into()),
                Some("arxiv:math/0309136v1".into()),
            ]
        );
    }

    #[test]
    fn arxiv_old_style_ids_drop_the_subject_class() {
        let parsed = [
            "arxiv:math.DG/0211159v1",
            "https://arxiv.org/abs/math.DG/0211159",
        ]
        .map(arxiv);

        assert_eq!(
            parsed,
            [
                Some("arxiv:math/0211159v1".into()),
                Some("arxiv:math/0211159".into())
            ]
        );
    }

    #[test]
    fn arxiv_rejects_unrecognizable_inputs() {
        let parsed = [
            "2401.01234",
            "arxiv:2413.01234",
            "arxiv:2401.123",
            "arxiv:2401.01234v0",
            "arxiv:2401.01234v",
            "arxiv:HEP-TH/9901001",
            "arxiv:hep-th/990100",
            "https://example.org/abs/2401.01234",
            "https://arxiv.org/list/cs.AI/recent",
            "https://arxiv.org/abs/",
            "https://arxiv.org/pdf/2401.01234v2.pdf.pdf",
            "https://arxiv.org/html/2401.01234v2/x1.png",
            "https://arxiv.org/src/2401.01234v2",
            "https://arxiv.org/abs/2401.01234.pdf",
            "https://arxiv.org/html/hep-th/9901001/extra",
        ]
        .map(arxiv);

        assert_eq!(parsed, [const { None }; 15]);
    }

    #[test]
    fn arxiv_canonical_urls_round_trip_with_and_without_versions() {
        for input in [
            "arxiv:2401.01234v2",
            "arxiv:2401.01234",
            "arxiv:hep-th/9901001v3",
            "arxiv:math.GT/0309136",
        ] {
            let reference = PlatformRef::parse(Platform::Arxiv, input).expect("valid ref");

            let round_trip = PlatformRef::parse(Platform::Arxiv, &reference.canonical_url());

            assert_eq!(round_trip.ok(), Some(reference), "input={input}");
        }
    }

    #[test]
    fn arxiv_canonical_url_carries_the_version_only_when_the_ref_does() {
        let urls = ["arxiv:2401.01234v2", "arxiv:2401.01234"]
            .map(|input| ArxivRef::parse(input).expect("valid ref").canonical_url());

        assert_eq!(
            urls,
            [
                "https://arxiv.org/abs/2401.01234v2",
                "https://arxiv.org/abs/2401.01234"
            ]
        );
    }

    #[test]
    fn arxiv_content_urls_use_the_ref_version() {
        let reference = ArxivRef::parse("arxiv:hep-th/9901001v3").expect("valid ref");

        assert_eq!(
            [reference.html_url(), reference.pdf_url()],
            [
                "https://arxiv.org/html/hep-th/9901001v3",
                "https://arxiv.org/pdf/hep-th/9901001v3"
            ]
        );
    }

    #[test]
    fn unrecognized_refs_explain_the_accepted_forms() {
        let error = PlatformRef::parse(Platform::Arxiv, "https://bit.ly/abc").unwrap_err();

        assert_eq!(
            error.to_string(),
            "unrecognized arxiv reference `https://bit.ly/abc`; pass an `arxiv:<id>[v<n>]` ref or an original arxiv.org URL"
        );
    }

    fn request(query: &str, options: ArxivSearchOptions) -> PlatformSearchRequest {
        PlatformSearchRequest {
            query: query.into(),
            limit: 10,
            options: PlatformSearchOptions::Arxiv(options),
            page: None,
        }
    }

    fn date(value: &str) -> NaiveDate {
        value.parse().expect("valid date")
    }

    #[test]
    fn arxiv_request_needs_a_query_word_or_a_filter() {
        let results = [
            request("", ArxivSearchOptions::default()),
            request(" \" ", ArxivSearchOptions::default()),
            request(
                "",
                ArxivSearchOptions {
                    submitted_from: Some(date("2024-01-01")),
                    ..ArxivSearchOptions::default()
                },
            ),
            request("electron", ArxivSearchOptions::default()),
            request(
                "",
                ArxivSearchOptions {
                    categories: vec!["q-fin.PM".into()],
                    ..ArxivSearchOptions::default()
                },
            ),
        ]
        .map(|request| request.validate().is_ok());

        assert_eq!(results, [false, false, false, true, true]);
    }

    #[test]
    fn arxiv_request_rejects_a_reversed_date_range() {
        let error = request(
            "electron",
            ArxivSearchOptions {
                submitted_from: Some(date("2024-02-01")),
                submitted_to: Some(date("2024-01-31")),
                ..ArxivSearchOptions::default()
            },
        )
        .validate()
        .unwrap_err();

        assert_eq!(
            error,
            "--submitted-from must not be later than --submitted-to"
        );
    }

    #[test]
    fn arxiv_request_accepts_a_single_day_range() {
        let result = request(
            "electron",
            ArxivSearchOptions {
                submitted_from: Some(date("2024-02-01")),
                submitted_to: Some(date("2024-02-01")),
                ..ArxivSearchOptions::default()
            },
        )
        .validate();

        assert_eq!(result, Ok(()));
    }

    #[test]
    fn arxiv_request_rejects_malformed_categories_and_empty_phrases() {
        let results = [
            ArxivSearchOptions {
                categories: vec!["cond-mat.str-el".into(), "hep-th".into(), "cs.AI".into()],
                ..ArxivSearchOptions::default()
            },
            ArxivSearchOptions {
                categories: vec!["cs.AI OR all:x".into()],
                ..ArxivSearchOptions::default()
            },
            ArxivSearchOptions {
                categories: vec!["CS.AI".into()],
                ..ArxivSearchOptions::default()
            },
            ArxivSearchOptions {
                author: Some("  ".into()),
                ..ArxivSearchOptions::default()
            },
            ArxivSearchOptions {
                title: Some("\"\"".into()),
                ..ArxivSearchOptions::default()
            },
        ]
        .map(|options| request("electron", options).validate().is_ok());

        assert_eq!(results, [true, false, false, false, false]);
    }

    #[test]
    fn arxiv_request_limits_page_size_to_one_through_one_hundred() {
        let results = [0, 1, 100, 101].map(|limit| {
            PlatformSearchRequest {
                limit,
                ..request("electron", ArxivSearchOptions::default())
            }
            .validate()
            .is_ok()
        });

        assert_eq!(results, [false, true, true, false]);
    }

    #[test]
    fn search_words_treat_double_quotes_as_separators() {
        let words = ArxivSearchOptions::words(" \"dark  matter\" AND (halo) ").collect::<Vec<_>>();

        assert_eq!(words, ["dark", "matter", "AND", "(halo)"]);
    }

    #[test]
    fn search_words_treat_backslashes_as_separators() {
        let words = ArxivSearchOptions::words(r"C:\ a\b").collect::<Vec<_>>();

        assert_eq!(words, ["C:", "a", "b"]);
    }

    #[test]
    fn search_requests_round_trip_through_json() {
        let original = PlatformSearchRequest {
            page: Some("20".into()),
            ..request(
                "electron",
                ArxivSearchOptions {
                    categories: vec!["hep-th".into()],
                    author: Some("Witten".into()),
                    title: None,
                    submitted_from: Some(date("2024-01-01")),
                    submitted_to: None,
                    sort: ArxivSort::Updated,
                },
            )
        };

        let decoded: PlatformSearchRequest =
            serde_json::from_str(&serde_json::to_string(&original).expect("encode request"))
                .expect("decode request");

        assert_eq!(decoded, original);
    }
}
