//! The notification decision: from the scheduled run records, whether each provider's monitoring
//! issue opens, is updated, closes or stays as it is, and the issue text. The text holds check
//! identifiers, error kinds and HTTP statuses only, so no configuration value, endpoint or usage
//! figure reaches the public issue.

use std::collections::BTreeMap;
use std::fmt::Write;

use serde::Serialize;

use super::record::{PastCheck, PastRun};
use crate::catalog::ProviderId;
use crate::types::AttemptErrorKind;

/// What happens to a provider's monitoring issue.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Action {
    /// Its failing set became non-empty.
    Open,
    /// Its failing set changed its members or their error kinds.
    Update,
    /// Its failing set became empty, because those checks passed or are no longer declared.
    Close,
}

/// One provider's decision with the text the job posts: the issue is found by `label`, opened
/// with `title` and `body`, or commented on or closed with `body`.
#[derive(Debug, Serialize)]
pub(super) struct Notification {
    pub(super) provider: String,
    pub(super) action: Action,
    pub(super) label: String,
    pub(super) title: String,
    pub(super) body: String,
}

/// Each provider's checks whose latest concluded scheduled result is a failure, by identifier.
type FailingSets<'a> = BTreeMap<&'a str, BTreeMap<&'a str, &'a PastCheck>>;

/// The decision for every provider whose issue `current` changes, after the records in
/// `history`, oldest first. A provider's failing set holds its declared checks whose latest
/// scheduled result, passed or confirmed failed, is a failure; a run that is not scheduled decides
/// nothing. The set before `current` holds the checks the previous scheduled run declared, so a
/// check retired since leaves the set in the first run that no longer declares it.
pub(super) fn decide(history: &[PastRun], current: &PastRun) -> Vec<Notification> {
    if !current.scheduled {
        return Vec::new();
    }
    let previous = history.iter().rev().find(|run| run.scheduled);
    let before = failing_sets(
        history.iter(),
        previous.and_then(|run| run.declared.as_deref()),
    );
    let after = failing_sets(
        history.iter().chain(std::iter::once(current)),
        current.declared.as_deref(),
    );
    let mut providers = before.keys().chain(after.keys()).collect::<Vec<_>>();
    providers.sort_unstable();
    providers.dedup();
    providers
        .into_iter()
        .filter_map(|provider| {
            let action = match (before.get(provider), after.get(provider)) {
                (None, Some(_)) => Action::Open,
                (Some(_), None) => Action::Close,
                (Some(was), Some(is)) if kinds(was).ne(kinds(is)) => Action::Update,
                _ => return None,
            };
            Some(notification(provider, action, after.get(provider)))
        })
        .collect()
}

/// The failing sets after `runs`, among the `declared` checks; every check counts when a record
/// does not list them.
fn failing_sets<'a>(
    runs: impl Iterator<Item = &'a PastRun>,
    declared: Option<&[String]>,
) -> FailingSets<'a> {
    let mut latest = BTreeMap::<&str, &PastCheck>::new();
    for run in runs.filter(|run| run.scheduled) {
        for check in run.checks.iter().filter(|check| check.concluded()) {
            latest.insert(&check.id, check);
        }
    }
    let mut sets = FailingSets::new();
    for (id, check) in latest {
        let is_declared = declared.is_none_or(|declared| declared.iter().any(|known| known == id));
        if check.status == super::Status::Failed && is_declared {
            sets.entry(provider_of(id)).or_default().insert(id, check);
        }
    }
    sets
}

fn kinds<'a>(
    set: &'a BTreeMap<&'a str, &'a PastCheck>,
) -> impl Iterator<Item = (&'a str, Option<&'a str>)> {
    set.iter()
        .map(|(id, check)| (*id, check.error_kind.as_deref()))
}

/// A check identifier starts with its provider's name, or `pipeline` for the pipeline check.
fn provider_of(id: &str) -> &str {
    id.split_once(':').map_or(id, |(provider, _)| provider)
}

fn notification(
    provider: &str,
    action: Action,
    failing: Option<&BTreeMap<&str, &PastCheck>>,
) -> Notification {
    let body = match failing {
        Some(failing) => {
            let mut body = format!("The scheduled Live Checks of `{provider}` fail:\n\n");
            for (id, check) in failing {
                let _ = write!(body, "- `{id}`: `{}`", error_kind(check));
                if let Some(status) = check.http_status {
                    let _ = write!(body, ", HTTP {status}");
                }
                body.push('\n');
            }
            let _ = write!(
                body,
                "\nRun `{}` on the maintainer's machine for the detail, as \
                 docs/agents/live-checks.md describes.\n",
                rerun_command(provider)
            );
            body
        }
        None => format!("The scheduled Live Checks of `{provider}` pass again.\n"),
    };
    Notification {
        provider: provider.to_owned(),
        action,
        label: format!("live-check:{provider}"),
        title: format!("Live Checks of {provider} fail"),
        body,
    }
}

/// The command that runs a provider's unattended tiers by hand. A provider's checks are selected
/// by its name; the pipeline check, which no provider declares, only by its tier.
fn rerun_command(provider: &str) -> String {
    if ProviderId::parse(provider).is_some() {
        format!("forager doctor --live --provider {provider} --tier free,metered")
    } else {
        format!(
            "forager doctor --live --tier {}",
            super::PIPELINE.tier.as_str()
        )
    }
}

/// The failure's error kind when it is one forager names; the record's text comes from the
/// check's command, so anything else is shown as `unrecognised`.
fn error_kind(check: &PastCheck) -> &'static str {
    match check.error_kind.as_deref() {
        Some("contract") => "contract",
        Some("config") => "config",
        Some(kind) => serde_json::from_value::<AttemptErrorKind>(kind.into())
            .map_or("unrecognised", AttemptErrorKind::as_str),
        None => "unrecognised",
    }
}

#[cfg(test)]
#[path = "live_notification_tests.rs"]
mod tests;
