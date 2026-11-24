//! Projections that let a route test compare an outcome as a whole: a failure as its kind and
//! message, and a decoded value as JSON.

use serde_json::Value;

use super::shared::Refusal;
use crate::types::{AttemptErrorKind, ProviderError};

/// The kind and message of a failed route call.
pub(super) fn failure<T>(result: Result<T, ProviderError>) -> (AttemptErrorKind, String) {
    match result {
        Ok(_) => panic!("expected a failure"),
        Err(error) => (error.kind, error.message),
    }
}

/// A decoding step's result, with its refusal as a kind and a message.
pub(super) fn refused<T>(result: Result<T, Refusal>) -> Result<T, (AttemptErrorKind, String)> {
    result.map_err(|refusal| (refusal.kind, refusal.message))
}

pub(super) fn to_json<T: serde::Serialize + ?Sized>(value: &T) -> Value {
    serde_json::to_value(value).expect("serialize")
}
