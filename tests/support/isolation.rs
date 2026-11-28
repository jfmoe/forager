//! The environment every spawned forager binary starts from, shared by the command contract
//! tests and the permission tests, which run in their own test binary.

use std::ffi::OsStr;
use std::process::Command;

use super::process::proxy_through;

/// A program with an empty environment whose HTTP, and that of every process it starts, goes
/// through `proxy`. Windows keeps `SystemRoot`, without which a process cannot start.
pub(crate) fn command_with_empty_environment(program: impl AsRef<OsStr>, proxy: &str) -> Command {
    let mut command = Command::new(program);
    command.env_clear();
    proxy_through(&mut command, proxy);
    #[cfg(windows)]
    if let Some(system_root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", system_root);
    }
    command
}
