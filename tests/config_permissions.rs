mod support;

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

    fn mode(path: &std::path::Path) -> u32 {
        path.metadata().expect("read metadata").permissions().mode() & 0o777
    }
}

#[cfg(windows)]
mod windows {
    use std::path::Path;

    use forager::config::{create_private_file, ensure_private_directory};
    use serde_json::Value;
    use winapi::um::winnt::{FILE_ALL_ACCESS, PSID};
    use windows_acl::acl::{ACL, AceType};
    use windows_acl::helper::{sid_to_string, string_to_sid};

    use super::support::smoke::SmokeEnvironment;

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

    // `doctor` and offline `smoke` share this read-side check; smoke reports it as one field.
    #[test]
    fn configuration_access_granted_beyond_the_windows_owner_is_rejected() {
        let environment = SmokeEnvironment::new(minimal_config);
        let owner_only = permissions_ok(&environment);

        let config_file = environment.config_dir.join("config.toml");
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

        assert_eq!((owner_only, permissions_ok(&environment)), (true, false));
    }

    fn permissions_ok(environment: &SmokeEnvironment) -> bool {
        let output = environment.run(&["smoke"]);
        let payload: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "parse smoke JSON: {error}; stderr: {}",
                String::from_utf8_lossy(&output.stderr)
            )
        });
        payload["permissions"]["ok"]
            .as_bool()
            .expect("smoke reports the permission check")
    }

    // Debug path formatting supplies the quoted and escaped TOML string literal.
    #[expect(clippy::unnecessary_debug_formatting)]
    fn minimal_config(journal_dir: &Path) -> String {
        format!(
            "[providers.xai]\nurl = \"http://127.0.0.1:9\"\nkeys = [\"xai-secret\"]\n[journal]\ndir = {journal_dir:?}\n"
        )
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
