use std::fs::{self, OpenOptions};

use fs2::FileExt;
use serde_json::{Value, json};
use tempfile::tempdir;

use super::{CredentialPool, claim_persistent_index};

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
fn a_pool_rotates_through_the_state_file_its_caller_supplies() {
    let directory = tempdir().expect("create state directory");
    let path = directory.path().join("cursor.json");
    let pool = CredentialPool::in_state_file(
        "exa",
        vec!["first".into(), "second".into()],
        Some(path.clone()),
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("test runtime");

    let claims = runtime.block_on(async {
        let first = pool.claim().await;
        let second = pool.claim().await;
        [
            (first.index, first.diagnostic),
            (second.index, second.diagnostic),
        ]
    });

    assert_eq!((claims, path.is_file()), ([(0, None), (1, None)], true));
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

#[test]
fn a_corrupt_provider_cursor_is_reset_alone_and_the_repair_persists() {
    let directory = tempdir().expect("create state directory");
    let path = directory.path().join("credential_pool_state.json");
    fs::write(
        &path,
        r#"{"schema_version":1,"providers":{"exa":{"next_index":"bad"},"other":{"next_index":7}}}"#,
    )
    .expect("write corrupt provider cursor");

    let (index, diagnostic) = claim_persistent_index(&path, "exa", 2).expect("claim cursor");
    let state: Value = serde_json::from_slice(&fs::read(&path).expect("read repaired state"))
        .expect("parse repaired state");

    assert_eq!(
        (index, diagnostic.as_deref(), &state["providers"]),
        (
            0,
            Some("credential cursor for exa was reset"),
            &json!({"exa": {"next_index": 1}, "other": {"next_index": 7}}),
        )
    );
}

#[test]
fn a_busy_cursor_lock_falls_back_to_optimistic_selection() {
    let directory = tempdir().expect("create state directory");
    let path = directory.path().join("credential_pool_state.json");
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path.with_extension("lock"))
        .expect("open credential lock");
    lock.lock_exclusive().expect("hold credential lock");
    let pool = CredentialPool::in_state_file(
        "exa",
        vec!["first".into(), "second".into()],
        Some(path.clone()),
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("test runtime");

    let selection = runtime.block_on(pool.claim());
    FileExt::unlock(&lock).expect("release credential lock");

    assert_eq!(
        (
            selection.index,
            selection
                .diagnostic
                .is_some_and(|diagnostic| diagnostic.contains("optimistic selection")),
            path.exists(),
        ),
        (0, true, false)
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
    fs::write(&path, r#"{"schema_version":1,"providers":{}}"#).expect("create broad state file");
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
