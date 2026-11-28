//! The flows of `forager browser install` and `forager browser login`: setting up the Managed
//! Browser in the cache, and logging a user into a platform's Browser Profile.
//!
//! Install downloads the pinned package of this host, checks its SHA256, and copies the browser
//! out of it. Login opens a window on the platform's login entry and waits until the platform's
//! page logic sees a logged-in session, then closes the browser normally and records the login.

use std::fmt::Write as _;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use reqwest::{Client, StatusCode, Url};
use serde::Serialize;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::config::BrowserRuntimeConfig;
use crate::net::{AttemptFailure, truncate_message};
use crate::providers::managed_browser::{self, Package, VERSION};
use crate::providers::{
    self, BrowserLogin, BrowserPage, BrowserProfile, BrowserScreen, BrowserSession, WindowMode,
    run_browser,
};
use crate::types::{AttemptErrorKind, Deadline, Platform};

const MAX_REDIRECTS: usize = 5;
const LOGIN_POLL: Duration = Duration::from_secs(1);
const MAX_UNANSWERED_POLLS: u32 = 5;
const DOWNLOAD_PREFIX: &str = ".download-";
const STAGING_PREFIX: &str = ".staging-";
// Detaching runs even after the command's timeout, so that no disk image stays mounted.
const DETACH_LIMIT: Duration = Duration::from_secs(15);

/// Why a browser setup command failed.
#[derive(Debug)]
pub(crate) enum SetupFailure {
    /// The browser to launch cannot be found; a preflight failure.
    Config(String),
    /// The command started and failed, with the error kind its exit code follows.
    Failed(AttemptFailure),
}

impl SetupFailure {
    fn new(kind: AttemptErrorKind, message: impl Into<String>) -> Self {
        Self::Failed(AttemptFailure {
            kind,
            status: None,
            message: message.into(),
        })
    }
}

impl From<AttemptFailure> for SetupFailure {
    fn from(failure: AttemptFailure) -> Self {
        Self::Failed(failure)
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

/// The package being downloaded: written to its file and hashed as it arrives, so the SHA256
/// needs no second read of the file.
pub(crate) struct Archive {
    file: tokio::fs::File,
    hasher: Sha256,
}

impl Archive {
    async fn create(path: &Path) -> io::Result<Self> {
        Ok(Self {
            file: tokio::fs::File::create(path).await?,
            hasher: Sha256::new(),
        })
    }

    pub(crate) async fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.hasher.update(bytes);
        self.file.write_all(bytes).await
    }

    /// Writes out what is buffered and returns the SHA256 of everything written, in hex.
    async fn finish(mut self) -> io::Result<String> {
        self.file.flush().await?;
        Ok(hex(&self.hasher.finalize()))
    }
}

/// Fetches a package.
pub(crate) trait Download {
    /// Writes the body at `url` into `into`, reporting progress lines through `progress`.
    async fn download(
        &self,
        url: &str,
        into: &mut Archive,
        progress: &mut dyn FnMut(&str),
    ) -> Result<(), SetupFailure>;
}

/// Copies the browser bundle out of a downloaded package.
pub(crate) trait Unpack {
    /// Copies `bundle` from the package at `archive` into the directory `into` before
    /// `deadline`.
    async fn unpack(
        &self,
        archive: &Path,
        bundle: &str,
        into: &Path,
        deadline: Deadline,
    ) -> Result<(), SetupFailure>;
}

/// Installs the pinned version of this host's package into the cache within `deadline`,
/// unless it is already installed.
///
/// # Errors
///
/// Returns Runtime when the host has no package or no cache directory, when the download does
/// not match its SHA256, or when unpacking fails; Network when the download fails; Timeout when
/// the deadline passes first.
pub(crate) async fn install(
    client: &Client,
    deadline: Deadline,
    progress: &mut dyn FnMut(&str),
) -> Result<Installed, SetupFailure> {
    let package = managed_browser::host_package()
        .ok_or_else(|| runtime(managed_browser::LocateError::NoHostPackage.to_string()))?;
    let cache = crate::cache_dir::cache_directory()
        .ok_or_else(|| runtime(managed_browser::LocateError::NoCacheDirectory.to_string()))?;
    let install_directory = managed_browser::install_directory(&cache);
    let download = HttpDownload { client, deadline };
    install_package(
        package,
        &install_directory,
        &download,
        &DiskImage::default(),
        deadline,
        progress,
    )
    .await
}

async fn install_package(
    package: &Package,
    install_directory: &Path,
    download: &impl Download,
    unpack: &impl Unpack,
    deadline: Deadline,
    progress: &mut dyn FnMut(&str),
) -> Result<Installed, SetupFailure> {
    let browsers = install_directory
        .parent()
        .ok_or_else(|| runtime("the install directory has no parent"))?;
    place_package(
        package,
        install_directory,
        browsers,
        download,
        unpack,
        deadline,
        progress,
    )
    .await
}

/// Puts the pinned version into `install_directory`, unless it is already there.
async fn place_package(
    package: &Package,
    install_directory: &Path,
    browsers: &Path,
    download: &impl Download,
    unpack: &impl Unpack,
    deadline: Deadline,
    progress: &mut dyn FnMut(&str),
) -> Result<Installed, SetupFailure> {
    let executable = install_directory.join(package.executable);
    if is_file(&executable).await {
        return Ok(Installed {
            version: VERSION,
            executable,
            downloaded: false,
        });
    }
    tokio::fs::create_dir_all(browsers)
        .await
        .map_err(|error| io_failure("create the cache directory", &error))?;
    remove_leftovers(browsers).await;
    let archive = browsers.join(format!("{DOWNLOAD_PREFIX}{}", std::process::id()));
    let staging = browsers.join(format!("{STAGING_PREFIX}{}", std::process::id()));
    let installed = async {
        progress(&format!(
            "downloading fingerprint-chromium {VERSION} from {}",
            package.url
        ));
        let mut file = Archive::create(&archive)
            .await
            .map_err(|error| io_failure("create the download file", &error))?;
        download.download(package.url, &mut file, progress).await?;
        let digest = file
            .finish()
            .await
            .map_err(|error| io_failure("write the download", &error))?;
        if digest != package.sha256 {
            return Err(runtime(format!(
                "the downloaded package does not match its SHA256 (expected {}, got {digest}); it was deleted",
                package.sha256
            )));
        }
        progress("verified the SHA256; installing");
        let _ = tokio::fs::remove_dir_all(&staging).await;
        tokio::fs::create_dir_all(&staging)
            .await
            .map_err(|error| io_failure("create the staging directory", &error))?;
        unpack
            .unpack(&archive, package.bundle, &staging, deadline)
            .await?;
        if !is_file(&staging.join(package.executable)).await {
            return Err(runtime(format!(
                "the package has no {}",
                package.executable
            )));
        }
        let _ = tokio::fs::remove_dir_all(install_directory).await;
        tokio::fs::rename(&staging, install_directory)
            .await
            .map_err(|error| io_failure("move the browser into place", &error))?;
        Ok(Installed {
            version: VERSION,
            executable: executable.clone(),
            downloaded: true,
        })
    }
    .await;
    let _ = tokio::fs::remove_file(&archive).await;
    let _ = tokio::fs::remove_dir_all(&staging).await;
    installed
}

async fn is_file(path: &Path) -> bool {
    tokio::fs::metadata(path)
        .await
        .is_ok_and(|metadata| metadata.is_file())
}

/// Removes the downloads and staging directories that an install interrupted earlier left in
/// `directory`. Each carries the process ID of its install, and one whose process still runs
/// belongs to a concurrent install and stays.
async fn remove_leftovers(directory: &Path) {
    let Ok(mut entries) = tokio::fs::read_dir(directory).await else {
        return;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name();
        let Some(owner) = name
            .to_str()
            .and_then(|name| {
                name.strip_prefix(DOWNLOAD_PREFIX)
                    .or_else(|| name.strip_prefix(STAGING_PREFIX))
            })
            .map(|pid| pid.parse::<u32>().ok())
        else {
            continue;
        };
        if owner.is_some_and(process_runs) {
            continue;
        }
        let path = entry.path();
        if tokio::fs::remove_dir_all(&path).await.is_err() {
            let _ = tokio::fs::remove_file(&path).await;
        }
    }
}

#[cfg(unix)]
fn process_runs(pid: u32) -> bool {
    use nix::errno::Errno;
    use nix::sys::signal::kill;
    use nix::unistd::Pid;

    let Ok(pid) = i32::try_from(pid) else {
        return false;
    };
    !matches!(kill(Pid::from_raw(pid), None), Err(Errno::ESRCH))
}

#[cfg(not(unix))]
fn process_runs(pid: u32) -> bool {
    pid == std::process::id()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

fn runtime(message: impl Into<String>) -> SetupFailure {
    SetupFailure::new(AttemptErrorKind::Runtime, message)
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
        into: &mut Archive,
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
        into: &mut Archive,
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
            into.write(&chunk)
                .await
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
    SetupFailure::new(AttemptErrorKind::Network, message)
}

fn download_timeout() -> SetupFailure {
    SetupFailure::new(
        AttemptErrorKind::Timeout,
        "the download did not finish before the command timeout; retry with a longer --timeout",
    )
}

/// Copies the bundle out of a macOS disk image with `hdiutil` and `ditto`, which keeps the
/// bundle's symbolic links and code signature. The image is detached afterwards, also when
/// attaching or copying failed or ran out of time.
struct DiskImage {
    hdiutil: PathBuf,
    ditto: PathBuf,
}

impl Default for DiskImage {
    fn default() -> Self {
        Self {
            hdiutil: "hdiutil".into(),
            ditto: "ditto".into(),
        }
    }
}

impl Unpack for DiskImage {
    async fn unpack(
        &self,
        archive: &Path,
        bundle: &str,
        into: &Path,
        deadline: Deadline,
    ) -> Result<(), SetupFailure> {
        let mount =
            tempfile::tempdir().map_err(|error| io_failure("create the mount point", &error))?;
        let mut attach = Command::new(&self.hdiutil);
        attach
            .args([
                "attach",
                "-nobrowse",
                "-readonly",
                "-noautoopen",
                "-quiet",
                "-mountpoint",
            ])
            .arg(mount.path())
            .arg(archive);
        let mut copy = Command::new(&self.ditto);
        copy.arg(mount.path().join(bundle)).arg(into.join(bundle));
        let copied = match run_tool(&mut attach, deadline).await {
            Ok(()) => run_tool(&mut copy, deadline).await,
            Err(failure) => Err(failure),
        };
        // An attach that ran out of time may still have mounted the image.
        let detach_limit = Deadline::new(DETACH_LIMIT);
        let mut detach = Command::new(&self.hdiutil);
        detach.args(["detach", "-quiet"]).arg(mount.path());
        let mut detached = run_tool(&mut detach, detach_limit).await;
        if detached.is_err() {
            let mut force = Command::new(&self.hdiutil);
            force.args(["detach", "-quiet", "-force"]).arg(mount.path());
            detached = run_tool(&mut force, detach_limit).await;
        }
        copied.and(detached)
    }
}

/// Runs a tool to its end before `deadline`, killing it when the deadline passes first.
async fn run_tool(command: &mut Command, deadline: Deadline) -> Result<(), SetupFailure> {
    let program = command
        .as_std()
        .get_program()
        .to_string_lossy()
        .into_owned();
    let subcommand = command.as_std().get_args().next().map_or_else(
        || program.clone(),
        |argument| format!("{program} {}", argument.to_string_lossy()),
    );
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let timed_out = || {
        SetupFailure::new(
            AttemptErrorKind::Timeout,
            format!(
                "`{subcommand}` did not finish before the command timeout; retry with a longer --timeout"
            ),
        )
    };
    let remaining = deadline.remaining().ok_or_else(timed_out)?;
    let output = tokio::time::timeout(remaining, command.output())
        .await
        .map_err(|_| timed_out())?
        .map_err(|error| {
            runtime(format!(
                "cannot unpack the package: cannot run {program}: {error}"
            ))
        })?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(runtime(format!(
        "cannot unpack the package: {program} exited with {}: {}",
        output.status,
        truncate_message(stderr.trim())
    )))
}

/// The result of `forager browser login`.
#[derive(Debug, Serialize)]
pub(crate) struct LoginReport {
    pub(crate) platform: &'static str,
    pub(crate) logged_in: bool,
    pub(crate) profile: PathBuf,
}

/// The login page logic of `platform`.
///
/// # Errors
///
/// Returns the argument error that names the platforms with a login when `platform` has none.
pub(crate) fn platform_login(platform: Platform) -> Result<BrowserLogin, String> {
    providers::browser_login(platform).ok_or_else(|| {
        format!(
            "platform `{platform}` has no browser route that needs a login; `forager browser login` supports: {}",
            Platform::ALL
                .into_iter()
                .filter(|platform| providers::browser_login(*platform).is_some())
                .map(Platform::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        )
    })
}

/// Logs a user into the Browser Profile of `login` within `timeout`: finds the browser that
/// `browser` names, opens the profile, and takes the access permit of the login's route, then
/// opens a window on the login entry and waits, until the read deadline, for the page logic to
/// see a logged-in session. It closes the browser normally and records the login in the
/// Browser Profile, with the window's screen.
///
/// # Errors
///
/// Returns Config when the browser cannot be found, Timeout when no login is observed in time,
/// and Runtime for a busy or unusable profile, a browser that fails to launch or closes early,
/// or a failed handshake.
pub(crate) async fn login(
    login: &BrowserLogin,
    browser: &BrowserRuntimeConfig,
    timeout: Duration,
    progress: &mut dyn FnMut(&str),
) -> Result<LoginReport, SetupFailure> {
    let executable = managed_browser::locate(browser)
        .map_err(|error| SetupFailure::Config(error.to_string()))?;
    let profile =
        BrowserProfile::named(login.profile).map_err(|error| runtime(error.to_string()))?;
    let limiter = providers::route_limiter(login.route).ok_or_else(|| {
        runtime(format!(
            "{} declares no access policy for its browser commands",
            login.route.name()
        ))
    })?;
    progress(&format!(
        "opening a browser window on {}; log in there (waiting up to {} seconds)",
        login.page.entry_url,
        timeout.as_secs()
    ));
    let session = BrowserSession {
        executable: &executable,
        profile: &profile,
        language: login.language,
        window: WindowMode::Headed,
        capture: login.page.capture,
    };
    let (screen, mut locked) =
        run_browser(&session, &limiter, Deadline::new(timeout), async |page| {
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
async fn await_login(
    page: &mut BrowserPage,
    login: &BrowserLogin,
) -> Result<BrowserScreen, AttemptFailure> {
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
    use std::fs;
    use std::path::Path;
    use std::time::Duration;

    use sha2::{Digest, Sha256};

    use super::{Archive, Download, Installed, SetupFailure, Unpack, install_package};
    use crate::fixture_server::block_on;
    use crate::providers::managed_browser::{Package, VERSION};
    use crate::types::{AttemptErrorKind, Deadline};

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
            into: &mut Archive,
            _progress: &mut dyn FnMut(&str),
        ) -> Result<(), SetupFailure> {
            self.calls.set(self.calls.get() + 1);
            into.write(PACKAGE_BYTES).await.expect("write package");
            Ok(())
        }
    }

    /// Unpacks by creating the bundle's executable, as the real package holds it.
    struct FakeUnpack;

    impl Unpack for FakeUnpack {
        fn unpack(
            &self,
            archive: &Path,
            bundle: &str,
            into: &Path,
            _deadline: Deadline,
        ) -> impl Future<Output = Result<(), SetupFailure>> {
            assert_eq!(fs::read(archive).expect("read archive"), PACKAGE_BYTES);
            let executable = into.join(bundle).join("Contents/MacOS/Chromium");
            fs::create_dir_all(executable.parent().expect("bundle directory"))
                .expect("create bundle");
            fs::write(executable, "").expect("write executable");
            std::future::ready(Ok(()))
        }
    }

    fn run(
        package: &Package,
        install_directory: &Path,
        download: &FakeDownload,
    ) -> Result<Installed, SetupFailure> {
        block_on(install_package(
            package,
            install_directory,
            download,
            &FakeUnpack,
            Deadline::new(Duration::from_secs(10)),
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

    /// The error kind and message of a failed install.
    fn failure(failure: SetupFailure) -> (AttemptErrorKind, String) {
        match failure {
            SetupFailure::Failed(failure) => (failure.kind, failure.message),
            SetupFailure::Config(message) => panic!("unexpected config failure: {message}"),
        }
    }

    #[test]
    fn a_package_matching_its_hash_is_installed() {
        let cache = tempfile::tempdir().expect("create cache");
        let directory = cache.path().join("browser/fingerprint-chromium");

        let installed = run(&package(digest()), &directory, &FakeDownload::default());

        assert_eq!(
            (
                installed.map_err(failure),
                files(&cache.path().join("browser"))
            ),
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

        let (kind, message) =
            failure(run(&package("0000"), &directory, &FakeDownload::default()).unwrap_err());

        assert_eq!(
            (
                kind,
                message.contains("SHA256"),
                files(&cache.path().join("browser"))
            ),
            (AttemptErrorKind::Runtime, true, Vec::<String>::new())
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

    #[test]
    fn leftovers_of_an_interrupted_install_are_removed() {
        let cache = tempfile::tempdir().expect("create cache");
        let browsers = cache.path().join("browser");
        // No process runs with these IDs, above the highest process ID macOS and Linux assign.
        fs::create_dir_all(browsers.join(".staging-4194305/Chromium.app")).expect("staging");
        fs::write(browsers.join(".download-4194306"), "partial").expect("download");

        run(
            &package(digest()),
            &browsers.join("fingerprint-chromium"),
            &FakeDownload::default(),
        )
        .expect("install");

        assert_eq!(files(&browsers), ["fingerprint-chromium"]);
    }

    #[cfg(unix)]
    #[test]
    fn a_disk_image_that_never_attaches_ends_at_the_deadline_and_is_detached() {
        use std::os::unix::fs::PermissionsExt;

        let tools = tempfile::tempdir().expect("create tools");
        let hdiutil = tools.path().join("hdiutil");
        fs::write(
            &hdiutil,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$1\" >> '{}'\n[ \"$1\" = attach ] && exec /bin/sleep 60\nexit 0\n",
                tools.path().join("calls").display()
            ),
        )
        .expect("write hdiutil");
        fs::set_permissions(&hdiutil, fs::Permissions::from_mode(0o755))
            .expect("make hdiutil executable");
        let image = super::DiskImage {
            hdiutil,
            ditto: "/usr/bin/false".into(),
        };
        let into = tools.path().join("staging");

        let (kind, _) = failure(
            block_on(image.unpack(
                &tools.path().join("browser.dmg"),
                "Chromium.app",
                &into,
                Deadline::new(Duration::from_millis(500)),
            ))
            .unwrap_err(),
        );

        // Under load the deadline can pass before the stand-in records its attach.
        let calls = fs::read_to_string(tools.path().join("calls")).expect("calls");
        assert_eq!(
            (kind, calls.lines().last()),
            (AttemptErrorKind::Timeout, Some("detach")),
            "{calls}"
        );
    }
}

#[cfg(all(test, unix))]
mod real_browser_tests {
    use super::await_login;
    use crate::providers::real_browser::{Pages, Sandbox, executable, local_login};

    #[test]
    #[ignore = "launches the Managed Browser; see the docs of providers::real_browser"]
    fn a_login_completes_once_the_page_logic_sees_the_session() {
        let executable = executable();
        let pages = Pages::start();
        let login = local_login(&pages);

        let screen = Sandbox::new().run(&executable, async |page| await_login(page, &login).await);

        assert!(screen.device_pixel_ratio > 0.0, "{screen:?}");
    }
}
