use eframe::egui::{self, Button, Color32, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2};
use crossbeam_channel::{Receiver, Sender};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use crate::state::{Band, SdrState};

pub struct RecordingDeck {
    pub is_recording: Arc<AtomicBool>,
    pub record_rx: Receiver<f32>,
    pub playback_tx: Sender<f32>,

    // Recorded samples buffer (48 kHz mono PCM)
    pub samples: Arc<RwLock<Vec<f32>>>,
    pub sample_rate: u32,

    // Playback state
    pub is_playing: Arc<AtomicBool>,
    pub play_pos: Arc<AtomicUsize>,
    pub loop_playback: Arc<AtomicBool>,
    pub playback_volume: f32,

    feeder_running: Arc<AtomicBool>,

    // Metadata for current session
    pub record_start_time: Option<String>,
    pub record_band: String,
    pub record_mode: String,
    pub record_freq: u64,

    // Feedback message
    pub status_msg: Option<(String, Instant)>,

    // Recent recordings cache
    pub recent_recordings: Vec<PathBuf>,
    pub last_scan_time: Instant,
    pub loaded_filename: Option<String>,
}

impl RecordingDeck {
    pub fn new(
        is_recording: Arc<AtomicBool>,
        record_rx: Receiver<f32>,
        playback_tx: Sender<f32>,
    ) -> Self {
        let samples = Arc::new(RwLock::new(Vec::with_capacity(48000 * 30)));
        let is_playing = Arc::new(AtomicBool::new(false));
        let play_pos = Arc::new(AtomicUsize::new(0));
        let loop_playback = Arc::new(AtomicBool::new(false));
        let feeder_running = Arc::new(AtomicBool::new(true));

        // Background playback feeder thread
        let samples_clone = Arc::clone(&samples);
        let is_playing_clone = Arc::clone(&is_playing);
        let play_pos_clone = Arc::clone(&play_pos);
        let loop_clone = Arc::clone(&loop_playback);
        let feeder_running_clone = Arc::clone(&feeder_running);
        let play_tx_clone = playback_tx.clone();

        std::thread::spawn(move || {
            while feeder_running_clone.load(Ordering::Relaxed) {
                if is_playing_clone.load(Ordering::Relaxed) {
                    let lock = samples_clone.read().unwrap();
                    let total = lock.len();
                    if total > 0 {
                        let mut cur_pos = play_pos_clone.load(Ordering::Relaxed);
                        let chunk_size = 480.min(total.saturating_sub(cur_pos));
                        for i in 0..chunk_size {
                            let s = lock[cur_pos + i];
                            let _ = play_tx_clone.try_send(s);
                        }
                        cur_pos += chunk_size;
                        if cur_pos >= total {
                            if loop_clone.load(Ordering::Relaxed) {
                                cur_pos = 0;
                            } else {
                                is_playing_clone.store(false, Ordering::Relaxed);
                                cur_pos = 0;
                            }
                        }
                        play_pos_clone.store(cur_pos, Ordering::Relaxed);
                    } else {
                        is_playing_clone.store(false, Ordering::Relaxed);
                    }
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        });

        Self {
            is_recording,
            record_rx,
            playback_tx,
            samples,
            sample_rate: 48000,
            is_playing,
            play_pos,
            loop_playback,
            playback_volume: 1.0,
            feeder_running,
            record_start_time: None,
            record_band: "40m".to_string(),
            record_mode: "LSB".to_string(),
            record_freq: 7_200_000,
            status_msg: None,
            recent_recordings: Vec::new(),
            last_scan_time: Instant::now() - Duration::from_secs(60),
            loaded_filename: None,
        }
    }

    /// Set status message with automatic timestamp
    pub fn set_status(&mut self, text: &str) {
        self.status_msg = Some((text.to_string(), Instant::now()));
    }

    /// Pull newly captured audio samples from audio capture thread
    pub fn update_recorder(&mut self) {
        if self.is_recording.load(Ordering::Relaxed) {
            let mut buf = self.samples.write().unwrap();
            while let Ok(sample) = self.record_rx.try_recv() {
                buf.push(sample);
            }
        }
    }

    pub fn start_recording(&mut self, state: &SdrState) {
        self.stop_playback();
        self.samples.write().unwrap().clear();
        self.play_pos.store(0, Ordering::Relaxed);
        self.record_start_time = Some(current_local_timestamp());
        self.record_band = Band::from_freq(state.tune_freq).map_or("Unknown".to_string(), |b| b.name().to_string());
        self.record_mode = state.mode.name().to_string();
        self.record_freq = state.tune_freq;
        self.loaded_filename = None;
        self.is_recording.store(true, Ordering::Relaxed);
        self.set_status("🔴 Recording started...");
    }

    pub fn stop_recording(&mut self) {
        self.is_recording.store(false, Ordering::Relaxed);
        self.update_recorder();
        let len_secs = self.samples.read().unwrap().len() as f32 / self.sample_rate as f32;
        self.set_status(&format!("⏹ Recording stopped ({:.1}s captured)", len_secs));
    }

    pub fn stop_playback(&mut self) {
        self.is_playing.store(false, Ordering::Relaxed);
        self.play_pos.store(0, Ordering::Relaxed);
    }

    pub fn seek_relative(&mut self, delta_secs: f32) {
        let total = self.samples.read().unwrap().len();
        if total == 0 {
            return;
        }
        let cur = self.play_pos.load(Ordering::Relaxed);
        let step = (delta_secs * self.sample_rate as f32) as i64;
        let new_pos = (cur as i64 + step).clamp(0, total as i64) as usize;
        self.play_pos.store(new_pos, Ordering::Relaxed);
    }

    pub fn clear(&mut self) {
        self.stop_playback();
        self.samples.write().unwrap().clear();
        self.play_pos.store(0, Ordering::Relaxed);
        self.record_start_time = None;
        self.loaded_filename = None;
        self.set_status("🗑 Audio buffer cleared");
    }

    pub fn save_recording(&mut self, state: &SdrState) -> Result<PathBuf, String> {
        let samples = self.samples.read().unwrap().clone();
        if samples.is_empty() {
            let err = "No recorded audio in buffer".to_string();
            self.set_status(&err);
            return Err(err);
        }

        let dir = PathBuf::from(&state.recordings_dir);
        let _ = std::fs::create_dir_all(&dir);

        let timestamp = self.record_start_time.clone().unwrap_or_else(current_local_timestamp);
        let band = &self.record_band;
        let mode = &self.record_mode;
        let freq_mhz = self.record_freq as f64 / 1_000_000.0;

        let filename = format!("{}_{}_{}_{:.4}MHz.wav", timestamp, band, mode, freq_mhz);
        let path = dir.join(&filename);

        match write_wav_file(&path, &samples, self.sample_rate) {
            Ok(_) => {
                let secs = samples.len() as f32 / self.sample_rate as f32;
                let msg = format!("💾 Saved {} ({:.1}s)", filename, secs);
                self.set_status(&msg);
                self.loaded_filename = Some(filename);
                self.scan_recent_recordings(&state.recordings_dir);
                Ok(path)
            }
            Err(e) => {
                let msg = format!("Error saving WAV: {}", e);
                self.set_status(&msg);
                Err(msg)
            }
        }
    }

    pub fn load_file(&mut self, path: &Path) -> Result<(), String> {
        self.stop_playback();
        match read_wav_file(path) {
            Ok((data, rate)) => {
                let fname = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                let secs = data.len() as f32 / rate as f32;
                *self.samples.write().unwrap() = data;
                self.sample_rate = rate;
                self.play_pos.store(0, Ordering::Relaxed);
                self.loaded_filename = Some(fname.clone());
                self.set_status(&format!("▶ Loaded {} ({:.1}s)", fname, secs));
                Ok(())
            }
            Err(e) => {
                let msg = format!("Error loading WAV: {}", e);
                self.set_status(&msg);
                Err(msg)
            }
        }
    }

    pub fn scan_recent_recordings(&mut self, dir_path: &str) {
        let dir = Path::new(dir_path);
        let mut list = Vec::new();
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_file() && p.extension().map_or(false, |ext| ext == "wav") {
                    list.push(p);
                }
            }
        }
        // Sort newest modified first
        list.sort_by(|a, b| {
            let meta_a = a.metadata().and_then(|m| m.modified()).ok();
            let meta_b = b.metadata().and_then(|m| m.modified()).ok();
            meta_b.cmp(&meta_a)
        });
        self.recent_recordings = list;
        self.last_scan_time = Instant::now();
    }

    /// Render full Recording Deck UI
    pub fn show(&mut self, ui: &mut Ui, state: &mut SdrState, state_changed: &mut bool) {
        self.update_recorder();

        // Auto-refresh recent recordings every 10 seconds if needed
        if self.last_scan_time.elapsed() >= Duration::from_secs(10) {
            self.scan_recent_recordings(&state.recordings_dir);
        }

        let is_rec = self.is_recording.load(Ordering::Relaxed);
        let is_play = self.is_playing.load(Ordering::Relaxed);
        let cur_pos = self.play_pos.load(Ordering::Relaxed);

        let total_samples = self.samples.read().unwrap().len();
        let total_secs = total_samples as f32 / self.sample_rate as f32;
        let cur_secs = cur_pos as f32 / self.sample_rate as f32;

        egui::Frame::group(ui.style())
            .fill(Color32::from_rgb(14, 18, 26))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(45, 60, 85)))
            .inner_margin(8.0)
            .show(ui, |ui| {
                // Deck Header
                ui.horizontal(|ui| {
                    ui.label(RichText::new("📼 RX AUDIO RECORDING DECK").size(12.0).strong().color(Color32::from_rgb(100, 200, 255)));

                    if is_rec {
                        ui.label(RichText::new("● RECORDING IN PROGRESS").size(11.0).strong().color(Color32::from_rgb(255, 60, 60)));
                    } else if let Some(fname) = &self.loaded_filename {
                        ui.label(RichText::new(format!("File: {}", fname)).size(11.0).color(Color32::from_rgb(200, 220, 160)));
                    }

                    if let Some((msg, time)) = &self.status_msg {
                        if time.elapsed().as_secs_f32() < 8.0 {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(RichText::new(msg).size(11.0).color(Color32::from_rgb(255, 220, 100)));
                            });
                        }
                    }
                });

                ui.add_space(4.0);

                // --- 1. Waveform Canvas ---
                let available_w = ui.available_width().max(300.0);
                let (rect, response) = ui.allocate_exact_size(
                    Vec2::new(available_w, 64.0),
                    Sense::click_and_drag(),
                );
                let painter = ui.painter_at(rect);

                // Oscilloscope background
                painter.rect_filled(rect, 3.0, Color32::from_rgb(10, 14, 22));
                painter.rect_stroke(rect, 3.0, Stroke::new(1.0_f32, Color32::from_gray(50)));

                let center_y = rect.center().y;
                let half_h = (rect.height() * 0.5) - 3.0;

                // Center zero line
                painter.line_segment(
                    [Pos2::new(rect.min.x, center_y), Pos2::new(rect.max.x, center_y)],
                    Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 25)),
                );

                if total_samples == 0 {
                    painter.text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "Buffer empty. Click [🔴 Record] to capture live RX audio, or load a recent WAV below.",
                        egui::FontId::proportional(11.0),
                        Color32::from_gray(120),
                    );
                } else {
                    let w = rect.width();
                    let step_px = 2.0f32;
                    let num_bins = (w / step_px).ceil() as usize;

                    // Progress shaded background up to playhead
                    let norm_play = (cur_pos as f32 / total_samples as f32).clamp(0.0, 1.0);
                    let play_x = rect.min.x + norm_play * w;
                    let prog_rect = Rect::from_min_max(
                        rect.min,
                        Pos2::new(play_x, rect.max.y),
                    );
                    painter.rect_filled(prog_rect, 0.0, Color32::from_rgba_unmultiplied(40, 140, 120, 35));

                    // Downsampled waveform envelope (scoped lock)
                    {
                        let samples_lock = self.samples.read().unwrap();
                        for b in 0..num_bins {
                            let x = rect.min.x + (b as f32 * step_px);
                            let s_start = (b * total_samples) / num_bins;
                            let s_end = (((b + 1) * total_samples) / num_bins).min(total_samples);

                            let mut min_val = 0.0f32;
                            let mut max_val = 0.0f32;
                            for &sample in &samples_lock[s_start..s_end] {
                                if sample < min_val { min_val = sample; }
                                if sample > max_val { max_val = sample; }
                            }

                            let y_top = center_y - (max_val.min(1.0) * half_h);
                            let y_bot = center_y - (min_val.max(-1.0) * half_h);

                            let col = if x <= play_x {
                                Color32::from_rgb(60, 230, 180) // Played region: bright aqua
                            } else {
                                Color32::from_rgb(40, 140, 110) // Ahead: muted sea green
                            };

                            painter.line_segment(
                                [Pos2::new(x, y_top), Pos2::new(x, y_bot.max(y_top + 1.0))],
                                Stroke::new(1.5_f32, col),
                            );
                        }
                    }

                    // Playhead Line & Badge
                    painter.line_segment(
                        [Pos2::new(play_x, rect.min.y), Pos2::new(play_x, rect.max.y)],
                        Stroke::new(2.0_f32, Color32::from_rgb(255, 230, 50)),
                    );

                    let play_badge = format_time(cur_secs);
                    painter.text(
                        Pos2::new(play_x.clamp(rect.min.x + 25.0, rect.max.x - 25.0), rect.min.y + 4.0),
                        egui::Align2::CENTER_TOP,
                        play_badge,
                        egui::FontId::monospace(9.0),
                        Color32::from_rgb(255, 230, 50),
                    );

                    // Hover ghost line & scrub indicator
                    if let Some(pos) = ui.input(|i| i.pointer.hover_pos()) {
                        if rect.contains(pos) {
                            let norm_h = ((pos.x - rect.min.x) / w).clamp(0.0, 1.0);
                            let hover_secs = norm_h * total_secs;

                            painter.line_segment(
                                [Pos2::new(pos.x, rect.min.y), Pos2::new(pos.x, rect.max.y)],
                                Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 120)),
                            );

                            painter.text(
                                Pos2::new(pos.x.clamp(rect.min.x + 25.0, rect.max.x - 25.0), rect.max.y - 4.0),
                                egui::Align2::CENTER_BOTTOM,
                                format_time(hover_secs),
                                egui::FontId::monospace(9.0),
                                Color32::WHITE,
                            );

                            // Mouse wheel scrubbing
                            let scroll = ui.input(|i| i.raw_scroll_delta.y);
                            if scroll.abs() > 0.0 {
                                let step_secs = if scroll > 0.0 { 1.0 } else { -1.0 };
                                let step_samples = (step_secs * self.sample_rate as f32) as i64;
                                let new_p = (cur_pos as i64 + step_samples).clamp(0, total_samples as i64) as usize;
                                self.play_pos.store(new_p, Ordering::Relaxed);
                            }
                        }
                    }

                    // Click or drag seek
                    if response.clicked() || response.dragged() {
                        if let Some(pos) = response.interact_pointer_pos() {
                            let norm_p = ((pos.x - rect.min.x) / w).clamp(0.0, 1.0);
                            let new_p = ((norm_p * total_samples as f32).round() as usize).min(total_samples);
                            self.play_pos.store(new_p, Ordering::Relaxed);
                        }
                    }
                }

                ui.add_space(4.0);

                // --- 2. Transport & Playback Controls Row ---
                ui.horizontal(|ui| {
                    // Record button
                    if is_rec {
                        let btn = Button::new(RichText::new("⏹ Stop Rec").strong().color(Color32::WHITE))
                            .fill(Color32::from_rgb(180, 40, 40));
                        if ui.add(btn).clicked() {
                            self.stop_recording();
                        }
                    } else {
                        let btn = Button::new(RichText::new("🔴 Record").strong().color(Color32::WHITE))
                            .fill(Color32::from_rgb(140, 30, 30));
                        if ui.add(btn).on_hover_text("Start recording RX demodulated audio").clicked() {
                            self.start_recording(state);
                        }
                    }

                    // Playback controls (active when samples exist)
                    ui.add_enabled_ui(!is_rec && total_samples > 0, |ui| {
                        if is_play {
                            if ui.button(RichText::new("⏸ Pause").size(11.0)).clicked() {
                                self.is_playing.store(false, Ordering::Relaxed);
                            }
                        } else {
                            if ui.button(RichText::new("▶ Play").size(11.0)).clicked() {
                                self.is_playing.store(true, Ordering::Relaxed);
                            }
                        }

                        if ui.button(RichText::new("⏹ Stop").size(11.0)).clicked() {
                            self.stop_playback();
                        }

                        if ui.button(RichText::new("⏪ -5s").size(11.0)).on_hover_text("Rewind 5 seconds").clicked() {
                            self.seek_relative(-5.0);
                        }

                        if ui.button(RichText::new("⏩ +5s").size(11.0)).on_hover_text("Fast-forward 5 seconds").clicked() {
                            self.seek_relative(5.0);
                        }

                        let is_loop = self.loop_playback.load(Ordering::Relaxed);
                        let loop_btn = Button::new(RichText::new("🔁 Loop").size(11.0))
                            .fill(if is_loop { Color32::from_rgb(40, 120, 70) } else { Color32::from_gray(40) });
                        if ui.add(loop_btn).on_hover_text("Toggle continuous loop playback").clicked() {
                            self.loop_playback.store(!is_loop, Ordering::Relaxed);
                        }
                    });

                    // Time display
                    let time_str = format!("{} / {}", format_time(cur_secs), format_time(total_secs));
                    ui.label(RichText::new(time_str).size(12.0).monospace().strong().color(Color32::from_rgb(255, 230, 70)));

                    ui.separator();

                    ui.add_enabled_ui(!is_rec && total_samples > 0, |ui| {
                        if ui.button(RichText::new("🗑 Clear").size(11.0)).on_hover_text("Clear recorded audio buffer").clicked() {
                            self.clear();
                        }

                        let save_btn = Button::new(RichText::new("💾 Save WAV").strong().color(Color32::WHITE))
                            .fill(Color32::from_rgb(35, 120, 70));
                        if ui.add(save_btn).on_hover_text("Save recording to disk as 16-bit 48kHz WAV").clicked() {
                            let _ = self.save_recording(state);
                        }
                    });
                });

                ui.add_space(2.0);

                // --- 3. Directory & Recalled Recordings Row ---
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Folder:").size(10.0).color(Color32::from_gray(160)));
                    let folder_resp = ui.add(
                        egui::TextEdit::singleline(&mut state.recordings_dir)
                            .desired_width(180.0)
                            .font(egui::FontId::monospace(10.0)),
                    );
                    if folder_resp.changed() {
                        *state_changed = true;
                    }

                    if ui.button(RichText::new("📁 Open").size(10.0)).on_hover_text("Open recordings folder in file manager").clicked() {
                        let _ = std::fs::create_dir_all(&state.recordings_dir);
                        let _ = std::process::Command::new("xdg-open").arg(&state.recordings_dir).spawn();
                    }

                    ui.separator();

                    // Recall Recent Recordings Menu
                    let recent_count = self.recent_recordings.len();
                    egui::ComboBox::from_id_salt("recent_recordings_menu")
                        .selected_text(RichText::new(format!("Recalled Files ({} found) ▾", recent_count)).size(10.0))
                        .width(280.0)
                        .show_ui(ui, |ui| {
                            if self.recent_recordings.is_empty() {
                                ui.label(RichText::new("No WAV files found in directory").size(10.0).italics());
                            } else {
                                for path in &self.recent_recordings.clone() {
                                    let fname = path.file_name().unwrap_or_default().to_string_lossy();
                                    ui.horizontal(|ui| {
                                        if ui.button(RichText::new("▶ Load").size(10.0)).clicked() {
                                            let _ = self.load_file(path);
                                        }
                                        ui.label(RichText::new(fname.to_string()).size(10.0).monospace());
                                    });
                                }
                            }
                        });
                });
            });
    }
}

/// Helper function to format seconds into MM:SS.S
fn format_time(secs: f32) -> String {
    let s = secs.max(0.0);
    let m = (s / 60.0).floor() as u32;
    let rem = s - (m as f32 * 60.0);
    format!("{:02}:{:04.1}", m, rem)
}

/// Helper to get local timestamp string: YYYY-MM-DD_HH-MM-SS
pub fn current_local_timestamp() -> String {
    if let Ok(output) = std::process::Command::new("date")
        .arg("+%Y-%m-%d_%H-%M-%S")
        .output()
    {
        let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !s.is_empty() {
            return s;
        }
    }
    let d = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", d.as_secs())
}

/// Write uncompressed 16-bit PCM 48 kHz mono WAV file
pub fn write_wav_file(path: &Path, samples: &[f32], sample_rate: u32) -> std::io::Result<()> {
    use std::io::Write;
    let mut file = std::fs::File::create(path)?;
    let num_samples = samples.len() as u32;
    let num_channels = 1u16;
    let bits_per_sample = 16u16;
    let byte_rate = sample_rate * (num_channels as u32) * (bits_per_sample as u32) / 8;
    let block_align = num_channels * bits_per_sample / 8;
    let data_chunk_size = num_samples * (block_align as u32);
    let riff_chunk_size = 36 + data_chunk_size;

    // RIFF header
    file.write_all(b"RIFF")?;
    file.write_all(&riff_chunk_size.to_le_bytes())?;
    file.write_all(b"WAVE")?;

    // fmt subchunk
    file.write_all(b"fmt ")?;
    file.write_all(&16u32.to_le_bytes())?; // Subchunk1Size (16 for PCM)
    file.write_all(&1u16.to_le_bytes())?;  // AudioFormat (1 = PCM)
    file.write_all(&num_channels.to_le_bytes())?;
    file.write_all(&sample_rate.to_le_bytes())?;
    file.write_all(&byte_rate.to_le_bytes())?;
    file.write_all(&block_align.to_le_bytes())?;
    file.write_all(&bits_per_sample.to_le_bytes())?;

    // data subchunk
    file.write_all(b"data")?;
    file.write_all(&data_chunk_size.to_le_bytes())?;

    // 16-bit PCM samples
    let mut buf = Vec::with_capacity(samples.len() * 2);
    for &s in samples {
        let i = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
        buf.extend_from_slice(&i.to_le_bytes());
    }
    file.write_all(&buf)?;
    file.flush()?;
    Ok(())
}

/// Read 16-bit PCM WAV file into f32 samples
pub fn read_wav_file(path: &Path) -> std::io::Result<(Vec<f32>, u32)> {
    let bytes = std::fs::read(path)?;
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Not a valid WAV file"));
    }

    let mut idx = 12;
    let mut sample_rate = 48000u32;
    let mut num_channels = 1u16;
    let mut bits_per_sample = 16u16;
    let mut samples = Vec::new();

    while idx + 8 <= bytes.len() {
        let chunk_id = &bytes[idx..idx + 4];
        let chunk_size = u32::from_le_bytes(bytes[idx + 4..idx + 8].try_into().unwrap()) as usize;
        let data_start = idx + 8;
        let data_end = (data_start + chunk_size).min(bytes.len());

        if chunk_id == b"fmt " && data_start + 14 <= bytes.len() {
            num_channels = u16::from_le_bytes(bytes[data_start + 2..data_start + 4].try_into().unwrap());
            sample_rate = u32::from_le_bytes(bytes[data_start + 4..data_start + 8].try_into().unwrap());
            bits_per_sample = u16::from_le_bytes(bytes[data_start + 14..data_start + 16].try_into().unwrap());
        } else if chunk_id == b"data" {
            if bits_per_sample == 16 {
                let step = 2 * num_channels as usize;
                let mut s_idx = data_start;
                while s_idx + 2 <= data_end {
                    let s_i16 = i16::from_le_bytes([bytes[s_idx], bytes[s_idx + 1]]);
                    samples.push(s_i16 as f32 / 32768.0);
                    s_idx += step;
                }
            }
        }
        idx = data_start + chunk_size;
    }

    Ok((samples, sample_rate))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wav_write_and_read_roundtrip() {
        let temp_dir = std::env::temp_dir();
        let test_wav = temp_dir.join("test_rx_deck.wav");

        let original_samples: Vec<f32> = (0..4800)
            .map(|i| ((i as f32 * 440.0 * 2.0 * std::f32::consts::PI) / 48000.0).sin() * 0.8)
            .collect();

        let write_res = write_wav_file(&test_wav, &original_samples, 48000);
        assert!(write_res.is_ok(), "Failed to write WAV file");

        let read_res = read_wav_file(&test_wav);
        assert!(read_res.is_ok(), "Failed to read WAV file");

        let (loaded_samples, sample_rate) = read_res.unwrap();
        assert_eq!(sample_rate, 48000);
        assert_eq!(loaded_samples.len(), original_samples.len());

        for (orig, loaded) in original_samples.iter().zip(loaded_samples.iter()) {
            assert!((orig - loaded).abs() < 0.001);
        }

        let _ = std::fs::remove_file(test_wav);
    }
}
