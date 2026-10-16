//! Dual Audio - play your PC's audio through several output devices at once.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod audio;
mod engine;
mod icon;
mod settings;
mod tray;
mod ui;

use std::sync::{Arc, mpsc};

use eframe::egui;

use crate::app::Controller;
use crate::tray::{Tray, TrayCmd, TrayEvents};
use crate::ui::{ContextSlot, MainWindow};

pub const APP_NAME: &str = "Dual Audio";

fn main() {
    install_panic_log();
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--selftest") {
        std::process::exit(selftest());
    }
    if args.iter().any(|a| a == "--version") {
        println!("{APP_NAME} {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    if args.iter().any(|a| a == "--list-devices") {
        list_devices();
        return;
    }

    let slot: ContextSlot = Arc::default();
    let (tx, rx) = mpsc::channel();
    let notify = {
        let slot = slot.clone();
        Box::new(move || {
            if let Some(ctx) = slot.lock().unwrap().as_ref() {
                ctx.request_repaint();
            }
        })
    };
    let tray = Tray::spawn(TrayEvents::new(tx, notify));
    let mut ctl = Controller::new(tray, rx);
    let start_hidden = args.iter().any(|a| a == "--tray") && ctl.has_tray();
    if start_hidden {
        ctl.start();
    }

    // Closing the window keeps sharing in the tray; "Show" opens a new window.
    let mut show = !start_hidden;
    loop {
        if show {
            if let Err(e) = run_window(&mut ctl, &slot) {
                eprintln!("Cannot open the window: {e}");
                break;
            }
            *slot.lock().unwrap() = None;
            if ctl.quit_requested || !ctl.has_tray() {
                break;
            }
            ctl.save();
        }
        show = ctl.wait_for_tray() == TrayCmd::Show;
        if ctl.quit_requested {
            break;
        }
    }
    ctl.shutdown();
}

fn run_window(ctl: &mut Controller, slot: &ContextSlot) -> eframe::Result {
    let size = 64;
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(APP_NAME)
            .with_app_id(settings::APP_ID)
            .with_inner_size([820.0, 720.0])
            .with_min_inner_size([680.0, 560.0])
            .with_icon(egui::IconData {
                rgba: icon::rgba(size),
                width: size,
                height: size,
            }),
        run_and_return: true,
        centered: true,
        ..Default::default()
    };
    eframe::run_native(
        APP_NAME,
        options,
        Box::new(|cc| Ok(Box::new(MainWindow::new(cc, ctl, slot)))),
    )
}

fn list_devices() {
    match audio::list_devices() {
        Ok(list) => {
            for d in &list.outputs {
                let mark = if list.default_id.as_ref() == Some(&d.id) {
                    "*"
                } else {
                    " "
                };
                println!("{mark} {}  [{}]", d.name, d.id);
            }
        }
        Err(e) => eprintln!("{e}"),
    }
}

/// Smoke test used by CI: exercises the parts that need no audio hardware.
fn selftest() -> i32 {
    let dir = settings::config_dir();
    let ok = dir.join("selftest.ok");
    let err = dir.join("selftest.err");
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::remove_file(&ok);
    let _ = std::fs::remove_file(&err);
    let icon = icon::rgba(64);
    let result = if icon.len() == 64 * 64 * 4 && icon.iter().any(|&b| b != 0) {
        Ok(())
    } else {
        Err("icon rasterisation failed")
    };
    match result {
        Ok(()) => {
            let _ = std::fs::write(ok, env!("CARGO_PKG_VERSION"));
            0
        }
        Err(e) => {
            let _ = std::fs::write(err, e);
            1
        }
    }
}

fn install_panic_log() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let dir = settings::config_dir();
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join("error.log"), info.to_string());
        default(info);
    }));
}
