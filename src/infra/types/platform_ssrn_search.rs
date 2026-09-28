//! Typed SSRN search criteria and validation.

use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};

/// The fields searched for the positional query.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SsrnSearchScope {
    /// The route's default fields.
    #[default]
    All,
    /// Title fields; this does not require an exact phrase.
    Title,
    /// Crossref bibliographic fields.
    Bibliographic,
}

/// Complete SSRN search criteria, also stored in page cursors.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct SsrnSearchOptions {
    pub scope: SsrnSearchScope,
    pub author: Option<String>,
    pub affiliation: Option<String>,
    pub published: SsrnDateRange,
    pub created: SsrnDateRange,
    pub updated: SsrnDateRange,
    pub has_abstract: bool,
    pub work_type: Option<SsrnWorkType>,
    /// A bare ORCID, including its check digit.
    pub orcid: Option<String>,
    /// An Open Funder Registry DOI, in `10.13039/<digits>` form.
    pub funder: Option<String>,
    pub sort: SsrnSort,
    pub order: SsrnSortOrder,
}

impl SsrnSearchOptions {
    pub(super) fn validate(&self, query: &str, limit: u16) -> Result<(), String> {
        if !(1..=100).contains(&limit) {
            return Err("--limit must be between 1 and 100".into());
        }
        if query.trim().is_empty() {
            return Err("ssrn search needs a query".into());
        }
        for (name, value) in [("author", &self.author), ("affiliation", &self.affiliation)] {
            if value
                .as_deref()
                .is_some_and(|value| value.trim().is_empty())
            {
                return Err(format!("--{name} must not be empty"));
            }
        }
        for (name, range) in [
            ("published", &self.published),
            ("created", &self.created),
            ("updated", &self.updated),
        ] {
            range.validate(name)?;
        }
        if self
            .orcid
            .as_deref()
            .is_some_and(|value| !valid_orcid(value))
        {
            return Err("--orcid needs a valid bare ORCID, such as 0000-0002-1825-0097".into());
        }
        if self.funder.as_deref().is_some_and(|value| {
            !value
                .strip_prefix("10.13039/")
                .is_some_and(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()))
        }) {
            return Err(
                "--funder needs an Open Funder Registry DOI in 10.13039/<digits> form".into(),
            );
        }
        Ok(())
    }
}

/// An inclusive calendar-day range; either bound may be absent.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct SsrnDateRange {
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
}

impl SsrnDateRange {
    fn validate(&self, name: &str) -> Result<(), String> {
        if self
            .from
            .into_iter()
            .chain(self.to)
            .any(|date| !(1..=9999).contains(&date.year()))
        {
            return Err(format!(
                "--{name}-from and --{name}-to need years between 0001 and 9999"
            ));
        }
        if matches!((self.from, self.to), (Some(from), Some(to)) if from > to) {
            return Err(format!("--{name}-from must not be after --{name}-to"));
        }
        Ok(())
    }
}

/// The metric used to rank SSRN results.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SsrnSort {
    #[default]
    Relevance,
    Published,
    Created,
    Updated,
    Citations,
}

/// The direction of the selected ranking.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SsrnSortOrder {
    Asc,
    #[default]
    Desc,
}

fn valid_orcid(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 19
        || !bytes.iter().enumerate().all(|(i, b)| match i {
            4 | 9 | 14 => *b == b'-',
            18 => b.is_ascii_digit() || *b == b'X',
            _ => b.is_ascii_digit(),
        })
    {
        return false;
    }
    let total = bytes[..18]
        .iter()
        .filter(|b| **b != b'-')
        .fold(0_u32, |total, b| (total + u32::from(*b - b'0')) * 2);
    let check = (12 - total % 11) % 11;
    let expected = if check == 10 {
        b'X'
    } else {
        b'0' + u8::try_from(check).expect("single check digit")
    };
    bytes[18] == expected
}

macro_rules! work_types {
    ($($variant:ident => $name:literal),+ $(,)?) => {
        /// A registered Crossref work type.
        #[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
        pub enum SsrnWorkType {
            $(#[serde(rename = $name)] $variant),+
        }

        impl SsrnWorkType {
            /// Accepted work type names.
            pub const NAMES: &[&str] = &[$($name),+];

            /// Returns the Crossref work type name.
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $name),+ }
            }
        }

        impl std::str::FromStr for SsrnWorkType {
            type Err = String;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                match value {
                    $($name => Ok(Self::$variant),)+
                    _ => Err(format!("unknown Crossref work type `{value}`")),
                }
            }
        }
    };
}

work_types! {
    Book => "book",
    BookChapter => "book-chapter",
    BookPart => "book-part",
    BookSection => "book-section",
    BookSeries => "book-series",
    BookSet => "book-set",
    BookTrack => "book-track",
    Component => "component",
    Database => "database",
    Dataset => "dataset",
    Dissertation => "dissertation",
    EditedBook => "edited-book",
    Grant => "grant",
    Journal => "journal",
    JournalArticle => "journal-article",
    JournalIssue => "journal-issue",
    JournalVolume => "journal-volume",
    Monograph => "monograph",
    Other => "other",
    PeerReview => "peer-review",
    PostedContent => "posted-content",
    Proceedings => "proceedings",
    ProceedingsArticle => "proceedings-article",
    ProceedingsSeries => "proceedings-series",
    ReferenceBook => "reference-book",
    ReferenceEntry => "reference-entry",
    Report => "report",
    ReportComponent => "report-component",
    ReportSeries => "report-series",
    Standard => "standard",
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn criteria_reject_blank_names_inverted_ranges_and_invalid_identifiers() {
        for value in [
            json!({"author":" "}),
            json!({"affiliation":"\t"}),
            json!({"published":{"from":"2024-01-02","to":"2024-01-01"}}),
            json!({"created":{"from":"2024-01-02","to":"2024-01-01"}}),
            json!({"updated":{"from":"2024-01-02","to":"2024-01-01"}}),
            json!({"orcid":"0000-0002-1825-0098"}),
            json!({"orcid":"0000-0002-1825-0097,funder:123"}),
            json!({"funder":"10.13039/"}),
            json!({"funder":"10.13039/123,type:other"}),
        ] {
            let options: SsrnSearchOptions =
                serde_json::from_value(value.clone()).expect("criteria");
            assert!(options.validate("momentum", 10).is_err(), "{value}");
        }
    }

    #[test]
    fn criteria_accept_open_ranges_and_valid_identifier_check_digits() {
        for value in [
            json!({"published":{"from":"2024-02-29"}}),
            json!({"created":{"to":"2024-01-01"}}),
            json!({"updated":{"from":"2024-01-01","to":"2024-01-01"}}),
            json!({"orcid":"0000-0002-1825-0097"}),
            json!({"orcid":"0000-0002-1694-233X"}),
            json!({"funder":"10.13039/100000001"}),
        ] {
            let options: SsrnSearchOptions =
                serde_json::from_value(value.clone()).expect("criteria");
            assert_eq!(options.validate("momentum", 10), Ok(()), "{value}");
        }
    }

    #[test]
    fn missing_cursor_fields_keep_default_search_semantics() {
        assert_eq!(
            serde_json::from_str::<SsrnSearchOptions>("{}").expect("legacy options"),
            SsrnSearchOptions::default()
        );
    }
}
