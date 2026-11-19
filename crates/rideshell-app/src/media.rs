//! "Now playing" for the status bar: the Spotify desktop app, read and
//! controlled through Windows' own media controls (the same ones the volume
//! flyout uses). No sign-in, no network, no window: Spotify only has to be
//! running.
//!
//! Everything happens on one background thread. It checks about once a
//! second and tells the UI only when something changed, so the terminals
//! never wait for it.

use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::time::Duration;

use serde::Serialize;
use tao::event_loop::EventLoopProxy;

use crate::UserEvent;

#[derive(Debug, Clone, Copy)]
pub enum Command {
    Toggle,
    Next,
    Previous,
}

/// What the status bar shows. `None` (sent as `null`) hides the player.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NowPlaying {
    pub title: String,
    pub artist: String,
    pub playing: bool,
}

/// Start the watcher; send commands through the returned sender.
pub fn start(proxy: EventLoopProxy<UserEvent>) -> Sender<Command> {
    let (tx, rx) = mpsc::channel::<Command>();
    std::thread::spawn(move || {
        let mut last: Option<Option<NowPlaying>> = None;
        // The player appears once Spotify has played in this run, and stays
        // while it is paused (so it can be resumed); it goes when Spotify closes.
        let mut has_played = false;
        let mut wait = Duration::ZERO;
        loop {
            match rx.recv_timeout(wait) {
                Ok(cmd) => imp::send(cmd),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            let now = imp::read();
            match &now {
                Some(n) if n.playing => has_played = true,
                None => has_played = false,
                _ => {}
            }
            let shown = now.filter(|_| has_played);
            wait = Duration::from_millis(if shown.is_some() { 800 } else { 2000 });
            if last.as_ref() != Some(&shown) {
                let _ = proxy.send_event(UserEvent::Media(serde_json::to_value(&shown).unwrap_or_default()));
                last = Some(shown);
            }
        }
    });
    tx
}

#[cfg(windows)]
mod imp {
    use windows::Media::Control::{
        GlobalSystemMediaTransportControlsSession as Session,
        GlobalSystemMediaTransportControlsSessionManager as Manager,
        GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
    };

    use super::{Command, NowPlaying};

    fn spotify() -> Option<Session> {
        let manager = Manager::RequestAsync().ok()?.join().ok()?;
        let sessions = manager.GetSessions().ok()?;
        sessions
            .into_iter()
            .find(|s| s.SourceAppUserModelId().is_ok_and(|id| id.to_string().to_ascii_lowercase().contains("spotify")))
    }

    pub fn read() -> Option<NowPlaying> {
        let s = spotify()?;
        let props = s.TryGetMediaPropertiesAsync().ok()?.join().ok()?;
        let title = props.Title().map(|t| t.to_string()).unwrap_or_default();
        if title.is_empty() {
            return None; // Spotify open, nothing loaded
        }
        let playing = s.GetPlaybackInfo().and_then(|i| i.PlaybackStatus()).is_ok_and(|st| st == Status::Playing);
        Some(NowPlaying { title, artist: props.Artist().map(|a| a.to_string()).unwrap_or_default(), playing })
    }

    pub fn send(cmd: Command) {
        let Some(s) = spotify() else { return };
        let op = match cmd {
            Command::Toggle => s.TryTogglePlayPauseAsync(),
            Command::Next => s.TrySkipNextAsync(),
            Command::Previous => s.TrySkipPreviousAsync(),
        };
        if let Ok(op) = op {
            let _ = op.join();
        }
        // Spotify updates its state a moment after the command.
        std::thread::sleep(std::time::Duration::from_millis(150));
    }
}

#[cfg(not(windows))]
mod imp {
    use super::{Command, NowPlaying};
    pub fn read() -> Option<NowPlaying> {
        None
    }
    pub fn send(_cmd: Command) {}
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "reads the real Spotify app"]
    fn live_read() {
        println!("NOW: {:?}", super::imp::read());
        #[cfg(windows)]
        std::thread::spawn(|| {
            use windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager as M;
            let m = M::RequestAsync().unwrap().join().unwrap();
            let ids: Vec<String> =
                m.GetSessions().unwrap().into_iter().map(|s| s.SourceAppUserModelId().unwrap().to_string()).collect();
            println!("SESSIONS: {ids:?}");
        })
        .join()
        .unwrap();
    }
}
