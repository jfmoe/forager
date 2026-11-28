#[path = "support/isolation.rs"]
mod isolation;
#[expect(
    dead_code,
    reason = "these tests start only the binary, never a program that keeps its environment"
)]
#[path = "support/process.rs"]
mod process;

use std::path::{Path, PathBuf};

/// Runs one retrieval command that ends before any request, isolated under `root`, and returns
/// the telemetry directory and the record it wrote.
fn record_one_invocation(root: &Path) -> (PathBuf, PathBuf) {
    let config_dir = root.join("config");
    std::fs::create_dir_all(&config_dir).expect("create config directory");
    let mut command = isolation::command_with_empty_environment(
        env!("CARGO_BIN_EXE_forager"),
        process::CLOSED_PROXY,
    );
    command
        .env("HOME", root)
        .env("FORAGER_CONFIG_DIR", &config_dir)
        .env("XDG_STATE_HOME", root.join("state"))
        .args(["map", "https://example.test"]);
    let output = process::run_command(&mut command, None);
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    let directory = root.join("state").join("forager").join("telemetry");
    let record = std::fs::read_dir(&directory)
        .expect("read telemetry directory")
        .map(|entry| entry.expect("a telemetry entry").path())
        .find(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .expect("a telemetry record");
    (directory, record)
}

#[cfg(unix)]
mod unix {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    use forager::config::{create_private_file, ensure_private_directory};

    #[test]
    fn config_write_primitives_enforce_private_permissions() {
        let root = tempfile::tempdir().expect("create temporary root");
        let config_dir = root.path().join("forager");
        let config_file = config_dir.join("config.toml");
        let temporary_file = config_dir.join(".config.toml.tmp");

        fs::create_dir_all(&config_dir).expect("create broad config directory");
        fs::set_permissions(&config_dir, fs::Permissions::from_mode(0o777))
            .expect("set broad directory permissions");
        fs::write(&config_file, "").expect("create broad config file");
        fs::set_permissions(&config_file, fs::Permissions::from_mode(0o666))
            .expect("set broad config permissions");
        fs::write(&temporary_file, "").expect("create broad temporary file");
        fs::set_permissions(&temporary_file, fs::Permissions::from_mode(0o666))
            .expect("set broad temporary permissions");

        ensure_private_directory(&config_dir).expect("create private config directory");
        create_private_file(&config_file).expect("create private config file");
        create_private_file(&temporary_file).expect("create private temporary file");

        let modes = [mode(&config_dir), mode(&config_file), mode(&temporary_file)];
        assert_eq!(modes, [0o700, 0o600, 0o600]);
    }

    #[test]
    fn telemetry_records_are_private() {
        let root = tempfile::tempdir().expect("create temporary root");

        let (directory, record) = super::record_one_invocation(root.path());

        assert_eq!([mode(&directory), mode(&record)], [0o700, 0o600]);
    }

    fn mode(path: &std::path::Path) -> u32 {
        path.metadata().expect("read metadata").permissions().mode() & 0o777
    }
}

#[cfg(windows)]
mod windows {
    use std::path::Path;

    use forager::config::{create_private_file, ensure_private_directory, has_private_permissions};
    use winapi::um::winnt::{FILE_ALL_ACCESS, PSID};
    use windows_acl::acl::{ACL, AceType};
    use windows_acl::helper::{sid_to_string, string_to_sid};

    #[test]
    fn config_write_primitives_restrict_access_to_the_windows_owner() {
        let root = tempfile::tempdir().expect("create temporary root");
        let config_dir = root.path().join("forager");
        let config_file = config_dir.join("config.toml");
        let temporary_file = config_dir.join(".config.toml.tmp");

        ensure_private_directory(&config_dir).expect("create private config directory");
        create_private_file(&config_file).expect("create private config file");
        create_private_file(&temporary_file).expect("create private temporary file");

        let owner_only = [&config_dir, &config_file, &temporary_file]
            .into_iter()
            .all(|path| acl_is_owner_only(path));
        assert!(owner_only);
    }

    #[test]
    fn telemetry_records_are_restricted_to_the_windows_owner() {
        let root = tempfile::tempdir().expect("create temporary root");

        let (directory, record) = super::record_one_invocation(root.path());

        assert!(acl_is_owner_only(&directory) && acl_is_owner_only(&record));
    }

    // `doctor` reads the configuration through this check.
    #[test]
    fn configuration_access_granted_beyond_the_windows_owner_is_rejected() {
        let root = tempfile::tempdir().expect("create temporary root");
        let config_dir = root.path().join("forager");
        let config_file = config_dir.join("config.toml");
        ensure_private_directory(&config_dir).expect("create private config directory");
        create_private_file(&config_file).expect("create private config file");
        let owner_only = is_private(&config_file);

        let mut acl = ACL::from_file_path(config_file.to_str().expect("Unicode test path"), false)
            .expect("read config ACL");
        let everyone = string_to_sid("S-1-1-0").expect("create Everyone SID");
        acl.add_entry(
            everyone.as_ptr() as PSID,
            AceType::AccessAllow,
            0,
            FILE_ALL_ACCESS,
        )
        .expect("broaden config ACL");

        assert_eq!((owner_only, is_private(&config_file)), (true, false));
    }

    fn is_private(config_file: &Path) -> bool {
        has_private_permissions(config_file, 0o600).expect("read the config file's ACL")
    }

    fn acl_is_owner_only(path: &Path) -> bool {
        let acl = ACL::from_file_path(path.to_str().expect("Unicode test path"), false)
            .expect("read ACL");
        let entries = acl.all().expect("enumerate ACL");
        !entries.is_empty()
            && entries.iter().all(|entry| {
                let is_owner = entry.sid.as_ref().is_some_and(|sid| {
                    sid_to_string(sid.as_ptr() as PSID).expect("format SID") == "S-1-3-4"
                });
                let is_allow = matches!(
                    entry.entry_type,
                    AceType::AccessAllow
                        | AceType::AccessAllowCallback
                        | AceType::AccessAllowObject
                        | AceType::AccessAllowCallbackObject
                );
                is_owner && is_allow
            })
    }
}
