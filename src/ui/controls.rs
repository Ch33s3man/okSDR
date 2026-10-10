use egui::{Button, Color32, ComboBox, Rect, RichText, Stroke, Ui, Vec2};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::rig::RigClient;
use crate::state::{AgcSpeed, Band, RadioMode, SdrState, ACTIVITY_SPOTS};
use crate::ui::recording_deck::RecordingDeck;
use crate::ui::scrollable_slider;

fn truncate_str(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let mut truncated: String = s.chars().take(max_chars.saturating_sub(3)).collect();
        truncated.push_str("...");
        truncated
    }
}

pub struct ControlPanel {
    pub s_meter: (f32, f32, f32), // (s_units, over_s9_db, dbfs)
    pub show_config_dialog: bool,
    pub available_audio_inputs: Vec<String>,
    pub available_serial_ports: Vec<String>,
    pub devices_scanned: bool,
    pub cat_test_status: Arc<Mutex<Option<(bool, String)>>>,
    pub cat_testing: Arc<AtomicBool>,
    pub custom_serial_input: String,
    pub show_recording_deck: bool,
    pub recording_deck: RecordingDeck,
    peak_active_width: f32,
    last_peak_time: std::time::Instant,
    last_frame_time: std::time::Instant,
}

impl ControlPanel {
    pub fn new(recording_deck: RecordingDeck) -> Self {
        let mut panel = Self {
            s_meter: (0.0, 0.0, -120.0),
            show_config_dialog: false,
            available_audio_inputs: Vec::new(),
            available_serial_ports: Vec::new(),
            devices_scanned: false,
            cat_test_status: Arc::new(Mutex::new(None)),
            cat_testing: Arc::new(AtomicBool::new(false)),
            custom_serial_input: String::new(),
            show_recording_deck: false,
            recording_deck,
            peak_active_width: 0.0,
            last_peak_time: std::time::Instant::now(),
            last_frame_time: std::time::Instant::now(),
        };
        panel.refresh_devices();
        panel
    }

    pub fn refresh_devices(&mut self) {
        self.available_audio_inputs = crate::audio::AudioEngine::list_available_input_devices();
        self.available_serial_ports = crate::rig::rigctld::list_available_serial_devices();
        self.devices_scanned = true;
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        state_arc: &Arc<Mutex<SdrState>>,
        rig_client: &Option<RigClient>,
    ) {
        // Keep recording buffer serviced continuously even when recording deck drawer is closed
        self.recording_deck.update_recorder();

        let mut state = state_arc.lock().unwrap().clone();
        let mut state_changed = false;

        let avail_w = ui.available_width();
        let left_w = (avail_w * 0.44).clamp(460.0, 520.0);

        ui.horizontal(|ui| {
            // Left column: Frequencies, Band Buttons & S-Meter
            ui.vertical(|ui| {
                ui.set_width(left_w);

                // --- 1. Top Bar: Frequency Display & Rig Status ---
                ui.horizontal(|ui| {
                    // LO Frequency (Radio VFO) - enlarged by 60% (35.0 size)
                    ui.group(|ui| {
                        ui.vertical(|ui| {
                            ui.label(RichText::new("RADIO VFO / LO").size(11.0).color(Color32::from_gray(160)));
                            let lo_mhz = state.lo_freq as f64 / 1_000_000.0;
                            ui.label(
                                RichText::new(format!("{:09.4} MHz", lo_mhz))
                                    .size(35.0)
                                    .monospace()
                                    .strong()
                                    .color(Color32::from_rgb(255, 90, 90)),
                            );
                        });
                    });

                    // Hamlib Connection Indicator & Recording Deck Toggle Button
                    ui.vertical(|ui| {
                        ui.set_width(190.0);
                        ui.add_space(3.0);

                        // Center the CAT status horizontally above the record deck button
                        ui.allocate_ui_with_layout(
                            Vec2::new(190.0, 20.0),
                            egui::Layout::left_to_right(egui::Align::Center).with_main_align(egui::Align::Center),
                            |ui| {
                                let rig_col = if state.rigctld_connected {
                                    Color32::from_rgb(40, 220, 80)
                                } else {
                                    Color32::from_rgb(220, 50, 50)
                                };
                                ui.label(RichText::new("●").size(15.0).color(rig_col));
                                ui.label(
                                    RichText::new(if state.rigctld_connected {
                                        "G90 CAT Active"
                                    } else {
                                        "G90 CAT Offline"
                                    })
                                    .size(12.0)
                                    .color(rig_col),
                                );

                                if !state.rigctld_connected {
                                    if ui.button(RichText::new("↺ Re-scan").size(10.0))
                                        .on_hover_text("Search USB ports (/dev/ttyUSB*, /dev/ttyACM*) and re-establish Xiegu G90 CAT connection")
                                        .clicked()
                                    {
                                        if let Some(rig) = rig_client {
                                            rig.reconnect();
                                        }
                                    }
                                }
                            },
                        );

                        ui.add_space(4.0);

                        let is_rec = self.recording_deck.is_recording.load(std::sync::atomic::Ordering::Relaxed);
                        let is_play = self.recording_deck.is_playing.load(std::sync::atomic::Ordering::Relaxed);
                        let deck_btn_col = if is_rec {
                            Color32::from_rgb(180, 40, 40)
                        } else if is_play {
                            Color32::from_rgb(40, 140, 80)
                        } else if self.show_recording_deck {
                            Color32::from_rgb(45, 95, 160)
                        } else {
                            Color32::from_rgb(50, 55, 65)
                        };

                        // Center the button and center label in the middle of the button with record circle icon and REC DECK text
                        ui.allocate_ui_with_layout(
                            Vec2::new(190.0, 38.0),
                            egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                            |ui| {
                                let btn = Button::new(
                                    RichText::new("🔴 REC DECK")
                                        .size(13.0)
                                        .strong()
                                        .color(Color32::WHITE),
                                )
                                .fill(deck_btn_col);

                                if ui.add(btn)
                                    .on_hover_text("Open/Close RX Audio Recording Deck (Waveform, Playback, Scrub, Save WAV)")
                                    .clicked()
                                {
                                    self.show_recording_deck = !self.show_recording_deck;
                                }
                            },
                        );
                    });
                });

                // --- 2. HF Band Quick Switcher Row ---
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.5;
                    ui.label(RichText::new("BAND:").size(11.0).strong());
                    let current_band = Band::from_freq(state.tune_freq);

                    for b in &Band::SSB_BANDS {
                        let is_active = current_band == Some(*b);
                        let calling_freq = b.ssb_calling_freq();
                        let btn = Button::new(RichText::new(b.name()).size(10.5).strong())
                            .fill(if is_active {
                                Color32::from_rgb(45, 90, 160)
                            } else {
                                Color32::from_gray(40)
                            });

                        let hover_info = format!(
                            "Tune to {} SSB Calling: {:.4} MHz ({})",
                            b.name(),
                            calling_freq as f64 / 1_000_000.0,
                            b.default_mode().name()
                        );

                        if ui.add(btn).on_hover_text(hover_info).clicked() {
                            // Remember current frequency before switching
                            if let Some(cb) = current_band {
                                state.band_memories.insert(cb, state.tune_freq);
                            }

                            // Directly tune to widely agreed upon SSB calling frequency for that band
                            let target_freq = calling_freq;
                            let target_mode = b.default_mode();

                            state.lo_freq = target_freq;
                            state.tune_freq = target_freq;
                            state.mode = target_mode;
                            let (f_low, f_high) = target_mode.default_filter_edges();
                            state.filter_low = f_low;
                            state.filter_high = f_high;
                            state.band_memories.insert(*b, target_freq);

                            if let Some(rig) = rig_client {
                                rig.set_freq(target_freq);
                                rig.set_mode(target_mode);
                            }
                            state_changed = true;
                        }
                    }

                    // Activity presets dropdown
                    ComboBox::from_id_salt("activity_spots")
                        .selected_text("Presets ▾")
                        .show_ui(ui, |ui| {
                            for spot in ACTIVITY_SPOTS {
                                let label = format!("{} ({:.3} MHz)", spot.name, spot.freq as f64 / 1e6);
                                if ui.button(label).clicked() {
                                    state.lo_freq = spot.freq;
                                    state.tune_freq = spot.freq;
                                    state.mode = spot.mode;
                                    let (f_low, f_high) = spot.mode.default_filter_edges();
                                    state.filter_low = f_low;
                                    state.filter_high = f_high;

                                    if let Some(rig) = rig_client {
                                        rig.set_freq(spot.freq);
                                        rig.set_mode(spot.mode);
                                    }
                                    state_changed = true;
                                }
                            }
                        });
                });

                // --- 3. S-Meter Bar ---
                ui.horizontal(|ui| {
                    ui.label(RichText::new("S-METER:").size(11.0).strong());
                    self.render_s_meter(ui);
                    let (s_units, over_s9, dbfs) = self.s_meter;
                    let meter_text = if over_s9 > 0.0 {
                        format!("S9+{:02.0} dB ({:04.1} dBFS)", over_s9, dbfs)
                    } else {
                        format!("S{:02.1} ({:04.1} dBFS)", s_units, dbfs)
                    };
                    let text_col = if over_s9 > 0.0 {
                        Color32::from_rgb(255, 120, 120)
                    } else if s_units >= 7.0 {
                        Color32::from_rgb(255, 210, 100)
                    } else {
                        Color32::from_rgb(150, 220, 180)
                    };
                    ui.label(
                        RichText::new(meter_text)
                            .monospace()
                            .size(10.5)
                            .strong()
                            .color(text_col),
                    );
                });
            });

            ui.separator();

            // Right column: Mode, Filter, AGC, Volume, IQ Calib
            ui.vertical(|ui| {
                // --- 4. Mode Selection ---
                ui.horizontal(|ui| {
                    ui.label(RichText::new("MODE:").size(11.0).strong());
                    for mode in &[
                        RadioMode::USB,
                        RadioMode::LSB,
                        RadioMode::CW,
                        RadioMode::AM,
                        RadioMode::FM,
                    ] {
                        let is_active = state.mode == *mode;
                        let btn = Button::new(RichText::new(mode.name()).size(11.0).strong())
                            .fill(if is_active {
                                Color32::from_rgb(180, 110, 20)
                            } else {
                                Color32::from_gray(40)
                            });

                        if ui.add(btn).clicked() {
                            state.mode = *mode;
                            if *mode == RadioMode::CW {
                                state.filter_low = (state.cw_pitch_hz - 250.0).max(50.0);
                                state.filter_high = state.cw_pitch_hz + 250.0;
                            } else {
                                let (f_low, f_high) = mode.default_filter_edges();
                                state.filter_low = f_low;
                                state.filter_high = f_high;
                            }

                            if let Some(rig) = rig_client {
                                rig.set_mode(*mode);
                            }
                            state_changed = true;
                        }
                    }
                });

                // CW Pitch & Waterfall Alignment Offset (only in CW mode)
                if state.mode == RadioMode::CW {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("CW PITCH:").size(11.0).strong().color(Color32::from_rgb(255, 200, 50)));

                        for &p in &[600.0, 700.0, 800.0] {
                            let is_sel = (state.cw_pitch_hz - p).abs() < 5.0;
                            let btn = Button::new(RichText::new(format!("{:.0}Hz", p)).size(10.0))
                                .fill(if is_sel {
                                    Color32::from_rgb(180, 110, 20)
                                } else {
                                    Color32::from_gray(40)
                                });
                            if ui.add(btn).clicked() {
                                let cur_bw = (state.filter_high - state.filter_low).abs().max(100.0);
                                state.cw_pitch_hz = p;
                                state.filter_low = (p - cur_bw * 0.5).max(50.0).round();
                                state.filter_high = (p + cur_bw * 0.5).round();
                                state_changed = true;
                            }
                        }

                        let pitch_txt = format!("{:.0} Hz", state.cw_pitch_hz);
                        if scrollable_slider(
                            ui,
                            &mut state.cw_pitch_hz,
                            400.0..=1000.0,
                            10.0,
                            |s| s.text(pitch_txt).show_value(false),
                        )
                        .on_hover_text("CW sidetone pitch & waterfall offset (matches Xiegu G90 CW pitch setting)")
                        .changed()
                        {
                            let cur_bw = (state.filter_high - state.filter_low).abs().max(100.0);
                            state.filter_low = (state.cw_pitch_hz - cur_bw * 0.5).max(50.0).round();
                            state.filter_high = (state.cw_pitch_hz + cur_bw * 0.5).round();
                            state_changed = true;
                        }
                    });
                }

                // --- 5. Filter Controls ---
                ui.horizontal(|ui| {
                    ui.label(RichText::new("FILTER:").size(11.0).strong());

                    if ui.button("2.4k").clicked() {
                        match state.mode {
                            RadioMode::LSB => {
                                state.filter_low = -2400.0;
                                state.filter_high = -100.0;
                            }
                            RadioMode::USB => {
                                state.filter_low = 100.0;
                                state.filter_high = 2400.0;
                            }
                            RadioMode::CW => {
                                state.filter_low = (state.cw_pitch_hz - 1200.0).max(50.0);
                                state.filter_high = state.cw_pitch_hz + 1200.0;
                            }
                            RadioMode::AM | RadioMode::FM => {
                                state.filter_low = -1200.0;
                                state.filter_high = 1200.0;
                            }
                        }
                        state_changed = true;
                    }
                    if ui.button("1.8k").clicked() {
                        match state.mode {
                            RadioMode::LSB => {
                                state.filter_low = -1800.0;
                                state.filter_high = -100.0;
                            }
                            RadioMode::USB => {
                                state.filter_low = 100.0;
                                state.filter_high = 1800.0;
                            }
                            RadioMode::CW => {
                                state.filter_low = (state.cw_pitch_hz - 900.0).max(50.0);
                                state.filter_high = state.cw_pitch_hz + 900.0;
                            }
                            RadioMode::AM | RadioMode::FM => {
                                state.filter_low = -900.0;
                                state.filter_high = 900.0;
                            }
                        }
                        state_changed = true;
                    }
                    if ui.button("500Hz").clicked() {
                        match state.mode {
                            RadioMode::LSB => {
                                state.filter_low = -600.0;
                                state.filter_high = -100.0;
                            }
                            RadioMode::USB => {
                                state.filter_low = 100.0;
                                state.filter_high = 600.0;
                            }
                            RadioMode::CW => {
                                state.filter_low = (state.cw_pitch_hz - 250.0).max(50.0);
                                state.filter_high = state.cw_pitch_hz + 250.0;
                            }
                            RadioMode::AM | RadioMode::FM => {
                                state.filter_low = -250.0;
                                state.filter_high = 250.0;
                            }
                        }
                        state_changed = true;
                    }
                    if ui.button("Wide").clicked() {
                        match state.mode {
                            RadioMode::AM => {
                                state.filter_low = -4500.0;
                                state.filter_high = 4500.0;
                            }
                            RadioMode::FM => {
                                state.filter_low = -7500.0;
                                state.filter_high = 7500.0;
                            }
                            RadioMode::LSB => {
                                state.filter_low = -3000.0;
                                state.filter_high = -100.0;
                            }
                            RadioMode::USB => {
                                state.filter_low = 100.0;
                                state.filter_high = 3000.0;
                            }
                            RadioMode::CW => {
                                state.filter_low = (state.cw_pitch_hz - 600.0).max(50.0);
                                state.filter_high = state.cw_pitch_hz + 600.0;
                            }
                        }
                        state_changed = true;
                    }

                    let cur_bw = (state.filter_high - state.filter_low).abs();
                    ui.label(
                        RichText::new(format!("BW: {:.0} Hz", cur_bw))
                            .strong()
                            .color(Color32::from_rgb(255, 220, 80)),
                    );
                });

                // Dual sliders for independent Left and Right filter passband edges
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("EDGES:").size(11.0).strong());

                    let (def_low_min, def_low_max, def_high_min, def_high_max): (f32, f32, f32, f32) = match state.mode {
                        RadioMode::USB => (0.0, 3000.0, 200.0, 6000.0),
                        RadioMode::LSB => (-6000.0, -200.0, -3000.0, 0.0),
                        RadioMode::CW => (0.0, 2000.0, 100.0, 3000.0),
                        RadioMode::AM => (-12000.0, -200.0, 200.0, 12000.0),
                        RadioMode::FM => (-18000.0, -1000.0, 1000.0, 18000.0),
                    };
                    let low_min = def_low_min.min(state.filter_low);
                    let low_max = def_low_max.max(state.filter_low);
                    let high_min = def_high_min.min(state.filter_high);
                    let high_max = def_high_max.max(state.filter_high);

                    let edge_slider_w = ((ui.available_width() - 150.0) / 2.0).clamp(65.0, 110.0);
                    ui.spacing_mut().slider_width = edge_slider_w;

                    ui.label("Left:");
                    let left_txt = format!("{:+.0} Hz", state.filter_low);
                    if scrollable_slider(
                        ui,
                        &mut state.filter_low,
                        low_min..=low_max,
                        50.0,
                        |s| s.text(left_txt).show_value(false),
                    )
                    .on_hover_text("Scroll or drag to expand/contract filter left edge (50 Hz step)")
                    .changed()
                    {
                        if state.filter_low > state.filter_high - 50.0 {
                            state.filter_high = state.filter_low + 50.0;
                        }
                        state_changed = true;
                    }

                    ui.label("Right:");
                    let right_txt = format!("{:+.0} Hz", state.filter_high);
                    if scrollable_slider(
                        ui,
                        &mut state.filter_high,
                        high_min..=high_max,
                        50.0,
                        |s| s.text(right_txt).show_value(false),
                    )
                    .on_hover_text("Scroll or drag to expand/contract filter right edge (50 Hz step)")
                    .changed()
                    {
                        if state.filter_high < state.filter_low + 50.0 {
                            state.filter_low = state.filter_high - 50.0;
                        }
                        state_changed = true;
                    }
                });

                // --- 6. AGC & Gain ---
                ui.horizontal(|ui| {
                    ui.label(RichText::new("AGC:").size(11.0).strong());
                    for spd in &[AgcSpeed::Off, AgcSpeed::Slow, AgcSpeed::Med, AgcSpeed::Fast] {
                        let name = match spd {
                            AgcSpeed::Off => "Off",
                            AgcSpeed::Slow => "Slow",
                            AgcSpeed::Med => "Med",
                            AgcSpeed::Fast => "Fast",
                        };
                        let is_active = state.agc_speed == *spd;
                        let btn = Button::new(RichText::new(name).size(10.0))
                            .fill(if is_active {
                                Color32::from_rgb(60, 130, 80)
                            } else {
                                Color32::from_gray(40)
                            });
                        if ui.add(btn).clicked() {
                            state.agc_speed = *spd;
                            state_changed = true;
                        }
                    }

                    if state.agc_speed == AgcSpeed::Off {
                        ui.label("Gain:");
                        if scrollable_slider(
                            ui,
                            &mut state.agc_gain,
                            0.1..=10.0,
                            0.1,
                            |s| s.logarithmic(true),
                        )
                        .on_hover_text("Scroll or drag AGC manual gain")
                        .changed()
                        {
                            state_changed = true;
                        }
                    }
                });

                // --- 7. Audio & RF Gain Sliders ---
                ui.horizontal_wrapped(|ui| {
                    let audio_slider_w = ((ui.available_width() - 200.0) / 2.0).clamp(65.0, 115.0);
                    ui.spacing_mut().slider_width = audio_slider_w;

                    ui.label(RichText::new("VOL:").size(11.0).strong());
                    let vol_pct = (state.volume * 100.0).round() as u32;
                    if scrollable_slider(
                        ui,
                        &mut state.volume,
                        0.0..=1.0,
                        0.02,
                        |s| s.text(format!("{}%", vol_pct)).show_value(false),
                    )
                    .on_hover_text("Scroll or drag volume (2% step)")
                    .changed()
                    {
                        state_changed = true;
                    }

                    ui.label(RichText::new("RF GAIN:").size(11.0).strong());
                    let rf_text = format!("{:+0.0} dB", state.rf_preamp_db);
                    if scrollable_slider(
                        ui,
                        &mut state.rf_preamp_db,
                        0.0..=36.0,
                        1.0,
                        |s| s.text(rf_text).show_value(false),
                    )
                    .on_hover_text("Digital RF Preamp boost for Xiegu G90 I/Q Line-In (1 dB step)")
                    .changed()
                    {
                        state_changed = true;
                    }

                    // CONFIG Options popover toggle (50% larger button)
                    let cfg_btn = Button::new(RichText::new("⚙ CONFIG").size(14.0).strong())
                        .min_size(Vec2::new(95.0, 30.0));
                    if ui.add(cfg_btn).clicked() {
                        self.show_config_dialog = !self.show_config_dialog;
                        if self.show_config_dialog {
                            if self.custom_serial_input.is_empty() {
                                self.custom_serial_input = state.serial_device.clone();
                            }
                        }
                    }
                });
            });
        });

        // --- RX Audio Recording Deck Drawer ---
        if self.show_recording_deck {
            ui.add_space(4.0);
            ui.separator();
            self.recording_deck.show(ui, &mut state, &mut state_changed);
        }

        // --- CONFIG Options Popup Window ---
        if self.show_config_dialog {
            egui::Window::new("CONFIG Options")
                .collapsible(false)
                .resizable(false)
                .default_width(450.0)
                .show(ui.ctx(), |ui| {
                    ui.label(RichText::new("Hardware & Sound Card Configuration").strong().size(13.0));
                    ui.add_space(3.0);

                    // 1. Audio Input Device (I/Q Line-In)
                    ui.label(RichText::new("I/Q Audio Line-In Device:").strong());
                    ui.horizontal(|ui| {
                        let current_audio = if state.audio_input_device.is_empty() {
                            "Auto (Cubilux / Default)".to_string()
                        } else {
                            state.audio_input_device.clone()
                        };

                        ComboBox::from_id_salt("audio_input_select")
                            .width(340.0)
                            .selected_text(truncate_str(&current_audio, 42))
                            .show_ui(ui, |ui| {
                                let auto_label = "Auto (Cubilux / Default)";
                                if ui.selectable_label(state.audio_input_device == auto_label || state.audio_input_device.is_empty(), auto_label).clicked() {
                                    state.audio_input_device = auto_label.to_string();
                                    state_changed = true;
                                }

                                for dev in &self.available_audio_inputs {
                                    if dev != auto_label {
                                        if ui.selectable_label(state.audio_input_device == *dev, dev).clicked() {
                                            state.audio_input_device = dev.clone();
                                            state_changed = true;
                                        }
                                    }
                                }
                            });

                        if ui.button("↺").on_hover_text("Refresh sound card input list").clicked() {
                            self.refresh_devices();
                        }
                    });
                    ui.label(
                        RichText::new("Note: Selecting a different sound card takes effect on next app restart.")
                            .size(10.5)
                            .color(Color32::from_gray(160)),
                    );

                    ui.add_space(6.0);
                    ui.separator();

                    // 2. Xiegu G90 CAT Serial Port
                    ui.label(RichText::new("Xiegu G90 CAT Serial Device:").strong());
                    ui.horizontal(|ui| {
                        let current_serial = if state.serial_device.is_empty() {
                            "Auto-Detect".to_string()
                        } else {
                            state.serial_device.clone()
                        };

                        ComboBox::from_id_salt("serial_device_select")
                            .width(230.0)
                            .selected_text(truncate_str(&current_serial, 28))
                            .show_ui(ui, |ui| {
                                let auto_serial = "Auto-Detect";
                                if ui.selectable_label(state.serial_device == auto_serial || state.serial_device.is_empty(), auto_serial).clicked() {
                                    state.serial_device = auto_serial.to_string();
                                    self.custom_serial_input = auto_serial.to_string();
                                    state_changed = true;
                                }

                                for port in &self.available_serial_ports {
                                    if port != auto_serial {
                                        if ui.selectable_label(state.serial_device == *port, port).clicked() {
                                            state.serial_device = port.clone();
                                            self.custom_serial_input = port.clone();
                                            state_changed = true;
                                        }
                                    }
                                }
                            });

                        // Test CAT button
                        let is_testing = self.cat_testing.load(Ordering::Relaxed);
                        let test_btn = Button::new(RichText::new("Test CAT").strong())
                            .fill(Color32::from_rgb(30, 95, 160));
                        if ui.add_enabled(!is_testing, test_btn)
                            .on_hover_text("Probe Xiegu G90 CAT status without keying transmitter")
                            .clicked()
                        {
                            self.cat_testing.store(true, Ordering::Relaxed);
                            let status_arc = Arc::clone(&self.cat_test_status);
                            let testing_arc = Arc::clone(&self.cat_testing);
                            let port_to_test = state.serial_device.clone();
                            std::thread::spawn(move || {
                                let res = crate::rig::rigctld::test_cat_connection(&port_to_test);
                                *status_arc.lock().unwrap() = Some(res);
                                testing_arc.store(false, Ordering::Relaxed);
                            });
                        }

                        if ui.button("↺").on_hover_text("Refresh serial device list").clicked() {
                            self.refresh_devices();
                        }
                    });

                    // Custom serial device text entry
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Custom Port:").size(11.0));
                        let resp = ui.add(
                            egui::TextEdit::singleline(&mut self.custom_serial_input)
                                .desired_width(220.0)
                                .hint_text("/dev/ttyUSB0"),
                        );
                        if resp.lost_focus() && !self.custom_serial_input.is_empty() && self.custom_serial_input != state.serial_device {
                            state.serial_device = self.custom_serial_input.clone();
                            state_changed = true;
                        }
                    });

                    // Live CAT Test Feedback
                    if self.cat_testing.load(Ordering::Relaxed) {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(RichText::new("Probing Xiegu G90 CAT communication...").color(Color32::from_rgb(255, 200, 50)));
                        });
                    } else if let Some((ok, ref msg)) = *self.cat_test_status.lock().unwrap() {
                        let col = if ok {
                            Color32::from_rgb(50, 220, 90)
                        } else {
                            Color32::from_rgb(230, 70, 70)
                        };
                        ui.label(RichText::new(msg).size(11.5).strong().color(col));
                    }

                    ui.add_space(6.0);
                    ui.separator();

                    // 3. I/Q Calibration & Squelch Controls
                    ui.label(RichText::new("I/Q Balance & Squelch Calibration").strong().size(13.0));
                    ui.add_space(2.0);

                    if ui.checkbox(&mut state.iq_cal.dc_removal, "DC Offset Removal (Center Notch)").changed() {
                        state_changed = true;
                    }
                    if ui.checkbox(&mut state.iq_cal.swap_iq, "Swap I / Q Channels (Invert Sideband)").changed() {
                        state_changed = true;
                    }

                    ui.separator();
                    ui.label("Squelch Threshold (SQL):");
                    let sql_text = if state.squelch_db <= -125.0 {
                        "Off".to_string()
                    } else {
                        format!("{:.0} dB", state.squelch_db)
                    };
                    if scrollable_slider(
                        ui,
                        &mut state.squelch_db,
                        -130.0..=0.0,
                        1.0,
                        |s| s.text(sql_text),
                    )
                    .on_hover_text("Scroll or drag squelch threshold (-130 to 0 dBFS)")
                    .changed()
                    {
                        state_changed = true;
                    }

                    ui.separator();
                    ui.label("Amplitude Gain Balance:");
                    if scrollable_slider(
                        ui,
                        &mut state.iq_cal.gain_balance,
                        -0.3..=0.3,
                        0.01,
                        |s| s,
                    )
                    .changed()
                    {
                        state_changed = true;
                    }

                    ui.label("Phase Balance (radians):");
                    if scrollable_slider(
                        ui,
                        &mut state.iq_cal.phase_balance,
                        -0.3..=0.3,
                        0.01,
                        |s| s,
                    )
                    .changed()
                    {
                        state_changed = true;
                    }

                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if ui.button("Reset DSP Defaults").clicked() {
                            state.iq_cal.gain_balance = 0.0;
                            state.iq_cal.phase_balance = 0.0;
                            state.squelch_db = -130.0;
                            state_changed = true;
                        }

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button(RichText::new("Close").strong()).clicked() {
                                self.show_config_dialog = false;
                            }
                        });
                    });
                });
        }

        if state_changed {
            *state_arc.lock().unwrap() = state;
        }
    }

    fn render_s_meter(&mut self, ui: &mut Ui) {
        let (s_units, over_s9, _dbfs) = self.s_meter;
        // Widen S-meter graph to 280.0 px so all ticks including +80 are clearly displayed
        let (rect, _) = ui.allocate_exact_size(Vec2::new(280.0, 22.0), egui::Sense::hover());
        let painter = ui.painter_at(rect);

        // Meter background
        painter.rect_filled(rect, 3.0, Color32::from_rgb(18, 22, 28));

        // Scale: 0 to 9 S-units occupies 55% of width, +10 to +80 dB occupies remaining 45%
        // Inset scale by 16px on the right so +80 dB text label is never cut off by edge of bar
        let scale_w = rect.width() - 16.0;
        let s_width = scale_w * 0.55;
        let over_width = scale_w * 0.45;

        let active_width = if s_units < 9.0 {
            (s_units / 9.0) * s_width
        } else {
            s_width + (over_s9 / 80.0) * over_width
        };

        // Update peak hold decay
        let now = std::time::Instant::now();
        let dt = now.duration_since(self.last_frame_time).as_secs_f32().min(0.1);
        self.last_frame_time = now;

        if active_width >= self.peak_active_width {
            self.peak_active_width = active_width;
            self.last_peak_time = now;
        } else if now.duration_since(self.last_peak_time) >= std::time::Duration::from_millis(700) {
            let decay_speed = 160.0; // Decay smoothly towards current level
            self.peak_active_width = (self.peak_active_width - decay_speed * dt).max(active_width);
        }

        // Draw meter bar fill
        let fill_rect = Rect::from_min_size(rect.min, Vec2::new(active_width.min(scale_w), rect.height()));
        let bar_col = if over_s9 > 0.0 {
            Color32::from_rgb(240, 60, 60)
        } else if s_units >= 7.0 {
            Color32::from_rgb(240, 180, 40)
        } else {
            Color32::from_rgb(60, 210, 110)
        };
        painter.rect_filled(fill_rect, 3.0, bar_col);

        // Draw peak hold marker bar
        if self.peak_active_width > 2.0 {
            let peak_x = (rect.min.x + self.peak_active_width).clamp(rect.min.x, rect.min.x + scale_w - 2.5);
            let peak_rect = Rect::from_min_size(
                egui::pos2(peak_x, rect.min.y),
                Vec2::new(2.5, rect.height()),
            );
            painter.rect_filled(peak_rect, 1.0, Color32::from_rgb(255, 255, 210));
        }

        // Tick markers (S1, S3, S5, S7, S9, +20, +40, +60, +80)
        let ticks: &[(f32, &str)] = &[
            (1.0 / 9.0 * s_width, "1"),
            (3.0 / 9.0 * s_width, "3"),
            (5.0 / 9.0 * s_width, "5"),
            (7.0 / 9.0 * s_width, "7"),
            (s_width, "9"),
            (s_width + 20.0 / 80.0 * over_width, "+20"),
            (s_width + 40.0 / 80.0 * over_width, "+40"),
            (s_width + 60.0 / 80.0 * over_width, "+60"),
            (s_width + over_width, "+80"),
        ];

        for &(x_off, label) in ticks {
            let x = rect.min.x + x_off;
            painter.line_segment(
                [egui::pos2(x, rect.min.y), egui::pos2(x, rect.min.y + 4.0)],
                Stroke::new(1.0f32, Color32::from_gray(140)),
            );
            painter.text(
                egui::pos2(x, rect.max.y - 3.0),
                egui::Align2::CENTER_BOTTOM,
                label,
                egui::FontId::monospace(8.0),
                Color32::from_gray(200),
            );
        }
    }
}
