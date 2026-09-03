use std::sync::LazyLock;

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
    providers: &[ProviderId::Tavily, ProviderId::Firecrawl, ProviderId::Jina],
};
pub(crate) const DOCS_SEARCH: CapabilityCatalog = CapabilityCatalog {
    seam: "docs_search",
    providers: &[ProviderId::Context7, ProviderId::Exa],
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
}

impl ProviderId {
    pub(crate) const ALL: [Self; 8] = [
        Self::Xai,
        Self::OpenAiCompatible,
        Self::Exa,
        Self::Tavily,
        Self::Firecrawl,
        Self::Jina,
        Self::Context7,
        Self::Anysearch,
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
        }
    }
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
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ProviderSmokeCase {
    pub(crate) id: &'static str,
    pub(crate) operation: &'static str,
    pub(crate) transport: &'static str,
}

#[derive(Clone, Copy)]
pub(crate) struct ProviderRegistration {
    pub(crate) id: ProviderId,
    pub(crate) operations: &'static [&'static str],
    pub(crate) credentials_required: bool,
    pub(crate) probe: DoctorProbe,
    pub(crate) smoke_cases: &'static [ProviderSmokeCase],
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
    operation: "main_search",
    transport: "sse",
}];
const OPENAI_SMOKE: &[ProviderSmokeCase] = &[
    ProviderSmokeCase {
        id: "C02",
        operation: "main_search_stream_false",
        transport: "http",
    },
    ProviderSmokeCase {
        id: "C03",
        operation: "main_search_stream_true",
        transport: "sse",
    },
];
const TAVILY_SMOKE: &[ProviderSmokeCase] = &[
    ProviderSmokeCase {
        id: "C05",
        operation: "web_search",
        transport: "http",
    },
    ProviderSmokeCase {
        id: "C08",
        operation: "web_fetch",
        transport: "http",
    },
    ProviderSmokeCase {
        id: "C17",
        operation: "site_map",
        transport: "http",
    },
];
const FIRECRAWL_SMOKE: &[ProviderSmokeCase] = &[
    ProviderSmokeCase {
        id: "C06",
        operation: "web_search",
        transport: "http",
    },
    ProviderSmokeCase {
        id: "C09",
        operation: "web_fetch",
        transport: "http",
    },
];
const JINA_SMOKE: &[ProviderSmokeCase] = &[ProviderSmokeCase {
    id: "C07",
    operation: "web_fetch",
    transport: "http",
}];
const CONTEXT7_SMOKE: &[ProviderSmokeCase] = &[
    ProviderSmokeCase {
        id: "C10",
        operation: "library_resolve",
        transport: "mcp",
    },
    ProviderSmokeCase {
        id: "C11",
        operation: "docs",
        transport: "mcp",
    },
];
const EXA_SMOKE: &[ProviderSmokeCase] = &[
    ProviderSmokeCase {
        id: "C12",
        operation: "docs_search",
        transport: "http",
    },
    ProviderSmokeCase {
        id: "C13",
        operation: "similar",
        transport: "http",
    },
];
const ANYSEARCH_SMOKE: &[ProviderSmokeCase] = &[
    ProviderSmokeCase {
        id: "C14",
        operation: "academic.search",
        transport: "mcp",
    },
    ProviderSmokeCase {
        id: "C15",
        operation: "vertical_discovery",
        transport: "mcp",
    },
    ProviderSmokeCase {
        id: "C16",
        operation: "domains",
        transport: "mcp",
    },
];

const REGISTRY: &[ProviderRegistration] = &[
    ProviderRegistration {
        id: ProviderId::Xai,
        operations: &[],
        credentials_required: true,
        probe: DoctorProbe::MainSearch(XAI_PROBES),
        smoke_cases: XAI_SMOKE,
    },
    ProviderRegistration {
        id: ProviderId::OpenAiCompatible,
        operations: &[],
        credentials_required: true,
        probe: DoctorProbe::MainSearch(OPENAI_PROBES),
        smoke_cases: OPENAI_SMOKE,
    },
    ProviderRegistration {
        id: ProviderId::Tavily,
        operations: &["site_map"],
        credentials_required: true,
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
        probe: DoctorProbe::AnysearchDomains {
            name: "domains",
            transport: "mcp",
        },
        smoke_cases: ANYSEARCH_SMOKE,
    },
];

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
    let catalog_ids = CATALOGS
        .iter()
        .flat_map(|catalog| catalog.providers.iter().copied())
        .collect::<std::collections::BTreeSet<_>>();
    for registration in registry {
        if !catalog_ids.contains(&registration.id) && registration.operations.is_empty() {
            return Err(format!(
                "{} has neither a capability nor an operation",
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
        CATALOGS, DOCS_SEARCH, DoctorProbe, MAIN_SEARCH, ProviderId, REGISTRY, WEB_FETCH,
        WEB_SEARCH, registration, registrations, validate_registrations,
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
        let registry = CATALOGS
            .iter()
            .flat_map(|catalog| {
                catalog
                    .providers
                    .iter()
                    .map(move |provider| (provider.name(), catalog.seam))
            })
            .collect::<BTreeSet<_>>();
        let fixture_projection = manifest
            .transport_fixtures
            .iter()
            .map(|fixture| (fixture.provider.as_str(), fixture.seam.as_str()))
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
        assert_eq!(
            reordered
                .iter()
                .find(|registration| registration.id == ProviderId::Xai)
                .map(|registration| registration.id),
            Some(ProviderId::Xai)
        );

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
            };
            assert!(probe_is_supported, "{} probe", registration.id.name());
            assert!(
                !registration.smoke_cases.is_empty(),
                "{} smoke",
                registration.id.name()
            );
        }
    }
}
