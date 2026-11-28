use std::fs;

use tempfile::tempdir;

use super::*;
use crate::config::runtime_config_from_toml;
use crate::types::{
    AttemptDisposition, AttemptErrorKind, AttemptTarget, CapabilityGap, ProviderAttempt,
    ProviderError, SearchCandidate, Source,
};

#[test]
fn write_value_persists_json_that_can_be_read_back() {
    let root = tempdir().expect("create temporary directory");
    let config = journal_config(root.path().join("journal"));
    let expected = json!({"result": {"status": "ok"}});

    let written = write_value(&config, expected.clone()).expect("write journal record");
    let persisted: Value =
        serde_json::from_slice(&fs::read(&written.reference).expect("read journal record"))
            .expect("parse journal record");

    assert_eq!(persisted, expected);
}

#[test]
fn cleanup_warning_silences_permission_denied() {
    let error = io::Error::from(io::ErrorKind::PermissionDenied);

    let warning = cleanup_warning(&error);

    assert!(warning.is_none(), "unexpected cleanup warning: {warning:?}");
}

fn outcome() -> SearchOutcome {
    SearchOutcome {
        provider: "xai",
        answer: "answer".into(),
        sources: Vec::new(),
        extra_sources: Vec::new(),
        capabilities: Vec::new(),
        capability_gaps: Vec::new(),
        attempts: Vec::new(),
        diagnostic: None,
    }
}

fn record(plan_source: PlanSource, result: &Result<SearchOutcome, Box<SearchFailure>>) -> Value {
    build_record(SearchRecord {
        query: "query",
        budget: Duration::from_mins(3),
        elapsed: Duration::from_secs(1),
        capabilities: &[],
        plan_source,
        result,
    })
}

#[test]
fn every_record_has_schema_version_one_and_the_same_execution_keys() {
    let succeeded = record(PlanSource::Caller, &Ok(outcome()));
    let preflight = build_search_preflight_record(SearchPreflightRecord {
        query: "query",
        budget: Duration::from_mins(3),
        error_kind: "config",
        message: "no capability",
    });
    let shape = |record: &Value| {
        let mut keys = record["execution"]
            .as_object()
            .expect("execution object")
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        keys.sort();
        (record["schema_version"].clone(), keys)
    };
    let expected = (
        json!(1),
        [
            "capability_gaps",
            "deadline_budget",
            "plan_summary",
            "provider_attempts",
            "terminal_attribution",
        ]
        .map(str::to_owned)
        .to_vec(),
    );

    assert_eq!(
        [shape(&succeeded), shape(&preflight)],
        [expected.clone(), expected]
    );
}

#[test]
fn a_search_record_names_who_chose_its_capabilities() {
    let result = Ok(outcome());
    let summaries = [PlanSource::Caller, PlanSource::DefaultMainSearchOnly]
        .map(|source| record(source, &result)["execution"]["plan_summary"].clone());

    assert_eq!(
        summaries,
        [
            json!({"source": "caller", "capabilities": []}),
            json!({"source": "default_main_search_only", "capabilities": []}),
        ]
    );
}

#[test]
fn a_search_record_names_the_backend_that_answered() {
    let result = Ok(SearchOutcome {
        provider: "openai",
        ..outcome()
    });

    assert_eq!(
        record(PlanSource::Caller, &result)["result"]["provider"],
        json!("openai")
    );
}

#[test]
fn a_failed_search_record_keeps_each_candidate_with_its_summary_and_each_gap() {
    let candidate = SearchCandidate::from_source(
        Source {
            title: "Candidate".into(),
            url: "https://example.test/candidate".into(),
            published_date: None,
            author: None,
            text: Some("Candidate summary".into()),
            highlights: Vec::new(),
            id: None,
            image: None,
            favicon: None,
        },
        "tavily",
        Capability::WebSearch,
    )
    .expect("an HTTP candidate");
    let result = Err(Box::new(SearchFailure {
        error: ProviderError {
            kind: AttemptErrorKind::Auth,
            message: "bad key".into(),
            attempts: Vec::new(),
            diagnostic: None,
        },
        extra_sources: vec![candidate],
        capability_gaps: vec![CapabilityGap {
            capability: Capability::DocsSearch,
            reason: "no_configured_provider",
            providers_skipped: vec!["exa".into()],
        }],
    }));

    let record = record(PlanSource::Caller, &result);

    assert_eq!(
        (
            &record["result"]["extra_sources"],
            &record["execution"]["capability_gaps"],
        ),
        (
            &json!([{
                "provider": "tavily",
                "capability": "web_search",
                "title": "Candidate",
                "url": "https://example.test/candidate",
                "summary": "Candidate summary",
                "provider_data": {}
            }]),
            &json!([{
                "capability": "docs_search",
                "reason": "no_configured_provider",
                "providers_skipped": ["exa"]
            }]),
        )
    );
}

fn source(title: &str, url: &str) -> Source {
    Source {
        title: title.into(),
        url: url.into(),
        published_date: None,
        author: None,
        text: None,
        highlights: Vec::new(),
        id: None,
        image: None,
        favicon: None,
    }
}

/// One main-search attempt of xAI with the model `test-model`, ended as `kind`.
fn xai_attempt(kind: Option<AttemptErrorKind>) -> ProviderAttempt {
    ProviderAttempt {
        provider: "xai",
        target: AttemptTarget::seam("main_search"),
        disposition: if kind.is_some() {
            AttemptDisposition::Failed
        } else {
            AttemptDisposition::Succeeded
        },
        error_kind: kind,
        http_status: kind.is_none().then_some(200),
        duration_ms: 0,
        credential_index: 0,
        retry_count: 0,
        rotation_count: 0,
        anonymous: false,
        message: String::new(),
        model: Some("test-model".into()),
        transport: Some("sse"),
        endpoint_host: Some("127.0.0.1".into()),
    }
}

#[test]
fn a_search_record_keeps_the_query_plan_and_attempts_that_the_payload_leaves_out() {
    let sources = vec![
        source("Primary", "https://example.test/source?token=********"),
        source("", "https://example.test/inline"),
    ];
    let result = Ok(SearchOutcome {
        sources: sources.clone(),
        attempts: vec![xai_attempt(None)],
        ..outcome()
    });

    let record = build_record(SearchRecord {
        query: "Focused stdout",
        budget: Duration::from_mins(3),
        elapsed: Duration::from_secs(1),
        capabilities: &[Capability::DocsSearch, Capability::WebSearch],
        plan_source: PlanSource::Caller,
        result: &result,
    });

    assert_eq!(
        (
            &record["result"]["query"],
            &record["result"]["answer"],
            &record["result"]["sources"],
            &record["execution"]["plan_summary"]["capabilities"],
            &record["execution"]["provider_attempts"][0]["provider"],
            &record["execution"]["provider_attempts"][0]["model"],
        ),
        (
            &json!("Focused stdout"),
            &json!("answer"),
            &serde_json::to_value(&sources).expect("encode sources"),
            &json!(["docs_search", "web_search"]),
            &json!("xai"),
            &json!("test-model"),
        )
    );
}

#[test]
fn a_search_that_used_up_its_budget_is_recorded_with_its_attempt_chain() {
    let result = Err(Box::new(SearchFailure {
        error: ProviderError {
            kind: AttemptErrorKind::Timeout,
            message: "deadline exceeded".into(),
            attempts: vec![xai_attempt(Some(AttemptErrorKind::Timeout))],
            diagnostic: None,
        },
        extra_sources: Vec::new(),
        capability_gaps: Vec::new(),
    }));

    let record = build_record(SearchRecord {
        query: "Budget",
        budget: Duration::from_secs(1),
        elapsed: Duration::from_secs(1),
        capabilities: &[],
        plan_source: PlanSource::Caller,
        result: &result,
    });

    assert_eq!(
        (
            &record["result"]["error_kind"],
            &record["execution"]["terminal_attribution"],
            record["execution"]["provider_attempts"]
                .as_array()
                .map(Vec::len),
            &record["execution"]["provider_attempts"][0]["error_kind"],
            &record["execution"]["deadline_budget"]["exhausted"],
        ),
        (
            &json!("timeout"),
            &json!("timeout"),
            Some(1),
            &json!("timeout"),
            &json!(true),
        )
    );
}

/// Records a successful search under `config`.
fn record_success(config: &JournalRuntimeConfig) -> JournalOutcome {
    record_search(
        config,
        SearchRecord {
            query: "query",
            budget: Duration::from_mins(3),
            elapsed: Duration::from_secs(1),
            capabilities: &[],
            plan_source: PlanSource::DefaultMainSearchOnly,
            result: &Ok(outcome()),
        },
    )
}

#[test]
fn a_journal_that_cannot_be_written_is_reported_as_failed_with_a_warning() {
    let root = tempdir().expect("create temporary directory");
    let blocked = root.path().join("journal");
    fs::write(&blocked, "not a directory").expect("block the journal directory");

    let journal = record_success(&journal_config(blocked));

    assert_eq!(
        (
            journal.status,
            journal.reference,
            journal.warning.is_some_and(|warning| !warning.is_empty()),
        ),
        ("failed", None, true)
    );
}

/// Writes an earlier record into `dir`, named as written `age` ago.
fn earlier_record(dir: &std::path::Path, age: TimeDelta) -> std::path::PathBuf {
    let written = (Utc::now() - age)
        .timestamp_nanos_opt()
        .expect("a time in range");
    let path = dir.join(format!("search_result_{written}_2_3.json"));
    fs::write(&path, "{}").expect("create an earlier record");
    path
}

#[test]
fn writing_a_record_removes_the_records_named_as_written_before_the_default_retention() {
    let root = tempdir().expect("create temporary directory");
    let dir = root.path().join("journal");
    let document = format!(
        "[journal]\nenabled = true\ndir = {:?}\n",
        dir.to_string_lossy()
    );
    let config = runtime_config_from_toml(&document)
        .expect("valid configuration")
        .journal;
    fs::create_dir(&dir).expect("create journal directory");
    let expired = earlier_record(&dir, TimeDelta::days(31));
    let retained = earlier_record(&dir, TimeDelta::days(29));

    let journal = record_success(&config);

    assert_eq!(
        (
            journal.status,
            journal
                .reference
                .is_some_and(|reference| std::path::Path::new(&reference).is_file()),
            expired.exists(),
            retained.exists(),
        ),
        ("written", true, false, true)
    );
}

#[test]
fn a_journal_retained_indefinitely_keeps_every_earlier_record() {
    let root = tempdir().expect("create temporary directory");
    let config = JournalRuntimeConfig {
        retention_days: 0,
        ..journal_config(root.path().join("journal"))
    };
    fs::create_dir(&config.dir).expect("create journal directory");
    let earliest = earlier_record(&config.dir, TimeDelta::days(3650));

    let journal = record_success(&config);

    assert_eq!(
        (journal.status, journal.warning, earliest.exists()),
        ("written", None, true)
    );
}

fn journal_config(dir: std::path::PathBuf) -> JournalRuntimeConfig {
    JournalRuntimeConfig {
        enabled: true,
        dir,
        retention_days: 1,
        credentials: Vec::new(),
    }
}
