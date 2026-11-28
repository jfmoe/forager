use std::fs;
use std::path::Path;
use std::process::Output;

use crate::support::{forager_command, run_command, stderr};

#[test]
fn config_path_does_not_load_the_configuration_file() {
    let config_dir = tempfile::tempdir().expect("create temporary config directory");
    let xdg_config_home = tempfile::tempdir().expect("create alternate XDG directory");
    let home = tempfile::tempdir().expect("create alternate home");
    fs::write(
        config_dir.path().join("config.toml"),
        "[search]\nno_such_key = \"x\"\n",
    )
    .expect("write config with an undefined key");

    let output = run(&[
        ("FORAGER_CONFIG_DIR", config_dir.path()),
        ("XDG_CONFIG_HOME", xdg_config_home.path()),
        ("HOME", home.path()),
    ]);

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("{}\n", config_dir.path().join("config.toml").display()),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(stderr(&output), "");
}

#[test]
fn config_path_uses_xdg_config_home() {
    let xdg_config_home = tempfile::tempdir().expect("create temporary XDG directory");

    let output = run(&[("XDG_CONFIG_HOME", xdg_config_home.path())]);

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!(
            "{}\n",
            xdg_config_home.path().join("forager/config.toml").display()
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(stderr(&output), "");
}

#[test]
fn config_path_uses_the_xdg_default_below_home() {
    let home = tempfile::tempdir().expect("create temporary home");

    let output = run(&[("HOME", home.path())]);

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!(
            "{}\n",
            home.path().join(".config/forager/config.toml").display()
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(stderr(&output), "");
}

#[test]
fn config_path_reports_an_unavailable_default_directory() {
    let output = run(&[]);

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

    let output = run(&[("XDG_CONFIG_HOME", xdg_config_home.path())]);

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

fn run(variables: &[(&str, &Path)]) -> Output {
    let mut command = forager_command();
    command.args(["config", "path"]);
    for (name, value) in variables {
        command.env(name, value);
    }
    run_command(&mut command, None)
}
