//! Page Cursors: the request of the next page of a paged operation, bound to the route that
//! issued it.
//!
//! Search, Google Scholar cited-by, and YouTube videos are the paged operations. A cursor is
//! `v1.<route>.<payload>`, and its payload restores the complete request of the page, so only the
//! route that produced the cursor may run it.
//!
//! The no-wildcard rule covers every match in this module, so a new paged operation is a compile
//! error at every site here.
#![deny(clippy::wildcard_enum_match_arm)]

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};

use crate::catalog::{PlatformOperation, ProviderId};
use crate::types::{
    CITED_BY, Platform, PlatformSearchRequest, ScholarCitedByRequest, VIDEOS, YoutubeVideosRequest,
};

const CURSOR_VERSION: &str = "v1";

/// The request of one page of a paged operation; a page cursor encodes it completely. Each
/// variant denies the other's fields, so a cursor payload decodes as exactly one operation. The
/// enum is untagged so a search payload is the bare search request.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub(crate) enum PageRequest {
    Search(PlatformSearchRequest),
    ScholarCitedBy(ScholarCitedByRequest),
    YoutubeVideos(YoutubeVideosRequest),
}

impl PageRequest {
    fn platform(&self) -> Platform {
        match self {
            Self::Search(request) => request.options.platform(),
            Self::ScholarCitedBy(_) => Platform::Scholar,
            Self::YoutubeVideos(_) => Platform::Youtube,
        }
    }

    fn operation(&self) -> &'static str {
        match self {
            Self::Search(_) => PlatformOperation::Search.as_str(),
            Self::ScholarCitedBy(_) => CITED_BY,
            Self::YoutubeVideos(_) => VIDEOS,
        }
    }
}

/// The request of one page of a paged operation.
pub(crate) trait PagedRequest: Sized {
    /// The request with its page position replaced by `page`.
    fn at_page(&self, page: String) -> Self;

    fn validate(&self) -> Result<(), String>;

    fn into_page(self) -> PageRequest;

    /// The request a page request holds, or `None` when it belongs to another operation.
    fn from_page(page: PageRequest) -> Option<Self>;
}

impl PagedRequest for PlatformSearchRequest {
    fn at_page(&self, page: String) -> Self {
        Self {
            page: Some(page),
            ..self.clone()
        }
    }

    fn validate(&self) -> Result<(), String> {
        Self::validate(self)
    }

    fn into_page(self) -> PageRequest {
        PageRequest::Search(self)
    }

    fn from_page(page: PageRequest) -> Option<Self> {
        match page {
            PageRequest::Search(request) => Some(request),
            PageRequest::ScholarCitedBy(_) | PageRequest::YoutubeVideos(_) => None,
        }
    }
}

impl PagedRequest for ScholarCitedByRequest {
    fn at_page(&self, page: String) -> Self {
        Self {
            page: Some(page),
            ..self.clone()
        }
    }

    fn validate(&self) -> Result<(), String> {
        Self::validate(self)
    }

    fn into_page(self) -> PageRequest {
        PageRequest::ScholarCitedBy(self)
    }

    fn from_page(page: PageRequest) -> Option<Self> {
        match page {
            PageRequest::ScholarCitedBy(request) => Some(request),
            PageRequest::Search(_) | PageRequest::YoutubeVideos(_) => None,
        }
    }
}

impl PagedRequest for YoutubeVideosRequest {
    fn at_page(&self, page: String) -> Self {
        Self {
            page: Some(page),
            ..self.clone()
        }
    }

    fn validate(&self) -> Result<(), String> {
        Self::validate(self)
    }

    fn into_page(self) -> PageRequest {
        PageRequest::YoutubeVideos(self)
    }

    fn from_page(page: PageRequest) -> Option<Self> {
        match page {
            PageRequest::YoutubeVideos(request) => Some(request),
            PageRequest::Search(_) | PageRequest::ScholarCitedBy(_) => None,
        }
    }
}

/// Encodes the cursor of the page `request` asks for, which only `route` may run.
pub(crate) fn encode<Q: PagedRequest>(route: ProviderId, request: Q) -> String {
    let payload =
        serde_json::to_vec(&request.into_page()).expect("a page request always serializes to JSON");
    format!(
        "{CURSOR_VERSION}.{}.{}",
        route.name(),
        URL_SAFE_NO_PAD.encode(payload)
    )
}

/// Decodes a cursor of `platform`'s `operation`: the route that issued it, and the complete
/// request of its page.
///
/// # Errors
///
/// Returns why the cursor cannot run: it cannot be decoded, another operation issued it, or its
/// request is invalid.
pub(crate) fn decode<Q: PagedRequest>(
    cursor: &str,
    platform: Platform,
    operation: &'static str,
) -> Result<(ProviderId, Q), String> {
    let (route, page) = decode_cursor(cursor)?;
    let owner = (page.platform(), page.operation());
    let request = Q::from_page(page)
        .filter(|_| owner == (platform, operation))
        .ok_or_else(|| {
            format!(
                "the cursor belongs to {} {}, not {platform} {operation}",
                owner.0, owner.1
            )
        })?;
    request
        .validate()
        .map_err(|error| format!("invalid cursor: {error}"))?;
    Ok((route, request))
}

fn decode_cursor(cursor: &str) -> Result<(ProviderId, PageRequest), String> {
    let mut parts = cursor.splitn(3, '.');
    let (Some(version), Some(route), Some(payload)) = (parts.next(), parts.next(), parts.next())
    else {
        return Err("the cursor cannot be decoded; search again without --cursor".into());
    };
    if version != CURSOR_VERSION {
        return Err(format!(
            "unsupported cursor version `{version}`; search again without --cursor"
        ));
    }
    let route = ProviderId::parse(route)
        .ok_or_else(|| format!("the cursor names unknown route `{route}`"))?;
    let request = URL_SAFE_NO_PAD
        .decode(payload)
        .ok()
        .and_then(|payload| serde_json::from_slice::<PageRequest>(&payload).ok())
        .ok_or("the cursor cannot be decoded; search again without --cursor")?;
    Ok((route, request))
}

#[cfg(test)]
mod tests {
    use super::{PageRequest, decode_cursor, encode};
    use crate::catalog::ProviderId;
    use crate::types::{ArxivSearchOptions, PlatformSearchOptions, PlatformSearchRequest};

    fn request() -> PlatformSearchRequest {
        PlatformSearchRequest {
            query: "dark matter".into(),
            limit: 5,
            options: PlatformSearchOptions::Arxiv(ArxivSearchOptions {
                categories: vec!["cs.AI".into()],
                ..ArxivSearchOptions::default()
            }),
            page: Some("10".into()),
        }
    }

    #[test]
    fn a_cursor_restores_the_route_and_the_complete_request() {
        let cursor = encode(ProviderId::ArxivApi, request());

        let decoded = decode_cursor(&cursor);

        assert_eq!(
            (cursor.starts_with("v1.arxiv_api."), decoded),
            (
                true,
                Ok((ProviderId::ArxivApi, PageRequest::Search(request())))
            )
        );
    }

    #[test]
    fn malformed_cursors_are_rejected() {
        let valid = encode(ProviderId::ArxivApi, request());
        let payload = valid.rsplit('.').next().expect("payload");
        let results = [
            "garbage".to_owned(),
            format!("v2.arxiv_api.{payload}"),
            format!("v1.unknown_route.{payload}"),
            "v1.arxiv_api.not-base64!".to_owned(),
            "v1.arxiv_api.e30".to_owned(),
        ]
        .map(|cursor| decode_cursor(&cursor).err());

        assert_eq!(
            results,
            [
                Some("the cursor cannot be decoded; search again without --cursor".into()),
                Some("unsupported cursor version `v2`; search again without --cursor".into()),
                Some("the cursor names unknown route `unknown_route`".into()),
                Some("the cursor cannot be decoded; search again without --cursor".into()),
                Some("the cursor cannot be decoded; search again without --cursor".into()),
            ]
        );
    }
}
