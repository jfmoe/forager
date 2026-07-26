//! Configuration location and private filesystem primitives.

use std::env;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use thiserror::Error;

static WRITE_PROBE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// The directory and file used by forager configuration.
#[derive(Debug, Eq, PartialEq)]
pub struct ConfigLocation {
    config_dir: PathBuf,
}

impl ConfigLocation {
    /// Resolves the configuration location from the process environment.
    ///
    /// `FORAGER_CONFIG_DIR` takes precedence. Otherwise this follows the XDG
    /// configuration directory convention and verifies that the nearest
    /// existing ancestor can create files.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::DefaultDirectoryUnavailable`] when no absolute
    /// XDG default can be resolved or its nearest existing ancestor is not
    /// writable.
    pub fn discover() -> Result<Self, ConfigError> {
        if let Some(config_dir) = env::var_os("FORAGER_CONFIG_DIR")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
        {
            return Ok(Self { config_dir });
        }

        let config_dir = env::var_os("XDG_CONFIG_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .map(|path| path.join("forager"))
            .or_else(|| {
                env::var_os("HOME")
                    .filter(|value| !value.is_empty())
                    .map(PathBuf::from)
                    .filter(|path| path.is_absolute())
                    .map(|path| path.join(".config/forager"))
            })
            .ok_or(ConfigError::DefaultDirectoryUnavailable)?;

        verify_default_directory(&config_dir)
            .map_err(|_| ConfigError::DefaultDirectoryUnavailable)?;

        Ok(Self { config_dir })
    }

    /// Returns the resolved `config.toml` path.
    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }
}

/// Errors produced before configuration loading begins.
#[derive(Debug, Eq, Error, PartialEq)]
pub enum ConfigError {
    /// The XDG configuration directory could not be resolved or written.
    #[error("default configuration directory is unavailable; set FORAGER_CONFIG_DIR")]
    DefaultDirectoryUnavailable,
}

/// Creates a configuration directory and restricts it to the current Unix user.
///
/// # Errors
///
/// Returns an I/O error when the directory cannot be created or its mode cannot
/// be restricted to the current user.
#[cfg(unix)]
pub fn ensure_private_directory(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)?;
    set_mode(path, 0o700)
}

/// Creates a configuration directory and restricts its ACL to the Windows owner.
///
/// # Errors
///
/// Returns an I/O error when the directory cannot be created or its ACL cannot
/// be restricted.
#[cfg(windows)]
pub fn ensure_private_directory(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)?;
    restrict_windows_acl(path)
}

/// Opens or creates a configuration file and restricts it to the current Unix user.
///
/// This primitive applies equally to `config.toml` and same-directory temporary
/// files used by an atomic replacement.
///
/// # Errors
///
/// Returns an I/O error when the file cannot be opened or its mode cannot be
/// set to `0600`.
#[cfg(unix)]
pub fn create_private_file(path: &Path) -> io::Result<File> {
    let file = open_private_file(path)?;
    set_mode(path, 0o600)?;
    Ok(file)
}

/// Opens or creates a configuration file and restricts its ACL to the Windows owner.
///
/// # Errors
///
/// Returns an I/O error when the file cannot be opened or its ACL cannot be
/// restricted.
#[cfg(windows)]
pub fn create_private_file(path: &Path) -> io::Result<File> {
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(path)?;
    restrict_windows_acl(path)?;
    Ok(file)
}

#[cfg(unix)]
fn open_private_file(path: &Path) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;

    OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .mode(0o600)
        .open(path)
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

/// Rejects permission-sensitive directory creation on unsupported platforms.
#[cfg(not(any(unix, windows)))]
pub fn ensure_private_directory(_path: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "private configuration permissions are unavailable",
    ))
}

/// Rejects permission-sensitive file creation on unsupported platforms.
#[cfg(not(any(unix, windows)))]
pub fn create_private_file(_path: &Path) -> io::Result<File> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "private configuration permissions are unavailable",
    ))
}

#[cfg(windows)]
fn restrict_windows_acl(path: &Path) -> io::Result<()> {
    use winapi::um::winnt::{FILE_ALL_ACCESS, PSID};
    use windows_acl::acl::{ACL, AceType};
    use windows_acl::helper::{sid_to_string, string_to_sid};

    const OWNER_SID: &str = "S-1-3-4";

    let path = path.to_str().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "configuration path is not valid Unicode",
        )
    })?;
    let mut acl = ACL::from_file_path(path, false)
        .map_err(|code| io::Error::other(format!("cannot read Windows ACL: {code}")))?;
    let entries = acl
        .all()
        .map_err(|code| io::Error::other(format!("cannot enumerate Windows ACL: {code}")))?;
    let owner = string_to_sid(OWNER_SID)
        .map_err(|code| io::Error::other(format!("cannot create owner SID: {code}")))?;

    for entry in entries {
        let sid = entry
            .sid
            .ok_or_else(|| io::Error::other("cannot verify a Windows ACL entry without a SID"))?;
        let sid = sid.as_ptr() as PSID;
        sid_to_string(sid)
            .map_err(|code| io::Error::other(format!("cannot read ACL SID: {code}")))?;
        acl.remove(sid, None, None).map_err(|code| {
            io::Error::other(format!("cannot remove inherited ACL entry: {code}"))
        })?;
    }

    acl.add_entry(
        owner.as_ptr() as PSID,
        AceType::AccessAllow,
        0,
        FILE_ALL_ACCESS,
    )
    .map_err(|code| io::Error::other(format!("cannot grant owner access: {code}")))
    .map(|_| ())
}

fn verify_default_directory(config_dir: &Path) -> io::Result<()> {
    let mut writable_ancestor = None;
    for ancestor in config_dir.ancestors() {
        match fs::metadata(ancestor) {
            Ok(metadata) if metadata.is_dir() => {
                writable_ancestor = Some(ancestor);
                break;
            }
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::NotADirectory,
                    "configuration path ancestor is not a directory",
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    let writable_ancestor = writable_ancestor.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "configuration path has no existing ancestor",
        )
    })?;

    let sequence = WRITE_PROBE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let probe = writable_ancestor.join(format!(
        ".forager-write-probe-{}-{sequence}",
        std::process::id()
    ));
    let file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&probe)?;
    drop(file);
    fs::remove_file(probe)
}
