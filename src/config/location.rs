use std::env;
use std::fs::{self, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use thiserror::Error;

static WRITE_PROBE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// The directory and file used by forager configuration.
#[derive(Debug, Eq, PartialEq)]
pub struct ConfigLocation {
    pub(super) config_dir: PathBuf,
}

impl ConfigLocation {
    /// Resolves the configuration location from the process environment.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::DefaultDirectoryUnavailable`] when no writable
    /// absolute XDG default can be resolved.
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
    #[must_use]
    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }
}

/// Configuration loading and persistence errors.
#[derive(Debug, Eq, Error, PartialEq)]
pub enum ConfigError {
    /// The XDG configuration directory could not be resolved or written.
    #[error("default configuration directory is unavailable; set FORAGER_CONFIG_DIR")]
    DefaultDirectoryUnavailable,
    /// The configuration document is invalid.
    #[error("{}: {detail}", path.display())]
    Document { path: PathBuf, detail: String },
    /// A configuration operation failed.
    #[error("{0}")]
    Message(String),
}

impl ConfigError {
    // Callers transfer errors here because they have no further use for the source value.
    #[expect(clippy::needless_pass_by_value)]
    pub(super) fn io(path: &Path, error: io::Error) -> Self {
        Self::Message(format!("{}: {error}", path.display()))
    }
}

/// Errors from `config set` and `config unset`.
#[derive(Debug, Error)]
pub enum EditError {
    /// The requested key or value is invalid.
    #[error("{0}")]
    Argument(String),
    /// The configuration document could not be read or written.
    #[error(transparent)]
    Config(#[from] ConfigError),
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
