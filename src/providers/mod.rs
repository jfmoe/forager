#[derive(Clone, Copy)]
struct ProviderRegistration {
    name: &'static str,
    capabilities: &'static [&'static str],
}

const REGISTRY: &[ProviderRegistration] = &[
    ProviderRegistration {
        name: "tavily",
        capabilities: &["web_search", "web_fetch"],
    },
    ProviderRegistration {
        name: "firecrawl",
        capabilities: &["web_search", "web_fetch"],
    },
    ProviderRegistration {
        name: "jina",
        capabilities: &["web_fetch"],
    },
    ProviderRegistration {
        name: "context7",
        capabilities: &["docs_search"],
    },
    ProviderRegistration {
        name: "exa",
        capabilities: &["docs_search"],
    },
    ProviderRegistration {
        name: "anysearch",
        capabilities: &["vertical_search"],
    },
];

pub(crate) fn supports(capability: &str, provider: &str) -> bool {
    REGISTRY.iter().any(|registration| {
        registration.name == provider && registration.capabilities.contains(&capability)
    })
}
