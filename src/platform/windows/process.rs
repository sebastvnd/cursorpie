//! Small process and file helpers whose implementation is specific to this OS (see the table in
//! `platform/mod.rs`): the rest of the crate calls them through [`platform`](crate::platform) and
//! never reaches for `std::os::*` itself. (`std::os::windows` exists on Windows only, so the
//! calls that need it are compiled there; with `check-windows` on another OS they do nothing.)

use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;
use std::process::{Child, Command, Stdio};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

/// `CREATE_NO_WINDOW`: no console flashes on screen.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// `DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP`.
#[cfg(windows)]
const DETACHED: u32 = 0x0000_0208;

/// Starts `cmd` without a console window.
pub fn hide_console(cmd: &mut Command) {
    #[cfg(windows)]
    {
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    {
        let _ = cmd;
    }
}

/// Ends `child` and the programs it started. `cmd.exe /C` runs the person's command as a
/// grandchild: killing only the shell leaves that program holding the end of the output pipe, so
/// the thread that reads it would wait for ever, with its buffer, for every command that timed out.
pub fn terminate_tree(child: &mut Child) {
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .status();
    }
    let _ = child.kill();
}

/// Gives `cmd` the arguments that run `line` in `cmd.exe` as typed. The line is passed as it is
/// (`/S` makes cmd strip only the outer pair of quotes, as the *Command* block does).
pub fn shell_line(cmd: &mut Command, line: &str) {
    #[cfg(windows)]
    {
        cmd.raw_arg(format!("/S /C \"{line}\""));
    }
    #[cfg(not(windows))]
    {
        cmd.arg("/S").arg("/C").arg(line);
    }
}

/// Makes `cmd` outlive this program.
pub fn detach_from_parent(cmd: &mut Command) {
    #[cfg(windows)]
    {
        cmd.creation_flags(DETACHED);
    }
    #[cfg(not(windows))]
    {
        let _ = cmd;
    }
}

/// Makes `path` runnable: files have no such bit here.
pub fn make_executable(_path: &Path) -> io::Result<()> {
    Ok(())
}

/// Creates (or empties) `path`. The folder of the person's profile is theirs alone, so no mode is
/// set.
pub fn create_private_file(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
}
