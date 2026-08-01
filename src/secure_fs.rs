use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::Path;

/// Creates a configuration directory restricted to the current user.
///
/// # Errors
///
/// Returns an I/O error when the directory cannot be created or restricted.
#[cfg(unix)]
pub fn ensure_private_directory(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)?;
    set_mode(path, 0o700)
}

/// Creates a configuration directory restricted to the Windows owner.
///
/// # Errors
///
/// Returns an I/O error when the directory cannot be created or restricted.
#[cfg(windows)]
pub fn ensure_private_directory(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)?;
    restrict_windows_acl(path)
}

/// Opens or creates a private configuration file.
///
/// # Errors
///
/// Returns an I/O error when the file cannot be opened or restricted.
#[cfg(unix)]
pub fn create_private_file(path: &Path) -> io::Result<File> {
    let file = open_private_file(path)?;
    set_mode(path, 0o600)?;
    Ok(file)
}

/// Opens or creates a private configuration file on Windows.
///
/// # Errors
///
/// Returns an I/O error when the file cannot be opened or restricted.
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
pub(crate) fn create_new_private_file(path: &Path) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(path)
}

#[cfg(windows)]
pub(crate) fn create_new_private_file(path: &Path) -> io::Result<File> {
    let file = OpenOptions::new().create_new(true).write(true).open(path)?;
    restrict_windows_acl(path)?;
    Ok(file)
}

#[cfg(unix)]
pub(crate) fn restrict_private_file(path: &Path) -> io::Result<()> {
    set_mode(path, 0o600)
}

#[cfg(windows)]
pub(crate) fn restrict_private_file(path: &Path) -> io::Result<()> {
    restrict_windows_acl(path)
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

#[cfg(unix)]
pub(crate) fn has_private_permissions(path: &Path, expected: u32) -> io::Result<bool> {
    use std::os::unix::fs::PermissionsExt;

    let actual = fs::metadata(path)?.permissions().mode() & 0o777;
    Ok(actual & !expected == 0)
}

#[cfg(windows)]
pub(crate) fn has_private_permissions(path: &Path, _expected: u32) -> io::Result<bool> {
    use winapi::um::winnt::PSID;
    use windows_acl::acl::{ACL, AceType};
    use windows_acl::helper::sid_to_string;

    let path = path.to_str().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "configuration path is not valid Unicode",
        )
    })?;
    let acl = ACL::from_file_path(path, false)
        .map_err(|code| io::Error::other(format!("cannot read Windows ACL: {code}")))?;
    let entries = acl
        .all()
        .map_err(|code| io::Error::other(format!("cannot enumerate Windows ACL: {code}")))?;
    Ok(!entries.is_empty()
        && entries.iter().all(|entry| {
            let is_owner = entry.sid.as_ref().is_some_and(|sid| {
                sid_to_string(sid.as_ptr() as PSID).is_ok_and(|sid| sid == "S-1-3-4")
            });
            let is_allow = matches!(
                entry.entry_type,
                AceType::AccessAllow
                    | AceType::AccessAllowCallback
                    | AceType::AccessAllowObject
                    | AceType::AccessAllowCallbackObject
            );
            is_owner && is_allow
        }))
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn has_private_permissions(_path: &Path, _expected: u32) -> io::Result<bool> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "private configuration permissions are unavailable",
    ))
}

#[cfg(not(any(unix, windows)))]
pub fn ensure_private_directory(_path: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "private configuration permissions are unavailable",
    ))
}

#[cfg(not(any(unix, windows)))]
pub fn create_private_file(_path: &Path) -> io::Result<File> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "private configuration permissions are unavailable",
    ))
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn create_new_private_file(_path: &Path) -> io::Result<File> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "private configuration permissions are unavailable",
    ))
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn restrict_private_file(_path: &Path) -> io::Result<()> {
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
