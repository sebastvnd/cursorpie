//! Small process and file helpers whose implementation is specific to this OS (see the table in
//! `platform/mod.rs`): the rest of the crate calls them through [`platform`](crate::platform) and
//! never reaches for `std::os::*` itself.

use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command};

/// Nothing to hide: a program started from here has no console window of its own.
pub fn hide_console(_cmd: &mut Command) {}

/// Ends `child` (a shell line runs as the shell itself, so there is no tree to end).
pub fn terminate_tree(child: &mut Child) {
    let _ = child.kill();
}

/// Gives `cmd` the arguments that run `line` in the shell as typed (`/bin/sh -c`).
pub fn shell_line(cmd: &mut Command, line: &str) {
    cmd.arg("-c").arg(line);
}

/// Makes `cmd` outlive this program: it goes in a process group of its own.
pub fn detach_from_parent(cmd: &mut Command) {
    cmd.process_group(0);
}

/// Makes `path` runnable.
pub fn make_executable(path: &Path) -> io::Result<()> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
}

/// Creates (or empties) `path` so that only its owner can read it, also when it existed before
/// with a wider mode.
pub fn create_private_file(path: &Path) -> io::Result<File> {
    let file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.set_permissions(fs::Permissions::from_mode(0o600))?;
    Ok(file)
}
