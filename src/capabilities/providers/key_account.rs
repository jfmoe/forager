//! The account check of one key, shared by the routes that register an account probe.
//!
//! A route checks every key of its pool on its own; the doctor reports each key's result and
//! fails the probe on the first key that cannot serve requests now.

use crate::types::ProviderError;

/// The account check of one key: what it learned of the key's quota, and why the key cannot
/// serve requests now.
pub(crate) struct KeyAccount {
    pub(crate) quota: KeyQuota,
    pub(crate) failure: Option<ProviderError>,
}

/// What an account check learned of one key's quota.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum KeyQuota {
    /// The service reports no quota for its keys.
    Unreported,
    /// The service reports quotas, but the check could not read this key's.
    Unread,
    Read(AccountQuota),
}

/// The search quota of one key, as the service reports it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AccountQuota {
    pub(crate) searches_left: i64,
    pub(crate) plan_searches_left: i64,
    pub(crate) this_month_usage: i64,
    pub(crate) this_hour_searches: i64,
    pub(crate) hourly_limit: i64,
}
