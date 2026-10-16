//! StatusNotifierItem tray over D-Bus (KDE, GNOME with the AppIndicator
//! extension, XFCE, Cinnamon, ...). Pure Rust, no GTK needed.

use ksni::blocking::{Handle, TrayMethods};
use ksni::menu::{MenuItem, StandardItem};

use super::{TrayCmd, TrayEvents};
use crate::{APP_NAME, icon};

struct SniTray {
    events: TrayEvents,
    sharing: bool,
    icon: ksni::Icon,
}

impl ksni::Tray for SniTray {
    fn id(&self) -> String {
        crate::settings::APP_ID.into()
    }

    fn title(&self) -> String {
        APP_NAME.into()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        vec![self.icon.clone()]
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: format!(
                "{APP_NAME} - {}",
                if self.sharing { "sharing" } else { "idle" }
            ),
            ..Default::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.events.send(TrayCmd::Show);
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let item = |label: &str, cmd: TrayCmd, enabled: bool| -> MenuItem<Self> {
            StandardItem {
                label: label.into(),
                enabled,
                activate: Box::new(move |t: &mut Self| t.events.send(cmd)),
                ..Default::default()
            }
            .into()
        };
        vec![
            item("Show window", TrayCmd::Show, true),
            item("Start sharing", TrayCmd::Start, !self.sharing),
            item("Stop sharing", TrayCmd::Stop, self.sharing),
            MenuItem::Separator,
            item("Quit", TrayCmd::Quit, true),
        ]
    }
}

pub struct Tray {
    handle: Handle<SniTray>,
}

impl Tray {
    /// Returns `None` when the desktop has no tray (no StatusNotifierWatcher).
    pub fn spawn(events: TrayEvents) -> Option<Self> {
        const SIZE: u32 = 64;
        // ksni wants ARGB32 in network byte order.
        let data = icon::rgba(SIZE)
            .chunks_exact(4)
            .flat_map(|p| [p[3], p[0], p[1], p[2]])
            .collect();
        let tray = SniTray {
            events,
            sharing: false,
            icon: ksni::Icon {
                width: SIZE as i32,
                height: SIZE as i32,
                data,
            },
        };
        match tray.spawn() {
            Ok(handle) => Some(Self { handle }),
            Err(e) => {
                eprintln!("Tray unavailable: {e}");
                None
            }
        }
    }

    pub fn set_sharing(&self, sharing: bool) {
        self.handle.update(|t| t.sharing = sharing);
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        self.handle.shutdown().wait();
    }
}
