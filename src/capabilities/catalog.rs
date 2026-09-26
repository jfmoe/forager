use std::sync::LazyLock;
use std::time::Duration;

use crate::rate_limit::AccessPolicy;
use crate::types::Platform;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CapabilityCatalog {
    pub(crate) seam: &'static str,
    pub(crate) providers: &'static [ProviderId],
}

pub(crate) const MAIN_SEARCH: CapabilityCatalog = CapabilityCatalog {
    seam: "main_search",
    providers: &[ProviderId::Xai, ProviderId::OpenAiCompatible],
};
pub(crate) const WEB_SEARCH: CapabilityCatalog = CapabilityCatalog {
    seam: "web_search",
    providers: &[ProviderId::Tavily, ProviderId::Firecrawl],
};
pub(crate) const WEB_FETCH: CapabilityCatalog = CapabilityCatalog {
    seam: "web_fetch",
    providers: &[ProviderId::Firecrawl, ProviderId::Tavily, ProviderId::Jina],
};
pub(crate) const DOCS_SEARCH: CapabilityCatalog = CapabilityCatalog {
    seam: "docs_search",
    providers: &[ProviderId::Exa, ProviderId::Context7],
};
pub(crate) const VERTICAL_SEARCH: CapabilityCatalog = CapabilityCatalog {
    seam: "vertical_search",
    providers: &[ProviderId::Anysearch],
};

pub(crate) const CATALOGS: &[CapabilityCatalog] = &[
    MAIN_SEARCH,
    WEB_SEARCH,
    WEB_FETCH,
    DOCS_SEARCH,
    VERTICAL_SEARCH,
];

impl CapabilityCatalog {
    pub(crate) fn contains(self, id: ProviderId) -> bool {
        self.providers.contains(&id)
    }
}

pub(crate) fn by_seam(seam: &str) -> Option<CapabilityCatalog> {
    CATALOGS
        .iter()
        .copied()
        .find(|catalog| catalog.seam == seam)
}

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
    /// The default `platforms.<id>.order`. A route that users must enable themselves is a
    /// valid order entry but never appears here.
    pub(crate) default_order: &'static [ProviderId],
}

pub(crate) const ARXIV: PlatformCatalog = PlatformCatalog {
    platform: Platform::Arxiv,
    search: &[ProviderId::ArxivApi],
    fetch: &[ProviderId::ArxivApi],
    default_order: &[ProviderId::ArxivApi],
};

// `ssrn_browser` drives the user's own browser, so users must enable it themselves (ADR 0020).
pub(crate) const SSRN: PlatformCatalog = PlatformCatalog {
    platform: Platform::Ssrn,
    search: &[ProviderId::SsrnCrossref, ProviderId::SsrnBrowser],
    fetch: &[ProviderId::SsrnCrossref, ProviderId::SsrnBrowser],
    default_order: &[ProviderId::SsrnCrossref],
};

pub(crate) const PLATFORMS: &[PlatformCatalog] = &[ARXIV, SSRN];

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
        for route in self.search.iter().chain(self.fetch) {
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

pub(crate) fn platform(platform: Platform) -> PlatformCatalog {
    PLATFORMS
        .iter()
        .copied()
        .find(|catalog| catalog.platform == platform)
        .expect("every platform has a catalog")
}

pub(crate) fn supports(capability: &str, provider: &str) -> bool {
    let Some(id) = ProviderId::parse(provider) else {
        return false;
    };
    by_seam(capability).is_some_and(|catalog| catalog.contains(id))
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) enum ProviderId {
    Xai,
    OpenAiCompatible,
    Exa,
    Tavily,
    Firecrawl,
    Jina,
    Context7,
    Anysearch,
    ArxivApi,
    SsrnCrossref,
    SsrnBrowser,
}

impl ProviderId {
    pub(crate) const ALL: [Self; 11] = [
        Self::Xai,
        Self::OpenAiCompatible,
        Self::Exa,
        Self::Tavily,
        Self::Firecrawl,
        Self::Jina,
        Self::Context7,
        Self::Anysearch,
        Self::ArxivApi,
        Self::SsrnCrossref,
        Self::SsrnBrowser,
    ];

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "xai" => Some(Self::Xai),
            "openai_compatible" => Some(Self::OpenAiCompatible),
            "exa" => Some(Self::Exa),
            "tavily" => Some(Self::Tavily),
            "firecrawl" => Some(Self::Firecrawl),
            "jina" => Some(Self::Jina),
            "context7" => Some(Self::Context7),
            "anysearch" => Some(Self::Anysearch),
            "arxiv_api" => Some(Self::ArxivApi),
            "ssrn_crossref" => Some(Self::SsrnCrossref),
            "ssrn_browser" => Some(Self::SsrnBrowser),
            _ => None,
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Xai => "xai",
            Self::OpenAiCompatible => "openai_compatible",
            Self::Exa => "exa",
            Self::Tavily => "tavily",
            Self::Firecrawl => "firecrawl",
            Self::Jina => "jina",
            Self::Context7 => "context7",
            Self::Anysearch => "anysearch",
            Self::ArxivApi => "arxiv_api",
            Self::SsrnCrossref => "ssrn_crossref",
            Self::SsrnBrowser => "ssrn_browser",
        }
    }
}

/// How forager reaches a provider. Configuration checks and doctor branch on it, never on a
/// provider ID.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProviderTransport {
    /// HTTP requests to the configured `url`.
    Http,
    /// Commands of a forager-owned OpenCLI adapter, run through the configured `command`.
    OpenCli(OpenCliAdapter),
}

/// A forager-owned OpenCLI adapter: the site whose commands it installs, and the contract
/// version of the output envelope every command answers with.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct OpenCliAdapter {
    pub(crate) site: &'static str,
    pub(crate) contract: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ProbeShape {
    pub(crate) name: &'static str,
    pub(crate) transport: &'static str,
    pub(crate) stream: Option<bool>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DoctorProbe {
    MainSearch(&'static [ProbeShape]),
    WebSearch {
        name: &'static str,
        transport: &'static str,
    },
    WebFetch {
        name: &'static str,
        transport: &'static str,
    },
    DocsSearch {
        name: &'static str,
        transport: &'static str,
    },
    AnysearchDomains {
        name: &'static str,
        transport: &'static str,
    },
    PlatformSearch {
        platform: Platform,
        name: &'static str,
        transport: &'static str,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ProviderSmokeCase {
    pub(crate) id: &'static str,
    /// The platform whose operation the case exercises, for a platform route.
    pub(crate) platform: Option<Platform>,
    pub(crate) operation: &'static str,
    pub(crate) transport: &'static str,
}

#[derive(Clone, Copy)]
pub(crate) struct ProviderRegistration {
    pub(crate) id: ProviderId,
    pub(crate) operations: &'static [&'static str],
    pub(crate) credentials_required: bool,
    pub(crate) transport: ProviderTransport,
    /// The pacing every send to the provider must follow; a process route paces each OpenCLI
    /// command, not each HTTP request the browser sends.
    pub(crate) access_policy: Option<AccessPolicy>,
    pub(crate) probe: DoctorProbe,
    pub(crate) smoke_cases: &'static [ProviderSmokeCase],
}

impl ProviderRegistration {
    /// Returns whether the provider can run with `key_count` configured credentials.
    ///
    /// A provider that requires no credentials is always configured.
    pub(crate) fn is_configured(&self, key_count: usize) -> bool {
        !self.credentials_required || key_count > 0
    }
}

const XAI_PROBES: &[ProbeShape] = &[ProbeShape {
    name: "responses",
    transport: "sse",
    stream: None,
}];
const OPENAI_PROBES: &[ProbeShape] = &[
    ProbeShape {
        name: "non_stream",
        transport: "http",
        stream: Some(false),
    },
    ProbeShape {
        name: "stream",
        transport: "sse",
        stream: Some(true),
    },
];

const XAI_SMOKE: &[ProviderSmokeCase] = &[ProviderSmokeCase {
    id: "C01",
    platform: None,
    operation: "main_search",
    transport: "sse",
}];
const OPENAI_SMOKE: &[ProviderSmokeCase] = &[
    ProviderSmokeCase {
        id: "C02",
        platform: None,
        operation: "main_search_stream_false",
        transport: "http",
    },
    ProviderSmokeCase {
        id: "C03",
        platform: None,
        operation: "main_search_stream_true",
        transport: "sse",
    },
];
const TAVILY_SMOKE: &[ProviderSmokeCase] = &[
    ProviderSmokeCase {
        id: "C05",
        platform: None,
        operation: "web_search",
        transport: "http",
    },
    ProviderSmokeCase {
        id: "C08",
        platform: None,
        operation: "web_fetch",
        transport: "http",
    },
    ProviderSmokeCase {
        id: "C17",
        platform: None,
        operation: "site_map",
        transport: "http",
    },
];
const FIRECRAWL_SMOKE: &[ProviderSmokeCase] = &[
    ProviderSmokeCase {
        id: "C06",
        platform: None,
        operation: "web_search",
        transport: "http",
    },
    ProviderSmokeCase {
        id: "C09",
        platform: None,
        operation: "web_fetch",
        transport: "http",
    },
];
const JINA_SMOKE: &[ProviderSmokeCase] = &[ProviderSmokeCase {
    id: "C07",
    platform: None,
    operation: "web_fetch",
    transport: "http",
}];
const CONTEXT7_SMOKE: &[ProviderSmokeCase] = &[
    ProviderSmokeCase {
        id: "C10",
        platform: None,
        operation: "library_resolve",
        transport: "mcp",
    },
    ProviderSmokeCase {
        id: "C11",
        platform: None,
        operation: "docs",
        transport: "mcp",
    },
];
const EXA_SMOKE: &[ProviderSmokeCase] = &[
    ProviderSmokeCase {
        id: "C12",
        platform: None,
        operation: "docs_search",
        transport: "http",
    },
    ProviderSmokeCase {
        id: "C13",
        platform: None,
        operation: "similar",
        transport: "http",
    },
];
const ANYSEARCH_SMOKE: &[ProviderSmokeCase] = &[
    ProviderSmokeCase {
        id: "C14",
        platform: None,
        operation: "academic.search",
        transport: "mcp",
    },
    ProviderSmokeCase {
        id: "C15",
        platform: None,
        operation: "vertical_discovery",
        transport: "mcp",
    },
    ProviderSmokeCase {
        id: "C16",
        platform: None,
        operation: "domains",
        transport: "mcp",
    },
];

const ARXIV_API_SMOKE: &[ProviderSmokeCase] = &[
    ProviderSmokeCase {
        id: "C18",
        platform: Some(Platform::Arxiv),
        operation: "search",
        transport: "http",
    },
    ProviderSmokeCase {
        id: "C19",
        platform: Some(Platform::Arxiv),
        operation: "fetch",
        transport: "http",
    },
];

const SSRN_CROSSREF_SMOKE: &[ProviderSmokeCase] = &[
    ProviderSmokeCase {
        id: "C20",
        platform: Some(Platform::Ssrn),
        operation: "search",
        transport: "http",
    },
    ProviderSmokeCase {
        id: "C21",
        platform: Some(Platform::Ssrn),
        operation: "fetch",
        transport: "http",
    },
];

const SSRN_BROWSER_SMOKE: &[ProviderSmokeCase] = &[
    ProviderSmokeCase {
        id: "C22",
        platform: Some(Platform::Ssrn),
        operation: "search",
        transport: "process",
    },
    ProviderSmokeCase {
        id: "C23",
        platform: Some(Platform::Ssrn),
        operation: "fetch",
        transport: "process",
    },
];

// arXiv terms of use allow one request every three seconds over one connection.
const ARXIV_API_ACCESS: AccessPolicy = AccessPolicy {
    min_interval: Duration::from_secs(3),
    max_concurrency: 1,
};

// The anonymous Crossref pool announced 5 requests per second over one connection on
// 2026-09-26; one request per second stays well inside it.
const SSRN_CROSSREF_ACCESS: AccessPolicy = AccessPolicy {
    min_interval: Duration::from_secs(1),
    max_concurrency: 1,
};

// One browser operation every five seconds keeps the pace close to a person's (ADR 0020).
const SSRN_BROWSER_ACCESS: AccessPolicy = AccessPolicy {
    min_interval: Duration::from_secs(5),
    max_concurrency: 1,
};

const REGISTRY: &[ProviderRegistration] = &[
    ProviderRegistration {
        id: ProviderId::Xai,
        operations: &[],
        credentials_required: true,
        transport: ProviderTransport::Http,
        access_policy: None,
        probe: DoctorProbe::MainSearch(XAI_PROBES),
        smoke_cases: XAI_SMOKE,
    },
    ProviderRegistration {
        id: ProviderId::OpenAiCompatible,
        operations: &[],
        credentials_required: true,
        transport: ProviderTransport::Http,
        access_policy: None,
        probe: DoctorProbe::MainSearch(OPENAI_PROBES),
        smoke_cases: OPENAI_SMOKE,
    },
    ProviderRegistration {
        id: ProviderId::Tavily,
        operations: &["site_map"],
        credentials_required: true,
        transport: ProviderTransport::Http,
        access_policy: None,
        probe: DoctorProbe::WebSearch {
            name: "search",
            transport: "http",
        },
        smoke_cases: TAVILY_SMOKE,
    },
    ProviderRegistration {
        id: ProviderId::Firecrawl,
        operations: &[],
        credentials_required: true,
        transport: ProviderTransport::Http,
        access_policy: None,
        probe: DoctorProbe::WebSearch {
            name: "search",
            transport: "http",
        },
        smoke_cases: FIRECRAWL_SMOKE,
    },
    ProviderRegistration {
        id: ProviderId::Jina,
        operations: &[],
        credentials_required: true,
        transport: ProviderTransport::Http,
        access_policy: None,
        probe: DoctorProbe::WebFetch {
            name: "fetch",
            transport: "http",
        },
        smoke_cases: JINA_SMOKE,
    },
    ProviderRegistration {
        id: ProviderId::Context7,
        operations: &[],
        credentials_required: true,
        transport: ProviderTransport::Http,
        access_policy: None,
        probe: DoctorProbe::DocsSearch {
            name: "library",
            transport: "mcp",
        },
        smoke_cases: CONTEXT7_SMOKE,
    },
    ProviderRegistration {
        id: ProviderId::Exa,
        operations: &["similar"],
        credentials_required: true,
        transport: ProviderTransport::Http,
        access_policy: None,
        probe: DoctorProbe::DocsSearch {
            name: "search",
            transport: "http",
        },
        smoke_cases: EXA_SMOKE,
    },
    ProviderRegistration {
        id: ProviderId::Anysearch,
        operations: &["search", "domains"],
        credentials_required: true,
        transport: ProviderTransport::Http,
        access_policy: None,
        probe: DoctorProbe::AnysearchDomains {
            name: "domains",
            transport: "mcp",
        },
        smoke_cases: ANYSEARCH_SMOKE,
    },
    ProviderRegistration {
        id: ProviderId::ArxivApi,
        operations: &[],
        credentials_required: false,
        transport: ProviderTransport::Http,
        access_policy: Some(ARXIV_API_ACCESS),
        probe: DoctorProbe::PlatformSearch {
            platform: Platform::Arxiv,
            name: "search",
            transport: "http",
        },
        smoke_cases: ARXIV_API_SMOKE,
    },
    ProviderRegistration {
        id: ProviderId::SsrnCrossref,
        operations: &[],
        credentials_required: false,
        transport: ProviderTransport::Http,
        access_policy: Some(SSRN_CROSSREF_ACCESS),
        probe: DoctorProbe::PlatformSearch {
            platform: Platform::Ssrn,
            name: "search",
            transport: "http",
        },
        smoke_cases: SSRN_CROSSREF_SMOKE,
    },
    ProviderRegistration {
        id: ProviderId::SsrnBrowser,
        operations: &[],
        credentials_required: false,
        transport: ProviderTransport::OpenCli(OpenCliAdapter {
            site: "ssrn",
            contract: "forager-ssrn/1",
        }),
        access_policy: Some(SSRN_BROWSER_ACCESS),
        probe: DoctorProbe::PlatformSearch {
            platform: Platform::Ssrn,
            name: "search",
            transport: "process",
        },
        smoke_cases: SSRN_BROWSER_SMOKE,
    },
];

/// Returns whether a registration is legitimate: it belongs to a capability catalog or a
/// platform catalog, or it owns an operation.
pub(crate) fn has_owner(registration: &ProviderRegistration) -> bool {
    CATALOGS
        .iter()
        .any(|catalog| catalog.contains(registration.id))
        || PLATFORMS
            .iter()
            .any(|platform| platform.contains(registration.id))
        || !registration.operations.is_empty()
}

static VALIDATED_REGISTRY: LazyLock<()> = LazyLock::new(|| {
    if let Err(error) = validate_registrations(REGISTRY) {
        panic!("invalid provider registry: {error}");
    }
});

pub(crate) fn registrations() -> &'static [ProviderRegistration] {
    LazyLock::force(&VALIDATED_REGISTRY);
    REGISTRY
}

pub(crate) fn registration(id: ProviderId) -> &'static ProviderRegistration {
    registrations()
        .iter()
        .find(|registration| registration.id == id)
        .expect("validated registry contains every provider ID")
}

fn validate_registrations(registry: &[ProviderRegistration]) -> Result<(), String> {
    let ids = registry
        .iter()
        .map(|registration| registration.id)
        .collect::<std::collections::BTreeSet<_>>();
    let expected = ProviderId::ALL
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
    if registry.len() != expected.len() || ids != expected {
        return Err("provider IDs must appear exactly once".into());
    }
    for registration in registry {
        if !has_owner(registration) {
            return Err(format!(
                "{} belongs to no capability or platform and owns no operation",
                registration.id.name()
            ));
        }
    }
    let smoke_ids = registry
        .iter()
        .flat_map(|registration| registration.smoke_cases.iter().map(|case| case.id))
        .collect::<Vec<_>>();
    let unique_smoke_ids = smoke_ids
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    if smoke_ids.len() != unique_smoke_ids.len() {
        return Err("provider smoke case IDs must be unique".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use serde::Deserialize;

    use super::{
        CATALOGS, DOCS_SEARCH, DoctorProbe, MAIN_SEARCH, PLATFORMS, PlatformOperation, ProviderId,
        ProviderRegistration, REGISTRY, WEB_FETCH, WEB_SEARCH, platform, registration,
        registrations, validate_registrations,
    };

    #[derive(Deserialize)]
    struct AcceptanceManifest {
        transport_fixtures: Vec<TransportFixture>,
    }

    #[derive(Deserialize)]
    struct TransportFixture {
        provider: String,
        seam: String,
        test: String,
    }

    #[test]
    fn provider_fixture_projection_matches_transport_manifest() {
        let manifest: AcceptanceManifest = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/acceptance-manifest.json"
        )))
        .expect("acceptance manifest");
        let capability_projection = CATALOGS.iter().flat_map(|catalog| {
            catalog
                .providers
                .iter()
                .map(move |provider| (provider.name().to_owned(), catalog.seam.to_owned()))
        });
        let platform_projection = PLATFORMS.iter().flat_map(|catalog| {
            PlatformOperation::ALL
                .into_iter()
                .flat_map(move |operation| {
                    catalog.routes(operation).iter().map(move |route| {
                        (
                            route.name().to_owned(),
                            format!("platform:{}:{}", catalog.platform, operation.as_str()),
                        )
                    })
                })
        });
        let registry = capability_projection
            .chain(platform_projection)
            .collect::<BTreeSet<_>>();
        let fixture_projection = manifest
            .transport_fixtures
            .iter()
            .map(|fixture| (fixture.provider.clone(), fixture.seam.clone()))
            .collect::<BTreeSet<_>>();

        assert_eq!(fixture_projection, registry);
        for fixture in manifest.transport_fixtures {
            assert!(
                !fixture.test.trim().is_empty(),
                "{} / {} lacks a fixture test",
                fixture.provider,
                fixture.seam
            );
        }
    }

    #[test]
    fn registration_lookup_is_identifier_based_and_rejects_missing_or_duplicate_ids() {
        let mut reordered = REGISTRY.to_vec();
        reordered.swap(0, 6);
        assert!(validate_registrations(&reordered).is_ok());

        let mut misaligned = reordered;
        misaligned[0] = misaligned[1];

        assert!(validate_registrations(&misaligned).is_err());
        for id in ProviderId::ALL {
            assert_eq!(registration(id).id, id);
        }
    }

    #[test]
    fn catalogs_project_every_registration_probe_and_smoke_case_consistently() {
        let catalog_ids = CATALOGS
            .iter()
            .flat_map(|catalog| catalog.providers.iter().copied())
            .chain(PLATFORMS.iter().flat_map(|catalog| catalog.all_routes()))
            .collect::<BTreeSet<_>>();
        let registration_ids = registrations()
            .iter()
            .map(|registration| registration.id)
            .collect::<BTreeSet<_>>();
        assert_eq!(catalog_ids, registration_ids);

        for registration in registrations() {
            let probe_is_supported = match registration.probe {
                DoctorProbe::MainSearch(_) => MAIN_SEARCH.contains(registration.id),
                DoctorProbe::WebSearch { .. } => WEB_SEARCH.contains(registration.id),
                DoctorProbe::WebFetch { .. } => WEB_FETCH.contains(registration.id),
                DoctorProbe::DocsSearch { .. } => DOCS_SEARCH.contains(registration.id),
                DoctorProbe::AnysearchDomains { .. } => registration.id == ProviderId::Anysearch,
                DoctorProbe::PlatformSearch {
                    platform: probed, ..
                } => platform(probed).search.contains(&registration.id),
            };
            assert!(probe_is_supported, "{} probe", registration.id.name());
            assert!(
                !registration.smoke_cases.is_empty(),
                "{} smoke",
                registration.id.name()
            );
        }
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
    fn registration_without_credentials_is_configured_without_keys() {
        let anonymous = ProviderRegistration {
            credentials_required: false,
            ..*registration(ProviderId::Jina)
        };

        assert!(anonymous.is_configured(0));
    }

    #[test]
    fn registration_with_credentials_requires_at_least_one_key() {
        let keyed = registration(ProviderId::Jina);

        assert_eq!(
            (keyed.is_configured(0), keyed.is_configured(1)),
            (false, true)
        );
    }

    #[test]
    fn registry_validation_accepts_a_route_that_only_a_platform_catalog_lists() {
        let arxiv_api = registration(ProviderId::ArxivApi);

        assert_eq!(
            (
                CATALOGS
                    .iter()
                    .any(|catalog| catalog.contains(ProviderId::ArxivApi)),
                arxiv_api.operations.is_empty(),
                validate_registrations(REGISTRY),
            ),
            (false, true, Ok(()))
        );
    }

    #[test]
    fn registry_validation_accepts_registrations_without_credentials() {
        let mut registry = REGISTRY.to_vec();
        registry[0].credentials_required = false;

        assert!(validate_registrations(&registry).is_ok());
    }
}
