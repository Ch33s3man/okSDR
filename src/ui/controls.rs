use egui::{Button, Color32, ComboBox, Rect, RichText, Stroke, Ui, Vec2};
use std::sync::{Arc, Mutex};

use crate::rig::RigClient;
use crate::state::{AgcSpeed, Band, RadioMode, RigStatus, SdrState, ACTIVITY_SPOTS};
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
    pub show_noise_dialog: bool,
    pub available_audio_inputs: Vec<String>,
    pub available_serial_ports: Vec<String>,
    pub devices_scanned: bool,
    /// Set when the user asks to open the radio connection screen.
    pub request_connection_dialog: bool,
    pub show_recording_deck: bool,
    pub recording_deck: RecordingDeck,
    peak_active_ratio: f32,
    last_peak_time: std::time::Instant,
    last_frame_time: std::time::Instant,
}

impl ControlPanel {
    pub fn new(recording_deck: RecordingDeck) -> Self {
        let mut panel = Self {
            s_meter: (0.0, 0.0, -120.0),
            show_config_dialog: false,
            show_noise_dialog: false,
            available_audio_inputs: Vec::new(),
            available_serial_ports: Vec::new(),
            devices_scanned: false,
            request_connection_dialog: false,
            show_recording_deck: false,
            recording_deck,
            peak_active_ratio: 0.0,
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

        let _avail_w = ui.available_width();
        let left_w = 465.0;

        ui.horizontal(|ui| {
            // =========================================================================
            // Left Column: VFO Readout, Bands, S-Meter
            // =========================================================================
            ui.vertical(|ui| {
                ui.set_width(left_w);

                // --- Row 1: VFO Readout, CAT Status & REC DECK ---
                ui.horizontal(|ui| {
                    ui.group(|ui| {
                        ui.vertical(|ui| {
                            ui.label(RichText::new("RADIO VFO / LO").size(10.5).color(Color32::from_gray(160)));
                            let lo_mhz = state.lo_freq as f64 / 1_000_000.0;
                            let vfo_lbl = ui.add(
                                egui::Label::new(
                                    RichText::new(format!("{:09.4} MHz", lo_mhz))
                                        .size(34.0)
                                        .monospace()
                                        .strong()
                                        .color(Color32::from_rgb(255, 90, 90)),
                                )
                                .sense(egui::Sense::hover()),
                            )
                            .on_hover_text("Scroll mouse wheel here to step frequency by 1 kHz");

                            let scroll = ui.input(|i| i.raw_scroll_delta.y);
                            if vfo_lbl.hovered() && scroll.abs() > 0.0 {
                                let step = if scroll > 0.0 { 1_000 } else { -1_000 };
                                let new_f = (state.lo_freq as i64 + step).max(100_000) as u64;
                                state.lo_freq = new_f;
                                state.tune_freq = new_f;
                                if let Some(rig) = rig_client {
                                    rig.set_freq(new_f);
                                }
                                state_changed = true;
                            }
                        });
                    });

                    // CAT connection indicator & Recording Deck toggle button
                    ui.vertical(|ui| {
                        ui.set_width(168.0);
                        ui.add_space(2.0);

                        ui.horizontal(|ui| {
                            let connecting = matches!(state.rig_status, RigStatus::Connecting(_));
                            let rig_col = if state.rigctld_connected {
                                Color32::from_rgb(40, 220, 80)
                            } else if connecting {
                                Color32::from_rgb(255, 190, 70)
                            } else {
                                Color32::from_rgb(220, 50, 50)
                            };
                            ui.label(RichText::new("●").size(13.0).color(rig_col));
                            ui.label(
                                RichText::new(if state.rigctld_connected {
                                    "G90 CAT Active"
                                } else if connecting {
                                    "Connecting..."
                                } else {
                                    "CAT Offline"
                                })
                                .size(11.5)
                                .strong()
                                .color(rig_col),
                            );

                            if !state.rigctld_connected {
                                if ui.button(RichText::new("⚙").size(10.0)).on_hover_text("Connect to Xiegu G90").clicked() {
                                    self.request_connection_dialog = true;
                                }
                            }
                        });

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
                            Color32::from_rgb(45, 50, 60)
                        };

                        let btn = Button::new(
                            RichText::new("🔴 REC DECK")
                                .size(12.5)
                                .strong()
                                .color(Color32::WHITE),
                        )
                        .min_size(Vec2::new(165.0, 30.0))
                        .fill(deck_btn_col);

                        if ui.add(btn).on_hover_text("Open/Close RX Audio Recording Deck").clicked() {
                            self.show_recording_deck = !self.show_recording_deck;
                        }
                    });
                });

                ui.add_space(3.0);

                // --- Row 2: HF Bands & Presets ---
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    ui.label(RichText::new("BAND:").size(11.0).strong());
                    let current_band = Band::from_freq(state.tune_freq);

                    for b in &Band::SSB_BANDS {
                        let is_active = current_band == Some(*b);
                        let calling_freq = b.ssb_calling_freq();
                        let btn = Button::new(RichText::new(b.name()).size(10.5).strong())
                            .min_size(Vec2::new(30.0, 20.0))
                            .fill(if is_active {
                                Color32::from_rgb(45, 95, 165)
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
                            if let Some(cb) = current_band {
                                state.band_memories.insert(cb, state.tune_freq);
                            }

                            let target_freq = calling_freq;
                            let target_mode = b.default_mode();

                            state.lo_freq = target_freq;
                            state.tune_freq = target_freq;
                            state.switch_mode(target_mode);
                            state.band_memories.insert(*b, target_freq);

                            if let Some(rig) = rig_client {
                                rig.set_freq(target_freq);
                                rig.set_mode(target_mode);
                            }
                            state_changed = true;
                        }
                    }

                    ComboBox::from_id_salt("activity_spots_combo")
                        .selected_text("Presets ▾")
                        .show_ui(ui, |ui| {
                            for spot in ACTIVITY_SPOTS {
                                let label = format!("{} ({:.3} MHz)", spot.name, spot.freq as f64 / 1e6);
                                if ui.button(label).clicked() {
                                    state.lo_freq = spot.freq;
                                    state.tune_freq = spot.freq;
                                    state.switch_mode(spot.mode);

                                    if let Some(rig) = rig_client {
                                        rig.set_freq(spot.freq);
                                        rig.set_mode(spot.mode);
                                    }
                                    state_changed = true;
                                }
                            }
                        });
                });

                ui.add_space(3.0);

                // --- Row 3: S-Meter Bar & Readout ---
                ui.horizontal(|ui| {
                    ui.label(RichText::new("S-METER:").size(11.0).strong());
                    let meter_w = (left_w - 185.0).max(180.0);
                    self.render_s_meter(ui, meter_w);

                    let (s_units, over_s9, dbfs) = self.s_meter;
                    let meter_text = if over_s9 > 0.0 {
                        format!("S9+{:02.0} ({:04.1} dB)", over_s9, dbfs)
                    } else {
                        format!("S{:02.1} ({:04.1} dB)", s_units, dbfs)
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

            // =========================================================================
            // Right Column: Mode, Grouped Filters, Gain, AGC & Robust Noise Controls
            // =========================================================================
            ui.vertical(|ui| {
                let right_w = ui.available_width();

                // --- Row 1: Mode Selection & Audio / RF Gain Sliders & Config ---
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    ui.label(RichText::new("MODE:").size(11.5).strong());

                    let mode_btn_w = (right_w * 0.045).clamp(42.0, 58.0);
                    for mode in &[
                        RadioMode::USB,
                        RadioMode::LSB,
                        RadioMode::CW,
                        RadioMode::AM,
                        RadioMode::FM,
                    ] {
                        let is_active = state.mode == *mode;
                        let btn = Button::new(RichText::new(mode.name()).size(11.5).strong())
                            .min_size(Vec2::new(mode_btn_w, 26.0))
                            .fill(if is_active {
                                Color32::from_rgb(185, 115, 20)
                            } else {
                                Color32::from_gray(40)
                            });

                        if ui.add(btn).clicked() {
                            state.switch_mode(*mode);

                            if let Some(rig) = rig_client {
                                rig.set_mode(*mode);
                            }
                            state_changed = true;
                        }
                    }

                    ui.add_space(6.0);

                    let cfg_btn_w = (right_w * 0.08).clamp(92.0, 130.0);
                    let rem_for_sliders = (ui.available_width() - cfg_btn_w - 170.0).max(170.0);
                    let slider_w = rem_for_sliders / 2.0;
                    ui.spacing_mut().slider_width = slider_w;

                    ui.label(RichText::new("VOL:").size(11.5).strong());
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

                    ui.label(RichText::new("RF:").size(11.5).strong());
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

                    let cfg_btn = Button::new(RichText::new("⚙ CONFIG").size(11.5).strong())
                        .min_size(Vec2::new(cfg_btn_w, 26.0))
                        .fill(Color32::from_rgb(45, 52, 65));
                    if ui.add(cfg_btn)
                        .on_hover_text("Open hardware, sound card, and I/Q calibration configuration")
                        .clicked()
                    {
                        self.show_config_dialog = !self.show_config_dialog;
                    }
                });

                ui.add_space(3.0);

                // --- Row 2: Grouped Filter Controls (Presets, Live BW Badge, Edge Sliders) ---
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    ui.label(RichText::new("FILTER:").size(11.5).strong());

                    let cur_bw = (state.filter_high - state.filter_low).abs();

                    let apply_bw = |bw: f32, state: &mut SdrState| {
                        match state.mode {
                            RadioMode::LSB => {
                                state.filter_low = -(bw + 100.0);
                                state.filter_high = -100.0;
                            }
                            RadioMode::USB => {
                                state.filter_low = 100.0;
                                state.filter_high = bw + 100.0;
                            }
                            RadioMode::CW => {
                                let half = bw * 0.5;
                                state.filter_low = (state.cw_pitch_hz - half).max(50.0).round();
                                state.filter_high = (state.cw_pitch_hz + half).round();
                            }
                            RadioMode::AM | RadioMode::FM => {
                                let half = bw * 0.5;
                                state.filter_low = -half;
                                state.filter_high = half;
                            }
                        }
                        state.set_filter_for_mode(state.mode, state.filter_low, state.filter_high);
                    };

                    let presets: &[(&str, f32)] = &[
                        ("500Hz", 500.0),
                        ("1.8k", 1800.0),
                        ("2.4k", 2400.0),
                        ("2.8k", 2800.0),
                        ("Wide", match state.mode {
                            RadioMode::AM => 9000.0,
                            RadioMode::FM => 15000.0,
                            _ => 3200.0,
                        }),
                    ];

                    let preset_btn_w = (right_w * 0.044).clamp(42.0, 64.0);
                    for &(name, target_bw) in presets {
                        let is_sel = (cur_bw - target_bw).abs() < 60.0;
                        let btn = Button::new(RichText::new(name).size(11.0).strong())
                            .min_size(Vec2::new(preset_btn_w, 26.0))
                            .fill(if is_sel {
                                Color32::from_rgb(45, 100, 160)
                            } else {
                                Color32::from_gray(40)
                            });

                        if ui.add(btn).clicked() {
                            apply_bw(target_bw, &mut state);
                            state_changed = true;
                        }
                    }

                    ui.label(
                        RichText::new(format!("BW: {:.0} Hz", cur_bw))
                            .size(11.5)
                            .strong()
                            .color(Color32::from_rgb(255, 220, 80)),
                    );

                    ui.add_space(2.0);

                    // Passband Edge Sliders & Reset Filter button
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

                    let reset_btn_w = (right_w * 0.08).clamp(92.0, 125.0);
                    // Remaining space in this row after FILTER, presets, and BW:
                    // Allocate remaining space between Left slider, Right slider, and Reset Filter button
                    let rem_for_slider_bars = (ui.available_width() - reset_btn_w - 230.0).max(40.0);
                    let edge_slider_w = (rem_for_slider_bars / 2.0).clamp(20.0, 260.0);
                    ui.spacing_mut().slider_width = edge_slider_w;

                    ui.label(RichText::new("Left:").size(11.0).strong());
                    let left_txt = format!("{:+.0} Hz", state.filter_low);
                    if scrollable_slider(
                        ui,
                        &mut state.filter_low,
                        low_min..=low_max,
                        50.0,
                        |s| s.text(left_txt).show_value(false),
                    )
                    .on_hover_text("Scroll or drag left passband cutoff (50 Hz step)")
                    .changed()
                    {
                        if state.filter_low > state.filter_high - 50.0 {
                            state.filter_high = state.filter_low + 50.0;
                        }
                        state.set_filter_for_mode(state.mode, state.filter_low, state.filter_high);
                        state_changed = true;
                    }

                    ui.label(RichText::new("Right:").size(11.0).strong());
                    let right_txt = format!("{:+.0} Hz", state.filter_high);
                    if scrollable_slider(
                        ui,
                        &mut state.filter_high,
                        high_min..=high_max,
                        50.0,
                        |s| s.text(right_txt).show_value(false),
                    )
                    .on_hover_text("Scroll or drag right passband cutoff (50 Hz step)")
                    .changed()
                    {
                        if state.filter_high < state.filter_low + 50.0 {
                            state.filter_low = state.filter_high - 50.0;
                        }
                        state.set_filter_for_mode(state.mode, state.filter_low, state.filter_high);
                        state_changed = true;
                    }

                    ui.add_space(2.0);

                    // Button to reset filters for active mode to standard defaults (aligned on the right of the row)
                    let (def_low, def_high) = state.mode.default_filter_edges_with_pitch(state.cw_pitch_hz);
                    let reset_btn = Button::new(RichText::new("Reset Filter").size(11.0).strong())
                        .min_size(Vec2::new(reset_btn_w, 26.0))
                        .fill(Color32::from_rgb(50, 58, 70));
                    if ui.add(reset_btn)
                        .on_hover_text(format!(
                            "Reset {} filter to default passband ({:+.0} to {:+.0} Hz, {:.0} Hz BW)",
                            state.mode.name(),
                            def_low,
                            def_high,
                            (def_high - def_low).abs()
                        ))
                        .clicked()
                    {
                        state.reset_filter_for_mode(state.mode);
                        state_changed = true;
                    }
                });

                ui.add_space(3.0);

                // --- Row 3: AGC & Robust Noise Controls Suite ---
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    ui.label(RichText::new("AGC:").size(11.5).strong());

                    let agc_btn_w = (right_w * 0.045).clamp(42.0, 68.0);
                    for spd in &[AgcSpeed::Off, AgcSpeed::Slow, AgcSpeed::Med, AgcSpeed::Fast] {
                        let name = match spd {
                            AgcSpeed::Off => "Off",
                            AgcSpeed::Slow => "Slow",
                            AgcSpeed::Med => "Med",
                            AgcSpeed::Fast => "Fast",
                        };
                        let is_active = state.agc_speed == *spd;
                        let btn = Button::new(RichText::new(name).size(11.0).strong())
                            .min_size(Vec2::new(agc_btn_w, 26.0))
                            .fill(if is_active {
                                Color32::from_rgb(50, 130, 75)
                            } else {
                                Color32::from_gray(40)
                            });

                        if ui.add(btn).clicked() {
                            state.agc_speed = *spd;
                            state_changed = true;
                        }
                    }

                    if state.agc_speed == AgcSpeed::Off {
                        ui.label(RichText::new("Gain:").size(11.0).strong());
                        ui.spacing_mut().slider_width = 85.0;
                        if scrollable_slider(
                            ui,
                            &mut state.agc_gain,
                            0.1..=10.0,
                            0.1,
                            |s| s.logarithmic(true),
                        )
                        .changed()
                        {
                            state_changed = true;
                        }
                    }

                    ui.add_space(6.0);

                    // Robust Noise Suite Buttons: [NR] [NB] [ANF] [SQL] [⚙ NOISE ▾]
                    ui.label(RichText::new("NOISE:").size(11.5).strong().color(Color32::from_rgb(130, 210, 255)));

                    let avail_noise = ui.available_width();
                    let unit_w = ((avail_noise - 18.0) / 5.0).clamp(55.0, 160.0);
                    let nr_w = (unit_w * 1.0).round();
                    let nb_w = (unit_w * 1.0).round();
                    let anf_w = (unit_w * 0.82).round();
                    let sql_w = (unit_w * 1.05).round();
                    let cfg_w = (unit_w * 1.13).round();

                    // 1. LMS NR
                    let nr_txt = if state.noise.nr_enabled {
                        format!("NR {:.0}%", state.noise.nr_level * 100.0)
                    } else if nr_w >= 75.0 {
                        "NR OFF".to_string()
                    } else {
                        "NR".to_string()
                    };
                    let nr_col = if state.noise.nr_enabled {
                        Color32::from_rgb(20, 120, 160)
                    } else {
                        Color32::from_gray(40)
                    };
                    let nr_btn = Button::new(
                        RichText::new(nr_txt)
                            .size(11.0)
                            .strong()
                            .color(if state.noise.nr_enabled { Color32::WHITE } else { Color32::from_gray(180) }),
                    )
                    .min_size(Vec2::new(nr_w, 26.0))
                    .fill(nr_col);

                    if ui.add(nr_btn)
                        .on_hover_text("Toggle Adaptive LMS Noise Reduction (Click '⚙ NOISE' for options)")
                        .clicked()
                    {
                        state.noise.nr_enabled = !state.noise.nr_enabled;
                        state_changed = true;
                    }

                    // 2. Impulse Noise Blanker
                    let nb_txt = if state.noise.nb_enabled {
                        format!("NB {:.1}x", state.noise.nb_threshold)
                    } else if nb_w >= 75.0 {
                        "NB OFF".to_string()
                    } else {
                        "NB".to_string()
                    };
                    let nb_col = if state.noise.nb_enabled {
                        Color32::from_rgb(170, 100, 20)
                    } else {
                        Color32::from_gray(40)
                    };
                    let nb_btn = Button::new(
                        RichText::new(nb_txt)
                            .size(11.0)
                            .strong()
                            .color(if state.noise.nb_enabled { Color32::WHITE } else { Color32::from_gray(180) }),
                    )
                    .min_size(Vec2::new(nb_w, 26.0))
                    .fill(nb_col);

                    if ui.add(nb_btn)
                        .on_hover_text("Toggle Impulse Noise Blanker (Click '⚙ NOISE' for options)")
                        .clicked()
                    {
                        state.noise.nb_enabled = !state.noise.nb_enabled;
                        state_changed = true;
                    }

                    // 3. Auto Notch Filter
                    let anf_txt = if state.noise.anf_enabled {
                        "ANF ON"
                    } else if anf_w >= 65.0 {
                        "ANF OFF"
                    } else {
                        "ANF"
                    };
                    let anf_col = if state.noise.anf_enabled {
                        Color32::from_rgb(30, 130, 70)
                    } else {
                        Color32::from_gray(40)
                    };
                    let anf_btn = Button::new(
                        RichText::new(anf_txt)
                            .size(11.0)
                            .strong()
                            .color(if state.noise.anf_enabled { Color32::WHITE } else { Color32::from_gray(180) }),
                    )
                    .min_size(Vec2::new(anf_w, 26.0))
                    .fill(anf_col);

                    if ui.add(anf_btn)
                        .on_hover_text("Toggle Auto Notch Filter (Automatic carrier whistle canceller)")
                        .clicked()
                    {
                        state.noise.anf_enabled = !state.noise.anf_enabled;
                        state_changed = true;
                    }

                    // 4. Squelch Gate
                    let sql_txt = if state.noise.squelch_enabled {
                        format!("SQL {:.0}dB", state.squelch_db)
                    } else if sql_w >= 75.0 {
                        "SQL OFF".to_string()
                    } else {
                        "SQL".to_string()
                    };
                    let sql_col = if state.noise.squelch_enabled {
                        Color32::from_rgb(120, 50, 150)
                    } else {
                        Color32::from_gray(40)
                    };
                    let sql_btn = Button::new(
                        RichText::new(sql_txt)
                            .size(11.0)
                            .strong()
                            .color(if state.noise.squelch_enabled { Color32::WHITE } else { Color32::from_gray(180) }),
                    )
                    .min_size(Vec2::new(sql_w, 26.0))
                    .fill(sql_col);

                    if ui.add(sql_btn)
                        .on_hover_text("Toggle Audio Squelch Gate (Click '⚙ NOISE' for threshold)")
                        .clicked()
                    {
                        state.noise.squelch_enabled = !state.noise.squelch_enabled;
                        state_changed = true;
                    }

                    // Dedicated Noise Configuration Menu Button
                    let noise_cfg_txt = if cfg_w >= 115.0 {
                        "⚙ NOISE MENU ▾"
                    } else {
                        "⚙ NOISE ▾"
                    };
                    let noise_cfg_btn = Button::new(
                        RichText::new(noise_cfg_txt)
                            .size(11.0)
                            .strong()
                            .color(Color32::WHITE),
                    )
                    .min_size(Vec2::new(cfg_w, 26.0))
                    .fill(if self.show_noise_dialog {
                        Color32::from_rgb(50, 75, 105)
                    } else {
                        Color32::from_gray(45)
                    });

                    if ui.add(noise_cfg_btn)
                        .on_hover_text("Open dedicated Noise Reduction, Blanker, and Squelch control menu")
                        .clicked()
                    {
                        self.show_noise_dialog = !self.show_noise_dialog;
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

        // --- Robust Noise Reduction & Interference Suppression Menu ---
        if self.show_noise_dialog {
            self.render_noise_dialog(ui, &mut state, &mut state_changed);
        }

        // --- CONFIG Hardware & Sound Card Options Window ---
        if self.show_config_dialog {
            self.render_config_dialog(ui, &mut state, &mut state_changed);
        }

        if state_changed {
            let mut cur = state_arc.lock().unwrap();
            let rig_status = cur.rig_status.clone();
            let rig_connected = cur.rigctld_connected;
            *cur = state;
            cur.rig_status = rig_status;
            cur.rigctld_connected = rig_connected;
        }
    }

    // =========================================================================
    // Floating Window: Robust Noise Reduction & Interference Suppression
    // =========================================================================
    fn render_noise_dialog(&mut self, ui: &mut Ui, state: &mut SdrState, state_changed: &mut bool) {
        egui::Window::new("🎧 Noise Reduction & DSP Interference Suppression")
            .collapsible(false)
            .resizable(false)
            .default_width(430.0)
            .show(ui.ctx(), |ui| {
                ui.label(RichText::new("HF Noise & QRM Processing Controls").strong().size(13.0));
                ui.add_space(4.0);

                // 1. LMS Adaptive Noise Reduction (NR)
                egui::Frame::group(ui.style())
                    .fill(Color32::from_rgb(18, 24, 32))
                    .stroke(Stroke::new(1.0f32, Color32::from_rgb(35, 55, 75)))
                    .inner_margin(egui::Margin::same(8.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let mut enabled = state.noise.nr_enabled;
                            if ui.checkbox(
                                &mut enabled,
                                RichText::new("NR: Adaptive LMS Noise Reduction")
                                    .strong()
                                    .color(Color32::from_rgb(80, 190, 240)),
                            ).changed() {
                                state.noise.nr_enabled = enabled;
                                *state_changed = true;
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(
                                    RichText::new(format!("{:.0}%", state.noise.nr_level * 100.0))
                                        .strong()
                                        .color(Color32::from_rgb(80, 190, 240)),
                                );
                            });
                        });

                        ui.label(
                            RichText::new("Attenuates background hiss/white noise while preserving speech clarity & CW tones.")
                                .size(10.5)
                                .color(Color32::from_gray(160)),
                        );

                        ui.horizontal(|ui| {
                            ui.label("Depth:");
                            let nr_txt = format!("{:.0}%", state.noise.nr_level * 100.0);
                            if scrollable_slider(
                                ui,
                                &mut state.noise.nr_level,
                                0.1..=1.0,
                                0.05,
                                |s| s.text(nr_txt).show_value(false),
                            ).changed() {
                                *state_changed = true;
                            }
                            for &(lbl, val) in &[("Mild", 0.35), ("Medium", 0.60), ("Aggressive", 0.85)] {
                                if ui.button(lbl).clicked() {
                                    state.noise.nr_level = val;
                                    state.noise.nr_enabled = true;
                                    *state_changed = true;
                                }
                            }
                        });
                    });

                ui.add_space(4.0);

                // 2. Impulse Noise Blanker (NB)
                egui::Frame::group(ui.style())
                    .fill(Color32::from_rgb(26, 22, 18))
                    .stroke(Stroke::new(1.0f32, Color32::from_rgb(65, 50, 30)))
                    .inner_margin(egui::Margin::same(8.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let mut enabled = state.noise.nb_enabled;
                            if ui.checkbox(
                                &mut enabled,
                                RichText::new("NB: Impulse Noise Blanker")
                                    .strong()
                                    .color(Color32::from_rgb(250, 180, 60)),
                            ).changed() {
                                state.noise.nb_enabled = enabled;
                                *state_changed = true;
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(
                                    RichText::new(format!("{:.1}x", state.noise.nb_threshold))
                                        .strong()
                                        .color(Color32::from_rgb(250, 180, 60)),
                                );
                            });
                        });

                        ui.label(
                            RichText::new("Blanks automotive ignition pulses, electric fence clicks, and powerline arcing before IF ringing.")
                                .size(10.5)
                                .color(Color32::from_gray(160)),
                        );

                        ui.horizontal(|ui| {
                            ui.label("Sensitivity:");
                            let nb_txt = format!("{:.1}x", state.noise.nb_threshold);
                            if scrollable_slider(
                                ui,
                                &mut state.noise.nb_threshold,
                                1.5..=8.0,
                                0.2,
                                |s| s.text(nb_txt).show_value(false),
                            ).changed() {
                                *state_changed = true;
                            }
                            for &(lbl, val) in &[("High (2.0x)", 2.0), ("Med (3.5x)", 3.5), ("Low (5.5x)", 5.5)] {
                                if ui.button(lbl).clicked() {
                                    state.noise.nb_threshold = val;
                                    state.noise.nb_enabled = true;
                                    *state_changed = true;
                                }
                            }
                        });
                    });

                ui.add_space(4.0);

                // 3. Automatic Notch Filter (ANF)
                egui::Frame::group(ui.style())
                    .fill(Color32::from_rgb(18, 26, 20))
                    .stroke(Stroke::new(1.0f32, Color32::from_rgb(35, 65, 45)))
                    .inner_margin(egui::Margin::same(8.0))
                    .show(ui, |ui| {
                        let mut enabled = state.noise.anf_enabled;
                        if ui.checkbox(
                            &mut enabled,
                            RichText::new("ANF: Automatic Notch Filter")
                                .strong()
                                .color(Color32::from_rgb(60, 220, 110)),
                        ).changed() {
                            state.noise.anf_enabled = enabled;
                            *state_changed = true;
                        }
                        ui.label(
                            RichText::new("Automatically tracks and cancels steady carrier heterodynes and annoying tuner whistles.")
                                .size(10.5)
                                .color(Color32::from_gray(160)),
                        );
                    });

                ui.add_space(4.0);

                // 4. Audio Squelch (SQL)
                egui::Frame::group(ui.style())
                    .fill(Color32::from_rgb(24, 18, 28))
                    .stroke(Stroke::new(1.0f32, Color32::from_rgb(60, 35, 75)))
                    .inner_margin(egui::Margin::same(8.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let mut enabled = state.noise.squelch_enabled;
                            if ui.checkbox(
                                &mut enabled,
                                RichText::new("SQL: Audio Squelch Gate")
                                    .strong()
                                    .color(Color32::from_rgb(210, 120, 240)),
                            ).changed() {
                                state.noise.squelch_enabled = enabled;
                                *state_changed = true;
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let cur_db = self.s_meter.2;
                                let status = if cur_db >= state.squelch_db { "GATE OPEN" } else { "MUTED" };
                                let col = if cur_db >= state.squelch_db { Color32::from_rgb(50, 220, 90) } else { Color32::from_gray(150) };
                                ui.label(RichText::new(format!("{} (Cur: {:.1} dB)", status, cur_db)).size(10.5).strong().color(col));
                            });
                        });

                        ui.label(
                            RichText::new("Smoothly mutes audio background static below set signal threshold without clicks.")
                                .size(10.5)
                                .color(Color32::from_gray(160)),
                        );

                        ui.horizontal(|ui| {
                            ui.label("Threshold:");
                            let sql_txt = format!("{:.0} dBFS", state.squelch_db);
                            if scrollable_slider(ui, &mut state.squelch_db, -130.0..=0.0, 1.0, |s| s.text(sql_txt)).changed() {
                                *state_changed = true;
                            }
                        });
                    });

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("Reset Noise Defaults").clicked() {
                        state.noise = crate::state::NoiseControls::default();
                        *state_changed = true;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(RichText::new("Close").strong()).clicked() {
                            self.show_noise_dialog = false;
                        }
                    });
                });
            });
    }

    // =========================================================================
    // Floating Window: Hardware & Sound Card Configuration
    // =========================================================================
    fn render_config_dialog(&mut self, ui: &mut Ui, state: &mut SdrState, state_changed: &mut bool) {
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
                                *state_changed = true;
                            }

                            for dev in &self.available_audio_inputs {
                                if dev != auto_label {
                                    if ui.selectable_label(state.audio_input_device == *dev, dev).clicked() {
                                        state.audio_input_device = dev.clone();
                                        *state_changed = true;
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

                // 2. Xiegu G90 CAT connection summary
                ui.label(RichText::new("Xiegu G90 CAT Connection:").strong());
                ui.label(
                    RichText::new(format!(
                        "{}  ·  {} baud  ·  CI-V 0x{:02X}  ·  Hamlib model {}  ·  DTR/RTS LOW",
                        state.serial_device, state.cat_baud, state.civ_address, state.hamlib_model
                    ))
                    .size(11.0)
                    .color(Color32::from_gray(180)),
                );
                let (status_txt, status_col) = match &state.rig_status {
                    RigStatus::Connected => ("Connected".to_string(), Color32::from_rgb(50, 220, 90)),
                    RigStatus::Connecting(s) => (s.clone(), Color32::from_rgb(255, 200, 50)),
                    RigStatus::Failed(s) | RigStatus::Lost(s) => (s.clone(), Color32::from_rgb(230, 70, 70)),
                    RigStatus::Idle => ("Not connected (SDR only)".to_string(), Color32::from_gray(160)),
                };
                ui.label(RichText::new(status_txt).size(11.5).strong().color(status_col));
                let conn_btn = Button::new(RichText::new("📻 Radio Connection…").strong())
                    .fill(Color32::from_rgb(30, 95, 160));
                if ui.add(conn_btn)
                    .on_hover_text("Review serial settings, test CAT without keying the transmitter, and connect")
                    .clicked()
                {
                    self.request_connection_dialog = true;
                }

                ui.add_space(6.0);
                ui.separator();

                // 3. I/Q Calibration Controls
                ui.label(RichText::new("I/Q Balance Calibration").strong().size(13.0));
                ui.add_space(2.0);

                if ui.checkbox(&mut state.iq_cal.dc_removal, "DC Offset Removal (Center Notch)").changed() {
                    *state_changed = true;
                }
                if ui.checkbox(&mut state.iq_cal.swap_iq, "Swap I / Q Channels (Invert Sideband)").changed() {
                    *state_changed = true;
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
                    *state_changed = true;
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
                    *state_changed = true;
                }

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.button("Reset I/Q Defaults").clicked() {
                        state.iq_cal.gain_balance = 0.0;
                        state.iq_cal.phase_balance = 0.0;
                        *state_changed = true;
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(RichText::new("Close").strong()).clicked() {
                            self.show_config_dialog = false;
                        }
                    });
                });
            });
    }

    // =========================================================================
    // Dynamic-Width S-Meter
    // =========================================================================
    fn render_s_meter(&mut self, ui: &mut Ui, target_width: f32) {
        let (s_units, over_s9, _dbfs) = self.s_meter;
        let meter_w = target_width.clamp(140.0, 550.0);
        let (rect, _) = ui.allocate_exact_size(Vec2::new(meter_w, 20.0), egui::Sense::hover());
        let painter = ui.painter_at(rect);

        // Meter background
        painter.rect_filled(rect, 3.0, Color32::from_rgb(18, 22, 28));

        // Scale: 0 to 9 S-units occupies 55% of width, +10 to +80 dB occupies remaining 45%
        let scale_w = (rect.width() - 14.0).max(100.0);
        let s_width = scale_w * 0.55;
        let over_width = scale_w * 0.45;

        let active_width = if s_units < 9.0 {
            (s_units / 9.0) * s_width
        } else {
            s_width + (over_s9 / 80.0) * over_width
        };

        let current_ratio = (active_width / scale_w).clamp(0.0, 1.0);

        // Update peak hold decay
        let now = std::time::Instant::now();
        let dt = now.duration_since(self.last_frame_time).as_secs_f32().min(0.1);
        self.last_frame_time = now;

        if current_ratio >= self.peak_active_ratio {
            self.peak_active_ratio = current_ratio;
            self.last_peak_time = now;
        } else if now.duration_since(self.last_peak_time) >= std::time::Duration::from_millis(700) {
            let decay_speed = 0.55; // Normalized decay speed per sec
            self.peak_active_ratio = (self.peak_active_ratio - decay_speed * dt).max(current_ratio);
        }

        // Draw meter bar fill
        let fill_w = active_width.min(scale_w);
        let fill_rect = Rect::from_min_size(rect.min, Vec2::new(fill_w, rect.height()));
        let bar_col = if over_s9 > 0.0 {
            Color32::from_rgb(240, 60, 60)
        } else if s_units >= 7.0 {
            Color32::from_rgb(240, 180, 40)
        } else {
            Color32::from_rgb(60, 210, 110)
        };
        painter.rect_filled(fill_rect, 3.0, bar_col);

        // Draw peak hold marker bar
        if self.peak_active_ratio > 0.01 {
            let peak_x = (rect.min.x + self.peak_active_ratio * scale_w).clamp(rect.min.x, rect.min.x + scale_w - 2.5);
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
                [egui::pos2(x, rect.min.y), egui::pos2(x, rect.min.y + 3.5)],
                Stroke::new(1.0f32, Color32::from_gray(140)),
            );
            painter.text(
                egui::pos2(x, rect.max.y - 2.0),
                egui::Align2::CENTER_BOTTOM,
                label,
                egui::FontId::monospace(8.0),
                Color32::from_gray(200),
            );
        }
    }
}
