//! System tray icon. Runs on its own thread and reports clicks as [`TrayCmd`]s.

use std::sync::mpsc::Sender;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::Tray;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::Tray;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayCmd {
    Show,
    Start,
    Stop,
    Quit,
}

/// Called after a command is queued, so the UI can wake up and handle it.
pub type Notify = Box<dyn Fn() + Send + Sync + 'static>;

/// Sends commands to the app.
pub struct TrayEvents {
    tx: Sender<TrayCmd>,
    notify: Notify,
}

impl TrayEvents {
    pub fn new(tx: Sender<TrayCmd>, notify: Notify) -> Self {
        Self { tx, notify }
    }

    pub fn send(&self, cmd: TrayCmd) {
        let _ = self.tx.send(cmd);
        (self.notify)();
    }
}
