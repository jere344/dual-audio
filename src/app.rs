//! Application state shared by the window and the tray: device list,
//! per-device settings and the running [`Engine`].

use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::audio::{self, DeviceInfo, DeviceList};
use crate::engine::{self, Engine, OutputSpec};
use crate::settings::{DeviceSettings, Settings};
use crate::tray::{Tray, TrayCmd};

const SAVE_DEBOUNCE: Duration = Duration::from_millis(700);
const SILENCE_WARNING: Duration = Duration::from_millis(1500);
const TEST_TONE_LABEL: Duration = Duration::from_millis(1300);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Muted,
    Good,
    Bad,
}

#[derive(Clone, Debug)]
pub struct Status {
    pub text: String,
    pub tone: Tone,
}

impl Status {
    fn new(text: impl Into<String>, tone: Tone) -> Self {
        Self {
            text: text.into(),
            tone,
        }
    }
}

pub struct Row {
    pub device: DeviceInfo,
    pub settings: DeviceSettings,
    testing_until: Option<Instant>,
    test_error: Arc<Mutex<Option<String>>>,
}

/// What a device row should display next to its name.
pub enum RowState {
    Idle,
    Source,
    Testing,
    Starting,
    Live,
    Error(String),
}

pub struct Controller {
    pub settings: Settings,
    pub devices: DeviceList,
    pub rows: Vec<Row>,
    /// Capture source id; `None` follows the system default output.
    pub source: Option<String>,
    pub status: Status,
    engine: Option<Engine>,
    tray: Option<Tray>,
    tray_rx: Receiver<TrayCmd>,
    dirty_since: Option<Instant>,
    silent_since: Option<Instant>,
    pub quit_requested: bool,
}

impl Controller {
    pub fn new(tray: Option<Tray>, tray_rx: Receiver<TrayCmd>) -> Self {
        let mut ctl = Self {
            settings: Settings::load(),
            devices: DeviceList::default(),
            rows: Vec::new(),
            source: None,
            status: Status::new("Stopped", Tone::Muted),
            engine: None,
            tray,
            tray_rx,
            dirty_since: None,
            silent_since: None,
            quit_requested: false,
        };
        ctl.refresh_devices();
        ctl
    }

    pub fn has_tray(&self) -> bool {
        self.tray.is_some()
    }

    pub fn running(&self) -> bool {
        self.engine.is_some()
    }

    pub fn active_outputs(&self) -> usize {
        self.engine.as_ref().map_or(0, Engine::output_count)
    }

    pub fn peak(&self) -> f32 {
        self.engine.as_ref().map_or(0.0, Engine::peak)
    }

    /// The device whose audio is (or will be) captured.
    pub fn capture_device(&self) -> Option<&DeviceInfo> {
        if let Some(engine) = &self.engine {
            return self
                .devices
                .outputs
                .iter()
                .find(|d| d.id == engine.source().id);
        }
        match &self.source {
            Some(id) => self.devices.outputs.iter().find(|d| &d.id == id),
            None => self.devices.default_device(),
        }
    }

    pub fn refresh_devices(&mut self) {
        let was_running = self.running();
        self.engine = None;
        match audio::list_devices() {
            Ok(list) => self.devices = list,
            Err(e) => {
                self.devices = DeviceList::default();
                self.status = Status::new(format!("Audio system unavailable: {e}"), Tone::Bad);
            }
        }
        self.rows = self
            .devices
            .outputs
            .iter()
            .map(|d| Row {
                device: d.clone(),
                settings: self
                    .settings
                    .devices
                    .get(&d.name)
                    .cloned()
                    .unwrap_or_default(),
                testing_until: None,
                test_error: Arc::default(),
            })
            .collect();
        self.source = self
            .settings
            .source
            .as_ref()
            .and_then(|name| self.devices.outputs.iter().find(|d| &d.name == name))
            .map(|d| d.id.clone());
        if was_running {
            self.status = Status::new(
                "Devices refreshed - stopped. Press Start sharing.",
                Tone::Muted,
            );
            self.sync_tray();
        }
    }

    pub fn set_source(&mut self, id: Option<String>) {
        if self.source == id {
            return;
        }
        self.source = id;
        self.settings.source = self
            .source
            .as_ref()
            .and_then(|id| self.devices.outputs.iter().find(|d| &d.id == id))
            .map(|d| d.name.clone());
        self.mark_dirty();
        if self.running() {
            self.stop();
            self.start();
        }
    }

    pub fn toggle(&mut self) {
        if self.running() {
            self.stop()
        } else {
            self.start()
        }
    }

    pub fn start(&mut self) {
        if self.running() {
            return;
        }
        let source_id = self.capture_device().map(|d| d.id.clone());
        let specs: Vec<OutputSpec> = self
            .rows
            .iter()
            .filter(|r| r.settings.enabled && Some(&r.device.id) != source_id.as_ref())
            .map(spec)
            .collect();
        if specs.is_empty() {
            self.status = Status::new(
                "Cannot start: select at least one output device.",
                Tone::Bad,
            );
            return;
        }
        let source = self
            .source
            .as_ref()
            .and_then(|id| self.devices.outputs.iter().find(|d| &d.id == id));
        match Engine::start(source, &specs) {
            Ok(engine) => {
                self.engine = Some(engine);
                self.silent_since = None;
                self.status = Status::new(
                    format!("Sharing to {} device(s)", self.active_outputs()),
                    Tone::Good,
                );
            }
            Err(e) => self.status = Status::new(format!("Cannot start: {e}"), Tone::Bad),
        }
        self.sync_tray();
    }

    pub fn stop(&mut self) {
        self.engine = None;
        self.status = Status::new("Stopped", Tone::Muted);
        self.sync_tray();
    }

    pub fn set_enabled(&mut self, i: usize, enabled: bool) {
        let row = &mut self.rows[i];
        row.settings.enabled = enabled;
        let spec = spec(row);
        let source_id = self.capture_device().map(|d| d.id.clone());
        if let Some(engine) = &mut self.engine {
            if enabled && Some(&spec.device.id) != source_id.as_ref() {
                engine.add_output(&spec);
            } else {
                engine.remove_output(&spec.device.id);
            }
            if engine.output_count() == 0 {
                self.stop();
            } else {
                self.status = Status::new(
                    format!("Sharing to {} device(s)", engine.output_count()),
                    Tone::Good,
                );
            }
        }
        self.mark_dirty();
    }

    pub fn set_volume(&mut self, i: usize, volume: f32) {
        let row = &mut self.rows[i];
        row.settings.volume = volume;
        if let Some(out) = self.engine.as_ref().and_then(|e| e.output(&row.device.id)) {
            out.set_volume(volume);
        }
        self.mark_dirty();
    }

    pub fn set_delay(&mut self, i: usize, delay_ms: u32) {
        let row = &mut self.rows[i];
        row.settings.delay_ms = delay_ms;
        if let Some(out) = self.engine.as_ref().and_then(|e| e.output(&row.device.id)) {
            out.set_delay_ms(delay_ms);
        }
        self.mark_dirty();
    }

    pub fn test(&mut self, i: usize) {
        let row = &mut self.rows[i];
        row.testing_until = Some(Instant::now() + TEST_TONE_LABEL);
        *row.test_error.lock().unwrap() = None;
        let slot = row.test_error.clone();
        engine::play_test_tone(row.device.clone(), move |e| *slot.lock().unwrap() = Some(e));
    }

    pub fn row_state(&self, i: usize) -> RowState {
        let row = &self.rows[i];
        if let Some(e) = row.test_error.lock().unwrap().clone() {
            return RowState::Error(e);
        }
        if row.testing_until.is_some_and(|t| Instant::now() < t) {
            return RowState::Testing;
        }
        if self.capture_device().is_some_and(|d| d.id == row.device.id) {
            return RowState::Source;
        }
        let Some(out) = self.engine.as_ref().and_then(|e| e.output(&row.device.id)) else {
            return RowState::Idle;
        };
        if let Some(e) = out.error() {
            RowState::Error(e)
        } else if out.is_playing() {
            RowState::Live
        } else {
            RowState::Starting
        }
    }

    /// Periodic housekeeping: tray commands, engine health, settings flush.
    /// Returns `true` when the tray asked to show the window.
    pub fn tick(&mut self) -> bool {
        let mut show = false;
        while let Ok(cmd) = self.tray_rx.try_recv() {
            match cmd {
                TrayCmd::Show => show = true,
                TrayCmd::Start => self.start(),
                TrayCmd::Stop => self.stop(),
                TrayCmd::Quit => self.quit_requested = true,
            }
        }

        if let Some(err) = self.engine.as_ref().and_then(Engine::capture_error) {
            self.engine = None;
            self.status = Status::new(format!("Capture stopped: {err}"), Tone::Bad);
            self.sync_tray();
        }

        if self.running() {
            if self.peak() > 1e-4 {
                if self.silent_since.take().is_some() && self.status.tone == Tone::Bad {
                    self.status = Status::new(
                        format!("Sharing to {} device(s)", self.active_outputs()),
                        Tone::Good,
                    );
                }
            } else {
                let since = *self.silent_since.get_or_insert_with(Instant::now);
                if since.elapsed() > SILENCE_WARNING {
                    self.status = Status::new(
                        "No audio on the source. Play something, or pick another source above.",
                        Tone::Bad,
                    );
                }
            }
        } else {
            self.silent_since = None;
        }

        if self
            .dirty_since
            .is_some_and(|t| t.elapsed() > SAVE_DEBOUNCE)
        {
            self.save();
        }
        show
    }

    /// Blocks until the tray asks for something. Used while the window is closed.
    pub fn wait_for_tray(&mut self) -> TrayCmd {
        let cmd = self.tray_rx.recv().unwrap_or(TrayCmd::Quit);
        match cmd {
            TrayCmd::Start => self.start(),
            TrayCmd::Stop => self.stop(),
            TrayCmd::Quit => self.quit_requested = true,
            TrayCmd::Show => {}
        }
        cmd
    }

    pub fn save(&mut self) {
        self.dirty_since = None;
        for row in &self.rows {
            self.settings
                .devices
                .insert(row.device.name.clone(), row.settings.clone());
        }
        self.settings.save();
    }

    pub fn shutdown(&mut self) {
        self.save();
        self.engine = None;
        self.tray = None;
    }

    fn mark_dirty(&mut self) {
        self.dirty_since.get_or_insert_with(Instant::now);
    }

    fn sync_tray(&self) {
        if let Some(tray) = &self.tray {
            tray.set_sharing(self.running());
        }
    }
}

fn spec(row: &Row) -> OutputSpec {
    OutputSpec {
        device: row.device.clone(),
        volume: row.settings.volume,
        delay_ms: row.settings.delay_ms,
    }
}
