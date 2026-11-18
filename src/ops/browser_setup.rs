//! The flows of `forager browser install` and `forager browser login`: setting up the Managed
//! Browser in the cache, and logging a user into a platform's Browser Profile.
//!
//! Install downloads the pinned package of this host, checks its SHA256, and copies the browser
//! out of it. Login opens a window on the platform's login entry and waits until the platform's
//! page logic sees a logged-in session, then closes the browser normally and records the login.

use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use reqwest::{Client, StatusCode, Url};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::net::{AttemptFailure, truncate_message};
use crate::providers::managed_browser::{self, Package, VERSION};
use crate::providers::{
    BrowserDisplay, BrowserLogin, BrowserPage, BrowserProfile, BrowserSession, run_browser,
};
use crate::rate_limit::RateLimiter;
use crate::types::{AttemptErrorKind, Deadline, ErrorKind};

const MAX_REDIRECTS: usize = 5;
const LOGIN_POLL: Duration = Duration::from_secs(1);
const MAX_UNANSWERED_POLLS: u32 = 5;

/// Why a browser setup command failed, with the error kind its exit code follows.
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct SetupFailure {
    pub(crate) kind: ErrorKind,
    pub(crate) message: String,
}

impl SetupFailure {
    fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

impl From<AttemptFailure> for SetupFailure {
    fn from(failure: AttemptFailure) -> Self {
        Self::new(ErrorKind::from(failure.kind), failure.message)
    }
}

/// The result of `forager browser install`.
#[derive(Debug, Eq, PartialEq, Serialize)]
pub(crate) struct Installed {
    pub(crate) version: &'static str,
    pub(crate) executable: PathBuf,
    /// Whether this run downloaded the package; `false` when it was already installed.
    pub(crate) downloaded: bool,
}

/// Fetches a package into a file.
pub(crate) trait Download {
    /// Writes the body at `url` into `into`, reporting progress lines through `progress`.
    async fn download(
        &self,
        url: &str,
        into: &mut File,
        progress: &mut dyn FnMut(&str),
    ) -> Result<(), SetupFailure>;
}

/// Copies the browser bundle out of a downloaded package.
pub(crate) trait Unpack {
    /// Copies `bundle` from the package at `archive` into the directory `into`.
    fn unpack(&self, archive: &Path, bundle: &str, into: &Path) -> Result<(), String>;
}

/// Installs the pinned version of this host's package into the cache, unless it is already
/// installed.
///
/// # Errors
///
/// Returns Runtime when the host has no package or no cache directory, when the download does
/// not match its SHA256, or when unpacking fails; Network when the download fails.
pub(crate) async fn install(
    client: &Client,
    deadline: Deadline,
    progress: &mut dyn FnMut(&str),
) -> Result<Installed, SetupFailure> {
    let package = managed_browser::host_package().ok_or_else(|| {
        SetupFailure::new(
            ErrorKind::Runtime,
            managed_browser::LocateError::NoHostPackage.to_string(),
        )
    })?;
    let cache = crate::cache_dir::cache_directory().ok_or_else(|| {
        SetupFailure::new(
            ErrorKind::Runtime,
            managed_browser::LocateError::NoCacheDirectory.to_string(),
        )
    })?;
    let install_directory = managed_browser::install_directory(&cache);
    let download = HttpDownload { client, deadline };
    install_package(package, &install_directory, &download, &DiskImage, progress).await
}

async fn install_package(
    package: &Package,
    install_directory: &Path,
    download: &impl Download,
    unpack: &impl Unpack,
    progress: &mut dyn FnMut(&str),
) -> Result<Installed, SetupFailure> {
    let executable = install_directory.join(package.executable);
    if executable.is_file() {
        return Ok(Installed {
            version: VERSION,
            executable,
            downloaded: false,
        });
    }
    let parent = install_directory
        .parent()
        .ok_or_else(|| runtime("the install directory has no parent"))?;
    fs::create_dir_all(parent).map_err(|error| io_failure("create the cache directory", &error))?;
    let archive = parent.join(format!(".download-{}", std::process::id()));
    let staging = parent.join(format!(".staging-{}", std::process::id()));
    let installed = async {
        progress(&format!(
            "downloading fingerprint-chromium {VERSION} from {}",
            package.url
        ));
        let mut file = File::create(&archive)
            .map_err(|error| io_failure("create the download file", &error))?;
        download.download(package.url, &mut file, progress).await?;
        drop(file);
        let digest = sha256_of(&archive).map_err(|error| io_failure("read the download", &error))?;
        if digest != package.sha256 {
            return Err(runtime(format!(
                "the downloaded package does not match its SHA256 (expected {}, got {digest}); it was deleted",
                package.sha256
            )));
        }
        progress("verified the SHA256; installing");
        let _ = fs::remove_dir_all(&staging);
        fs::create_dir_all(&staging).map_err(|error| io_failure("create the staging directory", &error))?;
        unpack
            .unpack(&archive, package.bundle, &staging)
            .map_err(|reason| runtime(format!("cannot unpack the package: {reason}")))?;
        if !staging.join(package.executable).is_file() {
            return Err(runtime(format!(
                "the package has no {}",
                package.executable
            )));
        }
        let _ = fs::remove_dir_all(install_directory);
        fs::rename(&staging, install_directory)
            .map_err(|error| io_failure("move the browser into place", &error))?;
        Ok(Installed {
            version: VERSION,
            executable: executable.clone(),
            downloaded: true,
        })
    }
    .await;
    let _ = fs::remove_file(&archive);
    let _ = fs::remove_dir_all(&staging);
    installed
}

fn sha256_of(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex(&hasher.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

fn runtime(message: impl Into<String>) -> SetupFailure {
    SetupFailure::new(ErrorKind::Runtime, message)
}

fn io_failure(action: &str, error: &io::Error) -> SetupFailure {
    runtime(format!("cannot {action}: {error}"))
}

/// Downloads over HTTPS with the shared client. The client never follows redirects by itself,
/// so a release download, which redirects to its storage host, follows them here, HTTPS only.
struct HttpDownload<'a> {
    client: &'a Client,
    deadline: Deadline,
}

impl Download for HttpDownload<'_> {
    async fn download(
        &self,
        url: &str,
        into: &mut File,
        progress: &mut dyn FnMut(&str),
    ) -> Result<(), SetupFailure> {
        let mut url =
            Url::parse(url).map_err(|error| runtime(format!("bad package URL: {error}")))?;
        for _ in 0..=MAX_REDIRECTS {
            if url.scheme() != "https" {
                return Err(network(format!(
                    "the download redirected to a non-HTTPS URL {url}"
                )));
            }
            let remaining = self.deadline.remaining().ok_or_else(download_timeout)?;
            let response = tokio::time::timeout(remaining, self.client.get(url.clone()).send())
                .await
                .map_err(|_| download_timeout())?
                .map_err(|error| network(format!("the download failed: {error}")))?;
            let status = response.status();
            if status.is_redirection() {
                let location = response
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|value| value.to_str().ok())
                    .ok_or_else(|| network("the download redirected without a location"))?;
                url = url.join(location).map_err(|error| {
                    network(format!("the download redirected to a bad URL: {error}"))
                })?;
                continue;
            }
            if status != StatusCode::OK {
                return Err(network(format!("the download answered HTTP {status}")));
            }
            return self.write_body(response, into, progress).await;
        }
        Err(network("the download redirected too many times"))
    }
}

impl HttpDownload<'_> {
    async fn write_body(
        &self,
        mut response: reqwest::Response,
        into: &mut File,
        progress: &mut dyn FnMut(&str),
    ) -> Result<(), SetupFailure> {
        let total = response.content_length();
        let mut written: u64 = 0;
        let mut reported = 0;
        loop {
            let remaining = self.deadline.remaining().ok_or_else(download_timeout)?;
            let chunk = tokio::time::timeout(remaining, response.chunk())
                .await
                .map_err(|_| download_timeout())?
                .map_err(|error| network(format!("the download failed: {error}")))?;
            let Some(chunk) = chunk else {
                return Ok(());
            };
            into.write_all(&chunk)
                .map_err(|error| io_failure("write the download", &error))?;
            written += chunk.len() as u64;
            if let Some(total) = total.filter(|total| *total > 0) {
                let tenth = written * 10 / total;
                if tenth > reported {
                    reported = tenth;
                    progress(&format!(
                        "downloaded {} of {} MiB",
                        written / (1024 * 1024),
                        total / (1024 * 1024)
                    ));
                }
            }
        }
    }
}

fn network(message: impl Into<String>) -> SetupFailure {
    SetupFailure::new(ErrorKind::Network, message)
}

fn download_timeout() -> SetupFailure {
    SetupFailure::new(
        ErrorKind::Timeout,
        "the download did not finish before the command timeout; retry with a longer --timeout",
    )
}

/// Copies the bundle out of a macOS disk image with `hdiutil` and `ditto`, which keeps the
/// bundle's symbolic links and code signature.
struct DiskImage;

impl Unpack for DiskImage {
    fn unpack(&self, archive: &Path, bundle: &str, into: &Path) -> Result<(), String> {
        let mount = tempfile::tempdir().map_err(|error| error.to_string())?;
        run_tool(
            Command::new("hdiutil")
                .args([
                    "attach",
                    "-nobrowse",
                    "-readonly",
                    "-noautoopen",
                    "-quiet",
                    "-mountpoint",
                ])
                .arg(mount.path())
                .arg(archive),
        )?;
        let copied = run_tool(
            Command::new("ditto")
                .arg(mount.path().join(bundle))
                .arg(into.join(bundle)),
        );
        let detached = run_tool(
            Command::new("hdiutil")
                .args(["detach", "-quiet"])
                .arg(mount.path()),
        )
        .or_else(|_| {
            run_tool(
                Command::new("hdiutil")
                    .args(["detach", "-quiet", "-force"])
                    .arg(mount.path()),
            )
        });
        copied.and(detached)
    }
}

fn run_tool(command: &mut Command) -> Result<(), String> {
    let output = command
        .output()
        .map_err(|error| format!("cannot run {}: {error}", command.get_program().display()))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(format!(
        "{} exited with {}: {}",
        command.get_program().display(),
        output.status,
        truncate_message(stderr.trim())
    ))
}

/// The result of `forager browser login`.
#[derive(Debug, Serialize)]
pub(crate) struct LoginReport {
    pub(crate) platform: &'static str,
    pub(crate) logged_in: bool,
    pub(crate) profile: PathBuf,
}

/// Opens a window on the login entry of `login` and waits, until the read deadline of
/// `deadline`, for its page logic to see a logged-in session; then closes the browser normally
/// and records the login in the Browser Profile, with the window's screen.
///
/// # Errors
///
/// Returns Timeout when no login is observed in time, and Runtime for a busy or unusable
/// profile, a browser that fails to launch or closes early, or a failed handshake.
pub(crate) async fn login(
    login: &BrowserLogin,
    executable: &Path,
    profile: &BrowserProfile,
    limiter: &RateLimiter,
    deadline: Deadline,
) -> Result<LoginReport, SetupFailure> {
    let session = BrowserSession {
        executable,
        profile,
        language: login.language,
        display: BrowserDisplay::Headed,
        capture: login.page.capture,
    };
    let (screen, mut locked) = run_browser(&session, limiter, deadline, async |page| {
        await_login(page, login).await
    })
    .await?;
    locked
        .record_login(Some(screen))
        .map_err(|error| runtime(error.to_string()))?;
    Ok(LoginReport {
        platform: login.profile,
        logged_in: true,
        profile: locked.root().to_path_buf(),
    })
}

/// Opens the login entry and polls what the page captured until the page logic sees a login.
/// Returns the screen of the window the login happened in.
///
/// # Errors
///
/// Returns Timeout at the read deadline, and Runtime when the window closes first.
pub(crate) async fn await_login(
    page: &mut BrowserPage,
    login: &BrowserLogin,
) -> Result<crate::providers::BrowserScreen, AttemptFailure> {
    page.navigate(login.page.entry_url).await?;
    let mut exchanges = Vec::new();
    let mut unanswered = 0;
    let mut screen = None;
    while page.reading() {
        // A page that is navigating, as a site does once a login succeeds, briefly answers no
        // script; a window that stays silent, or a browser that exited, has been closed.
        match page.screen().await {
            Ok(read) => {
                screen = Some(read);
                unanswered = 0;
            }
            Err(_) if page.closed() || unanswered >= MAX_UNANSWERED_POLLS => {
                return Err(AttemptFailure {
                    kind: AttemptErrorKind::Runtime,
                    status: None,
                    message: "the browser window closed before a login was observed".into(),
                });
            }
            Err(_) => unanswered += 1,
        }
        exchanges.extend(page.take_exchanges()?);
        if let Some(screen) = screen
            && (login.page.logged_in)(&exchanges)
        {
            return Ok(screen);
        }
        page.rest(LOGIN_POLL).await;
    }
    Err(AttemptFailure {
        kind: AttemptErrorKind::Timeout,
        status: None,
        message: format!(
            "no login to {} was observed before the timeout; run `forager browser login {}` again",
            login.page.entry_url, login.profile
        ),
    })
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::fs::{self, File};
    use std::io::Write;
    use std::path::Path;

    use sha2::{Digest, Sha256};

    use super::{Download, Installed, SetupFailure, Unpack, install_package};
    use crate::providers::managed_browser::{Package, VERSION};
    use crate::types::ErrorKind;

    const PACKAGE_BYTES: &[u8] = b"a disk image";

    fn package(sha256: &'static str) -> Package {
        Package {
            url: "https://example.test/browser.dmg",
            sha256,
            bundle: "Chromium.app",
            executable: "Chromium.app/Contents/MacOS/Chromium",
        }
    }

    fn digest() -> &'static str {
        Box::leak(super::hex(&Sha256::digest(PACKAGE_BYTES)).into_boxed_str())
    }

    #[derive(Default)]
    struct FakeDownload {
        calls: Cell<usize>,
    }

    impl Download for FakeDownload {
        async fn download(
            &self,
            _url: &str,
            into: &mut File,
            _progress: &mut dyn FnMut(&str),
        ) -> Result<(), SetupFailure> {
            self.calls.set(self.calls.get() + 1);
            into.write_all(PACKAGE_BYTES).expect("write package");
            Ok(())
        }
    }

    /// Unpacks by creating the bundle's executable, as the real package holds it.
    struct FakeUnpack;

    impl Unpack for FakeUnpack {
        fn unpack(&self, archive: &Path, bundle: &str, into: &Path) -> Result<(), String> {
            assert_eq!(fs::read(archive).expect("read archive"), PACKAGE_BYTES);
            let executable = into.join(bundle).join("Contents/MacOS/Chromium");
            fs::create_dir_all(executable.parent().expect("bundle directory"))
                .expect("create bundle");
            fs::write(executable, "").expect("write executable");
            Ok(())
        }
    }

    fn run(
        package: &Package,
        install_directory: &Path,
        download: &FakeDownload,
    ) -> Result<Installed, SetupFailure> {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("build test runtime")
            .block_on(install_package(
                package,
                install_directory,
                download,
                &FakeUnpack,
                &mut |_| {},
            ))
    }

    fn files(directory: &Path) -> Vec<String> {
        let mut names = fs::read_dir(directory)
            .expect("read directory")
            .map(|entry| {
                entry
                    .expect("entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect::<Vec<_>>();
        names.sort();
        names
    }

    #[test]
    fn a_package_matching_its_hash_is_installed() {
        let cache = tempfile::tempdir().expect("create cache");
        let directory = cache.path().join("browser/fingerprint-chromium");

        let installed = run(&package(digest()), &directory, &FakeDownload::default());

        assert_eq!(
            (installed, files(&cache.path().join("browser"))),
            (
                Ok(Installed {
                    version: VERSION,
                    executable: directory.join("Chromium.app/Contents/MacOS/Chromium"),
                    downloaded: true
                }),
                vec!["fingerprint-chromium".to_owned()]
            )
        );
    }

    #[test]
    fn a_package_with_another_hash_is_deleted_and_fails() {
        let cache = tempfile::tempdir().expect("create cache");
        let directory = cache.path().join("browser/fingerprint-chromium");

        let failure = run(&package("0000"), &directory, &FakeDownload::default()).unwrap_err();

        assert_eq!(
            (
                failure.kind,
                failure.message.contains("SHA256"),
                files(&cache.path().join("browser"))
            ),
            (ErrorKind::Runtime, true, Vec::<String>::new())
        );
    }

    #[test]
    fn an_installed_version_is_not_downloaded_again() {
        let cache = tempfile::tempdir().expect("create cache");
        let directory = cache.path().join("browser/fingerprint-chromium");
        let download = FakeDownload::default();
        run(&package(digest()), &directory, &download).expect("first install");

        let again = run(&package(digest()), &directory, &download).expect("second install");

        assert_eq!((again.downloaded, download.calls.get()), (false, 1));
    }
}
