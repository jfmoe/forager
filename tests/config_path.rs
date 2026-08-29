mod support;

use std::fs;
use std::process::Command;

use support::run_command;

#[test]
fn config_path_does_not_load_a_removed_validation_key() {
    let config_dir = tempfile::tempdir().expect("create temporary config directory");
    let xdg_config_home = tempfile::tempdir().expect("create alternate XDG directory");
    let home = tempfile::tempdir().expect("create alternate home");
    fs::write(
        config_dir.path().join("config.toml"),
        "[search]\nvalidation = \"strict\"\n",
    )
    .expect("write legacy config");

    let mut command = Command::new(env!("CARGO_BIN_EXE_forager"));
    command
        .args(["config", "path"])
        .env_clear()
        .env("FORAGER_CONFIG_DIR", config_dir.path())
        .env("XDG_CONFIG_HOME", xdg_config_home.path())
        .env("HOME", home.path());
    let output = run_command(&mut command, None);

    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("{}\n", config_dir.path().join("config.toml").display()),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn config_path_uses_xdg_config_home() {
    let xdg_config_home = tempfile::tempdir().expect("create temporary XDG directory");

    let mut command = Command::new(env!("CARGO_BIN_EXE_forager"));
    command
        .args(["config", "path"])
        .env_clear()
        .env("XDG_CONFIG_HOME", xdg_config_home.path());
    let output = run_command(&mut command, None);

    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!(
            "{}\n",
            xdg_config_home.path().join("forager/config.toml").display()
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn config_path_uses_the_xdg_default_below_home() {
    let home = tempfile::tempdir().expect("create temporary home");

    let mut command = Command::new(env!("CARGO_BIN_EXE_forager"));
    command
        .args(["config", "path"])
        .env_clear()
        .env("HOME", home.path());
    let output = run_command(&mut command, None);

    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!(
            "{}\n",
            home.path().join(".config/forager/config.toml").display()
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn config_path_reports_an_unavailable_default_directory() {
    let mut command = Command::new(env!("CARGO_BIN_EXE_forager"));
    command.args(["config", "path"]).env_clear();
    let output = run_command(&mut command, None);

    assert_eq!(
        (
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).contains("FORAGER_CONFIG_DIR")
        ),
        (Some(3), true),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(unix)]
#[test]
fn config_path_reports_an_unwritable_default_directory() {
    use std::os::unix::fs::PermissionsExt;

    let xdg_config_home = tempfile::tempdir().expect("create temporary XDG directory");
    fs::set_permissions(xdg_config_home.path(), fs::Permissions::from_mode(0o500))
        .expect("make XDG directory read-only");

    let mut command = Command::new(env!("CARGO_BIN_EXE_forager"));
    command
        .args(["config", "path"])
        .env_clear()
        .env("XDG_CONFIG_HOME", xdg_config_home.path());
    let output = run_command(&mut command, None);

    fs::set_permissions(xdg_config_home.path(), fs::Permissions::from_mode(0o700))
        .expect("restore XDG directory permissions");

    assert_eq!(
        (
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).contains("FORAGER_CONFIG_DIR")
        ),
        (Some(3), true),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
