//! Radio connection screen shown before the SDR becomes active (and whenever the CAT link
//! fails). Lets the user review/correct serial settings, run a read-only CI-V test that
//! cannot key the G90, and only then start Hamlib rigctld.

use egui::{Align, Align2, Button, Color32, ComboBox, Frame, Layout, Margin, RichText, Rounding, Stroke, Vec2};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::rig::rigctld::{self, HamlibInfo, RigConfig, HAMLIB_INSTALL_HINT};
use crate::rig::serial::{self, ProbeReport, G90_BAUD_RATES, G90_DEFAULT_BAUD, G90_DEFAULT_CIV};
use crate::rig::RigClient;
use crate::state::{RigStatus, SdrState, HAMLIB_MODEL_G90};

const ACCENT: Color32 = Color32::from_rgb(64, 156, 255);
const OK_GREEN: Color32 = Color32::from_rgb(60, 215, 120);
const WARN_AMBER: Color32 = Color32::from_rgb(255, 190, 70);
const ERR_RED: Color32 = Color32::from_rgb(240, 85, 85);
const CARD_BG: Color32 = Color32::from_rgb(24, 29, 38);
const PANEL_BG: Color32 = Color32::from_rgb(17, 21, 28);
const MUTED: Color32 = Color32::from_rgb(140, 150, 165);

#[derive(Clone, Copy, PartialEq)]
enum Severity {
    Info,
    Busy,
    Ok,
    Error,
}

#[derive(Clone, Copy, PartialEq)]
enum JobKind {
    Test,
    Scan,
    Connect,
    Release,
}

enum JobResult {
    Probe(JobKind, ProbeReport),
    Release(Result<String, String>),
}

pub struct ConnectionDialog {
    pub open: bool,
    /// True until the user connects or chooses SDR-only for the first time.
    pub startup_gate: bool,

    ports: Vec<String>,
    serial_device: String,
    baud: u32,
    civ_text: String,
    model: u32,
    auto_connect: bool,

    hamlib: Arc<Mutex<Option<HamlibInfo>>>,
    job_running: Arc<AtomicBool>,
    job_kind: Option<JobKind>,
    job_result: Arc<Mutex<Option<JobResult>>>,

    last_probe: Option<ProbeReport>,
    status: (Severity, String),
    waiting_for_worker: bool,
    pending_auto_connect: bool,
}

impl ConnectionDialog {
    pub fn new(state: &SdrState) -> Self {
        let hamlib = Arc::new(Mutex::new(None));
        let h = Arc::clone(&hamlib);
        std::thread::spawn(move || {
            *h.lock().unwrap() = Some(rigctld::detect_hamlib());
        });

        let mut dlg = Self {
            open: true,
            startup_gate: true,
            ports: Vec::new(),
            serial_device: state.serial_device.clone(),
            baud: state.cat_baud,
            civ_text: format!("{:02X}", state.civ_address),
            model: state.hamlib_model,
            auto_connect: state.cat_auto_connect,
            hamlib,
            job_running: Arc::new(AtomicBool::new(false)),
            job_kind: None,
            job_result: Arc::new(Mutex::new(None)),
            last_probe: None,
            status: (
                Severity::Info,
                "Review the settings below, then press Test CAT or Connect. \
                 Nothing is sent to the radio until you do."
                    .into(),
            ),
            waiting_for_worker: false,
            pending_auto_connect: state.cat_auto_connect,
        };
        dlg.refresh_ports();
        dlg
    }

    /// Re-open the dialog (e.g. from the status bar or CONFIG panel).
    pub fn reopen(&mut self, state: &SdrState) {
        self.open = true;
        self.serial_device = state.serial_device.clone();
        self.baud = state.cat_baud;
        self.civ_text = format!("{:02X}", state.civ_address);
        self.model = state.hamlib_model;
        self.auto_connect = state.cat_auto_connect;
        self.refresh_ports();
        if !self.job_running.load(Ordering::Relaxed) && !self.waiting_for_worker {
            self.status = match &state.rig_status {
                RigStatus::Connected => (Severity::Ok, "G90 CAT is connected.".into()),
                _ => (Severity::Info, "Review the radio connection settings.".into()),
            };
        }
    }

    /// Re-open the dialog because the CAT link failed.
    pub fn open_with_error(&mut self, state: &SdrState, msg: &str) {
        self.reopen(state);
        self.status = (Severity::Error, msg.to_string());
    }

    fn refresh_ports(&mut self) {
        self.ports = rigctld::list_available_serial_devices();
    }

    fn civ(&self) -> Option<u8> {
        let t = self.civ_text.trim().trim_start_matches("0x").trim_start_matches("0X");
        u8::from_str_radix(t, 16).ok()
    }

    fn resolved_device(&self) -> Option<String> {
        rigctld::resolve_serial_device(&self.serial_device)
    }

    fn apply_to_state(&self, state_arc: &Arc<Mutex<SdrState>>) {
        let mut st = state_arc.lock().unwrap();
        st.serial_device = self.serial_device.clone();
        st.cat_baud = self.baud;
        if let Some(c) = self.civ() {
            st.civ_address = c;
        }
        st.hamlib_model = self.model;
        st.cat_auto_connect = self.auto_connect;
    }

    fn start_job(&mut self, kind: JobKind, state_arc: &Arc<Mutex<SdrState>>, rig: Option<&RigClient>) {
        let Some(dev) = self.resolved_device() else {
            self.status = (
                Severity::Error,
                "No serial device found. Plug in the G90 CAT cable (DE-19 / USB) and press ↺.".into(),
            );
            return;
        };
        let Some(civ) = self.civ() else {
            self.status = (Severity::Error, format!("'{}' is not a valid hex CI-V address (G90 default: 70).", self.civ_text));
            return;
        };
        self.apply_to_state(state_arc);

        // If our rigctld currently owns the port, release it first so the probe gets the replies.
        let rig_active = {
            let st = state_arc.lock().unwrap();
            matches!(st.rig_status, RigStatus::Connected | RigStatus::Connecting(_))
        };
        if rig_active {
            if let Some(r) = rig {
                r.disconnect();
            }
        }

        let baud = self.baud;
        let result = Arc::clone(&self.job_result);
        let running = Arc::clone(&self.job_running);
        running.store(true, Ordering::Relaxed);
        self.job_kind = Some(kind);
        self.last_probe = None;
        self.status = (
            Severity::Busy,
            match kind {
                JobKind::Test => format!("Asking G90 for its frequency on {} @ {} baud (read-only)…", dev, baud),
                JobKind::Scan => format!("Scanning baud rates on {}…", dev),
                JobKind::Connect => format!("Checking G90 on {} before starting Hamlib…", dev),
                JobKind::Release => format!("Forcing DTR/RTS low on {}…", dev),
            },
        );

        std::thread::spawn(move || {
            if rig_active {
                std::thread::sleep(Duration::from_millis(1200));
            }
            let r = match kind {
                JobKind::Test | JobKind::Connect => JobResult::Probe(kind, serial::probe_g90(&dev, baud, civ)),
                JobKind::Scan => JobResult::Probe(kind, serial::scan_g90_baud(&dev, baud, civ)),
                JobKind::Release => JobResult::Release(serial::force_lines_low(&dev)),
            };
            *result.lock().unwrap() = Some(r);
            running.store(false, Ordering::Relaxed);
        });
    }

    fn poll_jobs(&mut self, state_arc: &Arc<Mutex<SdrState>>, rig: Option<&RigClient>) {
        let finished = self.job_result.lock().unwrap().take();
        if let Some(res) = finished {
            self.job_kind = None;
            match res {
                JobResult::Release(Ok(msg)) => self.status = (Severity::Ok, msg),
                JobResult::Release(Err(msg)) => self.status = (Severity::Error, msg),
                JobResult::Probe(kind, report) => {
                    if report.ok {
                        if report.baud != self.baud {
                            self.baud = report.baud;
                        }
                        self.apply_to_state(state_arc);
                        if kind == JobKind::Connect {
                            match RigConfig::from_state(&state_arc.lock().unwrap()) {
                                Ok(cfg) => {
                                    if let Some(r) = rig {
                                        r.connect(cfg);
                                        self.waiting_for_worker = true;
                                        self.status = (Severity::Busy, format!("{} — starting Hamlib rigctld…", report.message));
                                    }
                                }
                                Err(e) => self.status = (Severity::Error, e),
                            }
                        } else {
                            self.status = (Severity::Ok, format!("✔ {}", report.message));
                        }
                    } else {
                        self.status = (Severity::Error, report.message.clone());
                    }
                    self.last_probe = Some(report);
                }
            }
        }

        if self.waiting_for_worker {
            let st = state_arc.lock().unwrap().rig_status.clone();
            match st {
                RigStatus::Connected => {
                    self.waiting_for_worker = false;
                    self.status = (Severity::Ok, "✔ G90 CAT connected.".into());
                    self.open = false;
                    self.startup_gate = false;
                }
                RigStatus::Failed(msg) | RigStatus::Lost(msg) => {
                    self.waiting_for_worker = false;
                    self.status = (Severity::Error, format!("Hamlib connection failed:\n{}", msg));
                }
                RigStatus::Connecting(step) => self.status = (Severity::Busy, step),
                RigStatus::Idle => {}
            }
        }
    }

    pub fn busy(&self) -> bool {
        self.job_running.load(Ordering::Relaxed) || self.waiting_for_worker
    }

    /// Draw the full-window connection screen. Call instead of the SDR panels while `open`.
    pub fn show(&mut self, ctx: &egui::Context, state_arc: &Arc<Mutex<SdrState>>, rig: Option<&RigClient>) {
        if self.pending_auto_connect {
            self.pending_auto_connect = false;
            self.start_job(JobKind::Connect, state_arc, rig);
        }
        self.poll_jobs(state_arc, rig);
        if self.busy() {
            ctx.request_repaint_after(Duration::from_millis(50));
        }

        egui::CentralPanel::default()
            .frame(Frame::none().fill(PANEL_BG))
            .show(ctx, |ui| paint_backdrop(ui));

        egui::Window::new("radio_connection")
            .title_bar(false)
            .resizable(false)
            .collapsible(false)
            .movable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .fixed_size(Vec2::new(600.0, 0.0))
            .frame(
                Frame::none()
                    .fill(CARD_BG)
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(52, 64, 84)))
                    .rounding(Rounding::same(12.0))
                    .inner_margin(Margin::same(22.0))
                    .shadow(egui::epaint::Shadow {
                        offset: Vec2::new(0.0, 10.0),
                        blur: 30.0,
                        spread: 0.0,
                        color: Color32::from_black_alpha(140),
                    }),
            )
            .show(ctx, |ui| self.contents(ui, state_arc, rig));
    }

    fn contents(&mut self, ui: &mut egui::Ui, state_arc: &Arc<Mutex<SdrState>>, rig: Option<&RigClient>) {
        let busy = self.busy();
        ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);

        // ---- Header ----
        ui.horizontal(|ui| {
            ui.label(RichText::new("📻").size(30.0));
            ui.vertical(|ui| {
                ui.label(RichText::new("Xiegu G90 Radio Connection").size(21.0).strong().color(Color32::WHITE));
                ui.label(RichText::new("okSDR · CAT control via Hamlib rigctld").size(12.0).color(MUTED));
            });
        });
        ui.add_space(4.0);

        // ---- Hamlib status ----
        let hamlib = self.hamlib.lock().unwrap().clone();
        section(ui, "HAMLIB", |ui| match &hamlib {
            None => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new("Looking for Hamlib…").color(MUTED));
                });
            }
            Some(h) if h.rigctld_path.is_some() => {
                ui.label(
                    RichText::new(format!("✔ {}", h.version.clone().unwrap_or_else(|| "rigctld found".into())))
                        .color(OK_GREEN),
                );
            }
            Some(_) => {
                ui.label(RichText::new("✖ rigctld not found — Hamlib is not installed.").color(ERR_RED).strong());
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Install with:").color(MUTED));
                    ui.label(RichText::new(HAMLIB_INSTALL_HINT).monospace().color(Color32::WHITE));
                    if ui.small_button("📋 Copy").clicked() {
                        ui.output_mut(|o| o.copied_text = HAMLIB_INSTALL_HINT.to_string());
                    }
                });
                ui.label(
                    RichText::new("You can still use Test CAT (it talks to the radio directly).")
                        .size(11.0)
                        .color(MUTED),
                );
            }
        });

        // ---- Serial settings ----
        section(ui, "SERIAL PORT", |ui| {
            ui.add_enabled_ui(!busy, |ui| {
                egui::Grid::new("conn_grid").num_columns(2).spacing([14.0, 10.0]).show(ui, |ui| {
                    ui.label(RichText::new("Device").color(MUTED));
                    ui.horizontal(|ui| {
                        ComboBox::from_id_salt("conn_port")
                            .width(360.0)
                            .selected_text(truncate(&self.serial_device, 46))
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.serial_device, "Auto-Detect".to_string(), "Auto-Detect");
                                for p in self.ports.clone() {
                                    ui.selectable_value(&mut self.serial_device, p.clone(), p);
                                }
                            });
                        if ui.button("↺").on_hover_text("Rescan serial devices").clicked() {
                            self.refresh_ports();
                        }
                    });
                    ui.end_row();

                    ui.label(RichText::new("Custom path").color(MUTED));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.serial_device)
                            .desired_width(360.0)
                            .hint_text("/dev/ttyUSB0"),
                    );
                    ui.end_row();

                    ui.label("");
                    let resolved = self.resolved_device();
                    ui.label(
                        RichText::new(match &resolved {
                            Some(d) => format!("→ will use {}", d),
                            None => "→ no serial device detected".into(),
                        })
                        .size(11.0)
                        .color(if resolved.is_some() { MUTED } else { WARN_AMBER }),
                    );
                    ui.end_row();

                    ui.label(RichText::new("Baud rate").color(MUTED));
                    ui.horizontal(|ui| {
                        ComboBox::from_id_salt("conn_baud")
                            .width(110.0)
                            .selected_text(self.baud.to_string())
                            .show_ui(ui, |ui| {
                                for b in G90_BAUD_RATES {
                                    let label = if b == G90_DEFAULT_BAUD { format!("{} (default)", b) } else { b.to_string() };
                                    ui.selectable_value(&mut self.baud, b, label);
                                }
                            });
                        ui.label(RichText::new("CI-V address  0x").color(MUTED));
                        let civ_ok = self.civ().is_some();
                        ui.add(
                            egui::TextEdit::singleline(&mut self.civ_text)
                                .desired_width(34.0)
                                .char_limit(2)
                                .text_color(if civ_ok { Color32::WHITE } else { ERR_RED }),
                        )
                        .on_hover_text(format!("G90 default: {:02X}", G90_DEFAULT_CIV));
                    });
                    ui.end_row();

                    ui.label(RichText::new("Hamlib model").color(MUTED));
                    ui.horizontal(|ui| {
                        let mut models: Vec<(u32, String)> = hamlib.as_ref().map(|h| h.xiegu_models.clone()).unwrap_or_default();
                        if !models.iter().any(|(n, _)| *n == HAMLIB_MODEL_G90) {
                            models.insert(0, (HAMLIB_MODEL_G90, "Xiegu G90".into()));
                        }
                        let current_name = models
                            .iter()
                            .find(|(n, _)| *n == self.model)
                            .map(|(n, d)| format!("{} · {}", n, d))
                            .unwrap_or_else(|| format!("{} · (custom)", self.model));
                        ComboBox::from_id_salt("conn_model")
                            .width(220.0)
                            .selected_text(current_name)
                            .show_ui(ui, |ui| {
                                for (n, d) in &models {
                                    ui.selectable_value(&mut self.model, *n, format!("{} · {}", n, d));
                                }
                            });
                        ui.add(egui::DragValue::new(&mut self.model).range(1..=99999));
                        if self.model != HAMLIB_MODEL_G90 {
                            if ui.small_button("Use G90").clicked() {
                                self.model = HAMLIB_MODEL_G90;
                            }
                        }
                    });
                    ui.end_row();
                });
            });
        });

        // ---- Control lines (fixed for G90) ----
        section(ui, "PTT SAFETY  (fixed for the G90)", |ui| {
            ui.horizontal_wrapped(|ui| {
                pill(ui, "DTR  LOW 🔒", OK_GREEN);
                pill(ui, "RTS  LOW 🔒", OK_GREEN);
                pill(ui, "Flow control: none", ACCENT);
                pill(ui, "Hamlib PTT: disabled", ACCENT);
            });
            ui.label(
                RichText::new(
                    "The G90 transmits when DTR or RTS is high. okSDR forces both low the moment the \
                     port opens and passes dtr_state=OFF, rts_state=OFF, ptt_type=None to Hamlib.",
                )
                .size(11.0)
                .color(MUTED),
            );
        });

        // ---- Status ----
        let (sev, msg) = self.status.clone();
        let (col, icon) = match sev {
            Severity::Info => (ACCENT, "ℹ"),
            Severity::Busy => (WARN_AMBER, ""),
            Severity::Ok => (OK_GREEN, ""),
            Severity::Error => (ERR_RED, "⚠"),
        };
        Frame::none()
            .fill(col.linear_multiply(0.08))
            .stroke(Stroke::new(1.0_f32, col.linear_multiply(0.6)))
            .rounding(Rounding::same(8.0))
            .inner_margin(Margin::symmetric(12.0, 10.0))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    if sev == Severity::Busy {
                        ui.spinner();
                    } else if !icon.is_empty() {
                        ui.label(RichText::new(icon).size(16.0).color(col));
                    }
                    ui.add(egui::Label::new(RichText::new(&msg).color(col.gamma_multiply(1.1))).wrap());
                });
                if let Some(p) = &self.last_probe {
                    if let Some((dtr, rts)) = p.lines {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Measured lines:").size(11.0).color(MUTED));
                            line_badge(ui, "DTR", dtr);
                            line_badge(ui, "RTS", rts);
                            if let Some(a) = p.responder_civ {
                                ui.label(RichText::new(format!("· radio CI-V 0x{:02X}", a)).size(11.0).color(MUTED));
                            }
                        });
                    }
                }
            });

        ui.add_space(4.0);
        ui.checkbox(
            &mut self.auto_connect,
            RichText::new("Connect automatically at startup (this screen returns if the radio doesn't answer)")
                .size(12.0),
        );
        ui.add_space(6.0);

        // ---- Buttons ----
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!busy, Button::new("🔍 Test CAT").min_size(Vec2::new(0.0, 32.0)))
                .on_hover_text("Read-only: asks the G90 for its frequency directly over serial with DTR/RTS low. Does not use Hamlib.")
                .clicked()
            {
                self.start_job(JobKind::Test, state_arc, rig);
            }
            if ui
                .add_enabled(!busy, Button::new("⟳ Scan baud").min_size(Vec2::new(0.0, 32.0)))
                .on_hover_text("Try every G90 baud rate (one port open, read-only queries).")
                .clicked()
            {
                self.start_job(JobKind::Scan, state_arc, rig);
            }
            if ui
                .add_enabled(!busy, Button::new(RichText::new("⏹ Release PTT").color(WARN_AMBER)).min_size(Vec2::new(0.0, 32.0)))
                .on_hover_text("Emergency: open the port and force DTR + RTS low (use if the radio is stuck in TX).")
                .clicked()
            {
                self.start_job(JobKind::Release, state_arc, rig);
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let connect_btn = Button::new(RichText::new("Connect  ➜").strong().size(15.0).color(Color32::WHITE))
                    .fill(ACCENT.linear_multiply(0.85))
                    .min_size(Vec2::new(120.0, 34.0));
                if ui.add_enabled(!busy, connect_btn).clicked() {
                    self.start_job(JobKind::Connect, state_arc, rig);
                }

                let skip_label = if self.startup_gate { "Continue without CAT" } else { "Close" };
                if ui.add_enabled(!self.job_running.load(Ordering::Relaxed), Button::new(skip_label).min_size(Vec2::new(0.0, 34.0))).clicked() {
                    self.apply_to_state(state_arc);
                    if self.startup_gate || self.waiting_for_worker {
                        if let Some(r) = rig {
                            r.disconnect();
                        }
                    }
                    self.waiting_for_worker = false;
                    self.open = false;
                    self.startup_gate = false;
                }
            });
        });
    }
}

fn section(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.label(RichText::new(title).size(10.5).strong().color(ACCENT));
    Frame::none()
        .fill(Color32::from_rgb(30, 36, 47))
        .rounding(Rounding::same(8.0))
        .inner_margin(Margin::symmetric(12.0, 10.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui);
        });
    ui.add_space(2.0);
}

fn pill(ui: &mut egui::Ui, text: &str, col: Color32) {
    Frame::none()
        .fill(col.linear_multiply(0.12))
        .stroke(Stroke::new(1.0_f32, col.linear_multiply(0.7)))
        .rounding(Rounding::same(20.0))
        .inner_margin(Margin::symmetric(10.0, 3.0))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(11.5).strong().color(col));
        });
}

fn line_badge(ui: &mut egui::Ui, name: &str, high: bool) {
    let (txt, col) = if high { (format!("{} HIGH ⚠", name), ERR_RED) } else { (format!("{} LOW ✔", name), OK_GREEN) };
    ui.label(RichText::new(txt).size(11.0).strong().color(col));
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let tail: String = s.chars().rev().take(max - 1).collect::<Vec<_>>().into_iter().rev().collect();
        format!("…{}", tail)
    }
}

/// Subtle "spectrum" backdrop behind the dialog.
fn paint_backdrop(ui: &mut egui::Ui) {
    let rect = ui.max_rect();
    let painter = ui.painter();
    let mut mesh = egui::Mesh::default();
    let top = Color32::from_rgb(14, 18, 26);
    let bottom = Color32::from_rgb(10, 30, 52);
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.right_bottom(), bottom);
    mesh.colored_vertex(rect.left_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(mesh));

    let grid = Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(80, 140, 220, 14));
    let mut x = rect.left();
    while x < rect.right() {
        painter.line_segment([egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())], grid);
        x += 48.0;
    }
    let mut y = rect.top();
    while y < rect.bottom() {
        painter.line_segment([egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)], grid);
        y += 48.0;
    }

    // Faint spectrum trace along the bottom
    let t = ui.input(|i| i.time) as f32;
    let base = rect.bottom() - 70.0;
    let pts: Vec<egui::Pos2> = (0..=200)
        .map(|i| {
            let fx = i as f32 / 200.0;
            let x = rect.left() + fx * rect.width();
            let n = (fx * 37.0 + t * 0.7).sin() * 6.0 + (fx * 91.0 - t * 1.3).sin() * 4.0;
            let peak = (-((fx - 0.62) * 40.0).powi(2)).exp() * 45.0 + (-((fx - 0.3) * 70.0).powi(2)).exp() * 25.0;
            egui::pos2(x, base - n.abs() - peak)
        })
        .collect();
    painter.add(egui::Shape::line(pts, Stroke::new(1.5_f32, Color32::from_rgba_unmultiplied(64, 156, 255, 60))));
    ui.ctx().request_repaint_after(Duration::from_millis(33));
}
