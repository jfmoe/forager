use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};

use crate::redact::{CREDENTIAL_MASK, Secret};
use crate::state_file::{self, StateLock, serialized_blocking};

const STATE_FILE: &str = "credential_pool_state.json";
const STATE_SCHEMA_VERSION: u8 = 1;
#[cfg(not(test))]
const LOCK_WAIT: Duration = Duration::from_millis(100);
// The test build's cursor state lives in a directory of the test process, whose claims
// `serialized_blocking` already orders, so only a test that holds the lock itself finds it busy
// and need not wait for it.
#[cfg(test)]
const LOCK_WAIT: Duration = Duration::ZERO;

#[derive(Clone, Debug)]
pub(crate) struct CredentialPool {
    provider: &'static str,
    keys: Vec<Secret>,
    state_file: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub(crate) struct CredentialSelection {
    pub(crate) index: usize,
    pub(crate) diagnostic: Option<String>,
}

impl CredentialPool {
    pub(crate) fn new(provider: &'static str, keys: Vec<Secret>) -> Self {
        Self::in_state_file(
            provider,
            keys,
            state_file::state_directory().map(|directory| directory.join(STATE_FILE)),
        )
    }

    /// A pool whose cursor lives in `state_file`; without one, claims fall back to optimistic
    /// selection.
    pub(crate) fn in_state_file(
        provider: &'static str,
        keys: Vec<Secret>,
        state_file: Option<PathBuf>,
    ) -> Self {
        Self {
            provider,
            keys,
            state_file,
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.keys.len()
    }

    pub(crate) fn key(&self, index: usize) -> &Secret {
        &self.keys[index]
    }

    pub(crate) async fn claim(&self) -> CredentialSelection {
        // One key has nothing to rotate, so its claim skips the cursor's lock and state write.
        if self.keys.len() == 1 {
            return CredentialSelection {
                index: 0,
                diagnostic: None,
            };
        }
        let Some(path) = &self.state_file else {
            return CredentialSelection {
                index: 0,
                diagnostic: Some(
                    "credential cursor state is unavailable; using optimistic selection".into(),
                ),
            };
        };
        let state_file = path.clone();
        let provider = self.provider;
        let key_count = self.keys.len();
        match serialized_blocking(path, move || {
            claim_persistent_index(&state_file, provider, key_count)
        })
        .await
        {
            Ok(Ok((index, diagnostic))) => CredentialSelection { index, diagnostic },
            Ok(Err(error)) => CredentialSelection {
                index: 0,
                diagnostic: Some(format!(
                    "credential cursor unavailable; using optimistic selection: {error}"
                )),
            },
            Err(error) => CredentialSelection {
                index: 0,
                diagnostic: Some(format!(
                    "credential cursor task failed; using optimistic selection: {error}"
                )),
            },
        }
    }

    pub(crate) fn redact(&self, message: &str) -> String {
        self.keys.iter().fold(message.to_owned(), |redacted, key| {
            let key = key.expose();
            if redacted.contains(key) {
                redacted.replace(key, CREDENTIAL_MASK)
            } else {
                redacted
            }
        })
    }
}

fn claim_persistent_index(
    path: &Path,
    provider: &str,
    key_count: usize,
) -> io::Result<(usize, Option<String>)> {
    debug_assert!(key_count > 0);
    let _lock = StateLock::acquire(&path.with_extension("lock"), LOCK_WAIT)?;
    let (mut state, mut diagnostic) = read_state(path)?;
    if state.get("schema_version").and_then(Value::as_u64) != Some(u64::from(STATE_SCHEMA_VERSION))
    {
        state = empty_state();
        diagnostic = Some("credential cursor schema was reset".into());
    }
    if state.get("providers").and_then(Value::as_object).is_none() {
        state = empty_state();
        diagnostic = Some("credential cursor providers were reset".into());
    }
    let providers = state
        .get_mut("providers")
        .and_then(Value::as_object_mut)
        .expect("validated cursor state has a providers object");
    let next_index = providers
        .get(provider)
        .and_then(|cursor| cursor.get("next_index"))
        .and_then(Value::as_u64)
        .and_then(|index| usize::try_from(index).ok());
    if providers.contains_key(provider) && next_index.is_none() {
        diagnostic = Some(format!("credential cursor for {provider} was reset"));
    }
    let index = next_index.unwrap_or(0) % key_count;
    providers.insert(
        provider.to_owned(),
        json!({"next_index": (index + 1) % key_count}),
    );
    state_file::write_state(path, &state)?;
    Ok((index, diagnostic))
}

fn empty_state() -> Value {
    json!({
        "schema_version": STATE_SCHEMA_VERSION,
        "providers": {}
    })
}

fn read_state(path: &Path) -> io::Result<(Value, Option<String>)> {
    let Some(content) = state_file::read_state(path)? else {
        return Ok((empty_state(), None));
    };
    Ok(serde_json::from_slice(&content).map_or_else(
        |_| {
            (
                empty_state(),
                Some("credential cursor state was corrupt and has been reset".into()),
            )
        },
        |state| (state, None),
    ))
}

#[cfg(test)]
#[path = "credentials_tests.rs"]
mod tests;
