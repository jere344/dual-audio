//! The main window.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::egui::{
    self, Align, Color32, CornerRadius, FontId, Frame, Layout, Margin, RichText, Sense, Stroke,
    Vec2,
};

use crate::APP_NAME;
use crate::app::{Controller, RowState, Tone};
use crate::engine::{MAX_DELAY_MS, MAX_VOLUME};

const BG: Color32 = Color32::from_rgb(0x0f, 0x11, 0x17);
const CARD: Color32 = Color32::from_rgb(0x18, 0x1b, 0x24);
const CARD_ALT: Color32 = Color32::from_rgb(0x1f, 0x23, 0x30);
const TRACK: Color32 = Color32::from_rgb(0x3a, 0x3f, 0x4f);
const CARD_HOVER: Color32 = Color32::from_rgb(0x2a, 0x2f, 0x3d);
const ACCENT: Color32 = Color32::from_rgb(0x4f, 0x46, 0xe5);
const ACCENT_HOVER: Color32 = Color32::from_rgb(0x63, 0x66, 0xf1);
const TEXT: Color32 = Color32::from_rgb(0xe7, 0xe9, 0xee);
const MUTED: Color32 = Color32::from_rgb(0x8b, 0x93, 0xa7);
const GOOD: Color32 = Color32::from_rgb(0x22, 0xc5, 0x5e);
const BAD: Color32 = Color32::from_rgb(0xef, 0x44, 0x44);
const BAD_HOVER: Color32 = Color32::from_rgb(0xdc, 0x26, 0x26);

pub type ContextSlot = Arc<Mutex<Option<egui::Context>>>;

pub struct MainWindow<'a> {
    ctl: &'a mut Controller,
}

impl<'a> MainWindow<'a> {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        ctl: &'a mut Controller,
        slot: &ContextSlot,
    ) -> Self {
        apply_style(&cc.egui_ctx);
        *slot.lock().unwrap() = Some(cc.egui_ctx.clone());
        Self { ctl }
    }
}

fn tone_color(tone: Tone) -> Color32 {
    match tone {
        Tone::Muted => MUTED,
        Tone::Good => GOOD,
        Tone::Bad => BAD,
    }
}

fn apply_style(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = BG;
    visuals.window_fill = CARD;
    visuals.override_text_color = Some(TEXT);
    visuals.selection.bg_fill = ACCENT;
    visuals.selection.stroke = Stroke::new(1.0, TEXT);
    visuals.slider_trailing_fill = true;
    let w = &mut visuals.widgets;
    for (state, fill) in [
        (&mut w.inactive, CARD_ALT),
        (&mut w.hovered, CARD_HOVER),
        (&mut w.active, ACCENT),
        (&mut w.open, CARD_ALT),
    ] {
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        state.corner_radius = CornerRadius::same(8);
        state.bg_stroke = Stroke::NONE;
        state.fg_stroke = Stroke::new(1.5, TEXT);
    }
    w.noninteractive.bg_stroke = Stroke::new(1.0, CARD_ALT);
    ctx.set_visuals(visuals);
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = Vec2::new(8.0, 6.0);
        s.spacing.button_padding = Vec2::new(12.0, 5.0);
        s.spacing.interact_size.y = 24.0;
    });
}

fn card() -> Frame {
    Frame::new()
        .fill(CARD)
        .corner_radius(14)
        .inner_margin(Margin::same(16))
}

fn section_title(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(11.0).strong().color(MUTED));
}

fn level_bar(ui: &mut egui::Ui, level: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(150.0, 8.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 4.0, CARD_ALT);
    let mut fill = rect;
    fill.set_width((rect.width() * level.clamp(0.0, 1.0)).max(rect.height()));
    painter.rect_filled(fill, 4.0, GOOD);
}

/// A value label of fixed width followed by a slider filling the rest of the line.
fn labeled_slider(ui: &mut egui::Ui, value: String, slider: egui::Slider<'_>) -> egui::Response {
    ui.horizontal(|ui| {
        ui.add_sized(
            [56.0, 18.0],
            egui::Label::new(RichText::new(value).color(TEXT)),
        );
        ui.spacing_mut().slider_width = (ui.available_width() - 12.0).max(60.0);
        ui.add(slider.show_value(false))
    })
    .inner
}

impl eframe::App for MainWindow<'_> {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.ctl.tick() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
        if self.ctl.quit_requested {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        let period = if self.ctl.running() { 80 } else { 400 };
        ctx.request_repaint_after(Duration::from_millis(period));
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::bottom("footer")
            .frame(Frame::new().fill(BG).inner_margin(Margin {
                left: 22,
                right: 22,
                top: 6,
                bottom: 18,
            }))
            .show_separator_line(false)
            .show(ui, |ui| self.footer(ui));
        egui::CentralPanel::no_frame()
            .frame(Frame::new().fill(BG).inner_margin(Margin {
                left: 22,
                right: 22,
                top: 20,
                bottom: 6,
            }))
            .show(ui, |ui| {
                self.header(ui);
                ui.add_space(10.0);
                self.source_card(ui);
                ui.add_space(10.0);
                self.devices_card(ui);
            });
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        BG.to_normalized_gamma_f32()
    }
}

impl MainWindow<'_> {
    fn header(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new(APP_NAME).size(28.0).strong().color(TEXT));
        ui.label(
            RichText::new(
                "Play your PC's audio through several headsets or speakers at the same time.",
            )
            .size(13.0)
            .color(MUTED),
        );
    }

    fn source_card(&mut self, ui: &mut egui::Ui) {
        card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            let peak = self.ctl.peak();
            ui.horizontal(|ui| {
                section_title(ui, "SYSTEM AUDIO SOURCE");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let (text, color) = if peak > 1e-4 {
                        (format!("input: {:.0} dB", 20.0 * peak.log10()), GOOD)
                    } else {
                        ("input: -inf dB".to_owned(), MUTED)
                    };
                    ui.label(RichText::new(text).size(11.0).color(color));
                });
            });

            ui.horizontal(|ui| {
                let selected = match &self.ctl.source {
                    None => "System default".to_owned(),
                    Some(id) => self
                        .ctl
                        .devices
                        .outputs
                        .iter()
                        .find(|d| &d.id == id)
                        .map_or_else(|| "System default".to_owned(), |d| d.name.clone()),
                };
                let mut choice = self.ctl.source.clone();
                egui::ComboBox::from_id_salt("source")
                    .selected_text(selected)
                    .width(380.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut choice, None, "System default");
                        for d in &self.ctl.devices.outputs {
                            ui.selectable_value(&mut choice, Some(d.id.clone()), &d.name);
                        }
                    });
                if choice != self.ctl.source {
                    self.ctl.set_source(choice);
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| level_bar(ui, peak));
            });

            let capturing = match self.ctl.capture_device() {
                Some(d) => format!("Capturing: {}", d.name),
                None => "Capturing: system default".to_owned(),
            };
            ui.label(RichText::new(capturing).size(12.0).color(MUTED));
            ui.label(
                RichText::new(
                    "Everything playing on this device is mirrored to the outputs you enable below.",
                )
                .size(11.0)
                .color(MUTED),
            );
        });
    }

    fn devices_card(&mut self, ui: &mut egui::Ui) {
        card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_min_height(ui.available_height());
            ui.horizontal(|ui| {
                section_title(ui, "OUTPUT DEVICES");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button(RichText::new("Refresh").size(12.0)).clicked() {
                        self.ctl.refresh_devices();
                    }
                });
            });
            ui.add_space(4.0);

            let source_id = self.ctl.capture_device().map(|d| d.id.clone());
            let has_targets = self
                .ctl
                .rows
                .iter()
                .any(|r| Some(&r.device.id) != source_id.as_ref());
            if !has_targets {
                ui.add_space(12.0);
                ui.label(
                    RichText::new(
                        "No additional output devices found.\nConnect a headset or speaker, then press Refresh.",
                    )
                    .size(13.0)
                    .color(MUTED),
                );
            }

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for i in 0..self.ctl.rows.len() {
                        self.device_row(ui, i);
                        ui.add_space(6.0);
                    }
                });
        });
    }

    fn device_row(&mut self, ui: &mut egui::Ui, i: usize) {
        let state = self.ctl.row_state(i);
        let is_source = matches!(state, RowState::Source);
        let row = &self.ctl.rows[i];
        let name = row.device.name.clone();
        let mut enabled = row.settings.enabled;
        let mut volume = row.settings.volume * 100.0;
        let mut delay = row.settings.delay_ms;

        Frame::new()
            .fill(CARD_ALT)
            .corner_radius(12)
            .inner_margin(Margin::symmetric(14, 10))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let label = RichText::new(&name).size(14.0).strong();
                    ui.visuals_mut().widgets.inactive.bg_stroke = Stroke::new(1.5, MUTED);
                    let check =
                        ui.add_enabled(!is_source, egui::Checkbox::new(&mut enabled, label));
                    if check.changed() {
                        self.ctl.set_enabled(i, enabled);
                    }
                    if is_source {
                        check.on_disabled_hover_text(
                            "This device is the capture source; it already plays the audio.",
                        );
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let test = egui::Button::new(RichText::new("Test").size(11.0)).fill(CARD);
                        if ui.add(test).clicked() {
                            self.ctl.test(i);
                        }
                        let (text, color, tip) = match &state {
                            RowState::Idle => ("", MUTED, None),
                            RowState::Source => ("source", MUTED, None),
                            RowState::Testing => ("testing...", MUTED, None),
                            RowState::Starting => ("starting", MUTED, None),
                            RowState::Live => ("live", GOOD, None),
                            RowState::Error(e) => ("error", BAD, Some(e.clone())),
                        };
                        let resp = ui.label(RichText::new(text).size(12.0).color(color));
                        if let Some(tip) = tip {
                            resp.on_hover_text(tip);
                        }
                    });
                });

                ui.visuals_mut().widgets.inactive.bg_fill = TRACK;
                ui.columns(2, |cols| {
                    cols[0].label(RichText::new("Volume").size(12.0).color(MUTED));
                    let vol = labeled_slider(
                        &mut cols[0],
                        format!("{volume:.0}%"),
                        egui::Slider::new(&mut volume, 0.0..=MAX_VOLUME * 100.0).step_by(1.0),
                    );
                    if vol.changed() {
                        self.ctl.set_volume(i, volume / 100.0);
                    }

                    cols[1].label(RichText::new("Sync delay").size(12.0).color(MUTED));
                    let del = labeled_slider(
                        &mut cols[1],
                        format!("{delay} ms"),
                        egui::Slider::new(&mut delay, 0..=MAX_DELAY_MS).step_by(10.0),
                    )
                    .on_hover_text(
                        "Delay this device to line it up with a slower one (e.g. Bluetooth).",
                    );
                    if del.changed() {
                        self.ctl.set_delay(i, delay);
                    }
                });
            });
    }

    fn footer(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_centered(|ui| {
            let status = &self.ctl.status;
            ui.label(
                RichText::new(&status.text)
                    .size(13.0)
                    .color(tone_color(status.tone)),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let running = self.ctl.running();
                let (text, fill, hover) = if running {
                    ("Stop sharing", BAD, BAD_HOVER)
                } else {
                    ("Start sharing", ACCENT, ACCENT_HOVER)
                };
                let button = egui::Button::new(
                    RichText::new(text)
                        .font(FontId::proportional(15.0))
                        .strong()
                        .color(Color32::WHITE),
                )
                .min_size(Vec2::new(170.0, 46.0))
                .corner_radius(12);
                let resp = ui.scope(|ui| {
                    let w = &mut ui.visuals_mut().widgets;
                    w.inactive.weak_bg_fill = fill;
                    w.hovered.weak_bg_fill = hover;
                    w.active.weak_bg_fill = hover;
                    ui.add(button)
                });
                if resp.inner.clicked() {
                    self.ctl.toggle();
                }
            });
        });
    }
}
