//! Child processes that never open a console window.
//!
//! `phoecord` is a console program, but the installer window starts it without a console, and the
//! window itself has none. On Windows a console program started from such a process (certutil, reg,
//! powershell, taskkill) gets a console window of its own, which flashes on screen. Every child goes
//! through [`command`] or [`hide_console`] so that never happens. Output is still captured through
//! the usual pipes, so nothing changes when `phoecord` runs in a terminal.

use std::ffi::OsStr;
use std::process::Command;

/// The Windows process creation flag that starts a console program without a console window.
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// `Command::new(program)` that opens no console window on Windows.
pub fn command(program: impl AsRef<OsStr>) -> Command {
    let mut c = Command::new(program);
    hide_console(&mut c);
    c
}

/// Make `cmd` open no console window on Windows; a no-op elsewhere.
pub fn hide_console(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}
