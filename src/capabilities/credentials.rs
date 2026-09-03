use std::env;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};
use std::thread;
use std::time::{Duration, Instant};

use fs2::FileExt;
use serde_json::{Value, json};
use tokio::sync::Mutex;

use crate::redact::{CREDENTIAL_MASK, Secret};
use crate::secure_fs::{create_private_file, ensure_private_directory};

const STATE_SCHEMA_VERSION: u8 = 1;
const LOCK_WAIT: Duration = Duration::from_millis(100);
static CLAIM_MUTEX: LazyLock<Arc<Mutex<()>>> = LazyLock::new(|| Arc::new(Mutex::new(())));

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
            state_file: state_file(),
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
        match serialized_blocking_claim(move || {
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

    pub(crate) fn rotated_index(&self, start: usize, rotation_count: usize) -> usize {
        (start + rotation_count) % self.keys.len()
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

fn state_file() -> Option<PathBuf> {
    env::var_os("XDG_STATE_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .map(|path| path.join("forager/credential_pool_state.json"))
        .or_else(|| {
            env::var_os("HOME")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .map(|path| path.join(".local/state/forager/credential_pool_state.json"))
        })
}

pub(crate) fn state_directory() -> Option<PathBuf> {
    state_file().and_then(|path| path.parent().map(Path::to_path_buf))
}

fn claim_persistent_index(
    path: &Path,
    provider: &str,
    key_count: usize,
) -> io::Result<(usize, Option<String>)> {
    debug_assert!(key_count > 0);
    let directory = path
        .parent()
        .ok_or_else(|| io::Error::other("credential state path has no parent"))?;
    ensure_private_directory(directory)?;
    let lock_path = directory.join("credential_pool_state.lock");
    let lock = open_private_lock(&lock_path)?;
    acquire_bounded_lock(&lock)?;

    let result = (|| {
        let (mut state, mut diagnostic) = read_state(path)?;
        if state.get("schema_version").and_then(Value::as_u64)
            != Some(u64::from(STATE_SCHEMA_VERSION))
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
        write_state(path, &state)?;
        Ok((index, diagnostic))
    })();
    let _ = FileExt::unlock(&lock);
    result
}

fn acquire_bounded_lock(file: &File) -> io::Result<()> {
    let deadline = Instant::now() + LOCK_WAIT;
    loop {
        match file.try_lock_exclusive() {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err(io::Error::new(
                        io::ErrorKind::WouldBlock,
                        "credential state lock timed out",
                    ));
                }
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => return Err(error),
        }
    }
}

fn empty_state() -> Value {
    json!({
        "schema_version": STATE_SCHEMA_VERSION,
        "providers": {}
    })
}

fn read_state(path: &Path) -> io::Result<(Value, Option<String>)> {
    let mut content = String::new();
    match File::open(path) {
        Ok(mut file) => {
            file.read_to_string(&mut content)?;
            match serde_json::from_str(&content) {
                Ok(state) => Ok((state, None)),
                Err(_) => Ok((
                    empty_state(),
                    Some("credential cursor state was corrupt and has been reset".into()),
                )),
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok((empty_state(), None)),
        Err(error) => Err(error),
    }
}

fn write_state(path: &Path, state: &Value) -> io::Result<()> {
    let directory = path
        .parent()
        .ok_or_else(|| io::Error::other("credential state path has no parent"))?;
    let temporary = directory.join(format!(".credential_pool_state.{}.tmp", std::process::id()));
    let result = (|| {
        let mut file = create_private_file(&temporary)?;
        file.set_len(0)?;
        serde_json::to_writer(&mut file, state).map_err(io::Error::other)?;
        file.write_all(b"\n")?;
        file.flush()?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        drop(create_private_file(path)?);
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn open_private_lock(path: &Path) -> io::Result<File> {
    create_private_file(path)
}

async fn serialized_blocking_claim<T, F>(claim: F) -> Result<T, tokio::task::JoinError>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    let guard = Arc::clone(&CLAIM_MUTEX).lock_owned().await;
    tokio::task::spawn_blocking(move || {
        let _guard = guard;
        claim()
    })
    .await
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::serialized_blocking_claim;

    #[test]
    fn cancelling_claim_waiter_keeps_blocking_claims_serialized() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_time()
            .build()
            .expect("build test runtime");

        runtime.block_on(async {
            let (first_entered_tx, first_entered_rx) = mpsc::channel();
            let (release_first_tx, release_first_rx) = mpsc::channel();
            let first = tokio::spawn(serialized_blocking_claim(move || {
                first_entered_tx.send(()).expect("signal first claim");
                release_first_rx.recv().expect("release first claim");
            }));
            first_entered_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("first claim entered blocking work");
            first.abort();

            let (second_entered_tx, second_entered_rx) = mpsc::channel();
            let second = tokio::spawn(serialized_blocking_claim(move || {
                second_entered_tx.send(()).expect("signal second claim");
            }));
            assert!(
                second_entered_rx
                    .recv_timeout(Duration::from_millis(50))
                    .is_err(),
                "cancelling the waiter released the process claim lock"
            );

            release_first_tx.send(()).expect("release first claim");
            second
                .await
                .expect("join second claim")
                .expect("second claim");
        });
    }
}
