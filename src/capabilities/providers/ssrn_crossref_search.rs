//! Crossref search mapping, execution, and offset pagination.

use super::{ROUTE, SsrnCrossref, WorkList};
use crate::catalog::PlatformOperation;
use crate::net::combine_diagnostics;
use crate::providers::execution::execute_anonymous;
use crate::providers::shared::{other_platform_message, parameter_error};
use crate::types::{
    PlatformSearchOptions, PlatformSearchOutcome, PlatformSearchRequest, ProviderError,
    SsrnSearchOptions,
};
use crate::types::{SsrnSearchScope, SsrnSort, SsrnSortOrder};

const SSRN_DOI_PREFIX: &str = "10.2139";
const SELECT_FIELDS: &str = "DOI,title,author,abstract,published,type,created,resource";
const MAX_PAGE_END: u64 = 10_000;

/// Returns whether the route can run the request; it never sends a request. Only a page
/// position the route did not issue, or one past the Crossref offset limit, fails.
pub(crate) fn search_support(request: &PlatformSearchRequest) -> Result<(), String> {
    match &request.options {
        PlatformSearchOptions::Ssrn(options) => {
            let unsupported = [
                (
                    options.scope == SsrnSearchScope::FullText,
                    "--scope full-text",
                ),
                (options.mode.is_some(), "--mode"),
                (options.date.is_some(), "--date"),
                (
                    matches!(
                        options.sort,
                        SsrnSort::Posted | SsrnSort::Downloads | SsrnSort::Title
                    ),
                    "--sort",
                ),
            ]
            .into_iter()
            .filter_map(|(bad, name)| bad.then_some(name))
            .collect::<Vec<_>>();
            if !unsupported.is_empty() {
                return Err(format!(
                    "ssrn_crossref does not support {}",
                    unsupported.join(", ")
                ));
            }
            page_offset(request).map(|_| ())
        }
        PlatformSearchOptions::Arxiv(_) => {
            Err(other_platform_message(ROUTE, request.options.platform()))
        }
    }
}

impl SsrnCrossref {
    /// Searches the SSRN DOI prefix with the complete criteria. The page position is an absolute offset, and
    /// the last page is judged by the record count before records without an SSRN DOI are
    /// dropped.
    pub(crate) async fn search(
        &self,
        request: &PlatformSearchRequest,
    ) -> Result<PlatformSearchOutcome, ProviderError> {
        search_support(request).map_err(parameter_error)?;
        let PlatformSearchOptions::Ssrn(options) = &request.options else {
            return Err(parameter_error(other_platform_message(
                ROUTE,
                request.options.platform(),
            )));
        };
        let offset = page_offset(request).map_err(parameter_error)?;
        let url = format!("{}/prefixes/{SSRN_DOI_PREFIX}/works", self.base_url());
        let query = query_parameters(request, options, offset);
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

fn query_parameters(
    request: &PlatformSearchRequest,
    options: &SsrnSearchOptions,
    offset: u64,
) -> Vec<(&'static str, String)> {
    let query_field = match options.scope {
        SsrnSearchScope::All => "query",
        SsrnSearchScope::Title => "query.title",
        SsrnSearchScope::Bibliographic => "query.bibliographic",
        SsrnSearchScope::FullText => unreachable!("support checked"),
    };
    let mut query = vec![
        (query_field, request.query.clone()),
        ("rows", request.limit.to_string()),
        ("offset", offset.to_string()),
        (
            "sort",
            match options.sort {
                SsrnSort::Relevance => "score",
                SsrnSort::Published => "published",
                SsrnSort::Created => "created",
                SsrnSort::Updated => "updated",
                SsrnSort::Citations => "is-referenced-by-count",
                SsrnSort::Posted | SsrnSort::Downloads | SsrnSort::Title => {
                    unreachable!("support checked")
                }
            }
            .to_owned(),
        ),
        (
            "order",
            match options.order {
                SsrnSortOrder::Asc => "asc",
                SsrnSortOrder::Desc => "desc",
            }
            .to_owned(),
        ),
        ("select", SELECT_FIELDS.to_owned()),
    ];
    if let Some(author) = &options.author {
        query.push(("query.author", author.clone()));
    }
    if let Some(affiliation) = &options.affiliation {
        query.push(("query.affiliation", affiliation.clone()));
    }
    let mut filters = Vec::new();
    for (name, range) in [
        ("pub", &options.published),
        ("created", &options.created),
        ("update", &options.updated),
    ] {
        if let Some(from) = range.from {
            filters.push(format!("from-{name}-date:{from}"));
        }
        if let Some(to) = range.to {
            filters.push(format!("until-{name}-date:{to}"));
        }
    }
    if options.has_abstract {
        filters.push("has-abstract:true".into());
    }
    if let Some(work_type) = options.work_type {
        filters.push(format!("type:{}", work_type.as_str()));
    }
    if let Some(orcid) = &options.orcid {
        filters.push(format!("orcid:{orcid}"));
    }
    if let Some(funder) = &options.funder {
        filters.push(format!("funder:{funder}"));
    }
    if !filters.is_empty() {
        query.push(("filter", filters.join(",")));
    }
    query
}

#[cfg(test)]
mod tests {
    #[test]
    fn native_conditions_are_rejected_before_crossref_io() {
        for value in [
            serde_json::json!({"scope":"full_text"}),
            serde_json::json!({"mode":"boolean"}),
            serde_json::json!({"mode":"fuzzy"}),
            serde_json::json!({"date":"all_time"}),
            serde_json::json!({"date":"last_week"}),
            serde_json::json!({"sort":"posted"}),
            serde_json::json!({"sort":"downloads"}),
            serde_json::json!({"sort":"title"}),
        ] {
            let request = crate::types::PlatformSearchRequest {
                query: "momentum".into(),
                limit: 10,
                page: None,
                options: crate::types::PlatformSearchOptions::Ssrn(
                    serde_json::from_value(value.clone()).expect("criteria"),
                ),
            };
            assert!(super::search_support(&request).is_err(), "{value}");
        }
    }
}
