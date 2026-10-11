use crossbeam_channel::Receiver;
use eframe::egui;
use std::sync::{Arc, Mutex};

use crate::audio::{AudioEngine, AudioMessage};
use crate::rig::RigClient;
use crate::state::{RadioMode, RigStatus, SdrState};
use crate::ui::af_display::AfDisplay;
use crate::ui::connection_dialog::ConnectionDialog;
use crate::ui::controls::ControlPanel;
use crate::ui::cw_display::CwDisplay;
use crate::ui::recording_deck::RecordingDeck;
use crate::ui::rf_display::RfDisplay;

pub struct OkSdrApp {
    state: Arc<Mutex<SdrState>>,
    audio_msg_rx: Receiver<AudioMessage>,
    _audio_engine: Option<AudioEngine>,
    rig_client: Option<RigClient>,

    rf_display: RfDisplay,
    af_display: AfDisplay,
    cw_display: CwDisplay,
    control_panel: ControlPanel,
    connection_dialog: ConnectionDialog,
    last_rig_status: RigStatus,

    last_mode: RadioMode,
    last_saved_state: SdrState,
    last_save_check: std::time::Instant,
}

impl OkSdrApp {
    pub fn new(
        state: Arc<Mutex<SdrState>>,
        audio_msg_rx: Receiver<AudioMessage>,
        audio_engine: Option<AudioEngine>,
        rig_client: Option<RigClient>,
        recording_deck: RecordingDeck,
    ) -> Self {
        let (wf_floor, wf_range, rf_span) = {
            let st = state.lock().unwrap();
            (st.rf_waterfall_floor_db, st.rf_waterfall_range_db, st.rf_display_span_hz)
        };
        let initial_saved = state.lock().unwrap().clone();
        let initial_mode = initial_saved.mode;

        Self {
            state,
            audio_msg_rx,
            _audio_engine: audio_engine,
            rig_client,
            rf_display: RfDisplay::new(wf_floor, wf_range, rf_span),
            af_display: AfDisplay::new(),
            cw_display: CwDisplay::new(),
            control_panel: ControlPanel::new(recording_deck),
            connection_dialog: ConnectionDialog::new(&initial_saved),
            last_rig_status: RigStatus::Idle,
            last_mode: initial_mode,
            last_saved_state: initial_saved,
            last_save_check: std::time::Instant::now(),
        }
    }

    fn autosave(&mut self) {
        // Periodic auto-save when any configuration / slider has changed (debounced to 1.0s)
        if self.last_save_check.elapsed() >= std::time::Duration::from_millis(1000) {
            self.last_save_check = std::time::Instant::now();
            let current = self.state.lock().unwrap().clone();
            if current != self.last_saved_state {
                current.save();
                self.last_saved_state = current;
            }
        }
    }
}

impl eframe::App for OkSdrApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let (current_mode, af_panel_h, show_af) = {
            let st = self.state.lock().unwrap();
            (
                st.mode,
                st.af_panel_height.clamp(80.0, 450.0),
                st.show_af_display,
            )
        };
        let is_cw = current_mode == RadioMode::CW;

        // Reset or clear CW display when transitioning modes
        if current_mode != self.last_mode {
            if is_cw {
                self.cw_display.clear();
            }
            self.last_mode = current_mode;
        }

        // Poll incoming audio/spectrum messages from audio capture thread
        while let Ok(msg) = self.audio_msg_rx.try_recv() {
            if let Some(rf_spec) = msg.rf_fft {
                self.rf_display.push_spectrum(rf_spec);
            }
            if let Some(af_spec) = msg.af_fft {
                self.af_display.push_spectrum(af_spec);
            }
            self.control_panel.s_meter = msg.s_meter;

            if is_cw {
                for ch in msg.cw_chars {
                    self.cw_display.push_char(ch);
                }
                self.cw_display.update_state(msg.cw_wpm, msg.cw_tone);
            }
        }

        // Continuous ~60 FPS repaint to keep waterfall, panadapter, and CW ticker flowing smoothly
        ctx.request_repaint_after(std::time::Duration::from_millis(16));

        // --- CAT link supervision: re-open the connection screen if a working link drops ---
        let rig_status = self.state.lock().unwrap().rig_status.clone();
        if rig_status != self.last_rig_status {
            if let RigStatus::Lost(msg) = &rig_status {
                if !self.connection_dialog.open {
                    let st = self.state.lock().unwrap().clone();
                    self.connection_dialog.open_with_error(&st, msg);
                }
            }
            self.last_rig_status = rig_status;
        }
        if self.control_panel.request_connection_dialog {
            self.control_panel.request_connection_dialog = false;
            self.control_panel.show_config_dialog = false;
            let st = self.state.lock().unwrap().clone();
            self.connection_dialog.reopen(&st);
        }

        // --- Radio connection screen: the SDR UI stays inactive until the user connects or skips ---
        if self.connection_dialog.open {
            // Keep an in-progress recording serviced even while the SDR panels are hidden
            self.control_panel.recording_deck.update_recorder();
            self.connection_dialog.show(ctx, &self.state, self.rig_client.as_ref());
            self.autosave();
            return;
        }

        // Top Control Panel (fits controls snugly, expands when recording deck is open)
        egui::TopBottomPanel::top("controls_panel")
            .resizable(false)
            .show(ctx, |ui| {
                self.control_panel.show(ui, &self.state, &self.rig_client);
            });

        // Bottom AF (Audio) Spectrum & Waterfall Display (or compact CW bar if hidden)
        let mut toggle_af = false;
        if show_af {
            let af_inner = egui::TopBottomPanel::bottom("af_panel")
                .resizable(true)
                .default_height(af_panel_h)
                .min_height(80.0)
                .max_height(450.0)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        if ui
                            .button(egui::RichText::new("▼ Hide AF").size(10.0))
                            .on_hover_text("Hide AF spectrum and waterfall")
                            .clicked()
                        {
                            toggle_af = true;
                        }
                        ui.label(
                            egui::RichText::new("AF AUDIO SPECTRUM & WATERFALL (0 - 4 kHz)")
                                .size(11.0)
                                .strong(),
                        );
                        if is_cw {
                            self.cw_display.show(ui);
                        }
                    });
                    self.af_display.show(ui, &self.state);
                });

            let new_af_h = af_inner.response.rect.height();
            if (new_af_h - af_panel_h).abs() > 1.0 {
                let mut st = self.state.lock().unwrap();
                st.af_panel_height = new_af_h;
            }
        } else {
            egui::TopBottomPanel::bottom("af_panel_collapsed")
                .resizable(false)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        if ui
                            .button(egui::RichText::new("▲ Show AF").size(10.0))
                            .on_hover_text("Show AF spectrum and waterfall")
                            .clicked()
                        {
                            toggle_af = true;
                        }
                        if is_cw {
                            self.cw_display.show(ui);
                        }
                    });
                });
        }

        if toggle_af {
            let mut st = self.state.lock().unwrap();
            st.show_af_display = !st.show_af_display;
        }

        // Central Panel: RF Panadapter & Waterfall Display
        egui::CentralPanel::default().show(ctx, |ui| {
            let state_clone = Arc::clone(&self.state);
            let state_for_lo = Arc::clone(&self.state);
            let rig_ref_lo = self.rig_client.as_ref();

            self.rf_display.show(
                ui,
                &self.state,
                // On tune changed
                move |new_tune| {
                    let mut st = state_clone.lock().unwrap();
                    st.tune_freq = new_tune;
                },
                // On LO changed
                move |new_lo| {
                    let mut st = state_for_lo.lock().unwrap();
                    st.lo_freq = new_lo;
                    st.tune_freq = new_lo;

                    if let Some(rig) = rig_ref_lo {
                        rig.set_freq(new_lo);
                    }
                },
            );
        });

        self.autosave();
    }

    fn save(&mut self, _storage: &mut dyn eframe::Storage) {
        let current = self.state.lock().unwrap().clone();
        if current != self.last_saved_state {
            current.save();
            self.last_saved_state = current;
        }
    }
}

impl Drop for OkSdrApp {
    fn drop(&mut self) {
        let current = self.state.lock().unwrap().clone();
        if current != self.last_saved_state {
            current.save();
        }
    }
}
