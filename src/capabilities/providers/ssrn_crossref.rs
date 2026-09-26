//! The `ssrn_crossref` route: SSRN paper metadata from the anonymous Crossref REST API, limited
//! to the SSRN DOI prefix.

use std::fmt::Write as _;
use std::time::Duration;

use reqwest::Client;
use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::catalog::{PlatformOperation, ProviderId};
use crate::config::HttpRouteRuntimeConfig;
use crate::credentials::CredentialPool;
use crate::net::{
    AttemptFailure, RetryPolicy, combine_diagnostics, read_complete_protocol, send_provider_request,
};
use crate::providers::execution::{ExecutionSettings, execute_anonymous};
use crate::providers::shared::{
    acquire_window, other_platform_message, parameter_error, redacted_urls_message,
};
use crate::rate_limit::RateLimiter;
use crate::types::{
    AttemptErrorKind, AttemptTarget, ContentDepth, Deadline, Platform, PlatformFetchOutcome,
    PlatformFetchRequest, PlatformItem, PlatformItemData, PlatformRef, PlatformSearchOptions,
    PlatformSearchOutcome, PlatformSearchRequest, ProviderError, SsrnItemData, SsrnRef,
};

const ROUTE: ProviderId = ProviderId::SsrnCrossref;
const SSRN_DOI_PREFIX: &str = "10.2139";
const SELECT_FIELDS: &str = "DOI,title,author,abstract,published,type,created,resource";
// Crossref rejects a page whose offset plus rows exceeds 10000 with HTTP 400 (checked
// 2026-09-26); deeper pages need its cursor, which this route does not use.
const MAX_PAGE_END: u64 = 10_000;

/// Returns whether the route can run the request; it never sends a request. Only a page
/// position the route did not issue, or one past the Crossref offset limit, fails.
pub(crate) fn search_support(request: &PlatformSearchRequest) -> Result<(), String> {
    match request.options {
        PlatformSearchOptions::Ssrn(_) => page_offset(request).map(|_| ()),
        PlatformSearchOptions::Arxiv(_) => {
            Err(other_platform_message(ROUTE, request.options.platform()))
        }
    }
}

/// Returns whether the route can fetch at the requested depth; it never sends a request.
pub(crate) fn fetch_support(request: &PlatformFetchRequest) -> Result<(), String> {
    match request.depth {
        ContentDepth::Metadata | ContentDepth::Abstract => Ok(()),
        depth => Err(format!(
            "{} cannot fetch at depth `{}`",
            ROUTE.name(),
            depth.as_str()
        )),
    }
}

pub(crate) struct SsrnCrossref {
    config: HttpRouteRuntimeConfig,
    client: Client,
    // The route needs no credentials; the empty pool only drives shared redaction.
    credentials: CredentialPool,
    limiter: RateLimiter,
    retry_policy: RetryPolicy,
    deadline: Deadline,
}

impl SsrnCrossref {
    pub(crate) fn new(
        config: HttpRouteRuntimeConfig,
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

    /// Searches the SSRN DOI prefix by relevance. The page position is an absolute offset, and
    /// the last page is judged by the record count before records without an SSRN DOI are
    /// dropped.
    pub(crate) async fn search(
        &self,
        request: &PlatformSearchRequest,
    ) -> Result<PlatformSearchOutcome, ProviderError> {
        if !matches!(request.options, PlatformSearchOptions::Ssrn(_)) {
            return Err(parameter_error(other_platform_message(
                ROUTE,
                request.options.platform(),
            )));
        }
        let offset = page_offset(request).map_err(parameter_error)?;
        let url = format!("{}/prefixes/{SSRN_DOI_PREFIX}/works", self.base_url());
        let query = [
            ("query", request.query.clone()),
            ("rows", request.limit.to_string()),
            ("offset", offset.to_string()),
            ("sort", "score".to_owned()),
            ("order", "desc".to_owned()),
            ("select", SELECT_FIELDS.to_owned()),
        ];
        let (url, query) = (&url, &query);
        let execution = execute_anonymous(
            self.settings(PlatformOperation::Search),
            move |deadline| async move {
                self.send_once::<WorkList>(url, query, "work-list", deadline)
                    .await
            },
        )
        .await?;
        let WorkList {
            total_results,
            items: works,
        } = execution.value;
        let rows = u64::from(request.limit);
        let raw_count = works.len() as u64;
        let mut skipped_dois = Vec::new();
        let items = works
            .into_iter()
            .filter_map(|work| work.into_item().map_err(|doi| skipped_dois.push(doi)).ok())
            .collect();
        let next_offset = offset.saturating_add(raw_count);
        let has_next_page = raw_count == rows
            && next_offset < total_results
            && next_offset.saturating_add(rows) <= MAX_PAGE_END;
        Ok(PlatformSearchOutcome {
            items,
            next_page: has_next_page.then(|| next_offset.to_string()),
            attempts: execution.attempts,
            diagnostic: combine_diagnostics(
                execution
                    .diagnostic
                    .into_iter()
                    .chain(skipped_diagnostic(&skipped_dois)),
            ),
        })
    }

    /// Reads the Crossref record of the paper's SSRN DOI. A missing abstract fails only when
    /// the request asks for one.
    pub(crate) async fn fetch(
        &self,
        request: &PlatformFetchRequest,
    ) -> Result<PlatformFetchOutcome, ProviderError> {
        let PlatformRef::Ssrn(requested) = &request.reference else {
            return Err(parameter_error(other_platform_message(
                ROUTE,
                request.reference.platform(),
            )));
        };
        let url = format!("{}/works/{}", self.base_url(), requested.doi());
        let url = &url;
        let execution = execute_anonymous(
            self.settings(PlatformOperation::Fetch),
            move |deadline| async move {
                let (status, work) = self
                    .send_once::<Work>(url, &[], "work", deadline)
                    .await
                    .map_err(|failure| match failure.status {
                        Some(404) => AttemptFailure {
                            message: format!("SSRN paper not found in Crossref: ssrn:{requested}"),
                            ..failure
                        },
                        _ => failure,
                    })?;
                let failure = |kind, message| AttemptFailure {
                    kind,
                    status,
                    message,
                };
                let item = select_paper(work, requested)
                    .map_err(|message| failure(AttemptErrorKind::Runtime, message))?;
                if request.depth == ContentDepth::Abstract && item.depth != ContentDepth::Abstract {
                    return Err(failure(
                        AttemptErrorKind::Quality,
                        format!("Crossref has no abstract for ssrn:{requested}"),
                    ));
                }
                Ok((status, item))
            },
        )
        .await?;
        Ok(PlatformFetchOutcome {
            item: execution.value,
            content_urls: Vec::new(),
            attempts: execution.attempts,
            diagnostic: execution.diagnostic,
        })
    }

    fn base_url(&self) -> &str {
        self.config.url.trim_end_matches('/')
    }

    fn settings(&self, operation: PlatformOperation) -> ExecutionSettings {
        ExecutionSettings {
            provider: ROUTE.name(),
            target: AttemptTarget::platform(Platform::Ssrn.as_str(), operation.as_str()),
            retry_policy: self.retry_policy,
            deadline: self.deadline,
            attempt_timeout: Duration::from_secs(self.config.timeout_seconds),
            verbose: false,
            timeout_message: "Crossref request timed out",
            model: None,
            transport: Some("http"),
            endpoint_host: None,
            breaker_event: None,
        }
    }

    /// Sends one paced GET and decodes the `message` of the expected Crossref message type.
    async fn send_once<T: DeserializeOwned>(
        &self,
        url: &str,
        query: &[(&str, String)],
        message_type: &str,
        deadline: Deadline,
    ) -> Result<(Option<u16>, T), AttemptFailure> {
        let _permit = acquire_window(&self.limiter, deadline).await?;
        let request = self.client.get(url).query(query);
        let response = send_provider_request(request, &self.credentials).await?;
        let body = read_complete_protocol(response, &self.credentials, failure_message).await?;
        decode_message(&body.text, message_type)
            .map(|message| (Some(body.status), message))
            .map_err(|message| AttemptFailure {
                kind: AttemptErrorKind::Runtime,
                status: Some(body.status),
                message: redacted_urls_message(&message, &self.credentials),
            })
    }
}

fn page_offset(request: &PlatformSearchRequest) -> Result<u64, String> {
    let offset = request.page.as_deref().map_or(Ok(0), |page| {
        page.parse::<u64>()
            .map_err(|_| format!("invalid Crossref page position `{page}`"))
    })?;
    if offset.saturating_add(u64::from(request.limit)) > MAX_PAGE_END {
        return Err(format!(
            "{} cannot page past result {MAX_PAGE_END}",
            ROUTE.name()
        ));
    }
    Ok(offset)
}

fn skipped_diagnostic(dois: &[String]) -> Option<String> {
    (!dois.is_empty()).then(|| {
        format!(
            "{} skipped {} Crossref records without an SSRN DOI: {}",
            ROUTE.name(),
            dois.len(),
            dois.join(", ")
        )
    })
}

/// Returns the item of the Crossref record, which must carry the requested SSRN DOI.
fn select_paper(work: Work, requested: &SsrnRef) -> Result<PlatformItem, String> {
    let returned = work.doi.clone();
    match work.into_item() {
        Ok(item) if matches!(&item.reference, PlatformRef::Ssrn(reference) if reference == requested) => {
            Ok(item)
        }
        _ => Err(format!(
            "Crossref returned DOI {returned} for ssrn:{requested}"
        )),
    }
}

fn failure_message(body: &str, status: u16) -> String {
    serde_json::from_str::<ValidationFailure>(body)
        .ok()
        .and_then(|failure| failure.message.into_iter().next())
        .map_or_else(
            || format!("Crossref returned HTTP {status}"),
            |detail| format!("Crossref rejected the request: {}", detail.message),
        )
}

fn decode_message<T: DeserializeOwned>(body: &str, expected: &str) -> Result<T, String> {
    let envelope = serde_json::from_str::<Envelope>(body)
        .map_err(|error| format!("invalid Crossref response: {error}"))?;
    let message_type = envelope.message_type.as_deref().unwrap_or("none");
    if message_type != expected {
        return Err(format!(
            "Crossref returned message type `{message_type}`, not `{expected}`"
        ));
    }
    serde_json::from_value(envelope.message)
        .map_err(|error| format!("invalid Crossref {expected}: {error}"))
}

#[derive(Deserialize)]
struct Envelope {
    #[serde(rename = "message-type")]
    message_type: Option<String>,
    #[serde(default)]
    message: serde_json::Value,
}

#[derive(Deserialize)]
struct ValidationFailure {
    message: Vec<ValidationDetail>,
}

#[derive(Deserialize)]
struct ValidationDetail {
    message: String,
}

#[derive(Deserialize)]
struct WorkList {
    #[serde(rename = "total-results")]
    total_results: u64,
    #[serde(default)]
    items: Vec<Work>,
}

#[derive(Deserialize)]
struct Work {
    #[serde(rename = "DOI")]
    doi: String,
    #[serde(default)]
    title: Vec<String>,
    #[serde(default)]
    author: Vec<Author>,
    #[serde(rename = "abstract")]
    abstract_text: Option<String>,
    published: Option<DateParts>,
    #[serde(rename = "type")]
    kind: Option<String>,
    created: Option<Timestamp>,
}

#[derive(Deserialize)]
struct Author {
    given: Option<String>,
    family: Option<String>,
    /// The name of an organization author.
    name: Option<String>,
}

#[derive(Deserialize)]
struct DateParts {
    #[serde(rename = "date-parts", default)]
    date_parts: Vec<Vec<Option<i64>>>,
}

#[derive(Deserialize)]
struct Timestamp {
    #[serde(rename = "date-time")]
    date_time: Option<String>,
}

impl Work {
    /// Maps the record to an item, or returns its DOI when the DOI is not an SSRN DOI.
    fn into_item(self) -> Result<PlatformItem, String> {
        let Some(reference) = SsrnRef::from_doi(&self.doi) else {
            return Err(self.doi);
        };
        let abstract_text = self
            .abstract_text
            .as_deref()
            .map(|markup| markup_paragraphs(markup).join("\n\n"))
            .filter(|text| !text.is_empty());
        Ok(PlatformItem {
            url: reference.canonical_url(),
            reference: PlatformRef::Ssrn(reference),
            depth: if abstract_text.is_some() {
                ContentDepth::Abstract
            } else {
                ContentDepth::Metadata
            },
            title: self
                .title
                .iter()
                .map(|title| markup_paragraphs(title).join(" "))
                .find(|title| !title.is_empty())
                .unwrap_or_default(),
            authors: self
                .author
                .iter()
                .filter_map(Author::display_name)
                .collect(),
            published: self.published.as_ref().and_then(DateParts::to_date),
            data: PlatformItemData::Ssrn(SsrnItemData {
                abstract_text,
                doi: Some(self.doi),
                crossref_type: self.kind,
                crossref_created: self.created.and_then(|created| created.date_time),
                ..SsrnItemData::default()
            }),
        })
    }
}

impl Author {
    fn display_name(&self) -> Option<String> {
        let name = match (&self.given, &self.family) {
            (None, None) => self.name.clone()?,
            (given, family) => [given, family]
                .into_iter()
                .flatten()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(" "),
        };
        let name = fold_whitespace(&name);
        (!name.is_empty()).then_some(name)
    }
}

impl DateParts {
    /// Formats the first date at the precision Crossref reports: `2012`, `2012-04`, or
    /// `2012-04-19`. A year alone is never padded to a full date.
    fn to_date(&self) -> Option<String> {
        let mut parts = self.date_parts.first()?.iter().map_while(|part| *part);
        let mut date = format!("{:04}", parts.next()?);
        for part in parts.take(2) {
            let _ = write!(date, "-{part:02}");
        }
        Some(date)
    }
}

// JATS and HTML elements that end a paragraph; every other element is inline.
const BLOCK_ELEMENTS: [&str; 7] = ["p", "title", "sec", "list", "list-item", "br", "div"];

/// Splits Crossref JATS or HTML markup into plain-text paragraphs: tags are removed, block
/// elements end a paragraph, entities are decoded, and whitespace is folded.
fn markup_paragraphs(markup: &str) -> Vec<String> {
    let mut paragraphs = Vec::new();
    let mut paragraph = String::new();
    let mut rest = markup;
    while let Some(start) = rest.find('<') {
        paragraph.push_str(&decode_entities(&rest[..start]));
        let tail = &rest[start..];
        if let Some(cdata) = tail.strip_prefix("<![CDATA[") {
            let end = cdata.find("]]>").unwrap_or(cdata.len());
            paragraph.push_str(&cdata[..end]);
            rest = cdata.get(end + 3..).unwrap_or_default();
        } else if let Some(end) = tag_end(tail) {
            if BLOCK_ELEMENTS.contains(&element_name(&tail[1..end]).as_str()) {
                push_paragraph(&mut paragraphs, &mut paragraph);
            }
            rest = &tail[end + 1..];
        } else {
            paragraph.push('<');
            rest = &tail[1..];
        }
    }
    paragraph.push_str(&decode_entities(rest));
    push_paragraph(&mut paragraphs, &mut paragraph);
    paragraphs
}

/// Returns the index of the `>` that closes a tag at the start of `text`, skipping quoted
/// attribute values of elements; a `<` that does not start a tag, such as in `p < 0.05`, is text.
fn tag_end(text: &str) -> Option<usize> {
    let next = text[1..].chars().next()?;
    if matches!(next, '!' | '?') {
        return text.find('>');
    }
    if !(next.is_ascii_alphabetic() || next == '/') {
        return None;
    }
    let mut quote = None;
    for (index, character) in text.char_indices().skip(1) {
        match (quote, character) {
            (Some(open), _) if character == open => quote = None,
            (None, '"' | '\'') => quote = Some(character),
            (None, '>') => return Some(index),
            _ => {}
        }
    }
    None
}

/// Returns the lowercase local name of a tag body such as `/jats:p` or `br /`.
fn element_name(tag: &str) -> String {
    let name = tag
        .trim_start_matches('/')
        .split(|character: char| character.is_whitespace() || character == '/')
        .next()
        .unwrap_or_default();
    name.rsplit(':')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn push_paragraph(paragraphs: &mut Vec<String>, paragraph: &mut String) {
    let text = fold_whitespace(paragraph);
    if !text.is_empty() {
        paragraphs.push(text);
    }
    paragraph.clear();
}

fn decode_entities(text: &str) -> String {
    let mut decoded = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        decoded.push_str(&rest[..start]);
        let tail = &rest[start..];
        let entity = tail[1..]
            .find(';')
            .and_then(|end| Some((decode_entity(&tail[1..=end])?, end + 2)));
        if let Some((character, length)) = entity {
            decoded.push(character);
            rest = &tail[length..];
        } else {
            decoded.push('&');
            rest = &tail[1..];
        }
    }
    decoded.push_str(rest);
    decoded
}

/// Decodes a numeric character reference, an XML entity, or `&nbsp;`. Crossref abstracts are
/// XML, so any other named entity stays verbatim.
fn decode_entity(name: &str) -> Option<char> {
    let code = match name {
        "amp" => return Some('&'),
        "lt" => return Some('<'),
        "gt" => return Some('>'),
        "quot" => return Some('"'),
        "apos" => return Some('\''),
        "nbsp" => return Some('\u{a0}'),
        _ => name.strip_prefix('#')?,
    };
    let value = match code.strip_prefix(['x', 'X']) {
        Some(hex) => u32::from_str_radix(hex, 16).ok()?,
        None => code.parse().ok()?,
    };
    char::from_u32(value)
}

fn fold_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        Work, WorkList, decode_message, failure_message, fetch_support, markup_paragraphs,
        search_support, select_paper,
    };
    use crate::types::{
        ContentDepth, PlatformFetchRequest, PlatformRef, PlatformSearchOptions,
        PlatformSearchRequest, SsrnRef, SsrnSearchOptions,
    };

    fn work(value: serde_json::Value) -> Work {
        serde_json::from_value(value).expect("decode work")
    }

    fn item(value: serde_json::Value) -> serde_json::Value {
        let item = work(value).into_item().expect("SSRN DOI");
        serde_json::to_value(item).expect("serialize item")
    }

    #[test]
    fn a_full_record_maps_every_crossref_field() {
        let mapped = item(json!({
            "DOI": "10.2139/ssrn.2042750",
            "title": ["Risk Premia  Harvesting Through <i>Dual</i> Momentum"],
            "author": [
                {"given": "Gary", "family": "Antonacci", "sequence": "first"},
                {"name": "Example  Institute"}
            ],
            "abstract": "<jats:p>Momentum is the premier\n market anomaly.</jats:p><jats:p>Second &amp; last.</jats:p>",
            "published": {"date-parts": [[2012, 4, 19]]},
            "type": "posted-content",
            "created": {"date-parts": [[2012, 4, 25]], "date-time": "2012-04-25T11:01:44Z"},
            "resource": {"primary": {"URL": "https://www.ssrn.com/abstract=2042750"}}
        }));

        assert_eq!(
            mapped,
            json!({
                "ref": "ssrn:2042750",
                "url": "https://papers.ssrn.com/sol3/papers.cfm?abstract_id=2042750",
                "depth": "abstract",
                "title": "Risk Premia Harvesting Through Dual Momentum",
                "authors": ["Gary Antonacci", "Example Institute"],
                "published": "2012-04-19",
                "abstract": "Momentum is the premier market anomaly.\n\nSecond & last.",
                "snippet": null,
                "doi": "10.2139/ssrn.2042750",
                "crossref_type": "posted-content",
                "crossref_created": "2012-04-25T11:01:44Z",
                "posted": null,
                "last_revised": null,
                "date_written": null
            })
        );
    }

    #[test]
    fn a_record_without_an_abstract_has_metadata_depth_and_a_null_abstract() {
        let mapped = item(json!({"DOI": "10.2139/ssrn.1", "title": ["T"]}));

        assert_eq!(
            (&mapped["depth"], &mapped["abstract"], &mapped["published"]),
            (&json!("metadata"), &json!(null), &json!(null))
        );
    }

    #[test]
    fn an_abstract_that_is_empty_after_cleaning_counts_as_missing() {
        let mapped = item(json!({
            "DOI": "10.2139/ssrn.1",
            "abstract": "<jats:p> &#x20; </jats:p>"
        }));

        assert_eq!(
            (&mapped["depth"], &mapped["abstract"]),
            (&json!("metadata"), &json!(null))
        );
    }

    #[test]
    fn published_dates_keep_their_original_precision() {
        let dates = [
            json!([[2012]]),
            json!([[2012, 4]]),
            json!([[2012, 4, 9]]),
            json!([[null]]),
        ]
        .map(|parts| {
            item(json!({"DOI": "10.2139/ssrn.1", "published": {"date-parts": parts}}))["published"]
                .clone()
        });

        assert_eq!(
            dates,
            [
                json!("2012"),
                json!("2012-04"),
                json!("2012-04-09"),
                json!(null)
            ]
        );
    }

    #[test]
    fn the_crossref_created_date_never_fills_the_published_date() {
        let mapped = item(json!({
            "DOI": "10.2139/ssrn.1",
            "created": {"date-time": "2012-04-25T11:01:44Z"}
        }));

        assert_eq!(mapped["published"], json!(null));
    }

    #[test]
    fn a_record_without_an_ssrn_doi_yields_its_doi() {
        let result = work(json!({"DOI": "10.1016/j.jfineco.2020.01.001"})).into_item();

        assert_eq!(
            result.map(|_| ()),
            Err("10.1016/j.jfineco.2020.01.001".to_owned())
        );
    }

    #[test]
    fn jats_markup_becomes_plain_paragraphs() {
        let paragraphs = markup_paragraphs(
            "<jats:title>Abstract</jats:title>\n<jats:p>A <jats:italic>b</jats:italic> &lt;c&gt; &#8211; &#x2014; p < 0.05 &unknown;</jats:p><jats:sec><jats:p>Two</jats:p></jats:sec>",
        );

        assert_eq!(
            paragraphs,
            ["Abstract", "A b <c> – — p < 0.05 &unknown;", "Two"]
        );
    }

    #[test]
    fn cdata_text_is_kept() {
        let paragraphs = markup_paragraphs("<jats:p><![CDATA[Actual <abstract> text.]]></jats:p>");

        assert_eq!(paragraphs, ["Actual <abstract> text."]);
    }

    #[test]
    fn a_closing_bracket_in_a_quoted_attribute_stays_inside_the_tag() {
        let paragraphs = markup_paragraphs(
            r#"<jats:p>See <jats:ext-link xlink:href="https://example.test/?q=a>b">Evidence</jats:ext-link>.</jats:p>"#,
        );

        assert_eq!(paragraphs, ["See Evidence."]);
    }

    #[test]
    fn named_entities_outside_xml_stay_verbatim() {
        let paragraphs = markup_paragraphs("<p>Risk &ndash; return&nbsp;&#945;</p>");

        assert_eq!(paragraphs, ["Risk &ndash; return α"]);
    }

    #[test]
    fn an_escaped_tag_stays_text() {
        let paragraphs = markup_paragraphs("<p>&lt;p&gt;literal&lt;/p&gt;</p>");

        assert_eq!(paragraphs, ["<p>literal</p>"]);
    }

    #[test]
    fn a_returned_record_for_another_paper_is_rejected() {
        let requested = SsrnRef::parse("ssrn:2042750").expect("valid ref");

        let result = select_paper(work(json!({"DOI": "10.2139/ssrn.1"})), &requested);

        assert_eq!(
            result.map(|_| ()),
            Err("Crossref returned DOI 10.2139/ssrn.1 for ssrn:2042750".to_owned())
        );
    }

    #[test]
    fn a_returned_doi_in_another_case_is_the_same_paper() {
        let requested = SsrnRef::parse("ssrn:2042750").expect("valid ref");

        let result = select_paper(work(json!({"DOI": "10.2139/SSRN.2042750"})), &requested);

        assert!(result.is_ok());
    }

    #[test]
    fn an_unexpected_message_type_fails_to_decode() {
        let result = decode_message::<WorkList>(
            r#"{"status":"ok","message-type":"work","message":{"DOI":"10.2139/ssrn.1"}}"#,
            "work-list",
        );

        assert_eq!(
            result.map(|_| ()).unwrap_err(),
            "Crossref returned message type `work`, not `work-list`"
        );
    }

    #[test]
    fn a_body_that_is_not_json_fails_to_decode() {
        let result = decode_message::<WorkList>("<html>busy</html>", "work-list");

        assert!(result.is_err());
    }

    #[test]
    fn a_failed_status_reads_the_validation_message_or_names_the_status() {
        let validation = r#"{"status":"failed","message-type":"validation-failure","message":[{"type":"integer-not-valid","value":10001,"message":"Offset specified as 10001 is too large"}]}"#;

        assert_eq!(
            [
                failure_message(validation, 400),
                failure_message("Resource not found.", 404)
            ],
            [
                "Crossref rejected the request: Offset specified as 10001 is too large".to_owned(),
                "Crossref returned HTTP 404".to_owned()
            ]
        );
    }

    fn search_request(limit: u16, page: Option<&str>) -> PlatformSearchRequest {
        PlatformSearchRequest {
            query: "momentum".into(),
            limit,
            options: PlatformSearchOptions::Ssrn(SsrnSearchOptions::default()),
            page: page.map(str::to_owned),
        }
    }

    #[test]
    fn search_support_rejects_pages_that_end_past_the_crossref_offset_limit() {
        let results = [
            search_request(20, Some("9980")),
            search_request(20, Some("9981")),
            search_request(100, Some("9900")),
        ]
        .map(|request| search_support(&request).is_ok());

        assert_eq!(results, [true, false, true]);
    }

    #[test]
    fn search_support_rejects_a_page_position_it_did_not_issue() {
        let result = search_support(&search_request(10, Some("bogus")));

        assert_eq!(
            result,
            Err("invalid Crossref page position `bogus`".to_owned())
        );
    }

    #[test]
    fn fetch_supports_metadata_and_abstract_depths_only() {
        let reference = PlatformRef::parse(crate::types::Platform::Ssrn, "ssrn:1").expect("ref");
        let results = [
            ContentDepth::Metadata,
            ContentDepth::Abstract,
            ContentDepth::FullText,
        ]
        .map(|depth| {
            fetch_support(&PlatformFetchRequest {
                reference: reference.clone(),
                depth,
            })
            .is_ok()
        });

        assert_eq!(results, [true, true, false]);
    }
}
