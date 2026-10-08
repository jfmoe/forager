//! SerpApi's Account API: a free report of one key's remaining searches and hourly throughput.
//!
//! The response also carries the key itself and the account email, so only the numeric quota
//! fields are ever decoded.

use serde::Deserialize;

use super::{Serpapi, refusal};
use crate::net::AttemptFailure;
use crate::providers::execution::execute_pinned;
use crate::types::{AttemptErrorKind, AttemptTarget, ProviderError};

/// The quota of one key, as the Account API reports it.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
pub(crate) struct AccountQuota {
    #[serde(rename = "total_searches_left")]
    pub(crate) searches_left: i64,
    pub(crate) plan_searches_left: i64,
    pub(crate) this_month_usage: i64,
    pub(crate) this_hour_searches: i64,
    #[serde(rename = "account_rate_limit_per_hour")]
    pub(crate) hourly_limit: i64,
}

/// The account check of one key: its quota when the Account API reported one, and why the key
/// cannot search now.
pub(crate) struct KeyAccount {
    pub(crate) quota: Option<AccountQuota>,
    pub(crate) failure: Option<ProviderError>,
}

impl Serpapi {
    /// Checks every key in pool order. Each check sends only its own key, so the persistent
    /// credential cursor does not move.
    pub(crate) async fn accounts(&self) -> Vec<KeyAccount> {
        let url = account_url(&self.url);
        let mut accounts = Vec::with_capacity(self.credentials.len());
        for index in 0..self.credentials.len() {
            accounts.push(self.account(&url, index).await);
        }
        accounts
    }

    async fn account(&self, url: &str, index: usize) -> KeyAccount {
        let settings = self.settings(AttemptTarget::operation("account"));
        let result = execute_pinned(
            &self.credentials,
            index,
            settings,
            |credential, _| async move {
                let body = self.get(url, &[], &credential).await?;
                decode(&body.text)
                    .map(|quota| (Some(body.status), quota))
                    .map_err(|failure| self.redacted(failure))
            },
        )
        .await;
        match result {
            Ok(outcome) => KeyAccount {
                quota: Some(outcome.value),
                failure: verdict(outcome.value)
                    .err()
                    .map(|(kind, message)| ProviderError {
                        kind,
                        message,
                        attempts: outcome.attempts,
                        verbose: false,
                        diagnostic: outcome.diagnostic,
                        redirected_library_id: None,
                    }),
            },
            Err(error) => KeyAccount {
                quota: None,
                failure: Some(error),
            },
        }
    }
}

/// Returns the Account API URL on the origin of the search endpoint. An endpoint that is no URL
/// stays as it is, so the request fails the way a search would.
fn account_url(search_url: &str) -> String {
    let Ok(mut url) = reqwest::Url::parse(search_url) else {
        return search_url.to_owned();
    };
    url.set_path("/account.json");
    url.set_query(None);
    url.set_fragment(None);
    url.into()
}

fn decode(body: &str) -> Result<AccountQuota, AttemptFailure> {
    serde_json::from_str(body).map_err(|error| {
        refusal(
            AttemptErrorKind::Runtime,
            format!("invalid SerpApi account response: {error}"),
        )
    })
}

/// Decides whether a key with this quota can search now.
fn verdict(quota: AccountQuota) -> Result<(), (AttemptErrorKind, String)> {
    if quota.searches_left <= 0 {
        return Err((
            AttemptErrorKind::QuotaExhausted,
            "SerpApi account has no searches left".into(),
        ));
    }
    if quota.this_hour_searches >= quota.hourly_limit {
        return Err((
            AttemptErrorKind::RateLimited,
            format!(
                "SerpApi account used {} of its {} searches this hour",
                quota.this_hour_searches, quota.hourly_limit
            ),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{AccountQuota, account_url, decode, verdict};
    use crate::types::AttemptErrorKind;

    const HEALTHY: AccountQuota = AccountQuota {
        searches_left: 1,
        plan_searches_left: 1,
        this_month_usage: 249,
        this_hour_searches: 249,
        hourly_limit: 250,
    };

    #[test]
    fn a_key_fails_when_its_searches_or_this_hours_throughput_are_used_up() {
        let verdicts = [
            HEALTHY,
            AccountQuota {
                searches_left: 0,
                ..HEALTHY
            },
            AccountQuota {
                this_hour_searches: 250,
                ..HEALTHY
            },
        ]
        .map(|quota| verdict(quota).map_err(|(kind, _)| kind));

        assert_eq!(
            verdicts,
            [
                Ok(()),
                Err(AttemptErrorKind::QuotaExhausted),
                Err(AttemptErrorKind::RateLimited),
            ]
        );
    }

    #[test]
    fn the_account_endpoint_replaces_the_search_path_and_drops_its_query() {
        assert_eq!(
            account_url("https://serpapi.example/search.json?engine=google_scholar#top"),
            "https://serpapi.example/account.json"
        );
    }

    #[test]
    fn a_report_without_every_quota_field_is_a_runtime_failure() {
        let failure = decode(
            &json!({
                "total_searches_left": 219,
                "plan_searches_left": 219,
                "this_month_usage": 31,
                "account_rate_limit_per_hour": 250
            })
            .to_string(),
        )
        .unwrap_err();

        assert_eq!(
            (
                failure.kind,
                failure.message.starts_with(
                    "invalid SerpApi account response: missing field `this_hour_searches`"
                )
            ),
            (AttemptErrorKind::Runtime, true),
            "message: {}",
            failure.message
        );
    }
}
