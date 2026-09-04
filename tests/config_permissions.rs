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
    use forager::config::{create_private_file, ensure_private_directory};
    use winapi::um::winnt::PSID;
    use windows_acl::acl::{ACL, AceType};
    use windows_acl::helper::sid_to_string;

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

    fn acl_is_owner_only(path: &std::path::Path) -> bool {
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
