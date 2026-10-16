//! Windows notification-area icon. The icon lives on a dedicated thread that
//! pumps its own Win32 message loop, so it keeps working while the main
//! window is closed.

use std::sync::mpsc;
use std::thread::JoinHandle;

use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, TrayIconBuilder, TrayIconEvent};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, MSG, PostThreadMessageW, TranslateMessage, WM_APP, WM_QUIT,
};

use super::{TrayCmd, TrayEvents};
use crate::{APP_NAME, icon};

const WM_SET_SHARING: u32 = WM_APP + 1;

pub struct Tray {
    thread_id: u32,
    thread: Option<JoinHandle<()>>,
}

impl Tray {
    pub fn spawn(events: TrayEvents) -> Option<Self> {
        let (ready_tx, ready_rx) = mpsc::channel();
        let thread = std::thread::spawn(move || run(events, ready_tx));
        match ready_rx.recv() {
            Ok(Some(thread_id)) => Some(Self {
                thread_id,
                thread: Some(thread),
            }),
            _ => None,
        }
    }

    pub fn set_sharing(&self, sharing: bool) {
        unsafe {
            PostThreadMessageW(self.thread_id, WM_SET_SHARING, sharing as usize, 0);
        }
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        unsafe {
            PostThreadMessageW(self.thread_id, WM_QUIT, 0, 0);
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn run(events: TrayEvents, ready: mpsc::Sender<Option<u32>>) {
    let show = MenuItem::new("Show window", true, None);
    let start = MenuItem::new("Start sharing", true, None);
    let stop = MenuItem::new("Stop sharing", false, None);
    let quit = MenuItem::new("Quit", true, None);
    let menu = Menu::new();
    let built = menu
        .append_items(&[
            &show,
            &start,
            &stop,
            &PredefinedMenuItem::separator(),
            &quit,
        ])
        .map_err(|e| e.to_string())
        .and_then(|_| Icon::from_rgba(icon::rgba(64), 64, 64).map_err(|e| e.to_string()))
        .and_then(|icon| {
            TrayIconBuilder::new()
                .with_menu(Box::new(menu))
                .with_tooltip(format!("{APP_NAME} - idle"))
                .with_icon(icon)
                .build()
                .map_err(|e| e.to_string())
        });
    let tray = match built {
        Ok(tray) => tray,
        Err(e) => {
            eprintln!("Tray unavailable: {e}");
            let _ = ready.send(None);
            return;
        }
    };

    let events = std::sync::Arc::new(events);
    let ids = [
        (show.id().clone(), TrayCmd::Show),
        (start.id().clone(), TrayCmd::Start),
        (stop.id().clone(), TrayCmd::Stop),
        (quit.id().clone(), TrayCmd::Quit),
    ];
    {
        let events = events.clone();
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            if let Some((_, cmd)) = ids.iter().find(|(id, _)| *id == e.id) {
                events.send(*cmd);
            }
        }));
    }
    {
        let events = events.clone();
        TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
            if let TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            } = e
            {
                events.send(TrayCmd::Show);
            }
        }));
    }

    let _ = ready.send(Some(unsafe {
        windows_sys::Win32::System::Threading::GetCurrentThreadId()
    }));

    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            if msg.hwnd.is_null() && msg.message == WM_SET_SHARING {
                let sharing = msg.wParam != 0;
                start.set_enabled(!sharing);
                stop.set_enabled(sharing);
                let state = if sharing { "sharing" } else { "idle" };
                let _ = tray.set_tooltip(Some(format!("{APP_NAME} - {state}")));
                continue;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    MenuEvent::set_event_handler(None::<fn(MenuEvent)>);
    TrayIconEvent::set_event_handler(None::<fn(TrayIconEvent)>);
    drop(tray);
}
