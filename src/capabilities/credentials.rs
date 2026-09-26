use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};

use crate::redact::{CREDENTIAL_MASK, Secret};
use crate::state_file::{self, StateLock, serialized_blocking};

const STATE_SCHEMA_VERSION: u8 = 1;
const LOCK_WAIT: Duration = Duration::from_millis(100);

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
        Self {
            provider,
            keys,
            state_file: state_file::state_directory()
                .map(|directory| directory.join("credential_pool_state.json")),
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.keys.len()
    }

    pub(crate) fn key(&self, index: usize) -> &Secret {
        &self.keys[index]
    }

    pub(crate) async fn claim(&self) -> CredentialSelection {
        let Some(state_file) = &self.state_file else {
            return CredentialSelection {
                index: 0,
                diagnostic: Some(
                    "credential cursor state is unavailable; using optimistic selection".into(),
                ),
            };
        };
        let state_file = state_file.clone();
        let provider = self.provider;
        let key_count = self.keys.len();
        match serialized_blocking(move || claim_persistent_index(&state_file, provider, key_count))
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
    Ok(serde_json::from_str(&content).map_or_else(
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
mod tests {
    use std::fs;

    use serde_json::json;
    use tempfile::tempdir;

    use super::claim_persistent_index;

    #[test]
    fn persistent_claims_round_robin_independently_per_provider() {
        let directory = tempdir().expect("create state directory");
        let path = directory.path().join("credential_pool_state.json");

        let claims = [
            claim_persistent_index(&path, "exa", 3)
                .expect("first Exa claim")
                .0,
            claim_persistent_index(&path, "exa", 3)
                .expect("second Exa claim")
                .0,
            claim_persistent_index(&path, "tavily", 2)
                .expect("first Tavily claim")
                .0,
            claim_persistent_index(&path, "exa", 3)
                .expect("third Exa claim")
                .0,
            claim_persistent_index(&path, "tavily", 2)
                .expect("second Tavily claim")
                .0,
        ];

        assert_eq!(claims, [0, 1, 0, 2, 1]);
    }

    #[test]
    fn persistent_claims_reset_corrupt_state_and_fold_shrunken_key_counts() {
        let directory = tempdir().expect("create state directory");
        let path = directory.path().join("credential_pool_state.json");
        fs::write(&path, "not json").expect("write corrupt cursor state");

        let corrupt = claim_persistent_index(&path, "exa", 3).expect("claim corrupt state");
        fs::write(
            &path,
            serde_json::to_vec(&json!({
                "schema_version": 1,
                "providers": {"exa": {"next_index": 5}}
            }))
            .expect("serialize cursor state"),
        )
        .expect("write cursor state");
        let shrunken = claim_persistent_index(&path, "exa", 2).expect("claim shrunken pool");

        assert_eq!(
            (corrupt.0, corrupt.1.as_deref(), shrunken.0),
            (
                0,
                Some("credential cursor state was corrupt and has been reset"),
                1,
            )
        );
    }

    #[cfg(unix)]
    #[test]
    fn persistent_claim_restricts_existing_state_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempdir().expect("create state directory");
        let state_directory = directory.path().join("forager");
        let path = state_directory.join("credential_pool_state.json");
        let lock_path = state_directory.join("credential_pool_state.lock");
        fs::create_dir_all(&state_directory).expect("create broad state directory");
        fs::set_permissions(&state_directory, fs::Permissions::from_mode(0o777))
            .expect("set broad state directory permissions");
        fs::write(&path, r#"{"schema_version":1,"providers":{}}"#)
            .expect("create broad state file");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o666))
            .expect("set broad state file permissions");
        fs::write(&lock_path, "").expect("create broad lock file");
        fs::set_permissions(&lock_path, fs::Permissions::from_mode(0o666))
            .expect("set broad lock file permissions");

        claim_persistent_index(&path, "exa", 2).expect("claim credential index");

        let modes = (
            fs::metadata(&state_directory)
                .expect("state directory metadata")
                .permissions()
                .mode()
                & 0o777,
            fs::metadata(&path)
                .expect("state file metadata")
                .permissions()
                .mode()
                & 0o777,
            fs::metadata(lock_path)
                .expect("lock file metadata")
                .permissions()
                .mode()
                & 0o777,
        );

        assert_eq!(modes, (0o700, 0o600, 0o600));
    }
}
