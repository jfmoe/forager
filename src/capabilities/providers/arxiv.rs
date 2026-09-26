//! The `arxiv_api` route: arXiv Query API requests and Atom feed decoding.

use std::time::Duration;

use reqwest::Client;
use serde::Deserialize;

use crate::catalog::{PlatformOperation, ProviderId};
use crate::config::ArxivApiRuntimeConfig;
use crate::credentials::CredentialPool;
use crate::net::{AttemptFailure, RetryPolicy, read_complete_protocol, send_provider_request};
use crate::providers::execution::{ExecutionSettings, execute_anonymous};
use crate::providers::shared::redacted_urls_message;
use crate::rate_limit::RateLimiter;
use crate::types::{
    ArxivItemData, ArxivRef, ArxivSearchOptions, ArxivSort, AttemptErrorKind, AttemptTarget,
    ContentDepth, Deadline, Platform, PlatformItem, PlatformItemData, PlatformRef,
    PlatformSearchOptions, PlatformSearchOutcome, PlatformSearchRequest, ProviderError,
};

const ROUTE: ProviderId = ProviderId::ArxivApi;
const ERROR_ENTRY_PATH: &str = "arxiv.org/api/errors";
// arXiv submission dates start in 1991; the upper bound only closes an open range.
const EARLIEST_SUBMISSION: &str = "199101010000";
const LATEST_SUBMISSION: &str = "999912312359";

/// Returns whether the route can run the request with every explicit option; it never sends
/// a request. The route applies every arXiv option; only a page position it did not issue fails.
pub(crate) fn search_support(request: &PlatformSearchRequest) -> Result<(), String> {
    match request.options {
        PlatformSearchOptions::Arxiv(_) => page_start(request.page.as_deref()).map(|_| ()),
    }
}

pub(crate) struct ArxivApi {
    config: ArxivApiRuntimeConfig,
    client: Client,
    // The route needs no credentials; the empty pool only drives shared redaction.
    credentials: CredentialPool,
    limiter: RateLimiter,
    retry_policy: RetryPolicy,
    deadline: Deadline,
}

struct FeedPage {
    items: Vec<PlatformItem>,
    total_results: u64,
}

impl ArxivApi {
    pub(crate) fn new(
        config: ArxivApiRuntimeConfig,
        client: Client,
        limiter: RateLimiter,
        retry_policy: RetryPolicy,
        deadline: Deadline,
    ) -> Self {
        Self {
            config,
            client,
            credentials: CredentialPool::new(ROUTE.name(), Vec::new()),
            limiter,
            retry_policy,
            deadline,
        }
    }

    pub(crate) async fn search(
        &self,
        request: &PlatformSearchRequest,
    ) -> Result<PlatformSearchOutcome, ProviderError> {
        let PlatformSearchOptions::Arxiv(options) = &request.options;
        let start = page_start(request.page.as_deref()).map_err(|message| ProviderError {
            kind: AttemptErrorKind::Parameter,
            message,
            attempts: Vec::new(),
            verbose: false,
            diagnostic: None,
            redirected_library_id: None,
        })?;
        let query = [
            ("search_query", search_query(&request.query, options)),
            ("start", start.to_string()),
            ("max_results", request.limit.to_string()),
            ("sortBy", sort_by(options.sort).to_owned()),
            ("sortOrder", "descending".to_owned()),
        ];
        let query = &query;
        let execution = execute_anonymous(
            ExecutionSettings {
                provider: ROUTE.name(),
                target: AttemptTarget::platform(
                    Platform::Arxiv.as_str(),
                    PlatformOperation::Search.as_str(),
                ),
                retry_policy: self.retry_policy,
                deadline: self.deadline,
                attempt_timeout: Duration::from_secs(self.config.timeout_seconds),
                verbose: false,
                timeout_message: "arXiv API request timed out",
                model: None,
                transport: Some("http"),
                endpoint_host: None,
                breaker_event: None,
            },
            move |deadline| async move { self.send_once(query, deadline).await },
        )
        .await?;
        let FeedPage {
            items,
            total_results,
        } = execution.value;
        let next_start = start.saturating_add(items.len() as u64);
        let has_next_page = items.len() == usize::from(request.limit) && next_start < total_results;
        Ok(PlatformSearchOutcome {
            items,
            next_page: has_next_page.then(|| next_start.to_string()),
            attempts: execution.attempts,
            diagnostic: execution.diagnostic,
        })
    }

    async fn send_once(
        &self,
        query: &[(&str, String)],
        deadline: Deadline,
    ) -> Result<(Option<u16>, FeedPage), AttemptFailure> {
        let _permit = self
            .limiter
            .acquire(deadline)
            .await
            .map_err(|error| AttemptFailure {
                kind: error.kind(),
                status: None,
                message: error.to_string(),
            })?;
        let request = self.client.get(&self.config.url).query(query);
        let response = send_provider_request(request, &self.credentials).await?;
        let body = read_complete_protocol(response, &self.credentials, failure_message).await?;
        let status = body.status;
        let failure = |kind, message: String| AttemptFailure {
            kind,
            status: Some(status),
            message: redacted_urls_message(&message, &self.credentials),
        };
        let feed =
            decode_feed(&body.text).map_err(|error| failure(AttemptErrorKind::Runtime, error))?;
        if let Some(message) = feed.error_message() {
            return Err(failure(AttemptErrorKind::Parameter, message));
        }
        let total_results = feed.total_results;
        let items = feed
            .entries
            .into_iter()
            .map(Entry::into_item)
            .collect::<Result<_, _>>()
            .map_err(|error| failure(AttemptErrorKind::Runtime, error))?;
        Ok((
            Some(status),
            FeedPage {
                items,
                total_results,
            },
        ))
    }
}

fn page_start(page: Option<&str>) -> Result<u64, String> {
    page.map_or(Ok(0), |page| {
        page.parse()
            .map_err(|_| format!("invalid arXiv page position `{page}`"))
    })
}

/// Builds the `search_query` value: every word, category group, phrase, and date range must
/// match. Each word is quoted, so arXiv query syntax in it stays literal.
fn search_query(query: &str, options: &ArxivSearchOptions) -> String {
    let mut clauses = ArxivSearchOptions::words(query)
        .map(|word| format!("all:\"{word}\""))
        .collect::<Vec<_>>();
    match options.categories.as_slice() {
        [] => {}
        [category] => clauses.push(format!("cat:{category}")),
        categories => clauses.push(format!(
            "({})",
            categories
                .iter()
                .map(|category| format!("cat:{category}"))
                .collect::<Vec<_>>()
                .join(" OR ")
        )),
    }
    for (field, phrase) in [("au", &options.author), ("ti", &options.title)] {
        if let Some(phrase) = phrase {
            let words = ArxivSearchOptions::words(phrase).collect::<Vec<_>>();
            clauses.push(format!("{field}:\"{}\"", words.join(" ")));
        }
    }
    if options.submitted_from.is_some() || options.submitted_to.is_some() {
        let from = options.submitted_from.map_or_else(
            || EARLIEST_SUBMISSION.to_owned(),
            |date| format!("{}0000", date.format("%Y%m%d")),
        );
        let to = options.submitted_to.map_or_else(
            || LATEST_SUBMISSION.to_owned(),
            |date| format!("{}2359", date.format("%Y%m%d")),
        );
        clauses.push(format!("submittedDate:[{from} TO {to}]"));
    }
    clauses.join(" AND ")
}

fn sort_by(sort: ArxivSort) -> &'static str {
    match sort {
        ArxivSort::Relevance => "relevance",
        ArxivSort::Submitted => "submittedDate",
        ArxivSort::Updated => "lastUpdatedDate",
    }
}

fn failure_message(body: &str, status: u16) -> String {
    decode_feed(body)
        .ok()
        .and_then(|feed| feed.error_message())
        .unwrap_or_else(|| format!("arXiv returned HTTP {status}"))
}

fn decode_feed(body: &str) -> Result<Feed, String> {
    quick_xml::de::from_str(body).map_err(|error| format!("invalid arXiv Atom feed: {error}"))
}

// quick-xml matches element local names, so the `opensearch:` and `arxiv:` extension elements
// are named without their prefixes. Every arXiv feed carries `totalResults`, so requiring it
// rejects a successful response that is not an arXiv feed.
#[derive(Deserialize)]
struct Feed {
    #[serde(rename = "totalResults")]
    total_results: u64,
    #[serde(rename = "entry", default)]
    entries: Vec<Entry>,
}

impl Feed {
    fn error_message(&self) -> Option<String> {
        self.entries
            .iter()
            .find(|entry| entry.id.contains(ERROR_ENTRY_PATH))
            .map(|entry| {
                let message = entry.summary.as_deref().map(fold_whitespace);
                format!(
                    "arXiv rejected the request: {}",
                    message.as_deref().unwrap_or("unknown error")
                )
            })
    }
}

#[derive(Deserialize)]
struct Entry {
    id: String,
    title: Option<String>,
    summary: Option<String>,
    published: Option<String>,
    updated: Option<String>,
    #[serde(rename = "author", default)]
    authors: Vec<Author>,
    #[serde(rename = "link", default)]
    links: Vec<Link>,
    #[serde(rename = "category", default)]
    categories: Vec<Category>,
    primary_category: Option<Category>,
    doi: Option<String>,
    journal_ref: Option<String>,
    comment: Option<String>,
}

#[derive(Deserialize)]
struct Author {
    name: String,
}

#[derive(Deserialize)]
struct Link {
    #[serde(rename = "@href")]
    href: String,
    #[serde(rename = "@title")]
    title: Option<String>,
}

#[derive(Deserialize)]
struct Category {
    #[serde(rename = "@term")]
    term: String,
}

impl Entry {
    fn into_item(self) -> Result<PlatformItem, String> {
        let reference = ArxivRef::parse(&self.id)
            .map(PlatformRef::Arxiv)
            .map_err(|_| format!("arXiv entry has an unrecognized id `{}`", self.id))?;
        let pdf_url = self
            .links
            .into_iter()
            .find(|link| link.title.as_deref() == Some("pdf"))
            .map(|link| link.href);
        Ok(PlatformItem {
            url: reference.canonical_url(),
            reference,
            depth: ContentDepth::Abstract,
            title: self
                .title
                .as_deref()
                .map(fold_whitespace)
                .unwrap_or_default(),
            authors: self
                .authors
                .iter()
                .map(|author| fold_whitespace(&author.name))
                .filter(|name| !name.is_empty())
                .collect(),
            published: non_empty(self.published),
            data: PlatformItemData::Arxiv(ArxivItemData {
                abstract_text: self
                    .summary
                    .as_deref()
                    .map(str::trim)
                    .unwrap_or_default()
                    .to_owned(),
                updated: non_empty(self.updated),
                primary_category: self.primary_category.map(|category| category.term),
                categories: self
                    .categories
                    .into_iter()
                    .map(|category| category.term)
                    .collect(),
                doi: non_empty(self.doi),
                journal_ref: self.journal_ref.as_deref().map(fold_whitespace),
                comment: self.comment.as_deref().map(fold_whitespace),
                pdf_url,
            }),
        })
    }
}

fn non_empty(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn fold_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{decode_feed, failure_message};

    const FULL_ENTRY: &str = r#"<?xml version='1.0' encoding='UTF-8'?>
<feed xmlns:opensearch="http://a9.com/-/spec/opensearch/1.1/" xmlns:arxiv="http://arxiv.org/schemas/atom" xmlns="http://www.w3.org/2005/Atom">
  <id>https://arxiv.org/api/feed</id>
  <title>arXiv Query</title>
  <opensearch:itemsPerPage>1</opensearch:itemsPerPage>
  <opensearch:totalResults>42</opensearch:totalResults>
  <opensearch:startIndex>0</opensearch:startIndex>
  <entry>
    <id>http://arxiv.org/abs/2401.01234v2</id>
    <title>Dark   Matter
      Halos &amp; Galaxies</title>
    <updated>2024-02-03T04:05:06Z</updated>
    <link href="https://arxiv.org/abs/2401.01234v2" rel="alternate" type="text/html"/>
    <link href="https://arxiv.org/pdf/2401.01234v2" rel="related" type="application/pdf" title="pdf"/>
    <summary>  First line
  second line.
</summary>
    <category term="astro-ph.CO" scheme="http://arxiv.org/schemas/atom"/>
    <category term="hep-ph" scheme="http://arxiv.org/schemas/atom"/>
    <published>2024-01-02T18:59:59Z</published>
    <arxiv:comment>12 pages,
      3 figures</arxiv:comment>
    <arxiv:journal_ref>Phys. Rev. D 1 (2024)</arxiv:journal_ref>
    <arxiv:primary_category term="astro-ph.CO"/>
    <author>
      <name>Ada  Lovelace</name>
    </author>
    <author>
      <name>Alan Turing</name>
    </author>
    <arxiv:doi>10.1000/example</arxiv:doi>
    <link rel="related" href="https://doi.org/10.1000/example" title="doi"/>
  </entry>
</feed>"#;

    fn items(body: &str) -> serde_json::Value {
        let feed = decode_feed(body).expect("decode feed");
        let items = feed
            .entries
            .into_iter()
            .map(super::Entry::into_item)
            .collect::<Result<Vec<_>, _>>()
            .expect("map entries");
        serde_json::to_value(items).expect("serialize items")
    }

    #[test]
    fn a_full_entry_maps_every_metadata_field() {
        assert_eq!(
            items(FULL_ENTRY),
            json!([{
                "ref": "arxiv:2401.01234v2",
                "url": "https://arxiv.org/abs/2401.01234v2",
                "depth": "abstract",
                "title": "Dark Matter Halos & Galaxies",
                "authors": ["Ada Lovelace", "Alan Turing"],
                "published": "2024-01-02T18:59:59Z",
                "abstract": "First line\n  second line.",
                "updated": "2024-02-03T04:05:06Z",
                "primary_category": "astro-ph.CO",
                "categories": ["astro-ph.CO", "hep-ph"],
                "doi": "10.1000/example",
                "journal_ref": "Phys. Rev. D 1 (2024)",
                "comment": "12 pages, 3 figures",
                "pdf_url": "https://arxiv.org/pdf/2401.01234v2"
            }])
        );
    }

    #[test]
    fn an_entry_without_optional_metadata_maps_absent_fields_to_null() {
        let body = r#"<feed xmlns="http://www.w3.org/2005/Atom" xmlns:opensearch="http://a9.com/-/spec/opensearch/1.1/">
  <opensearch:totalResults>1</opensearch:totalResults>
  <entry>
    <id>http://arxiv.org/abs/hep-th/9901001v1</id>
    <title>Old paper</title>
    <summary>Abstract.</summary>
    <author><name>A. Author</name></author>
  </entry>
</feed>"#;

        assert_eq!(
            items(body),
            json!([{
                "ref": "arxiv:hep-th/9901001v1",
                "url": "https://arxiv.org/abs/hep-th/9901001v1",
                "depth": "abstract",
                "title": "Old paper",
                "authors": ["A. Author"],
                "published": null,
                "abstract": "Abstract.",
                "updated": null,
                "primary_category": null,
                "categories": [],
                "doi": null,
                "journal_ref": null,
                "comment": null,
                "pdf_url": null
            }])
        );
    }

    #[test]
    fn total_results_are_read_from_the_opensearch_extension() {
        let feed = decode_feed(FULL_ENTRY).expect("decode feed");

        assert_eq!(feed.total_results, 42);
    }

    #[test]
    fn a_successful_body_that_is_not_an_arxiv_feed_fails_to_decode() {
        let result = decode_feed("<html><body>Maintenance</body></html>");

        assert!(result.is_err());
    }

    #[test]
    fn an_error_entry_yields_the_arxiv_message() {
        let body = r#"<feed xmlns="http://www.w3.org/2005/Atom" xmlns:opensearch="http://a9.com/-/spec/opensearch/1.1/">
  <opensearch:totalResults>1</opensearch:totalResults>
  <entry>
    <id>https://arxiv.org/api/errors</id>
    <title>Error</title>
    <summary>Invalid query string: '('</summary>
  </entry>
</feed>"#;

        assert_eq!(
            decode_feed(body).expect("decode feed").error_message(),
            Some("arXiv rejected the request: Invalid query string: '('".into())
        );
    }

    #[test]
    fn a_failed_status_reads_the_error_entry_or_names_the_status() {
        let error_feed = r#"<feed xmlns="http://www.w3.org/2005/Atom"><totalResults>1</totalResults><entry><id>http://arxiv.org/api/errors#bad</id><summary>bad id</summary></entry></feed>"#;

        assert_eq!(
            [
                failure_message(error_feed, 400),
                failure_message("<html>busy</html>", 503)
            ],
            [
                "arXiv rejected the request: bad id".to_owned(),
                "arXiv returned HTTP 503".to_owned()
            ]
        );
    }

    #[test]
    fn an_entry_with_an_unrecognized_id_fails_to_map() {
        let feed = decode_feed(
            r#"<feed xmlns="http://www.w3.org/2005/Atom"><totalResults>1</totalResults><entry><id>http://example.org/x</id></entry></feed>"#,
        )
        .expect("decode feed");

        let result = feed
            .entries
            .into_iter()
            .map(super::Entry::into_item)
            .collect::<Result<Vec<_>, _>>();

        assert!(result.is_err());
    }
}
