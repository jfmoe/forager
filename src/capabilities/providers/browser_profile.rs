//! Browser Profile: the persistent browser environment of one platform. It keeps the browser's
//! own user data (and with it the platform's login state), the fingerprint seed generated once
//! when the profile is created, the screen of the window the user logged in with, and the marker
//! that a login once succeeded.
//!
//! A profile lives in `<state directory>/browser/<platform>`, readable only by the user. One
//! process at a time uses it, under a file lock that is never waited for. forager never reads,
//! exports, or injects the cookies the browser keeps there.

use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::secure_fs::ensure_private_directory;
use crate::state_file::{StateLock, read_state, state_directory, write_state};

const IDENTITY_VERSION: u32 = 1;

/// The screen and window of a headed browser, as its page reports them in CSS pixels.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct Screen {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) avail_top: u32,
    pub(crate) avail_height: u32,
    pub(crate) device_pixel_ratio: f64,
    pub(crate) window_width: u32,
    pub(crate) window_height: u32,
}

/// What stays fixed for the life of a profile.
#[derive(Debug, Deserialize, Serialize)]
struct Identity {
    version: u32,
    seed: u32,
    screen: Option<Screen>,
}

/// Why a profile cannot be used.
#[derive(Debug, thiserror::Error)]
pub(crate) enum ProfileError {
    #[error("the Browser Profile {0} is in use by another forager process; retry after it ends")]
    Busy(PathBuf),
    #[error(
        "cannot locate the Browser Profile: neither XDG_STATE_HOME nor HOME is an absolute path"
    )]
    NoStateDirectory,
    #[error("the Browser Profile {path} is unusable: {reason}")]
    Unusable { path: PathBuf, reason: String },
}

/// The Browser Profile of one platform.
#[derive(Clone, Debug)]
pub(crate) struct BrowserProfile {
    root: PathBuf,
}

impl BrowserProfile {
    /// The profile named `name` (a platform id) in the state directory.
    ///
    /// # Errors
    ///
    /// Returns [`ProfileError::NoStateDirectory`] when no state directory can be resolved.
    pub(crate) fn named(name: &str) -> Result<Self, ProfileError> {
        state_directory()
            .map(|state| Self::at(state.join("browser").join(name)))
            .ok_or(ProfileError::NoStateDirectory)
    }

    pub(crate) fn at(root: PathBuf) -> Self {
        Self { root }
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    /// Whether a login into this profile once succeeded. The session may have expired since.
    pub(crate) fn has_logged_in(&self) -> bool {
        self.login_marker().is_file()
    }

    /// Takes the profile for this process without waiting, creating it with a new fingerprint
    /// seed when it does not exist yet.
    ///
    /// # Errors
    ///
    /// Returns [`ProfileError::Busy`] while another process holds the profile, and
    /// [`ProfileError::Unusable`] when it cannot be created or its identity cannot be read.
    pub(crate) fn lock(&self) -> Result<LockedProfile, ProfileError> {
        if let Some(parent) = self.root.parent() {
            ensure_private_directory(parent).map_err(|error| self.unusable(&error))?;
        }
        ensure_private_directory(&self.root).map_err(|error| self.unusable(&error))?;
        let lock = match StateLock::acquire(&self.root.join("profile.lock"), Duration::ZERO) {
            Ok(lock) => lock,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                return Err(ProfileError::Busy(self.root.clone()));
            }
            Err(error) => return Err(self.unusable(&error)),
        };
        let identity = self.read_or_create_identity()?;
        Ok(LockedProfile {
            profile: self.clone(),
            identity,
            _lock: lock,
        })
    }

    fn read_or_create_identity(&self) -> Result<Identity, ProfileError> {
        let path = self.identity_file();
        let Some(content) = read_state(&path).map_err(|error| self.unusable(&error))? else {
            let identity = Identity {
                version: IDENTITY_VERSION,
                seed: fastrand::u32(1..=i32::MAX.cast_unsigned()),
                screen: None,
            };
            self.write_identity(&identity)?;
            return Ok(identity);
        };
        serde_json::from_str::<Identity>(&content)
            .ok()
            .filter(|identity| identity.version == IDENTITY_VERSION)
            .ok_or_else(|| ProfileError::Unusable {
                path: self.root.clone(),
                reason: format!(
                    "{} is not a profile identity; delete the profile directory and log in again",
                    path.display()
                ),
            })
    }

    fn write_identity(&self, identity: &Identity) -> Result<(), ProfileError> {
        let value = serde_json::to_value(identity).map_err(|error| self.unusable(&error))?;
        write_state(&self.identity_file(), &value).map_err(|error| self.unusable(&error))
    }

    fn identity_file(&self) -> PathBuf {
        self.root.join("profile.json")
    }

    fn login_marker(&self) -> PathBuf {
        self.root.join("login.json")
    }

    fn unusable(&self, error: &impl std::fmt::Display) -> ProfileError {
        ProfileError::Unusable {
            path: self.root.clone(),
            reason: error.to_string(),
        }
    }
}

/// A profile this process holds; dropping it releases the lock.
pub(crate) struct LockedProfile {
    profile: BrowserProfile,
    identity: Identity,
    _lock: StateLock,
}

impl std::fmt::Debug for LockedProfile {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LockedProfile")
            .field("root", &self.profile.root)
            .finish_non_exhaustive()
    }
}

impl LockedProfile {
    pub(crate) fn root(&self) -> &Path {
        self.profile.root()
    }

    /// The directory the browser keeps its own user data in.
    pub(crate) fn user_data_directory(&self) -> PathBuf {
        self.profile.root.join("user-data")
    }

    pub(crate) fn seed(&self) -> u32 {
        self.identity.seed
    }

    /// The screen of the window the user last logged in with, if any.
    pub(crate) fn screen(&self) -> Option<Screen> {
        self.identity.screen
    }

    /// Records a successful login, with the screen of the window it happened in.
    ///
    /// # Errors
    ///
    /// Returns [`ProfileError::Unusable`] when the identity or the marker cannot be written.
    pub(crate) fn record_login(&mut self, screen: Option<Screen>) -> Result<(), ProfileError> {
        if screen.is_some() {
            self.identity.screen = screen;
            self.profile.write_identity(&self.identity)?;
        }
        let marker = json!({"logged_in_at": chrono::Utc::now().to_rfc3339()});
        write_state(&self.profile.login_marker(), &marker)
            .map_err(|error| self.profile.unusable(&error))
    }
}

#[cfg(test)]
fn entries(path: &Path) -> Vec<String> {
    let mut names = std::fs::read_dir(path).map_or_default(|entries| {
        entries
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
    });
    names.sort();
    names
}

#[cfg(test)]
mod tests {
    use super::{BrowserProfile, ProfileError, Screen, entries};

    fn profile() -> (tempfile::TempDir, BrowserProfile) {
        let state = tempfile::tempdir().expect("create state directory");
        let profile = BrowserProfile::at(state.path().join("browser/xiaohongshu"));
        (state, profile)
    }

    #[test]
    fn the_seed_stays_the_same_across_uses() {
        let (_state, profile) = profile();

        let first = profile.lock().expect("lock new profile").seed();
        let second = profile.lock().expect("lock existing profile").seed();

        assert_eq!(first, second);
    }

    #[cfg(unix)]
    #[test]
    fn the_profile_directory_is_private_to_the_user() {
        use std::os::unix::fs::PermissionsExt;

        let (_state, profile) = profile();
        drop(profile.lock().expect("lock new profile"));
        let mode = std::fs::metadata(profile.root())
            .expect("profile metadata")
            .permissions()
            .mode();

        assert_eq!(mode & 0o777, 0o700);
    }

    #[test]
    fn a_held_profile_is_busy_for_another_holder() {
        let (_state, profile) = profile();
        let _held = profile.lock().expect("lock profile");

        let other = BrowserProfile::at(profile.root().to_path_buf()).lock();

        assert!(
            matches!(other, Err(ProfileError::Busy(_))),
            "a second holder got the profile: {other:?}"
        );
    }

    #[test]
    fn a_recorded_login_is_marked_and_keeps_its_screen() {
        let (_state, profile) = profile();
        let screen = Screen {
            width: 1512,
            height: 982,
            avail_top: 25,
            avail_height: 944,
            device_pixel_ratio: 2.0,
            window_width: 1200,
            window_height: 800,
        };
        profile
            .lock()
            .expect("lock profile")
            .record_login(Some(screen))
            .expect("record login");

        let reopened = profile.lock().expect("lock profile again");

        assert_eq!(
            (profile.has_logged_in(), reopened.screen()),
            (true, Some(screen))
        );
    }

    #[test]
    fn a_new_profile_has_no_login() {
        let (_state, profile) = profile();
        drop(profile.lock().expect("lock new profile"));

        assert_eq!(
            (profile.has_logged_in(), entries(profile.root())),
            (
                false,
                vec!["profile.json".to_owned(), "profile.lock".to_owned()]
            )
        );
    }
}
