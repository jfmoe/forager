use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use fs2::FileExt;

#[test]
fn non_interactive_setup_creates_a_complete_commented_template() {
    let config_dir = tempfile::tempdir().expect("create config directory");

    let output = run(config_dir.path(), &["setup", "--non-interactive"], None);
    let content = fs::read_to_string(config_dir.path().join("config.toml")).expect("read template");
    let value: toml::Value = toml::from_str(&content).expect("parse template");

    assert_eq!(
        (
            output.status.code(),
            count_leaves(&value),
            content.matches("# ").count(),
            value["classifier"]["keys"].as_array().map(Vec::len),
            value["providers"]["anysearch"]["keys"]
                .as_array()
                .map(Vec::len),
            content.contains("[providers.exa]"),
            String::from_utf8_lossy(&output.stdout).contains("forager doctor"),
        ),
        (Some(0), 48, 48, Some(0), Some(0), true, true),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn non_interactive_setup_refuses_to_overwrite_even_an_empty_file() {
    let config_dir = tempfile::tempdir().expect("create config directory");
    let path = config_dir.path().join("config.toml");
    fs::write(&path, "").expect("create empty config");

    let output = run(config_dir.path(), &["setup", "--non-interactive"], None);

    assert_eq!(
        (
            output.status.code(),
            fs::read_to_string(path).expect("read preserved config"),
        ),
        (Some(3), String::new())
    );
}

#[test]
fn setup_modes_time_out_without_writing_when_the_config_lock_is_held() {
    let config_dir = tempfile::tempdir().expect("create config directory");
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(config_dir.path().join(".config.lock"))
        .expect("open config lock");
    lock.lock_exclusive().expect("hold config lock");

    for arguments in [
        &["setup", "--lang", "en"][..],
        &["setup", "--non-interactive"][..],
    ] {
        let output = run(config_dir.path(), arguments, None);
        assert_eq!(output.status.code(), Some(3), "{output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("config lock timed out"),
            "stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert!(!config_dir.path().join("config.toml").exists());
}

#[test]
fn interactive_setup_holds_the_config_lock_until_its_update_is_saved() {
    let config_dir = tempfile::tempdir().expect("create config directory");
    let mut setup = Command::new(env!("CARGO_BIN_EXE_forager"))
        .args(["setup", "--lang", "en"])
        .env_clear()
        .env("FORAGER_CONFIG_DIR", config_dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn interactive setup");
    let lock_path = config_dir.path().join(".config.lock");
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        if let Ok(lock) = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)
        {
            match lock.try_lock_exclusive() {
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Ok(()) => FileExt::unlock(&lock).expect("release probe lock"),
                Err(error) => panic!("probe config lock: {error}"),
            }
        }
        assert!(
            Instant::now() < deadline,
            "setup did not acquire config lock"
        );
        thread::sleep(Duration::from_millis(5));
    }

    let edit = run(
        config_dir.path(),
        &["config", "set", "log.level", "debug"],
        None,
    );
    assert_eq!(edit.status.code(), Some(3), "{edit:?}");
    assert!(
        String::from_utf8_lossy(&edit.stderr).contains("config lock timed out"),
        "stderr: {}",
        String::from_utf8_lossy(&edit.stderr)
    );

    setup
        .stdin
        .take()
        .expect("open setup stdin")
        .write_all(b"\n\n\nn\n\n\n\n\n\n\n\n")
        .expect("complete setup input");
    let output = setup.wait_with_output().expect("wait for setup");
    assert_eq!(output.status.code(), Some(0), "{output:?}");
}

#[test]
fn interactive_setup_configures_the_four_stages_without_echoing_credentials() {
    let config_dir = tempfile::tempdir().expect("create config directory");
    let canary = "setup-canary-secret";
    let input = format!(
        "xai\nhttps://main.example/v1\n{canary}\nmain-model\ny\nhttps://classifier.example/v1\nclassifier-secret\nclassifier-model\nexa-secret\n\njina-secret\n\n\nanysearch-secret\n"
    );

    let output = run(config_dir.path(), &["setup", "--lang", "zh"], Some(&input));
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let config: toml::Value = toml::from_str(
        &fs::read_to_string(config_dir.path().join("config.toml")).expect("read config"),
    )
    .expect("parse config");

    assert_eq!(
        (
            output.status.code(),
            config.get("search").is_none(),
            config["providers"]["xai"]["model"].as_str(),
            config["classifier"]["model"].as_str(),
            config["providers"]["exa"]["keys"][0].as_str(),
            config["providers"]["jina"]["keys"][0].as_str(),
            config["providers"]["anysearch"]["keys"][0].as_str(),
            combined.contains("第 3 步：分类器（跳过后无法自动路由或生成 research 计划）"),
            combined.contains("forager doctor"),
            combined.contains(canary),
        ),
        (
            Some(0),
            true,
            Some("main-model"),
            Some("classifier-model"),
            Some("exa-secret"),
            Some("jina-secret"),
            Some("anysearch-secret"),
            true,
            true,
            false,
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn interactive_setup_second_run_preserves_skipped_and_unrelated_values() {
    let config_dir = tempfile::tempdir().expect("create config directory");
    let path = config_dir.path().join("config.toml");
    fs::write(
        &path,
        "# preserve\n[providers.xai]\nkeys = [\"old-main\"]\nmodel = \"old-model\"\n\
         [providers.exa]\nkeys = [\"old-exa\"]\ntimeout = 0\n\
         [unknown]\nvalue = true\n",
    )
    .expect("write existing config");
    let input = "\n\n\nnew-model\nn\nnew-exa\n\n\n\n\n\n";

    let output = run(config_dir.path(), &["setup", "--lang", "en"], Some(input));
    let content = fs::read_to_string(path).expect("read updated config");
    let config: toml::Value = toml::from_str(&content).expect("parse updated config");

    assert_eq!(
        (
            output.status.code(),
            config["providers"]["xai"]["keys"][0].as_str(),
            config["providers"]["xai"]["model"].as_str(),
            config["providers"]["exa"]["keys"][0].as_str(),
            config["providers"]["exa"]["timeout"].as_integer(),
            config["unknown"]["value"].as_bool(),
            content.contains("# preserve"),
        ),
        (
            Some(0),
            Some("old-main"),
            Some("new-model"),
            Some("new-exa"),
            Some(0),
            Some(true),
            true,
        )
    );
}

#[test]
fn interactive_setup_enter_preserves_a_single_backend_array() {
    let config_dir = tempfile::tempdir().expect("create config directory");
    let path = config_dir.path().join("config.toml");
    fs::write(
        &path,
        "[search]\nbackends = [\"openai_compatible\"]\n\
         [providers.openai_compatible]\nurl = \"https://main.example/v1\"\n\
         keys = [\"old-main\"]\nmodel = \"old-model\"\n\
         [classifier]\nurl = \"https://classifier.example/v1\"\n\
         keys = [\"classifier-key\"]\nmodel = \"classifier-model\"\n",
    )
    .expect("write existing config");

    let output = run(
        config_dir.path(),
        &["setup", "--lang", "en"],
        Some("\n\n\n\nn\n\n\n\n\n\n\n"),
    );
    let config: toml::Value =
        toml::from_str(&fs::read_to_string(path).expect("read config")).expect("parse config");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(
        (
            output.status.code(),
            config["search"]["backends"].as_array().map(Vec::len),
            config["search"]["backends"][0].as_str(),
            stderr.contains("Step 3: classifier (skip preserves current values)"),
            stderr.contains("skipping disables automatic routing and research plans"),
        ),
        (Some(0), Some(1), Some("openai_compatible"), true, false,),
        "stderr: {stderr}"
    );
}

#[test]
fn interactive_setup_rejects_a_malformed_document_without_replacing_it() {
    let config_dir = tempfile::tempdir().expect("create config directory");
    let path = config_dir.path().join("config.toml");
    let malformed = "[providers.exa\nkeys = [";
    fs::write(&path, malformed).expect("write malformed config");

    let output = run(config_dir.path(), &["setup", "--lang", "zh"], Some("\n"));

    assert_eq!(
        (
            output.status.code(),
            fs::read_to_string(path).expect("read malformed config"),
        ),
        (Some(3), malformed.to_owned())
    );
}

#[test]
fn setup_does_not_enable_subcommand_inference() {
    let config_dir = tempfile::tempdir().expect("create config directory");

    let output = run(config_dir.path(), &["setu", "--non-interactive"], None);

    assert_eq!(output.status.code(), Some(2));
}

#[cfg(unix)]
fn private_modes(config_dir: &Path) -> (u32, u32, usize) {
    use std::os::unix::fs::PermissionsExt;

    let directory_mode = config_dir
        .metadata()
        .expect("read directory metadata")
        .permissions()
        .mode()
        & 0o777;
    let file_mode = config_dir
        .join("config.toml")
        .metadata()
        .expect("read file metadata")
        .permissions()
        .mode()
        & 0o777;
    let temporary_files = fs::read_dir(config_dir)
        .expect("read config directory")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
        .count();
    (directory_mode, file_mode, temporary_files)
}

#[cfg(unix)]
#[test]
fn both_setup_modes_enforce_private_permissions_and_remove_temporary_files() {
    let interactive_dir = tempfile::tempdir().expect("create interactive config directory");
    let output = run(
        interactive_dir.path(),
        &["setup", "--lang", "en"],
        Some("\n\n\nn\n\n\n\n\n\n\n\n"),
    );
    assert_eq!(output.status.code(), Some(0), "{output:?}");

    let template_dir = tempfile::tempdir().expect("create template config directory");
    let output = run(template_dir.path(), &["setup", "--non-interactive"], None);
    assert_eq!(output.status.code(), Some(0), "{output:?}");

    assert_eq!(
        (
            private_modes(interactive_dir.path()),
            private_modes(template_dir.path()),
        ),
        ((0o700, 0o600, 0), (0o700, 0o600, 0))
    );
}

fn count_leaves(value: &toml::Value) -> usize {
    match value {
        toml::Value::Table(table) => table.values().map(count_leaves).sum(),
        _ => 1,
    }
}

fn run(config_dir: &Path, args: &[&str], stdin: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_forager"));
    command
        .args(args)
        .env_clear()
        .env("FORAGER_CONFIG_DIR", config_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if stdin.is_some() {
        command.stdin(Stdio::piped());
    } else {
        command.stdin(Stdio::null());
    }
    let mut child = command.spawn().expect("spawn forager");
    if let Some(stdin) = stdin {
        child
            .stdin
            .take()
            .expect("open stdin")
            .write_all(stdin.as_bytes())
            .expect("write stdin");
    }
    child.wait_with_output().expect("wait forager")
}
