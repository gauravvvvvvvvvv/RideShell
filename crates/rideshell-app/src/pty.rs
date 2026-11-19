//! Terminal sessions: one pseudo-console (ConPTY on Windows) per tab, each
//! running `rideshell open` (PowerShell with RideShell loaded).

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use tao::event_loop::EventLoopProxy;

use crate::UserEvent;

pub struct Session {
    master: Box<dyn MasterPty + Send>,
    /// Keystrokes go to a per-tab writer thread: a program that stops reading
    /// its input can then stall only its own tab, never the window.
    input: std::sync::mpsc::Sender<Vec<u8>>,
    killer: Box<dyn ChildKiller + Send + Sync>,
}

pub struct Spec<'a> {
    pub cli: &'a Path,
    pub profile: Option<&'a str>,
    pub cwd: &'a Path,
    pub command: Option<&'a str>,
    pub cols: u16,
    pub rows: u16,
}

/// The `rideshell` command-line program next to this app (or on PATH).
pub fn find_cli() -> Option<PathBuf> {
    let name = if cfg!(windows) { "rideshell.exe" } else { "rideshell" };
    if let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)) {
        let sibling = dir.join(name);
        if sibling.is_file() {
            return Some(sibling);
        }
    }
    which::which("rideshell").ok()
}

pub fn spawn(id: u32, spec: Spec, proxy: EventLoopProxy<UserEvent>) -> Result<Session> {
    let pair = native_pty_system()
        .openpty(PtySize { rows: spec.rows.max(2), cols: spec.cols.max(10), pixel_width: 0, pixel_height: 0 })
        .context("cannot create a pseudo console")?;

    let mut cmd = CommandBuilder::new(spec.cli);
    cmd.args(["open", "--no-banner"]);
    if let Some(p) = spec.profile {
        cmd.args(["--profile", p]);
    }
    if let Some(c) = spec.command {
        cmd.args(["--command", c]);
    }
    cmd.cwd(spec.cwd);
    cmd.env("TERM_PROGRAM", rideshell_core::brand::APP_NAME);
    cmd.env("COLORTERM", "truecolor");
    for v in rideshell_core::brand::env::ALL {
        if v != rideshell_core::brand::env::HOME {
            cmd.env_remove(v);
        }
    }

    let child = pair.slave.spawn_command(cmd).context("cannot start the RideShell session")?;
    drop(pair.slave);
    let killer = child.clone_killer();
    let mut reader = pair.master.try_clone_reader()?;
    let mut writer = pair.master.take_writer()?;
    let (input, keys) = std::sync::mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        // Ends when the tab is closed (sender dropped) or the session is gone.
        for data in keys {
            if writer.write_all(&data).and_then(|()| writer.flush()).is_err() {
                break;
            }
        }
    });

    let data_proxy = proxy.clone();
    std::thread::spawn(move || {
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if data_proxy.send_event(UserEvent::PtyData(id, buf[..n].to_vec())).is_err() {
                        break;
                    }
                }
            }
        }
    });
    let mut child = child;
    std::thread::spawn(move || {
        let code = child.wait().ok().map(|s| s.exit_code());
        let _ = proxy.send_event(UserEvent::PtyExit(id, code));
    });

    Ok(Session { master: pair.master, input, killer })
}

impl Session {
    pub fn write(&mut self, data: &[u8]) {
        let _ = self.input.send(data.to_vec());
    }

    pub fn resize(&self, cols: u16, rows: u16) {
        let _ = self.master.resize(PtySize { rows: rows.max(2), cols: cols.max(10), pixel_width: 0, pixel_height: 0 });
    }

    pub fn kill(&mut self) {
        let _ = self.killer.kill();
    }
}
