//! The Google Scholar engine of SerpApi: request parameters, result decoding, and support
//! checks.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;
use serde_json::Value;

use super::{ROUTE, SearchResults, Serpapi, refusal};
use crate::catalog::PlatformOperation;
use crate::net::{AttemptFailure, combine_diagnostics};
use crate::providers::shared::{is_http_url, other_platform_message, parameter_error};
use crate::types::{
    AttemptErrorKind, ContentDepth, FullTextSource, Platform, PlatformFetchOutcome,
    PlatformFetchRequest, PlatformItem, PlatformItemData, PlatformRef, PlatformSearchOptions,
    PlatformSearchOutcome, PlatformSearchRequest, ProviderError, SCHOLAR_MAX_LIMIT, ScholarCluster,
    ScholarItemData, ScholarRef, ScholarResource, ScholarResult, ScholarSearchOptions,
    ScholarVersion,
};

/// The platform this engine serves.
pub(super) const PLATFORM: Platform = Platform::Scholar;
const ENGINE: &str = "google_scholar";
// A fixed interface language keeps the `summary` byline in its `authors - source, year - host`
// form.
const LANGUAGE: &str = "en";
// Google Scholar serves no result past the 1000th; SerpApi still bills the empty page.
const MAX_PAGE_END: u64 = 1000;

/// A decoded search page.
struct DecodedPage {
    items: Vec<PlatformItem>,
    /// The diagnostic naming the results skipped for lacking a usable cluster ID.
    skipped: Option<String>,
    has_next_page: bool,
}

/// A decoded cluster page.
struct DecodedCluster {
    item: PlatformItem,
    has_more_versions: bool,
}

/// Returns whether the route can run the search; it never sends a request. Only a page
/// position the route did not issue, or one past Google Scholar's result limit, fails.
pub(crate) fn search_support(request: &PlatformSearchRequest) -> Result<(), String> {
    if !matches!(request.options, PlatformSearchOptions::Scholar(_)) {
        return Err(other_platform_message(ROUTE, request.options.platform()));
    }
    page_start(request).map(|_| ())
}

/// Returns whether the route can fetch the item at the requested depth; it never sends a
/// request. Google Scholar is an index, so only metadata is available.
pub(crate) fn fetch_support(request: &PlatformFetchRequest) -> Result<(), String> {
    fetch_target(request).map(|_| ())
}

fn fetch_target(request: &PlatformFetchRequest) -> Result<ScholarRef, String> {
    let PlatformRef::Scholar(requested) = request.reference else {
        return Err(other_platform_message(ROUTE, request.reference.platform()));
    };
    match request.depth {
        ContentDepth::Metadata => Ok(requested),
        depth => Err(format!(
            "{} cannot fetch at depth `{}`; Google Scholar provides metadata only",
            ROUTE.name(),
            depth.as_str()
        )),
    }
}

impl Serpapi {
    /// Searches one page of Google Scholar results at an absolute offset. Results without a
    /// verifiable cluster ID are skipped and reported in the diagnostic; the next page starts
    /// one page size later however many results were skipped.
    pub(crate) async fn search(
        &self,
        request: &PlatformSearchRequest,
    ) -> Result<PlatformSearchOutcome, ProviderError> {
        let PlatformSearchOptions::Scholar(options) = &request.options else {
            return Err(parameter_error(other_platform_message(
                ROUTE,
                request.options.platform(),
            )));
        };
        let start = page_start(request).map_err(parameter_error)?;
        let parameters = search_parameters(request, options, start);
        let execution = self
            .run(PlatformOperation::Search, &parameters, decode_page)
            .await?;
        let DecodedPage {
            items,
            skipped,
            has_next_page,
        } = execution.value;
        let page_size = u64::from(request.limit);
        let next_start = start.saturating_add(page_size);
        let has_next_page = has_next_page && next_start.saturating_add(page_size) <= MAX_PAGE_END;
        Ok(PlatformSearchOutcome {
            items,
            next_page: has_next_page.then(|| next_start.to_string()),
            attempts: execution.attempts,
            diagnostic: combine_diagnostics(execution.diagnostic.into_iter().chain(skipped)),
        })
    }

    /// Lists the first page of versions in the requested cluster. An empty cluster means the
    /// paper does not exist.
    pub(crate) async fn fetch(
        &self,
        request: &PlatformFetchRequest,
    ) -> Result<PlatformFetchOutcome, ProviderError> {
        let requested = fetch_target(request).map_err(parameter_error)?;
        // One page of versions costs one search whatever its size, so fetch always asks for the
        // most.
        let parameters = [
            ("engine", ENGINE.to_owned()),
            ("hl", LANGUAGE.to_owned()),
            ("cluster", requested.cluster_id().to_string()),
            ("num", SCHOLAR_MAX_LIMIT.to_string()),
        ];
        let execution = self
            .run(PlatformOperation::Fetch, &parameters, |results| {
                decode_cluster(results, requested)
            })
            .await?;
        let DecodedCluster {
            item,
            has_more_versions,
        } = execution.value;
        let incomplete = has_more_versions.then(|| {
            format!(
                "Google Scholar lists more versions of scholar:{requested} than this page holds; see {}",
                requested.canonical_url()
            )
        });
        Ok(PlatformFetchOutcome {
            item,
            content_source: FullTextSource::Urls(Vec::new()),
            attempts: execution.attempts,
            diagnostic: combine_diagnostics(execution.diagnostic.into_iter().chain(incomplete)),
        })
    }
}

/// Returns the absolute offset of the requested page. Google Scholar serves at most
/// `MAX_PAGE_END` results, and a page past them still costs a search.
fn page_start(request: &PlatformSearchRequest) -> Result<u64, String> {
    let start = request.page.as_deref().map_or(Ok(0), |page| {
        page.parse::<u64>()
            .map_err(|_| format!("invalid Google Scholar page position `{page}`"))
    })?;
    if start.saturating_add(u64::from(request.limit)) > MAX_PAGE_END {
        return Err(format!(
            "{} cannot page past Google Scholar result {MAX_PAGE_END}",
            ROUTE.name()
        ));
    }
    Ok(start)
}

fn search_parameters(
    request: &PlatformSearchRequest,
    options: &ScholarSearchOptions,
    start: u64,
) -> Vec<(&'static str, String)> {
    let mut parameters = vec![
        ("engine", ENGINE.to_owned()),
        ("hl", LANGUAGE.to_owned()),
        ("q", request.query.clone()),
        ("num", request.limit.to_string()),
    ];
    if start > 0 {
        parameters.push(("start", start.to_string()));
    }
    if let Some(from) = options.year_from {
        parameters.push(("as_ylo", from.to_string()));
    }
    if let Some(to) = options.year_to {
        parameters.push(("as_yhi", to.to_string()));
    }
    if options.review_only {
        parameters.push(("as_rr", "1".to_owned()));
    }
    parameters
}

/// Decodes a search page into items, the diagnostic of skipped results, and whether Google
/// Scholar offers a next page. A non-empty page whose every result is skipped is Runtime, never
/// a legitimately empty page.
fn decode_page(results: SearchResults) -> Result<DecodedPage, AttemptFailure> {
    let SearchResults::Found {
        results,
        has_next_page,
    } = results
    else {
        return Ok(DecodedPage {
            items: Vec::new(),
            skipped: None,
            has_next_page: false,
        });
    };
    let results = organic_results(results)?;
    let count = results.len();
    let mut skipped = Vec::new();
    let items = results
        .into_iter()
        .filter_map(|result| {
            result
                .into_item()
                .map_err(|reason| skipped.push(reason))
                .ok()
        })
        .collect::<Vec<_>>();
    let skipped = (!skipped.is_empty()).then(|| {
        format!(
            "{} skipped {} Google Scholar results without a usable cluster ID: {}",
            ROUTE.name(),
            skipped.len(),
            skipped.join("; ")
        )
    });
    if items.is_empty() {
        return Err(refusal(
            AttemptErrorKind::Runtime,
            format!(
                "SerpApi returned {count} Google Scholar results but none has a usable cluster ID; {}",
                skipped.unwrap_or_default()
            ),
        ));
    }
    Ok(DecodedPage {
        items,
        skipped,
        has_next_page,
    })
}

/// Decodes a cluster page into the fetched item and whether more versions exist.
fn decode_cluster(
    results: SearchResults,
    requested: ScholarRef,
) -> Result<DecodedCluster, AttemptFailure> {
    let SearchResults::Found {
        results,
        has_next_page,
    } = results
    else {
        return Err(refusal(
            AttemptErrorKind::Parameter,
            format!("Google Scholar has no cluster scholar:{requested}"),
        ));
    };
    let versions = organic_results(results)?;
    let (title, authors, published) = versions
        .first()
        .map(|first| {
            (
                first.title.clone().unwrap_or_default(),
                first.authors(),
                first.published(),
            )
        })
        .unwrap_or_default();
    let item = PlatformItem {
        reference: PlatformRef::Scholar(requested),
        url: requested.canonical_url(),
        depth: ContentDepth::Metadata,
        title,
        authors,
        published,
        data: PlatformItemData::Scholar(ScholarItemData::Cluster(ScholarCluster {
            versions: versions
                .into_iter()
                .map(OrganicResult::into_version)
                .collect(),
        })),
    };
    Ok(DecodedCluster {
        item,
        has_more_versions: has_next_page,
    })
}

fn organic_results(results: Vec<Value>) -> Result<Vec<OrganicResult>, AttemptFailure> {
    serde_json::from_value(Value::Array(results)).map_err(|error| {
        refusal(
            AttemptErrorKind::Runtime,
            format!("invalid Google Scholar results from SerpApi: {error}"),
        )
    })
}

#[derive(Deserialize)]
struct OrganicResult {
    title: Option<String>,
    result_id: Option<String>,
    link: Option<String>,
    snippet: Option<String>,
    #[serde(rename = "type")]
    result_type: Option<String>,
    publication_info: Option<PublicationInfo>,
    resources: Option<Vec<Resource>>,
    inline_links: Option<InlineLinks>,
}

#[derive(Deserialize)]
struct PublicationInfo {
    summary: Option<String>,
    authors: Option<Vec<Author>>,
}

#[derive(Deserialize)]
struct Author {
    name: Option<String>,
}

#[derive(Deserialize)]
struct Resource {
    title: Option<String>,
    file_format: Option<String>,
    link: Option<String>,
}

#[derive(Deserialize)]
struct InlineLinks {
    cited_by: Option<CitedBy>,
    versions: Option<Versions>,
}

#[derive(Deserialize)]
struct CitedBy {
    total: Option<u64>,
    cites_id: Option<String>,
}

#[derive(Deserialize)]
struct Versions {
    total: Option<u64>,
    cluster_id: Option<String>,
}

impl OrganicResult {
    /// Maps a search result to an item, or returns why it has no usable cluster ID.
    fn into_item(self) -> Result<PlatformItem, String> {
        let reference = self
            .reference()
            .map_err(|reason| format!("\"{}\" ({reason})", self.title.as_deref().unwrap_or("")))?;
        let (authors, published) = (self.authors(), self.published());
        let snippet = self.snippet.filter(|snippet| !snippet.trim().is_empty());
        let (cited_by, version_count) = self.inline_links.as_ref().map_or((None, None), |links| {
            (
                links.cited_by.as_ref().and_then(|cited_by| cited_by.total),
                links.versions.as_ref().and_then(|versions| versions.total),
            )
        });
        Ok(PlatformItem {
            reference: PlatformRef::Scholar(reference),
            url: reference.canonical_url(),
            depth: if snippet.is_some() {
                ContentDepth::Snippet
            } else {
                ContentDepth::Metadata
            },
            authors,
            published,
            data: PlatformItemData::Scholar(ScholarItemData::Result(ScholarResult {
                snippet,
                link: self.link,
                source: self.publication_info.and_then(|info| info.summary),
                cited_by,
                version_count,
                resources: resources(self.resources),
                result_type: self.result_type,
            })),
            title: self.title.unwrap_or_default(),
        })
    }

    fn into_version(self) -> ScholarVersion {
        ScholarVersion {
            title: self.title.unwrap_or_default(),
            link: self.link,
            source: self.publication_info.and_then(|info| info.summary),
            resources: resources(self.resources),
        }
    }

    /// Returns the cluster ID: the explicit versions ID, then the explicit citing-works ID, then
    /// the decoded `result_id`. Explicit IDs that disagree identify nothing.
    fn reference(&self) -> Result<ScholarRef, String> {
        let links = self.inline_links.as_ref();
        let explicit = |name: &str, value: Option<&String>| {
            value
                .map(|value| {
                    ScholarRef::from_cluster_id(value)
                        .ok_or_else(|| format!("invalid {name} `{value}`"))
                })
                .transpose()
        };
        let versions = explicit(
            "cluster_id",
            links.and_then(|links| links.versions.as_ref()?.cluster_id.as_ref()),
        )?;
        let cites = explicit(
            "cites_id",
            links.and_then(|links| links.cited_by.as_ref()?.cites_id.as_ref()),
        )?;
        match (versions, cites) {
            (Some(versions), Some(cites)) if versions != cites => Err(format!(
                "cluster_id {versions} differs from cites_id {cites}"
            )),
            (Some(reference), _) | (None, Some(reference)) => Ok(reference),
            (None, None) => self
                .result_id
                .as_deref()
                .and_then(result_id_cluster)
                .ok_or_else(|| "no explicit cluster ID and no decodable result_id".to_owned()),
        }
    }

    /// Returns the author names, or the byline names before the first ` - ` without the
    /// truncation marks when the result has no author array.
    fn authors(&self) -> Vec<String> {
        let info = self.publication_info.as_ref();
        let listed = info
            .and_then(|info| info.authors.as_ref())
            .into_iter()
            .flatten()
            .filter_map(|author| author.name.as_deref())
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if !listed.is_empty() {
            return listed;
        }
        info.and_then(|info| info.summary.as_deref())
            .and_then(|summary| summary.split_once(" - "))
            .map(|(names, _)| {
                names
                    .split(", ")
                    .map(|name| name.replace('…', "").trim().to_owned())
                    .filter(|name| !name.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Returns the year that ends the byline's source segment (`source, YYYY` or a bare `YYYY`),
    /// so numbers elsewhere, such as arXiv or SSRN IDs, are never read as years.
    fn published(&self) -> Option<String> {
        let summary = self.publication_info.as_ref()?.summary.as_deref()?;
        let segment = summary.split(" - ").nth(1)?.trim();
        let year = segment.rsplit_once(", ").map_or(segment, |(_, year)| year);
        (year.len() == 4 && year.bytes().all(|byte| byte.is_ascii_digit())).then(|| year.to_owned())
    }
}

/// Decodes the cluster ID inside a search result's `result_id`: unpadded base64url of the
/// 8-byte little-endian cluster ID followed by `0x09`. SerpApi does not document this layout;
/// every result with explicit IDs matched it on 2026-10-07, so anything else is rejected.
fn result_id_cluster(result_id: &str) -> Option<ScholarRef> {
    let bytes = URL_SAFE_NO_PAD.decode(result_id).ok()?;
    let Ok([b0, b1, b2, b3, b4, b5, b6, b7, 0x09]) = <[u8; 9]>::try_from(bytes) else {
        return None;
    };
    Some(ScholarRef::from(u64::from_le_bytes([
        b0, b1, b2, b3, b4, b5, b6, b7,
    ])))
}

/// Keeps the resources with an HTTP(S) URL, first occurrence of each URL only.
fn resources(resources: Option<Vec<Resource>>) -> Vec<ScholarResource> {
    let mut kept: Vec<ScholarResource> = Vec::new();
    for resource in resources.into_iter().flatten() {
        let Some(url) = resource.link.filter(|link| is_http_url(link)) else {
            continue;
        };
        if kept.iter().any(|existing| existing.url == url) {
            continue;
        }
        kept.push(ScholarResource {
            title: resource.title.unwrap_or_default(),
            file_format: resource.file_format,
            url,
        });
    }
    kept
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{OrganicResult, resources, result_id_cluster};
    use crate::types::ScholarRef;

    fn organic(value: Value) -> OrganicResult {
        serde_json::from_value(value).expect("decode result")
    }

    fn with_summary(summary: &str) -> OrganicResult {
        organic(json!({"publication_info": {"summary": summary}}))
    }

    #[test]
    fn a_result_id_holds_the_little_endian_cluster_id() {
        let clusters = ["fL6eJ05HsPwJ", "qzdExiweMh4J", "nA5StemgftIJ"]
            .map(|result_id| result_id_cluster(result_id).map(ScholarRef::cluster_id));

        assert_eq!(
            clusters,
            [
                Some(18_208_131_694_456_651_388),
                Some(2_175_834_747_627_124_651),
                Some(15_167_737_520_660_287_132),
            ]
        );
    }

    #[test]
    fn a_result_id_with_another_last_byte_length_or_padding_holds_no_cluster_id() {
        let clusters = ["nA5StemgftII", "nA5StemgftI", "nA5StemgftIJ=", ""]
            .map(|result_id| result_id_cluster(result_id).map(ScholarRef::cluster_id));

        assert_eq!(clusters, [None; 4]);
    }

    #[test]
    fn explicit_ids_that_disagree_identify_no_cluster() {
        let result = organic(json!({
            "result_id": "fL6eJ05HsPwJ",
            "inline_links": {
                "versions": {"cluster_id": "18208131694456651388"},
                "cited_by": {"cites_id": "1"}
            }
        }));

        assert_eq!(
            result.reference().map(ScholarRef::cluster_id),
            Err("cluster_id 18208131694456651388 differs from cites_id 1".to_owned())
        );
    }

    #[test]
    fn a_citation_entry_maps_to_a_metadata_item_identified_by_its_cites_id() {
        let item = organic(json!({
            "title": "Time Series Momentum",
            "result_id": "CFXLZdAFrk4J",
            "type": "Citation",
            "snippet": "",
            "publication_info": {
                "summary": "B Hurst, YH Ooi, LH Pedersen - Journal of Financial Economics, 2014",
                "authors": [{"name": "B Hurst"}, {"name": "YH Ooi"}, {"name": "LH Pedersen"}]
            },
            "inline_links": {"cited_by": {"total": 2, "cites_id": "5669475373525193992"}}
        }))
        .into_item()
        .expect("item");

        assert_eq!(
            serde_json::to_value(item).expect("serialize item"),
            json!({
                "ref": "scholar:5669475373525193992",
                "url": "https://scholar.google.com/scholar?cluster=5669475373525193992",
                "depth": "metadata",
                "title": "Time Series Momentum",
                "authors": ["B Hurst", "YH Ooi", "LH Pedersen"],
                "published": "2014",
                "snippet": null,
                "link": null,
                "source": "B Hurst, YH Ooi, LH Pedersen - Journal of Financial Economics, 2014",
                "cited_by": 2,
                "version_count": null,
                "resources": [],
                "result_type": "Citation"
            })
        );
    }

    #[test]
    fn listed_authors_drop_empty_names() {
        let result = organic(json!({
            "publication_info": {
                "summary": "…, J Quin, S Guillerme, A Moskowitz… - Journal of vascular …, 2010 - jvascsurg.org",
                "authors": [{"name": ""}, {"name": "J Quin"}, {"name": "S Guillerme"}, {"name": "A Moskowitz"}]
            }
        }));

        assert_eq!(result.authors(), ["J Quin", "S Guillerme", "A Moskowitz"]);
    }

    #[test]
    fn without_an_author_array_the_byline_names_lose_their_truncation_marks() {
        let result = with_summary(
            "…, J Quin, S Guillerme, A Moskowitz… - Journal of vascular …, 2010 - jvascsurg.org",
        );

        assert_eq!(result.authors(), ["J Quin", "S Guillerme", "A Moskowitz"]);
    }

    #[test]
    fn the_year_ends_the_source_segment_or_is_the_whole_segment() {
        let years = [
            "TJ Moskowitz, YH Ooi, LH Pedersen - Journal of financial economics, 2012 - Elsevier",
            "B Hurst, YH Ooi, LH Pedersen - Available at SSRN 2993026, 2017 - papers.ssrn.com",
            "B Hurst, YH Ooi, LH Pedersen - Journal of Financial Economics, 2014",
            "P Tan, M Curcic, B Haus, I Savelyev, S Matt - 2024 - scholarship.miami.edu",
        ]
        .map(|summary| with_summary(summary).published());

        assert_eq!(
            years,
            ["2012", "2017", "2014", "2024"].map(|year| Some(year.to_owned()))
        );
    }

    #[test]
    fn a_byline_without_a_source_year_has_no_year() {
        let years = [
            "J Hu, X Li, T Wang - papers.ssrn.com",
            "B Lim, S Zohren, S Roberts - arXiv preprint arXiv:1904.04912 - arxiv.org",
            "B Lim, S Zohren, S Roberts - Working paper, 19 - arxiv.org",
        ]
        .map(|summary| with_summary(summary).published());

        assert_eq!(years, [None, None, None]);
    }

    #[test]
    fn resources_keep_only_distinct_http_links_in_order() {
        let resources = resources(Some(
            serde_json::from_value(json!([
                {"title": "", "file_format": "HTML", "link": null},
                {"title": "", "file_format": "PDF"},
                {"title": "sciencedirect.com", "file_format": "HTML", "link": "https://www.sciencedirect.com/science/article/pii/S0304405X11002613"},
                {"title": "mirror", "file_format": "PDF", "link": "javascript:void(0)"},
                {"title": "copy", "file_format": "HTML", "link": "https://www.sciencedirect.com/science/article/pii/S0304405X11002613"},
                {"title": "", "file_format": null, "link": "https://example.org/tsm.pdf"}
            ]))
            .expect("decode resources"),
        ));

        assert_eq!(
            serde_json::to_value(resources).expect("serialize resources"),
            json!([
                {"title": "sciencedirect.com", "file_format": "HTML", "url": "https://www.sciencedirect.com/science/article/pii/S0304405X11002613"},
                {"title": "", "file_format": null, "url": "https://example.org/tsm.pdf"}
            ])
        );
    }
}
