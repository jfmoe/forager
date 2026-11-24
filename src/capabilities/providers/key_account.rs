//! The account check of one key, shared by the routes that declare an account check.
//!
//! A route checks every key of its pool on its own; the doctor reports each key's result and
//! fails the probe on the first key that cannot serve requests now.

use super::serpapi::KeyQuota;
use crate::types::ProviderError;

/// The account check of one key: what it learned of the key's quota, and why the key cannot
/// serve requests now.
pub(crate) struct KeyAccount {
    /// `None` for a service that reports no quota for its keys.
    pub(crate) quota: Option<KeyQuota>,
    pub(crate) failure: Option<ProviderError>,
}
