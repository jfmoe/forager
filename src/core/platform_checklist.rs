//! Checks what the compiler cannot see in a platform and its routes.
//!
//! The compiler names every dispatch site a new route must handle, but not whether facts declared
//! in separate places agree: the platform catalog, the provider registration, the route
//! declaration and the route built from it, and the configuration schema, nor whether a platform
//! and its routes have the samples, tests and live cases they need. Each [`Rule`] states one such
//! fact and what to add when it breaks.

use crate::catalog::{
    self, Credentials, PLATFORMS, PlatformCatalog, PlatformOperation, ProviderId,
    ProviderRegistration, ProviderTransport,
};
use crate::config;
use crate::providers;
use crate::types::{
    AccessToken, CITED_BY, ContentDepth, Platform, PlatformFetchRequest, PlatformRef,
    PlatformSearchOptions, PlatformSearchRequest,
};

/// One item of the integration checklist for a platform and its routes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Rule {
    Catalog,
    Registration,
    Factory,
    Declaration,
    BrowserEnvironment,
    ConfigLeaves,
    SmokeCase,
    SampleRefs,
    AccessWindowTest,
    ContractLayout,
}

impl Rule {
    /// The rule, then what to add to satisfy it.
    fn text(self) -> (&'static str, &'static str) {
        match self {
            Self::Catalog => (
                "the platform catalog lists every platform with a non-empty route set for search and for fetch",
                "add the platform and its routes to the platform catalog",
            ),
            Self::Registration => (
                "no route's provider ID is the bare platform name",
                "register the route in the provider registry under a name such as `<platform>_api` or `<platform>_browser`",
            ),
            Self::Factory => (
                "the provider factory builds every route of every operation: the route's declaration checks the operation's requests without sending, and a route configuration names the route",
                "give the route's declaration a support check for the operation, and add the route to the platform route runtime configuration and to its route configuration",
            ),
            Self::Declaration => (
                "what each route's declaration says it serves of search and fetch, and whether its registration declares an account check, is what the route built from it hands out",
                "make the route's declared support checks and account check agree with its `PlatformRoute` implementation",
            ),
            Self::BrowserEnvironment => (
                "a browser route, which needs a login into its platform's Browser Profile, has login Page Logic",
                "give the route's declaration the platform's login Page Logic",
            ),
            Self::ConfigLeaves => (
                "the configuration schema has the platform's order and each route's leaves: `url` and `timeout` for an HTTP route (`url` alone for a main-search backend, whose platform requests use the command's remaining time), `timeout` for a browser route, and `keys` exactly when the route reads credentials",
                "add the leaves to the configuration schema and to the runtime projection",
            ),
            Self::SmokeCase => (
                "each platform operation has a route that registers a live smoke case",
                "register a smoke case for the operation in a route's registration, under the next unused case number",
            ),
            Self::SampleRefs => (
                "the sample refs cover every kind of the platform, each round-trips through its canonical URL, and the first is a ref a fetch route of the platform accepts",
                "add or reorder the platform's refs in `sample_refs` in this test",
            ),
            Self::AccessWindowTest => (
                "every HTTP route with an Access Policy has a send in the access-window test",
                "add a send of the route to `SENDS` in `tests/command_contract/access_windows.rs`",
            ),
            Self::ContractLayout => (
                "the command contract tests of a platform with more than one route are grouped by route: every test is in a module `platforms::<platform>::<route>` of the command contract tests, named by one of its routes' provider IDs",
                "move the platform's command modules into a module per route, named by the route's provider ID",
            ),
        }
    }
}

/// One rule a platform breaks, and what it misses.
#[derive(Debug)]
struct Violation {
    platform: Platform,
    rule: Rule,
    detail: String,
}

impl std::fmt::Display for Violation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (rule, fix) = self.rule.text();
        write!(
            formatter,
            "{}: {}. Rule: {rule}. Add: {fix}.",
            self.platform, self.detail
        )
    }
}

/// Refs of every kind of each platform; the exhaustive match makes a new platform add samples.
fn sample_refs(platform: Platform) -> &'static [&'static str] {
    match platform {
        Platform::Arxiv => &[
            "arxiv:2401.01234v2",
            "arxiv:2401.01234",
            "arxiv:hep-th/9901001v1",
        ],
        Platform::Ssrn => &["ssrn:2042750"],
        Platform::Scholar => &["scholar:18208131694456651388"],
        Platform::Xiaohongshu => &["xiaohongshu:64a1b2c3d4e5f60718293a4b"],
        Platform::X => &["x:2110126570233094527"],
        Platform::Hn => &["hn:8863"],
        Platform::Youtube => &[
            "youtube:jNQXAC9IVRw",
            "youtube:playlist/PLZHQObOWTQDPD3MizzM2xVFitgF8hE_ab",
            "youtube:channel/UCYO_jab_esuFRV4b17AJtAw",
        ],
        Platform::Polymarket => &[
            "polymarket:presidential-election-winner-2024",
            "polymarket:market/will-donald-trump-win-the-2024-us-presidential-election",
        ],
        Platform::Reddit => &["reddit:17snc7d"],
    }
}

/// Everything the checklist inspects; the negative tests replace one part at a time.
struct Registry<'a> {
    platforms: &'a [PlatformCatalog],
    registrations: &'a [ProviderRegistration],
    has_adapter: &'a dyn Fn(Platform, PlatformOperation, ProviderId) -> bool,
    /// For each of search and fetch: whether the route's declaration checks the operation's
    /// requests, and whether the route built from the default configuration hands it out.
    /// `None` for a provider without a route configuration.
    route_operations: &'a dyn Fn(ProviderId) -> Option<[(bool, bool); 2]>,
    /// Whether the route built from the default configuration checks the account of each key.
    /// `None` for a provider without a route configuration.
    checks_accounts: &'a dyn Fn(ProviderId) -> Option<bool>,
    is_config_leaf: &'a dyn Fn(&str) -> bool,
    /// Whether the factory provides the login page logic of a platform's browser route.
    has_login: &'a dyn Fn(Platform) -> bool,
    sample_refs: &'a dyn Fn(Platform) -> &'static [&'static str],
    has_window_test: &'a dyn Fn(ProviderId) -> bool,
    /// The command contract modules that hold the platform's tests, as paths such as
    /// `platforms::youtube::youtube_web`.
    contract_modules: &'a dyn Fn(Platform) -> Vec<String>,
}

fn violation(platform: Platform, rule: Rule, detail: &str) -> Violation {
    Violation {
        platform,
        rule,
        detail: detail.to_owned(),
    }
}

fn violations(platform: Platform, registry: &Registry<'_>) -> Vec<Violation> {
    let mut found = Vec::new();
    let Some(catalog) = registry
        .platforms
        .iter()
        .find(|catalog| catalog.platform == platform)
    else {
        return vec![violation(
            platform,
            Rule::Catalog,
            "no platform catalog lists it",
        )];
    };
    for operation in PlatformOperation::ALL {
        if catalog.routes(operation).is_empty() {
            found.push(violation(
                platform,
                Rule::Catalog,
                &format!("its catalog has no {} route", operation.as_str()),
            ));
        }
    }
    let order_key = config::platform_order_key(platform);
    if !(registry.is_config_leaf)(&order_key) {
        found.push(violation(
            platform,
            Rule::ConfigLeaves,
            &format!("the configuration schema lacks `{order_key}`"),
        ));
    }
    let routes = catalog.all_routes();
    for route in &routes {
        check_route(platform, *route, registry, &mut found);
    }
    if routes.len() > 1 {
        for module in (registry.contract_modules)(platform) {
            let by_route = |route: &ProviderId| {
                module == format!("platforms::{}::{}", platform.as_str(), route.name())
            };
            if !routes.iter().any(by_route) {
                found.push(violation(
                    platform,
                    Rule::ContractLayout,
                    &format!(
                        "its contract tests in `{module}` are not in a module named after one of its routes"
                    ),
                ));
            }
        }
    }
    for operation in PlatformOperation::ALL {
        check_operation(
            platform,
            operation,
            catalog.routes(operation),
            registry,
            &mut found,
        );
    }
    for operation in catalog.operations {
        check_smoke_case(platform, operation.operation, registry, &mut found);
    }
    check_refs(
        platform,
        (registry.sample_refs)(platform),
        catalog.routes(PlatformOperation::Fetch),
        &mut found,
    );
    found
}

fn check_operation(
    platform: Platform,
    operation: PlatformOperation,
    routes: &[ProviderId],
    registry: &Registry<'_>,
    found: &mut Vec<Violation>,
) {
    let name = operation.as_str();
    for route in routes {
        if !(registry.has_adapter)(platform, operation, *route) {
            found.push(violation(
                platform,
                Rule::Factory,
                &format!(
                    "the platform factory cannot build {name} route `{}`",
                    route.name()
                ),
            ));
        }
    }
    check_smoke_case(platform, name, registry, found);
}

fn check_smoke_case(
    platform: Platform,
    operation: &str,
    registry: &Registry<'_>,
    found: &mut Vec<Violation>,
) {
    let has_smoke_case = registry.registrations.iter().any(|registration| {
        registration
            .smoke_cases
            .iter()
            .any(|case| case.platform == Some(platform) && case.operation == operation)
    });
    if !has_smoke_case {
        found.push(violation(
            platform,
            Rule::SmokeCase,
            &format!("no route registers a {operation} smoke case"),
        ));
    }
}

fn check_route(
    platform: Platform,
    route: ProviderId,
    registry: &Registry<'_>,
    found: &mut Vec<Violation>,
) {
    let name = route.name();
    if name == platform.as_str() {
        found.push(violation(
            platform,
            Rule::Registration,
            &format!("route `{name}` reuses the bare platform name"),
        ));
    }
    let registration = registry
        .registrations
        .iter()
        .find(|registration| registration.id == route)
        .expect("the provider registry validates that every provider ID is registered");
    check_declaration(platform, registration, registry, found);
    let leaves: &[&str] = match registration.transport {
        // A main-search backend has no timeout of its own; its platform requests use the
        // command's remaining time like main search (ADR-0026).
        ProviderTransport::Http if catalog::MAIN_SEARCH.contains(route) => &["url"],
        ProviderTransport::Http => &["url", "timeout"],
        ProviderTransport::Browser(_) => &["timeout"],
    };
    if matches!(registration.transport, ProviderTransport::Browser(_))
        && !(registry.has_login)(platform)
    {
        found.push(violation(
            platform,
            Rule::BrowserEnvironment,
            &format!("the declaration of browser route `{name}` has no login page logic"),
        ));
    }
    for leaf in leaves {
        let path = format!("providers.{name}.{leaf}");
        if !(registry.is_config_leaf)(&path) {
            found.push(violation(
                platform,
                Rule::ConfigLeaves,
                &format!("the configuration schema lacks `{path}`"),
            ));
        }
    }
    let keys = config::provider_keys_key(name);
    let reads_keys = registration.credentials != Credentials::Unused;
    if (registry.is_config_leaf)(&keys) != reads_keys {
        found.push(violation(
            platform,
            Rule::ConfigLeaves,
            &format!("`{keys}` must exist exactly when the route reads credentials ({reads_keys})"),
        ));
    }
    if registration.access_policy.is_some()
        && registration.transport == ProviderTransport::Http
        && !(registry.has_window_test)(route)
    {
        found.push(violation(
            platform,
            Rule::AccessWindowTest,
            &format!(
                "HTTP route `{name}` has an Access Policy and no send in the access-window test"
            ),
        ));
    }
}

/// Compares what the route's declaration and registration say it serves with what the route
/// built from it hands out: search, fetch, and the account check.
fn check_declaration(
    platform: Platform,
    registration: &ProviderRegistration,
    registry: &Registry<'_>,
    found: &mut Vec<Violation>,
) {
    let route = registration.id;
    let name = route.name();
    let declares_accounts = registration.account_check.is_some();
    if let Some(checks_accounts) = (registry.checks_accounts)(route)
        && checks_accounts != declares_accounts
    {
        found.push(violation(
            platform,
            Rule::Declaration,
            &format!(
                "the registration of route `{name}` {} an account check, but the route built from it {} the accounts of its keys",
                if declares_accounts { "declares" } else { "declares no" },
                if checks_accounts { "checks" } else { "does not check" },
            ),
        ));
    }
    if let Some(operations) = (registry.route_operations)(route) {
        for (operation, (declared, built)) in PlatformOperation::ALL.into_iter().zip(operations) {
            if declared != built {
                let operation = operation.as_str();
                found.push(violation(
                    platform,
                    Rule::Declaration,
                    &format!(
                        "the declaration of route `{name}` {} {operation} requests, but the route built from it {} {operation}",
                        if declared { "checks" } else { "checks no" },
                        if built { "serves" } else { "does not serve" },
                    ),
                ));
            }
        }
    }
}

fn check_refs(
    platform: Platform,
    samples: &[&str],
    fetch_routes: &[ProviderId],
    found: &mut Vec<Violation>,
) {
    let first_is_fetched = samples
        .first()
        .and_then(|sample| PlatformRef::parse(platform, sample).ok())
        .is_some_and(|reference| {
            [
                ContentDepth::Metadata,
                ContentDepth::Abstract,
                ContentDepth::FullText,
            ]
            .into_iter()
            .any(|depth| {
                // A route that needs an access token gets one; the others ignore it.
                let request = PlatformFetchRequest {
                    access: AccessToken::new("token"),
                    ..PlatformFetchRequest::new(reference.clone(), depth)
                };
                fetch_routes.iter().any(|route| {
                    providers::platform_fetch_support(*route, &request)
                        .is_some_and(|support| support.is_ok())
                })
            })
        });
    if !first_is_fetched {
        found.push(violation(
            platform,
            Rule::SampleRefs,
            "no fetch route of the platform accepts its first sample ref",
        ));
    }
    for kind in platform.kinds() {
        let covered = samples.iter().any(|sample| {
            PlatformRef::parse(platform, sample).is_ok_and(|reference| reference.kind() == *kind)
        });
        if !covered {
            found.push(violation(
                platform,
                Rule::SampleRefs,
                &format!("no sample ref covers its `{kind}` kind"),
            ));
        }
    }
    for sample in samples {
        let round_trips = PlatformRef::parse(platform, sample).is_ok_and(|reference| {
            reference.to_string() == *sample
                && PlatformRef::parse(platform, &reference.canonical_url()).ok() == Some(reference)
        });
        if !round_trips {
            found.push(violation(
                platform,
                Rule::SampleRefs,
                &format!("ref `{sample}` does not round-trip through its canonical URL"),
            ));
        }
    }
}

/// The factory covers a route when it has a support check for the operation and a route
/// configuration that names the route, which is what `build_platform_search` and
/// `build_platform_fetch` construct from.
fn has_adapter(platform: Platform, operation: PlatformOperation, route: ProviderId) -> bool {
    let has_support = match operation {
        PlatformOperation::Search => {
            let request = PlatformSearchRequest {
                query: String::new(),
                limit: 1,
                options: PlatformSearchOptions::defaults(platform),
                page: None,
            };
            providers::platform_search_support(route, &request).is_some()
        }
        PlatformOperation::Fetch => sample_refs(platform)
            .iter()
            .find_map(|sample| PlatformRef::parse(platform, sample).ok())
            .is_some_and(|reference| {
                let request = PlatformFetchRequest::new(reference, ContentDepth::Abstract);
                providers::platform_fetch_support(route, &request).is_some()
            }),
    };
    let routes = config::default_platform_routes();
    has_support
        && config::platform_route_config(route, &routes)
            .is_some_and(|route_config| route_config.route() == route)
}

fn has_window_test(route: ProviderId) -> bool {
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/command_contract/access_windows.rs"
    ))
    .contains(&format!("\"{}\"", route.name()))
}

/// The platform's own module when its file holds tests, then each module under it.
fn contract_modules(platform: Platform) -> Vec<String> {
    let platforms =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/command_contract/platforms");
    let name = platform.as_str();
    let module = format!("platforms::{name}");
    let holds_tests = |path: std::path::PathBuf| {
        std::fs::read_to_string(path).is_ok_and(|text| text.contains("#[test]"))
    };
    let mut modules = Vec::new();
    if holds_tests(platforms.join(format!("{name}.rs")))
        || holds_tests(platforms.join(name).join("mod.rs"))
    {
        modules.push(module.clone());
    }
    if let Ok(entries) = std::fs::read_dir(platforms.join(name)) {
        for entry in entries {
            let path = entry.expect("read a contract module entry").path();
            let stem = path.file_stem().expect("a contract module name");
            if stem != "mod" {
                modules.push(format!("{module}::{}", stem.to_string_lossy()));
            }
        }
    }
    modules
}

fn baseline() -> Registry<'static> {
    Registry {
        platforms: PLATFORMS,
        registrations: catalog::registrations(),
        has_adapter: &has_adapter,
        route_operations: &|route| {
            config::platform_route_config(route, &config::default_platform_routes())
                .map(providers::declared_and_built_operations)
        },
        checks_accounts: &|route| {
            config::platform_route_config(route, &config::default_platform_routes())
                .map(providers::built_route_checks_accounts)
        },
        is_config_leaf: &config::is_leaf,
        has_login: &|platform| providers::browser_login(platform).is_some(),
        sample_refs: &sample_refs,
        has_window_test: &has_window_test,
        contract_modules: &contract_modules,
    }
}

#[test]
fn every_platform_satisfies_the_integration_checklist() {
    let registry = baseline();

    let found = Platform::ALL
        .into_iter()
        .flat_map(|platform| violations(platform, &registry))
        .collect::<Vec<_>>();

    assert!(
        found.is_empty(),
        "platform checklist violations:\n{}",
        found
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    );
}

fn assert_reports(found: &[Violation], rule: Rule) {
    assert!(
        found.iter().any(|violation| violation.rule == rule),
        "expected a violation of {rule:?}, found: {found:#?}"
    );
}

#[test]
fn a_platform_without_a_catalog_breaks_the_catalog_rule() {
    let registry = Registry {
        platforms: &[],
        ..baseline()
    };

    assert_reports(&violations(Platform::Arxiv, &registry), Rule::Catalog);
}

#[test]
fn a_route_without_a_factory_breaks_the_factory_rule() {
    let registry = Registry {
        has_adapter: &|_, _, _| false,
        ..baseline()
    };

    assert_reports(&violations(Platform::Arxiv, &registry), Rule::Factory);
}

#[test]
fn a_declaration_that_disagrees_with_its_built_route_breaks_the_declaration_rule() {
    let registry = Registry {
        route_operations: &|_| Some([(true, false), (true, true)]),
        ..baseline()
    };

    let found = violations(Platform::Arxiv, &registry);

    assert_reports(&found, Rule::Declaration);
    assert!(
        found
            .iter()
            .any(|violation| violation.detail.contains("`arxiv_api`")),
        "{found:?}"
    );
}

#[test]
fn an_account_check_that_the_built_route_cannot_run_breaks_the_declaration_rule() {
    let registry = Registry {
        checks_accounts: &|_| Some(false),
        ..baseline()
    };

    let found = violations(Platform::Scholar, &registry);

    assert_reports(&found, Rule::Declaration);
    assert!(
        found
            .iter()
            .any(|violation| violation.detail.contains("`serpapi`")),
        "{found:?}"
    );
}

#[test]
fn a_platform_without_an_order_leaf_breaks_the_config_rule() {
    let registry = Registry {
        is_config_leaf: &|path| path != "platforms.arxiv.order" && config::is_leaf(path),
        ..baseline()
    };

    assert_reports(&violations(Platform::Arxiv, &registry), Rule::ConfigLeaves);
}

#[test]
fn a_browser_route_without_a_timeout_leaf_breaks_the_config_rule() {
    let registry = Registry {
        is_config_leaf: &|path| {
            path != "providers.xiaohongshu_browser.timeout" && config::is_leaf(path)
        },
        ..baseline()
    };

    let found = violations(Platform::Xiaohongshu, &registry);

    assert!(
        found.iter().any(|violation| violation
            .detail
            .contains("`providers.xiaohongshu_browser.timeout`")),
        "{found:?}"
    );
}

#[test]
fn a_main_search_backend_is_not_required_to_have_a_timeout_leaf() {
    let registry = baseline();

    let found = violations(Platform::X, &registry);

    assert!(
        !found
            .iter()
            .any(|violation| violation.detail.contains("providers.xai.timeout")),
        "{found:?}"
    );
}

#[test]
fn a_browser_route_is_not_required_to_have_a_url_leaf() {
    let registry = baseline();

    let found = violations(Platform::Xiaohongshu, &registry);

    assert!(
        !found.iter().any(|violation| violation
            .detail
            .contains("providers.xiaohongshu_browser.url")),
        "{found:?}"
    );
}

#[test]
fn a_platform_without_a_smoke_case_breaks_the_smoke_rule() {
    let registrations = catalog::registrations()
        .iter()
        .map(|registration| ProviderRegistration {
            smoke_cases: &[],
            ..*registration
        })
        .collect::<Vec<_>>();
    let registry = Registry {
        registrations: &registrations,
        ..baseline()
    };

    assert_reports(&violations(Platform::Arxiv, &registry), Rule::SmokeCase);
}

#[test]
fn an_operation_of_the_platform_without_a_smoke_case_breaks_the_smoke_rule() {
    let registrations = catalog::registrations()
        .iter()
        .map(|registration| ProviderRegistration {
            smoke_cases: registration
                .smoke_cases
                .iter()
                .copied()
                .filter(|case| case.operation != CITED_BY)
                .collect::<Vec<_>>()
                .leak(),
            ..*registration
        })
        .collect::<Vec<_>>();
    let registry = Registry {
        registrations: &registrations,
        ..baseline()
    };

    let found = violations(Platform::Scholar, &registry);

    assert_eq!(
        found
            .iter()
            .map(|violation| (violation.rule, violation.detail.as_str()))
            .collect::<Vec<_>>(),
        [(Rule::SmokeCase, "no route registers a cited_by smoke case")],
        "{found:#?}"
    );
}

#[test]
fn a_browser_route_without_login_page_logic_breaks_the_browser_rule() {
    let registry = Registry {
        has_login: &|_| false,
        ..baseline()
    };

    let found = violations(Platform::Xiaohongshu, &registry);

    assert_reports(&found, Rule::BrowserEnvironment);
    assert!(
        found
            .iter()
            .any(|violation| violation.detail.contains("login page logic")),
        "{found:?}"
    );
}

#[test]
fn samples_that_miss_a_kind_of_the_platform_break_the_sample_rule() {
    let registry = Registry {
        sample_refs: &|_| &["youtube:jNQXAC9IVRw"],
        ..baseline()
    };

    assert_reports(&violations(Platform::Youtube, &registry), Rule::SampleRefs);
}

#[test]
fn a_ref_that_does_not_round_trip_breaks_the_sample_rule() {
    let registry = Registry {
        sample_refs: &|_| &["arXiv:2401.01234"],
        ..baseline()
    };

    assert_reports(&violations(Platform::Arxiv, &registry), Rule::SampleRefs);
}

#[test]
fn samples_whose_first_ref_no_fetch_route_accepts_break_the_sample_rule() {
    let registry = Registry {
        sample_refs: &|_| {
            &[
                "youtube:playlist/PLZHQObOWTQDPD3MizzM2xVFitgF8hE_ab",
                "youtube:jNQXAC9IVRw",
                "youtube:channel/UCYO_jab_esuFRV4b17AJtAw",
            ]
        },
        contract_modules: &|_| Vec::new(),
        ..baseline()
    };

    let found = violations(Platform::Youtube, &registry);

    assert_eq!(
        found
            .iter()
            .map(|violation| violation.rule)
            .collect::<Vec<_>>(),
        [Rule::SampleRefs],
        "{found:#?}"
    );
}

#[test]
fn a_paced_http_route_without_a_window_test_breaks_the_window_rule() {
    let registry = Registry {
        has_window_test: &|_| false,
        ..baseline()
    };

    assert_reports(
        &violations(Platform::Arxiv, &registry),
        Rule::AccessWindowTest,
    );
    assert!(
        !violations(Platform::Reddit, &registry)
            .iter()
            .any(|violation| violation.rule == Rule::AccessWindowTest),
        "a browser route needs no window test"
    );
}

#[test]
fn a_multi_route_platform_with_tests_outside_its_route_modules_breaks_the_layout_rule() {
    let registry = Registry {
        contract_modules: &|platform| {
            ["", "::youtube_api", "::search"]
                .map(|module| format!("platforms::{}{module}", platform.as_str()))
                .to_vec()
        },
        ..baseline()
    };

    let layout = |platform| {
        violations(platform, &registry)
            .into_iter()
            .filter(|violation| violation.rule == Rule::ContractLayout)
            .map(|violation| violation.detail)
            .collect::<Vec<_>>()
    };

    assert_eq!(
        (layout(Platform::Youtube), layout(Platform::Arxiv)),
        (
            vec![
                "its contract tests in `platforms::youtube` are not in a module named after one of its routes".to_owned(),
                "its contract tests in `platforms::youtube::search` are not in a module named after one of its routes".to_owned(),
            ],
            Vec::new()
        ),
        "a platform with one route keeps a module per command"
    );
}
