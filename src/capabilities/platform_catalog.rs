//! The platform half of the Capability Catalog: which routes serve each platform and its
//! search and fetch operations, each platform's default order, and how users enable a route
//! that is off by default.

use super::ProviderId;
use crate::types::Platform;

/// A platform operation that every platform provides through its own route set.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PlatformOperation {
    Search,
    Fetch,
}

impl PlatformOperation {
    #[cfg(test)]
    pub(crate) const ALL: [Self; 2] = [Self::Search, Self::Fetch];

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Search => "search",
            Self::Fetch => "fetch",
        }
    }
}

/// The only source of which routes serve a platform and each of its operations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PlatformCatalog {
    pub(crate) platform: Platform,
    pub(crate) search: &'static [ProviderId],
    pub(crate) fetch: &'static [ProviderId],
    /// Routes that serve neither search nor fetch, only operations of the platform's own, such
    /// as a model that answers about a video. They are valid order entries like any route.
    pub(crate) operation_only: &'static [ProviderId],
    /// The default `platforms.<id>.order`. A route that users must enable themselves is a
    /// valid order entry but never appears here.
    pub(crate) default_order: &'static [ProviderId],
}

pub(crate) const ARXIV: PlatformCatalog = PlatformCatalog {
    platform: Platform::Arxiv,
    search: &[ProviderId::ArxivApi],
    fetch: &[ProviderId::ArxivApi],
    operation_only: &[],
    default_order: &[ProviderId::ArxivApi],
};

// `ssrn_browser` drives the user's own browser, so users must enable it themselves (ADR 0020).
pub(crate) const SSRN: PlatformCatalog = PlatformCatalog {
    platform: Platform::Ssrn,
    search: &[ProviderId::SsrnCrossref, ProviderId::SsrnBrowser],
    fetch: &[ProviderId::SsrnCrossref, ProviderId::SsrnBrowser],
    operation_only: &[],
    default_order: &[ProviderId::SsrnCrossref],
};

// Google Scholar has no public API; SerpApi is its only route (ADR 0021).
pub(crate) const SCHOLAR: PlatformCatalog = PlatformCatalog {
    platform: Platform::Scholar,
    search: &[ProviderId::Serpapi],
    fetch: &[ProviderId::Serpapi],
    operation_only: &[],
    default_order: &[ProviderId::Serpapi],
};

// Xiaohongshu has no public API and forbids crawlers, so its only route reads the pages of the
// user's own logged-in Chrome and stays out of every default order (ADR 0020).
pub(crate) const XIAOHONGSHU: PlatformCatalog = PlatformCatalog {
    platform: Platform::Xiaohongshu,
    search: &[ProviderId::XiaohongshuBrowser],
    fetch: &[ProviderId::XiaohongshuBrowser],
    operation_only: &[],
    default_order: &[],
};

// xAI's `x_search` tool reads X for the model, which relays what it read; the route shares the
// main search provider's configuration and credentials (ADR 0026).
pub(crate) const X: PlatformCatalog = PlatformCatalog {
    platform: Platform::X,
    search: &[ProviderId::Xai],
    fetch: &[ProviderId::Xai],
    operation_only: &[],
    default_order: &[ProviderId::Xai],
};

// One route combines Algolia's HN Search API with the official Firebase API (ADR 0027).
pub(crate) const HN: PlatformCatalog = PlatformCatalog {
    platform: Platform::Hn,
    search: &[ProviderId::HnApi],
    fetch: &[ProviderId::HnApi],
    operation_only: &[],
    default_order: &[ProviderId::HnApi],
};

// The official Data API needs the user's own key; one key per client, never a pool, because a
// pool would sidestep the project quota the Developer Policies bind to one project.
// `youtube_web` reads YouTube's undocumented web endpoints, so users must enable it themselves
// (ADR 0028). `gemini_api` serves only Ask and Transcribe: a model watches the video.
pub(crate) const YOUTUBE: PlatformCatalog = PlatformCatalog {
    platform: Platform::Youtube,
    search: &[ProviderId::YoutubeApi],
    fetch: &[ProviderId::YoutubeApi, ProviderId::YoutubeWeb],
    operation_only: &[ProviderId::GeminiApi],
    default_order: &[ProviderId::YoutubeApi, ProviderId::GeminiApi],
};

/// What `youtube_web` reads and what enabling it risks.
const YOUTUBE_WEB_RISK: &str = "youtube_web reads YouTube's undocumented web endpoints anonymously, which YouTube's terms do not allow, so it is off by default; its risk is a temporary block of your egress IP, never your account";

pub(crate) const PLATFORMS: &[PlatformCatalog] =
    &[ARXIV, SSRN, SCHOLAR, XIAOHONGSHU, X, HN, YOUTUBE];

impl PlatformCatalog {
    pub(crate) fn routes(self, operation: PlatformOperation) -> &'static [ProviderId] {
        match operation {
            PlatformOperation::Search => self.search,
            PlatformOperation::Fetch => self.fetch,
        }
    }

    /// Returns every route of the platform once, in declaration order.
    pub(crate) fn all_routes(self) -> Vec<ProviderId> {
        let mut routes = Vec::new();
        for route in self
            .search
            .iter()
            .chain(self.fetch)
            .chain(self.operation_only)
        {
            if !routes.contains(route) {
                routes.push(*route);
            }
        }
        routes
    }

    pub(crate) fn contains(self, id: ProviderId) -> bool {
        self.all_routes().contains(&id)
    }
}

/// Says what an HTTP platform route that users must enable themselves risks and how to enable
/// it, or `None` for any other provider. `order` is the current value of `order_key`: the
/// suggested order keeps it and places the route right after its last fetch route, so routes
/// that only serve other operations keep working.
pub(crate) fn http_opt_in(
    route: ProviderId,
    order_key: &str,
    order: &[ProviderId],
) -> Option<String> {
    if route != ProviderId::YoutubeWeb {
        return None;
    }
    let mut suggested = order.to_vec();
    let position = order
        .iter()
        .rposition(|id| YOUTUBE.fetch.contains(id))
        .map_or(0, |index| index + 1);
    suggested.insert(position, route);
    let names = suggested
        .iter()
        .map(|id| format!("\"{}\"", id.name()))
        .collect::<Vec<_>>()
        .join(", ");
    Some(format!(
        "{YOUTUBE_WEB_RISK}. To enable it, run `forager config set {order_key} '[{names}]'`"
    ))
}

pub(crate) fn platform(platform: Platform) -> PlatformCatalog {
    PLATFORMS
        .iter()
        .copied()
        .find(|catalog| catalog.platform == platform)
        .expect("every platform has a catalog")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{PLATFORMS, ProviderId, http_opt_in};

    fn suggested_command(order: &[ProviderId]) -> String {
        let hint = http_opt_in(ProviderId::YoutubeWeb, "platforms.youtube.order", order)
            .expect("youtube_web is opt-in");
        let (_, command) = hint
            .rsplit_once("run ")
            .unwrap_or_else(|| panic!("the hint names a command: {hint}"));
        command.to_owned()
    }

    #[test]
    fn every_default_platform_order_lists_unique_routes_of_its_platform() {
        for catalog in PLATFORMS {
            let unique = catalog.default_order.iter().collect::<BTreeSet<_>>();

            assert!(
                catalog
                    .default_order
                    .iter()
                    .all(|route| catalog.contains(*route))
                    && unique.len() == catalog.default_order.len(),
                "{} default order {:?}",
                catalog.platform,
                catalog.default_order
            );
        }
    }

    #[test]
    fn the_youtube_web_hint_keeps_the_order_and_adds_the_route_after_the_fetch_routes() {
        assert_eq!(
            suggested_command(&[ProviderId::YoutubeApi, ProviderId::GeminiApi]),
            r#"`forager config set platforms.youtube.order '["youtube_api", "youtube_web", "gemini_api"]'`"#
        );
    }

    #[test]
    fn the_youtube_web_hint_leads_an_order_without_fetch_routes() {
        assert_eq!(
            suggested_command(&[ProviderId::GeminiApi]),
            r#"`forager config set platforms.youtube.order '["youtube_web", "gemini_api"]'`"#
        );
    }
}
