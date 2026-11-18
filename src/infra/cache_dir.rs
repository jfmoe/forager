//! forager's cache directory: downloaded material that forager can fetch again, such as the
//! Managed Browser.
//!
//! The cache lives in `$XDG_CACHE_HOME/forager`, or `$HOME/.cache/forager` when
//! `XDG_CACHE_HOME` is not an absolute path.

use std::path::PathBuf;

use crate::state_file::forager_directory;

/// Returns the cache directory, or `None` when neither `XDG_CACHE_HOME` nor `HOME` is an
/// absolute path.
pub(crate) fn cache_directory() -> Option<PathBuf> {
    forager_directory("XDG_CACHE_HOME", ".cache")
}
