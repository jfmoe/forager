mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use support::run_command;
use tempfile::TempDir;

#[cfg(unix)]
#[test]
fn unix_gate_accepts_a_valid_release_archive() {
    let fixture = UnixFixture::new(true);

    let output = fixture.verify(env!("CARGO_PKG_VERSION"), &fixture.file_arch);

    assert!(output.status.success(), "{output:?}");
}

#[cfg(unix)]
#[test]
fn unix_gate_rejects_an_archive_without_the_binary() {
    let fixture = UnixFixture::new(false);

    let output = fixture.verify(env!("CARGO_PKG_VERSION"), &fixture.file_arch);

    assert!(!output.status.success(), "{output:?}");
}

#[cfg(unix)]
#[test]
fn unix_gate_rejects_a_version_mismatch() {
    let fixture = UnixFixture::new(true);

    let output = fixture.verify("0.0.0-fixture", &fixture.file_arch);

    assert!(!output.status.success(), "{output:?}");
}

#[cfg(unix)]
#[test]
fn unix_gate_rejects_an_architecture_mismatch() {
    let fixture = UnixFixture::new(true);

    let output = fixture.verify(env!("CARGO_PKG_VERSION"), "not-the-host-arch");

    assert!(!output.status.success(), "{output:?}");
}

#[cfg(unix)]
struct UnixFixture {
    _directory: TempDir,
    root: PathBuf,
    runner_temp: PathBuf,
    host_arch: String,
    file_arch: String,
}

#[cfg(unix)]
impl UnixFixture {
    fn new(include_binary: bool) -> Self {
        let directory = tempfile::tempdir().expect("create Unix gate fixture directory");
        let root = directory.path().to_path_buf();
        let artifacts = root.join("artifacts");
        let payload = root.join("payload");
        let runner_temp = root.join("runner-temp");
        fs::create_dir(&artifacts).expect("create Unix artifact directory");
        fs::create_dir(&payload).expect("create Unix payload directory");
        fs::create_dir(&runner_temp).expect("create Unix runner directory");
        if include_binary {
            fs::copy(env!("CARGO_BIN_EXE_forager"), payload.join("forager"))
                .expect("copy fixture binary");
        } else {
            fs::write(payload.join("README.txt"), "fixture without a binary")
                .expect("write fixture placeholder");
        }
        let target = "fixture-unix";
        let archive = artifacts.join(format!("forager-{target}.tar.xz"));
        let mut command = Command::new("tar");
        command
            .args(["-cJf"])
            .arg(&archive)
            .arg("-C")
            .arg(&payload)
            .arg(".");
        let output = run_command(&mut command, None);
        assert!(output.status.success(), "{output:?}");
        let host_arch = command_stdout(Command::new("uname").arg("-m"));
        let file_arch = match host_arch.as_str() {
            "x86_64" if cfg!(target_os = "linux") => "x86-64".to_owned(),
            "aarch64" if cfg!(target_os = "macos") => "arm64".to_owned(),
            _ => host_arch.clone(),
        };
        Self {
            _directory: directory,
            root,
            runner_temp,
            host_arch,
            file_arch,
        }
    }

    fn verify(&self, version: &str, file_arch: &str) -> Output {
        let mut command = Command::new("bash");
        command
            .arg(script_path("verify-release-unix.sh"))
            .current_dir(&self.root)
            .env(
                "PLAN",
                format!(r#"{{"releases":[{{"app_version":"{version}"}}]}}"#),
            )
            .env("TARGET", "fixture-unix")
            .env("HOST_ARCH", &self.host_arch)
            .env("FILE_ARCH", file_arch)
            .env("ARCHIVE", "tar.xz")
            .env("RUNNER_TEMP", &self.runner_temp);
        run_command(&mut command, None)
    }
}

#[cfg(unix)]
fn command_stdout(command: &mut Command) -> String {
    let output = run_command(command, None);
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout)
        .expect("fixture command UTF-8")
        .trim()
        .to_owned()
}

fn script_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(".github/scripts")
        .join(name)
}

#[cfg(windows)]
#[test]
fn windows_gate_accepts_a_valid_release_archive() {
    let fixture = WindowsFixture::new(true);

    let output = fixture.verify(env!("CARGO_PKG_VERSION"), fixture.machine);

    assert!(output.status.success(), "{output:?}");
}

#[cfg(windows)]
#[test]
fn windows_gate_rejects_an_archive_without_the_binary() {
    let fixture = WindowsFixture::new(false);

    let output = fixture.verify(env!("CARGO_PKG_VERSION"), fixture.machine);

    assert!(!output.status.success(), "{output:?}");
}

#[cfg(windows)]
#[test]
fn windows_gate_rejects_a_version_mismatch() {
    let fixture = WindowsFixture::new(true);

    let output = fixture.verify("0.0.0-fixture", fixture.machine);

    assert!(!output.status.success(), "{output:?}");
}

#[cfg(windows)]
#[test]
fn windows_gate_rejects_an_architecture_mismatch() {
    let fixture = WindowsFixture::new(true);

    let output = fixture.verify(env!("CARGO_PKG_VERSION"), "Unknown");

    assert!(!output.status.success(), "{output:?}");
}

#[cfg(windows)]
struct WindowsFixture {
    _directory: TempDir,
    root: PathBuf,
    machine: &'static str,
}

#[cfg(windows)]
impl WindowsFixture {
    fn new(include_binary: bool) -> Self {
        let directory = tempfile::tempdir().expect("create Windows gate fixture directory");
        let root = directory.path().to_path_buf();
        let artifacts = root.join("artifacts");
        let payload = root.join("payload");
        fs::create_dir(&artifacts).expect("create Windows artifact directory");
        fs::create_dir(&payload).expect("create Windows payload directory");
        if include_binary {
            fs::copy(env!("CARGO_BIN_EXE_forager"), payload.join("forager.exe"))
                .expect("copy fixture binary");
        } else {
            fs::write(payload.join("README.txt"), "fixture without a binary")
                .expect("write fixture placeholder");
        }
        let archive = artifacts.join("forager-fixture-windows.zip");
        let mut command = Command::new("pwsh");
        command
            .args([
                "-NoProfile",
                "-Command",
                "Compress-Archive -Path (Join-Path $env:FIXTURE_PAYLOAD '*') -DestinationPath $env:FIXTURE_ARCHIVE",
            ])
            .env("FIXTURE_PAYLOAD", &payload)
            .env("FIXTURE_ARCHIVE", &archive);
        let output = run_command(&mut command, None);
        assert!(output.status.success(), "{output:?}");
        Self {
            _directory: directory,
            root,
            machine: if cfg!(target_arch = "aarch64") {
                "Arm64"
            } else {
                "Amd64"
            },
        }
    }

    fn verify(&self, version: &str, machine: &str) -> Output {
        let runner_temp = tempfile::Builder::new()
            .prefix("runner-")
            .tempdir_in(&self.root)
            .expect("create Windows runner directory");
        let mut command = Command::new("pwsh");
        command
            .args(["-NoProfile", "-File"])
            .arg(script_path("verify-release-windows.ps1"))
            .current_dir(&self.root)
            .env(
                "PLAN",
                format!(r#"{{"releases":[{{"app_version":"{version}"}}]}}"#),
            )
            .env("TARGET", "fixture-windows")
            .env("EXPECTED_MACHINE", machine)
            .env("ARCHIVE", "zip")
            .env("RUNNER_TEMP", runner_temp.path());
        run_command(&mut command, None)
    }
}

#[cfg(unix)]
#[test]
fn checksum_gate_accepts_intact_archives_and_rejects_corruption() {
    let fixture = ChecksumFixture::new();

    assert!(fixture.verify().status.success());

    fs::write(&fixture.archive, b"corrupted archive").expect("corrupt fixture archive");
    assert!(!fixture.verify().status.success());
}

#[cfg(unix)]
struct ChecksumFixture {
    _directory: TempDir,
    artifacts: std::path::PathBuf,
    archive: std::path::PathBuf,
}

#[cfg(unix)]
impl ChecksumFixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().expect("create checksum fixture directory");
        let artifacts = directory.path().join("artifacts");
        fs::create_dir(&artifacts).expect("create artifact directory");
        let archive = artifacts.join("forager-x86_64-unknown-linux-gnu.tar.xz");
        fs::write(&archive, b"fixture archive").expect("write fixture archive");
        write_checksum(&archive);
        Self {
            _directory: directory,
            artifacts,
            archive,
        }
    }

    fn verify(&self) -> std::process::Output {
        let mut command = Command::new("bash");
        command
            .arg(".github/scripts/verify-release-checksums.sh")
            .arg(&self.artifacts)
            .env(
                "TARGETS",
                r#"[{"target":"x86_64-unknown-linux-gnu","archive":"tar.xz"}]"#,
            );
        run_command(&mut command, None)
    }
}

#[cfg(unix)]
fn write_checksum(archive: &Path) {
    let mut command = Command::new("sha256sum");
    command
        .arg(archive.file_name().expect("fixture archive file name"))
        .current_dir(archive.parent().expect("fixture archive parent"));
    let output = run_command(&mut command, None);
    assert!(output.status.success(), "{output:?}");
    let mut checksum = archive.as_os_str().to_owned();
    checksum.push(".sha256");
    fs::write(checksum, output.stdout).expect("write fixture checksum");
}
