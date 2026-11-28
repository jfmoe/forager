//! The Managed Browser: the fingerprint-chromium build that forager pins, downloads from its
//! upstream GitHub release, verifies by SHA256, and installs into its cache directory, and how
//! the browser transport finds the executable to launch.
//!
//! Each release of forager pins one version, with a package per supported host target. The
//! install directory carries the version, so after an upgrade that changes the pin the old
//! install no longer counts. A non-empty `browser.path` replaces the Managed Browser, and then no
//! install is checked.

use std::fmt;
use std::path::{Path, PathBuf};

use crate::cache_dir::cache_directory;
use crate::config::BrowserRuntimeConfig;

/// The pinned fingerprint-chromium version. Only versions whose patch source is public are
/// pinned.
pub(crate) const VERSION: &str = "148.0.7778.215";

const INSTALL_PREFIX: &str = "fingerprint-chromium-";

/// The downloadable package of the pinned version for one host target.
#[derive(Debug)]
pub(crate) struct Package {
    pub(crate) url: &'static str,
    pub(crate) sha256: &'static str,
    /// The application bundle inside the package, copied into the install directory.
    pub(crate) bundle: &'static str,
    /// The executable, relative to the install directory.
    pub(crate) executable: &'static str,
}

const MACOS_ARM64: Package = Package {
    url: "https://github.com/adryfish/fingerprint-chromium/releases/download/148.0.7778.215/ungoogled-chromium_148.0.7778.215-1.1_macos.dmg",
    sha256: "b72f091e2e1a7583eed389c4b8e3534ed355e568af8c8bbf8fc30a25e23ca679",
    bundle: "Chromium.app",
    executable: "Chromium.app/Contents/MacOS/Chromium",
};

/// Returns the package for this host, or `None` when the pinned version has none.
pub(crate) fn host_package() -> Option<&'static Package> {
    (cfg!(target_os = "macos") && cfg!(target_arch = "aarch64")).then_some(&MACOS_ARM64)
}

/// Where the pinned version is installed under the cache directory `cache`.
pub(crate) fn install_directory(cache: &Path) -> PathBuf {
    browsers_directory(cache).join(format!("{INSTALL_PREFIX}{VERSION}"))
}

fn browsers_directory(cache: &Path) -> PathBuf {
    cache.join("browser")
}

/// Why no browser executable can be launched.
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum LocateError {
    /// The pinned version is not installed; `installed` lists other installed versions.
    NotInstalled { installed: Vec<String> },
    /// The pinned version has no package for this host.
    NoHostPackage,
    /// Neither `XDG_CACHE_HOME` nor `HOME` names a cache directory.
    NoCacheDirectory,
}

impl fmt::Display for LocateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotInstalled { installed } if !installed.is_empty() => write!(
                formatter,
                "the Managed Browser fingerprint-chromium {VERSION} is not installed (installed: {}); this forager needs {VERSION}, so run `forager browser install`",
                installed.join(", ")
            ),
            Self::NotInstalled { .. } => write!(
                formatter,
                "the Managed Browser fingerprint-chromium {VERSION} is not installed; run `forager browser install`"
            ),
            Self::NoHostPackage => write!(
                formatter,
                "fingerprint-chromium {VERSION} has no package for this host ({}-{}); set `browser.path` to a local Chromium-based browser executable",
                std::env::consts::OS,
                std::env::consts::ARCH
            ),
            Self::NoCacheDirectory => formatter.write_str(
                "cannot locate the Managed Browser: neither XDG_CACHE_HOME nor HOME is an absolute path",
            ),
        }
    }
}

/// Returns the browser executable to launch: the configured `browser.path`, else the installed
/// Managed Browser of this host.
///
/// # Errors
///
/// Returns why the Managed Browser cannot be used when no `browser.path` is set.
pub(crate) fn locate(config: &BrowserRuntimeConfig) -> Result<PathBuf, LocateError> {
    if let Some(path) = &config.path {
        return Ok(path.clone());
    }
    let package = host_package().ok_or(LocateError::NoHostPackage)?;
    let cache = cache_directory().ok_or(LocateError::NoCacheDirectory)?;
    locate_in(&cache, package)
}

fn locate_in(cache: &Path, package: &Package) -> Result<PathBuf, LocateError> {
    let executable = install_directory(cache).join(package.executable);
    if executable.is_file() {
        return Ok(executable);
    }
    Err(LocateError::NotInstalled {
        installed: version_directories(&browsers_directory(cache))
            .into_iter()
            .map(|directory| directory.version)
            .filter(|version| version != VERSION)
            .collect(),
    })
}

/// A Managed Browser version directory: a directory named by the install prefix and a dotted,
/// all-numeric version.
#[derive(Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct VersionDirectory {
    // The order key comes first, so that versions order by their numbers.
    numbers: Vec<u64>,
    pub(crate) version: String,
    pub(crate) path: PathBuf,
}

/// Lists the version directories in the browser directory `browsers`, oldest first. An entry
/// with any other name, or a symbolic link, is not a version directory.
pub(crate) fn version_directories(browsers: &Path) -> Vec<VersionDirectory> {
    let Ok(entries) = std::fs::read_dir(browsers) else {
        return Vec::new();
    };
    let mut directories = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter_map(|entry| {
            let version = entry
                .file_name()
                .to_str()?
                .strip_prefix(INSTALL_PREFIX)?
                .to_owned();
            Some(VersionDirectory {
                numbers: version_numbers(&version)?,
                version,
                path: entry.path(),
            })
        })
        .collect::<Vec<_>>();
    directories.sort();
    directories
}

fn version_numbers(version: &str) -> Option<Vec<u64>> {
    version
        .split('.')
        .map(|part| {
            part.bytes()
                .all(|byte| byte.is_ascii_digit())
                .then(|| part.parse().ok())
                .flatten()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{LocateError, MACOS_ARM64, VERSION, install_directory, locate_in};

    #[test]
    fn the_installed_pinned_version_is_located_in_the_cache() {
        let cache = tempfile::tempdir().expect("create cache");
        let executable = install_directory(cache.path()).join(MACOS_ARM64.executable);
        fs::create_dir_all(executable.parent().expect("executable directory"))
            .expect("create install");
        fs::write(&executable, "").expect("write executable");

        assert_eq!(locate_in(cache.path(), &MACOS_ARM64), Ok(executable));
    }

    #[test]
    fn another_installed_version_does_not_count_and_is_named() {
        let cache = tempfile::tempdir().expect("create cache");
        let old = cache
            .path()
            .join("browser/fingerprint-chromium-147.0.1.2")
            .join(MACOS_ARM64.executable);
        fs::create_dir_all(old.parent().expect("old directory")).expect("create old install");
        fs::write(&old, "").expect("write old executable");

        let error = locate_in(cache.path(), &MACOS_ARM64).unwrap_err();

        assert_eq!(
            (error.to_string().contains(VERSION), error),
            (
                true,
                LocateError::NotInstalled {
                    installed: vec!["147.0.1.2".into()]
                }
            )
        );
    }

    #[test]
    fn other_installed_versions_are_named_in_numeric_order() {
        let cache = tempfile::tempdir().expect("create cache");
        for version in ["10.0.0.0", "9.0.0.0", "9.0.x"] {
            fs::create_dir_all(
                cache
                    .path()
                    .join(format!("browser/fingerprint-chromium-{version}")),
            )
            .expect("create version");
        }

        assert_eq!(
            locate_in(cache.path(), &MACOS_ARM64),
            Err(LocateError::NotInstalled {
                installed: vec!["9.0.0.0".into(), "10.0.0.0".into()]
            })
        );
    }
}
