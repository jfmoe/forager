//! Consistency checks for every registration point a platform needs.
//!
//! The type system cannot see a platform that misses a registration point, so this test walks
//! the integration checklist in `docs/spec/forager/07-platforms.md`. Each violation names the
//! checklist item (R1–R8) that the platform misses.

use std::collections::BTreeSet;

use crate::catalog::{
    self, DoctorProbe, PLATFORMS, PlatformCatalog, PlatformOperation, ProviderId,
    ProviderRegistration,
};
use crate::config::{self, ArxivApiRuntimeConfig};
use crate::providers;
use crate::types::{
    ContentDepth, Platform, PlatformFetchRequest, PlatformRef, PlatformSearchOptions,
    PlatformSearchRequest,
};

const CHECKLIST: &str = "docs/spec/forager/07-platforms.md";

/// Refs of every kind of each platform; the exhaustive match makes a new platform add samples.
fn sample_refs(platform: Platform) -> &'static [&'static str] {
    match platform {
        Platform::Arxiv => &[
            "arxiv:2401.01234v2",
            "arxiv:2401.01234",
            "arxiv:hep-th/9901001v1",
        ],
    }
}

/// The registration points the checklist inspects; tests replace one at a time.
struct Registry<'a> {
    platforms: &'a [PlatformCatalog],
    registrations: &'a [ProviderRegistration],
    has_adapter: &'a dyn Fn(Platform, PlatformOperation, ProviderId) -> bool,
    is_config_leaf: &'a dyn Fn(&str) -> bool,
    fixtures: &'a BTreeSet<(String, String)>,
    sample_refs: &'a dyn Fn(Platform) -> &'static [&'static str],
}

fn violation(platform: Platform, item: &str, detail: &str) -> String {
    format!("{platform}: {detail} (see {CHECKLIST}, integration checklist {item})")
}

fn violations(platform: Platform, registry: &Registry<'_>) -> Vec<String> {
    let mut found = Vec::new();
    let Some(catalog) = registry
        .platforms
        .iter()
        .find(|catalog| catalog.platform == platform)
    else {
        return vec![violation(platform, "R1", "no platform catalog lists it")];
    };
    for operation in PlatformOperation::ALL {
        if catalog.routes(operation).is_empty() {
            found.push(violation(
                platform,
                "R1",
                &format!("its catalog has no {} route", operation.as_str()),
            ));
        }
    }
    let order_key = config::platform_order_key(platform);
    if !(registry.is_config_leaf)(&order_key) {
        found.push(violation(
            platform,
            "R4",
            &format!("the configuration schema lacks `{order_key}`"),
        ));
    }
    for route in catalog.all_routes() {
        check_route(platform, route, registry, &mut found);
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
    check_refs(platform, (registry.sample_refs)(platform), &mut found);
    found
}

fn check_operation(
    platform: Platform,
    operation: PlatformOperation,
    routes: &[ProviderId],
    registry: &Registry<'_>,
    found: &mut Vec<String>,
) {
    let name = operation.as_str();
    for route in routes {
        if !(registry.has_adapter)(platform, operation, *route) {
            found.push(violation(
                platform,
                "R3",
                &format!(
                    "the platform factory cannot build {name} route `{}`",
                    route.name()
                ),
            ));
        }
        let seam = format!("platform:{platform}:{name}");
        if !registry
            .fixtures
            .contains(&(route.name().to_owned(), seam.clone()))
        {
            found.push(violation(
                platform,
                "R7",
                &format!(
                    "the acceptance manifest has no transport fixture for `{}` / `{seam}`",
                    route.name()
                ),
            ));
        }
    }
    let has_smoke_case = registry.registrations.iter().any(|registration| {
        registration
            .smoke_cases
            .iter()
            .any(|case| case.platform == Some(platform) && case.operation == name)
    });
    if !has_smoke_case {
        found.push(violation(
            platform,
            "R6",
            &format!("no route registers a {name} smoke case"),
        ));
    }
}

fn check_route(
    platform: Platform,
    route: ProviderId,
    registry: &Registry<'_>,
    found: &mut Vec<String>,
) {
    let name = route.name();
    if name == platform.as_str() {
        found.push(violation(
            platform,
            "R2",
            &format!("route `{name}` reuses the bare platform name"),
        ));
    }
    let Some(registration) = registry
        .registrations
        .iter()
        .find(|registration| registration.id == route)
    else {
        found.push(violation(
            platform,
            "R2",
            &format!("route `{name}` has no provider registration"),
        ));
        return;
    };
    for leaf in ["url", "timeout"] {
        let path = format!("providers.{name}.{leaf}");
        if !(registry.is_config_leaf)(&path) {
            found.push(violation(
                platform,
                "R4",
                &format!("the configuration schema lacks `{path}`"),
            ));
        }
    }
    let keys = format!("providers.{name}.keys");
    if (registry.is_config_leaf)(&keys) != registration.credentials_required {
        found.push(violation(
            platform,
            "R4",
            &format!(
                "`{keys}` must exist exactly when the route requires credentials ({})",
                registration.credentials_required
            ),
        ));
    }
    let probes_platform = matches!(
        registration.probe,
        DoctorProbe::PlatformSearch { platform: probed, .. } if probed == platform
    );
    let serves_capability = catalog::CATALOGS
        .iter()
        .any(|catalog| catalog.contains(route));
    if !probes_platform && !serves_capability {
        found.push(violation(
            platform,
            "R5",
            &format!("route `{name}` has no doctor probe for the platform"),
        ));
    }
}

fn check_refs(platform: Platform, samples: &[&str], found: &mut Vec<String>) {
    if samples.is_empty() {
        found.push(violation(platform, "R8", "no sample refs cover its kinds"));
    }
    for sample in samples {
        let round_trips = PlatformRef::parse(platform, sample).is_ok_and(|reference| {
            reference.to_string() == *sample
                && PlatformRef::parse(platform, &reference.canonical_url()).ok() == Some(reference)
        });
        if !round_trips {
            found.push(violation(
                platform,
                "R8",
                &format!("ref `{sample}` does not round-trip through its canonical URL"),
            ));
        }
    }
}

fn manifest_fixtures() -> BTreeSet<(String, String)> {
    let manifest: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/acceptance-manifest.json"
    )))
    .expect("acceptance manifest");
    manifest["transport_fixtures"]
        .as_array()
        .expect("transport fixtures")
        .iter()
        .map(|fixture| {
            (
                fixture["provider"].as_str().expect("provider").to_owned(),
                fixture["seam"].as_str().expect("seam").to_owned(),
            )
        })
        .collect()
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
            .first()
            .and_then(|sample| PlatformRef::parse(platform, sample).ok())
            .is_some_and(|reference| {
                let request = PlatformFetchRequest {
                    reference,
                    depth: ContentDepth::Abstract,
                };
                providers::platform_fetch_support(route, &request).is_some()
            }),
    };
    let arxiv_api = ArxivApiRuntimeConfig {
        url: String::new(),
        timeout_seconds: 1,
    };
    has_support
        && config::platform_route_config(route, &arxiv_api)
            .is_some_and(|route_config| route_config.route() == route)
}

fn baseline(fixtures: &BTreeSet<(String, String)>) -> Registry<'_> {
    Registry {
        platforms: PLATFORMS,
        registrations: catalog::registrations(),
        has_adapter: &has_adapter,
        is_config_leaf: &config::is_leaf,
        fixtures,
        sample_refs: &sample_refs,
    }
}

#[test]
fn every_platform_satisfies_the_integration_checklist() {
    let fixtures = manifest_fixtures();
    let registry = baseline(&fixtures);

    let found = Platform::ALL
        .into_iter()
        .flat_map(|platform| violations(platform, &registry))
        .collect::<Vec<_>>();

    assert!(
        found.is_empty(),
        "missing registration points:\n{}",
        found.join("\n")
    );
}

fn assert_reports(found: &[String], item: &str) {
    assert!(
        found.iter().any(|message| message.contains(CHECKLIST)
            && message.ends_with(&format!("integration checklist {item})"))),
        "expected a violation of {item}, found: {found:?}"
    );
}

#[test]
fn a_platform_without_a_catalog_violates_r1() {
    let fixtures = manifest_fixtures();
    let registry = Registry {
        platforms: &[],
        ..baseline(&fixtures)
    };

    assert_reports(&violations(Platform::Arxiv, &registry), "R1");
}

#[test]
fn a_route_without_a_registration_violates_r2() {
    let fixtures = manifest_fixtures();
    let registrations = catalog::registrations()
        .iter()
        .copied()
        .filter(|registration| registration.id != ProviderId::ArxivApi)
        .collect::<Vec<_>>();
    let registry = Registry {
        registrations: &registrations,
        ..baseline(&fixtures)
    };

    assert_reports(&violations(Platform::Arxiv, &registry), "R2");
}

#[test]
fn a_route_without_a_factory_violates_r3() {
    let fixtures = manifest_fixtures();
    let registry = Registry {
        has_adapter: &|_, _, _| false,
        ..baseline(&fixtures)
    };

    assert_reports(&violations(Platform::Arxiv, &registry), "R3");
}

#[test]
fn a_platform_without_an_order_leaf_violates_r4() {
    let fixtures = manifest_fixtures();
    let registry = Registry {
        is_config_leaf: &|path| path != "platforms.arxiv.order" && config::is_leaf(path),
        ..baseline(&fixtures)
    };

    assert_reports(&violations(Platform::Arxiv, &registry), "R4");
}

#[test]
fn a_route_without_a_platform_probe_violates_r5() {
    let fixtures = manifest_fixtures();
    let registrations = catalog::registrations()
        .iter()
        .map(|registration| ProviderRegistration {
            probe: DoctorProbe::WebFetch {
                name: "fetch",
                transport: "http",
            },
            ..*registration
        })
        .collect::<Vec<_>>();
    let registry = Registry {
        registrations: &registrations,
        ..baseline(&fixtures)
    };

    assert_reports(&violations(Platform::Arxiv, &registry), "R5");
}

#[test]
fn a_platform_without_a_smoke_case_violates_r6() {
    let fixtures = manifest_fixtures();
    let registrations = catalog::registrations()
        .iter()
        .map(|registration| ProviderRegistration {
            smoke_cases: &[],
            ..*registration
        })
        .collect::<Vec<_>>();
    let registry = Registry {
        registrations: &registrations,
        ..baseline(&fixtures)
    };

    assert_reports(&violations(Platform::Arxiv, &registry), "R6");
}

#[test]
fn a_route_without_a_transport_fixture_violates_r7() {
    let fixtures = BTreeSet::new();
    let registry = baseline(&fixtures);

    assert_reports(&violations(Platform::Arxiv, &registry), "R7");
}

#[test]
fn a_ref_that_does_not_round_trip_violates_r8() {
    let fixtures = manifest_fixtures();
    let registry = Registry {
        sample_refs: &|_| &["arXiv:2401.01234"],
        ..baseline(&fixtures)
    };

    assert_reports(&violations(Platform::Arxiv, &registry), "R8");
}
