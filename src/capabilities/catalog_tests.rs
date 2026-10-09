use std::collections::BTreeSet;

use serde::Deserialize;

use super::{
    CATALOGS, DOCS_SEARCH, DoctorProbe, MAIN_SEARCH, PLATFORMS, PlatformOperation, ProviderId,
    ProviderRegistration, ProviderTransport, REGISTRY, WEB_FETCH, WEB_SEARCH, has_owner, platform,
    registration, registrations, validate_registrations,
};

/// Providers without a live smoke case. `gemini_browser` gets only the offline registration
/// check: a live case would spend the user's Deep Research quota or read their conversations.
const WITHOUT_LIVE_SMOKE: &[ProviderId] = &[ProviderId::GeminiBrowser];

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

/// Returns whether a seam names a platform operation that no platform catalog lists.
fn is_single_route_operation_seam(seam: &str) -> bool {
    seam.strip_prefix("platform:")
        .and_then(|rest| rest.split_once(':'))
        .is_some_and(|(_, operation)| {
            !PlatformOperation::ALL
                .iter()
                .any(|catalog_operation| catalog_operation.as_str() == operation)
        })
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
    // Single-route platform operations live outside the catalog; the platform checklist
    // checks their fixtures.
    let fixture_projection = manifest
        .transport_fixtures
        .iter()
        .filter(|fixture| !is_single_route_operation_seam(&fixture.seam))
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
    for registration in registrations() {
        assert!(has_owner(registration), "{} owner", registration.id.name());
        let probe_is_supported = match registration.probe {
            DoctorProbe::MainSearch(_) => MAIN_SEARCH.contains(registration.id),
            DoctorProbe::WebSearch { .. } => WEB_SEARCH.contains(registration.id),
            DoctorProbe::WebFetch { .. } => WEB_FETCH.contains(registration.id),
            DoctorProbe::DocsSearch { .. } => DOCS_SEARCH.contains(registration.id),
            DoctorProbe::AnysearchDomains { .. } => registration.id == ProviderId::Anysearch,
            DoctorProbe::PlatformSearch {
                platform: probed, ..
            } => platform(probed).search.contains(&registration.id),
            DoctorProbe::ServiceAccount { .. } => registration.credentials_required,
            DoctorProbe::AdapterStatus { .. } => {
                matches!(registration.transport, ProviderTransport::OpenCli(_))
            }
        };
        assert!(probe_is_supported, "{} probe", registration.id.name());
        assert_eq!(
            registration.smoke_cases.is_empty(),
            WITHOUT_LIVE_SMOKE.contains(&registration.id),
            "{} smoke",
            registration.id.name()
        );
    }
}

#[test]
fn a_provider_outside_every_catalog_is_registered_through_its_operations() {
    let gemini = registration(ProviderId::GeminiBrowser);

    assert_eq!(
        (
            CATALOGS.iter().any(|catalog| catalog.contains(gemini.id)),
            PLATFORMS.iter().any(|catalog| catalog.contains(gemini.id)),
            gemini.operations,
            has_owner(gemini),
        ),
        (
            false,
            false,
            &["gemini_research_start", "gemini_research_result"][..],
            true
        )
    );
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
