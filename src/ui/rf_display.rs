use egui::{
    Color32, ColorImage, ImageData, Pos2, Rect, Sense, Stroke,
    TextureHandle, TextureOptions, Ui, Vec2,
};
use std::sync::{Arc, Mutex};

use crate::dsp::RF_FFT_SIZE;
use crate::state::{RadioMode, SdrState};
use crate::ui::colormap::sdr_colormap;
use crate::ui::scrollable_slider;

const WATERFALL_WIDTH: usize = 1024;
const WATERFALL_HEIGHT: usize = 200;

pub struct RfDisplay {
    texture: Option<TextureHandle>,
    waterfall_pixels: Vec<Color32>,
    latest_spectrum: Vec<f32>,
    pub waterfall_floor_db: f32,
    pub waterfall_range_db: f32,
    pub display_span_hz: f32,
    spectrum_min_db: f32,
    spectrum_max_db: f32,
    new_data_ready: bool,
}

impl RfDisplay {
    pub fn new(waterfall_floor_db: f32, waterfall_range_db: f32, display_span_hz: f32) -> Self {
        let size = WATERFALL_WIDTH * WATERFALL_HEIGHT;
        let waterfall_pixels = vec![Color32::from_rgb(0, 0, 40); size];

        Self {
            texture: None,
            waterfall_pixels,
            latest_spectrum: vec![-120.0; RF_FFT_SIZE],
            waterfall_floor_db,
            waterfall_range_db,
            display_span_hz,
            spectrum_min_db: -130.0,
            spectrum_max_db: -30.0,
            new_data_ready: false,
        }
    }

    pub fn push_spectrum(&mut self, spectrum: Vec<f32>) {
        if spectrum.is_empty() {
            return;
        }

        // Shift rows down by 1 row
        let row_len = WATERFALL_WIDTH;
        self.waterfall_pixels.copy_within(0..((WATERFALL_HEIGHT - 1) * row_len), row_len);

        // Resample/render top row
        let spec_len = spectrum.len();
        for x in 0..WATERFALL_WIDTH {
            let spec_idx = (x * spec_len) / WATERFALL_WIDTH;
            let db = spectrum[spec_idx];
            let norm = ((db - self.waterfall_floor_db) / self.waterfall_range_db).clamp(0.0, 1.0);
            self.waterfall_pixels[x] = sdr_colormap(norm);
        }

        self.latest_spectrum = spectrum;
        self.new_data_ready = true;
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        state_arc: &Arc<Mutex<SdrState>>,
        mut on_tune_changed: impl FnMut(u64),
        mut on_lo_changed: impl FnMut(u64),
    ) {
        let (lo_freq, tune_freq, filter_low, filter_high, sample_rate, mode, _cw_pitch_hz, state_span) = {
            let st = state_arc.lock().unwrap();
            (
                st.lo_freq,
                st.tune_freq,
                st.filter_low,
                st.filter_high,
                st.sample_rate as f32,
                st.mode,
                st.cw_pitch_hz,
                st.rf_display_span_hz,
            )
        };
        if state_span > 0.0 && (self.display_span_hz - state_span).abs() > 1.0 {
            self.display_span_hz = state_span;
        }

        let max_span = sample_rate as f64;
        let min_span = 10_000.0f64;
        let cur_span = if self.display_span_hz <= 0.0 {
            max_span
        } else {
            (self.display_span_hz as f64).clamp(min_span, max_span)
        };

        // In CW mode, incoming signals are offset from dial carrier by CW sidetone pitch
        let cw_offset = if mode == RadioMode::CW {
            (filter_low as f64 + filter_high as f64) * 0.5
        } else {
            0.0
        };

        // Header with controls for Waterfall floor/range/span
        ui.horizontal(|ui| {
            let is_full_span = (cur_span - max_span).abs() < 500.0;
            let title = if is_full_span {
                format!("RF PANADAPTER & WATERFALL (±{:.0} kHz Baseband)", (max_span * 0.5) / 1000.0)
            } else {
                format!(
                    "RF PANADAPTER & WATERFALL ({:.0} kHz Span / ±{:.0} kHz)",
                    cur_span / 1000.0,
                    (max_span * 0.5) / 1000.0
                )
            };
            ui.label(egui::RichText::new(title).size(11.0).strong());

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new("Click: Tune 1kHz | Drag: Move Filter | Scroll: 1kHz Step")
                        .size(10.0)
                        .color(Color32::from_gray(160)),
                );

                let range_txt = format!("Range: {:.0} dB", self.waterfall_range_db);
                let floor_txt = format!("Floor: {:.0} dB", self.waterfall_floor_db);
                let span_txt = format!("Span: {:.0} kHz", cur_span / 1000.0);

                let range_res = scrollable_slider(
                    ui,
                    &mut self.waterfall_range_db,
                    10.0..=80.0,
                    2.0,
                    |s| s.text(range_txt).show_value(false),
                )
                .on_hover_text("Scroll or drag waterfall dynamic range (2 dB step)");

                let floor_res = scrollable_slider(
                    ui,
                    &mut self.waterfall_floor_db,
                    -150.0..=-40.0,
                    2.0,
                    |s| s.text(floor_txt).show_value(false),
                )
                .on_hover_text("Scroll or drag waterfall noise floor (2 dB step)");

                let mut span_f32 = cur_span as f32;
                let span_res = scrollable_slider(
                    ui,
                    &mut span_f32,
                    min_span as f32..=max_span as f32,
                    2000.0,
                    |s| s.text(span_txt).show_value(false),
                )
                .on_hover_text(format!(
                    "Scroll or drag visible RF frequency width/span (10 kHz to {:.0} kHz max I/Q)",
                    max_span / 1000.0
                ));

                if span_res.changed() {
                    self.display_span_hz = span_f32;
                }

                if range_res.changed() || floor_res.changed() || span_res.changed() {
                    let mut st = state_arc.lock().unwrap();
                    st.rf_waterfall_range_db = self.waterfall_range_db;
                    st.rf_waterfall_floor_db = self.waterfall_floor_db;
                    st.rf_display_span_hz = self.display_span_hz;
                }
            });
        });

        // Ensure texture is created
        let texture = self.texture.get_or_insert_with(|| {
            ui.ctx().load_texture(
                "rf_waterfall",
                ImageData::Color(Arc::new(ColorImage {
                    size: [WATERFALL_WIDTH, WATERFALL_HEIGHT],
                    pixels: self.waterfall_pixels.clone(),
                })),
                TextureOptions::LINEAR,
            )
        });

        // ONLY upload to GPU when a new FFT row actually arrived!
        // (Stops the 60 MB/s texture transfer churn that caused GUI freezing!)
        if self.new_data_ready {
            texture.set(
                ImageData::Color(Arc::new(ColorImage {
                    size: [WATERFALL_WIDTH, WATERFALL_HEIGHT],
                    pixels: self.waterfall_pixels.clone(),
                })),
                TextureOptions::LINEAR,
            );
            self.new_data_ready = false;
        }

        let rf_spectrum_ratio = {
            let st = state_arc.lock().unwrap();
            st.rf_spectrum_ratio.clamp(0.10, 0.90)
        };

        let available_size = ui.available_size();
        let total_height = available_size.y.max(260.0);
        let scale_height = 24.0;
        let usable_height = (total_height - scale_height).max(100.0);
        let spectrum_height = (usable_height * rf_spectrum_ratio).clamp(40.0, usable_height - 40.0);
        let waterfall_height = usable_height - spectrum_height;

        let (rect, response) = ui.allocate_exact_size(
            Vec2::new(available_size.x, total_height),
            Sense::click_and_drag(),
        );

        let painter = ui.painter_at(rect);

        // Rect definitions
        let spec_rect = Rect::from_min_size(rect.min, Vec2::new(rect.width(), spectrum_height));
        let scale_rect = Rect::from_min_size(
            Pos2::new(rect.min.x, spec_rect.max.y),
            Vec2::new(rect.width(), scale_height),
        );
        let wf_rect = Rect::from_min_size(
            Pos2::new(rect.min.x, scale_rect.max.y),
            Vec2::new(rect.width(), waterfall_height),
        );

        // Splitter interaction on the frequency scale bar between panadapter and waterfall
        let scale_id = ui.make_persistent_id("rf_scale_splitter");
        let scale_response = ui.interact(scale_rect, scale_id, Sense::click_and_drag());
        if scale_response.hovered() || scale_response.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
        }
        if scale_response.dragged() {
            let dy = scale_response.drag_delta().y;
            if dy.abs() > 0.0 {
                let mut st = state_arc.lock().unwrap();
                let new_ratio = (st.rf_spectrum_ratio + dy / usable_height).clamp(0.10, 0.90);
                st.rf_spectrum_ratio = new_ratio;
            }
        }

        // Baseband coverage of the full I/Q capture
        let baseband_half = max_span * 0.5;
        let baseband_min = lo_freq as f64 - baseband_half;
        let baseband_max = lo_freq as f64 + baseband_half;

        // Visible display frequency range [f_min, f_max] based on Span zoom
        let half_span = cur_span * 0.5;
        let center_f = (tune_freq as f64).clamp(baseband_min + half_span, baseband_max - half_span);
        let f_min = center_f - half_span;
        let f_max = center_f + half_span;

        // Texture UV coordinates corresponding to visible [f_min, f_max]
        let u_min = ((f_min - baseband_min) / max_span).clamp(0.0, 1.0) as f32;
        let u_max = ((f_max - baseband_min) / max_span).clamp(0.0, 1.0) as f32;

        let freq_to_x = |f: f64| -> f32 {
            let norm = (f - f_min) / (f_max - f_min);
            rect.min.x + (norm as f32 * rect.width())
        };

        let x_to_freq = |x: f32| -> u64 {
            let norm = ((x - rect.min.x) / rect.width()).clamp(0.0, 1.0);
            let f = f_min + (norm as f64 * (f_max - f_min));
            (f.round()).max(0.0) as u64
        };

        // 1. Backgrounds & Scale splitter borders
        painter.rect_filled(spec_rect, 0.0, Color32::from_rgb(10, 14, 24));
        painter.rect_filled(scale_rect, 0.0, Color32::from_rgb(22, 27, 34));

        let scale_border_col = if scale_response.hovered() || scale_response.dragged() {
            Color32::from_rgb(255, 200, 50)
        } else {
            Color32::from_gray(55)
        };
        painter.line_segment([scale_rect.left_top(), scale_rect.right_top()], Stroke::new(1.0f32, scale_border_col));
        painter.line_segment([scale_rect.left_bottom(), scale_rect.right_bottom()], Stroke::new(1.0f32, scale_border_col));

        painter.image(
            texture.id(),
            wf_rect,
            Rect::from_min_max(Pos2::new(u_min, 0.0), Pos2::new(u_max, 1.0)),
            Color32::WHITE,
        );

        // 2. Grid lines & dB levels on spectrum
        let db_step = 20.0;
        let mut cur_db = self.spectrum_min_db;
        while cur_db <= self.spectrum_max_db {
            let y_norm = 1.0 - (cur_db - self.spectrum_min_db) / (self.spectrum_max_db - self.spectrum_min_db);
            let y = spec_rect.min.y + y_norm * spec_rect.height();
            painter.line_segment(
                [Pos2::new(spec_rect.min.x, y), Pos2::new(spec_rect.max.x, y)],
                Stroke::new(1.0f32, Color32::from_rgba_unmultiplied(255, 255, 255, 25)),
            );
            painter.text(
                Pos2::new(spec_rect.min.x + 5.0, y - 6.0),
                egui::Align2::LEFT_CENTER,
                format!("{:.0} dB", cur_db),
                egui::FontId::monospace(10.0),
                Color32::from_gray(140),
            );
            cur_db += db_step;
        }

        // 3. Draw Spectrum Line
        let spec_len = self.latest_spectrum.len();
        if spec_len > 1 {
            let bin_hz = max_span / (spec_len - 1) as f64;
            let start_bin = ((f_min - baseband_min) / bin_hz).floor().max(0.0) as usize;
            let end_bin = (((f_max - baseband_min) / bin_hz).ceil() as usize).min(spec_len - 1);
            let count = end_bin.saturating_sub(start_bin) + 1;

            let mut points = Vec::with_capacity(count);
            for i in start_bin..=end_bin {
                let bin_f = baseband_min + (i as f64 * bin_hz);
                let x = freq_to_x(bin_f);
                let db = self.latest_spectrum[i];
                let y_norm = 1.0 - ((db - self.spectrum_min_db) / (self.spectrum_max_db - self.spectrum_min_db)).clamp(0.0, 1.0);
                let y = spec_rect.min.y + y_norm * spec_rect.height();
                points.push(Pos2::new(x, y));
            }
            if points.len() > 1 {
                painter.add(egui::Shape::line(
                    points,
                    Stroke::new(1.2f32, Color32::from_rgb(80, 220, 120)),
                ));
            }
        }

        // 4. Passband highlight & markers across Spectrum, Scale, and Waterfall
        let tune_x = freq_to_x(tune_freq as f64);
        let pb_min_x = freq_to_x(tune_freq as f64 + filter_low as f64);
        let pb_max_x = freq_to_x(tune_freq as f64 + filter_high as f64);

        let pb_left = pb_min_x.min(pb_max_x);
        let pb_right = pb_min_x.max(pb_max_x);

        // Passband shaded region
        let pb_rect = Rect::from_min_max(
            Pos2::new(pb_left, rect.min.y),
            Pos2::new(pb_right, rect.max.y),
        );
        painter.rect_filled(
            pb_rect,
            0.0,
            Color32::from_rgba_unmultiplied(255, 200, 50, 45),
        );

        // In CW mode, draw center pitch guide line inside the passband
        if mode == RadioMode::CW {
            let pb_center_x = freq_to_x(tune_freq as f64 + cw_offset);
            painter.line_segment(
                [Pos2::new(pb_center_x, rect.min.y), Pos2::new(pb_center_x, rect.max.y)],
                Stroke::new(1.0f32, Color32::from_rgba_unmultiplied(255, 230, 40, 180)),
            );
        }

        // Tune carrier line (bright yellow)
        painter.line_segment(
            [Pos2::new(tune_x, rect.min.y), Pos2::new(tune_x, rect.max.y)],
            Stroke::new(1.5f32, Color32::from_rgb(255, 230, 40)),
        );

        // Radio LO center line (red)
        let lo_x = freq_to_x(lo_freq as f64);
        painter.line_segment(
            [Pos2::new(lo_x, rect.min.y), Pos2::new(lo_x, rect.max.y)],
            Stroke::new(1.5f32, Color32::from_rgb(240, 70, 70)),
        );

        // 5. Frequency Scale Markers (MHz / kHz)
        let step = if cur_span <= 20_000.0 {
            2_000.0
        } else if cur_span <= 40_000.0 {
            5_000.0
        } else {
            10_000.0
        };
        let start_marker = (f_min / step).ceil() * step;
        let mut f = start_marker;

        while f <= f_max {
            let x = freq_to_x(f);
            let is_major = if cur_span <= 20_000.0 {
                (f / 10_000.0).round() * 10_000.0 == f
            } else if cur_span <= 40_000.0 {
                (f / 20_000.0).round() * 20_000.0 == f
            } else {
                (f / 50_000.0).round() * 50_000.0 == f
            };

            let tick_h = if is_major { 8.0 } else { 5.0 };
            painter.line_segment(
                [Pos2::new(x, scale_rect.min.y), Pos2::new(x, scale_rect.min.y + tick_h)],
                Stroke::new(1.0f32, if is_major { Color32::from_gray(210) } else { Color32::from_gray(160) }),
            );

            // Numerical label for every frequency division
            let mhz = f / 1_000_000.0;
            let label_col = if is_major {
                Color32::from_gray(240)
            } else {
                Color32::from_gray(190)
            };
            painter.text(
                Pos2::new(x, scale_rect.min.y + 14.0),
                egui::Align2::CENTER_CENTER,
                format!("{:.3}", mhz),
                egui::FontId::monospace(10.0),
                label_col,
            );

            f += step;
        }


        // 6. Mouse hover cursor line & frequency readout indicator
        if let Some(pos) = ui.input(|i| i.pointer.hover_pos()) {
            if rect.contains(pos) && !scale_response.dragged() {
                // Vertical yellow line at mouse cursor across spectrum, scale, and waterfall
                painter.line_segment(
                    [Pos2::new(pos.x, rect.min.y), Pos2::new(pos.x, rect.max.y)],
                    Stroke::new(1.5f32, Color32::from_rgb(255, 230, 40)),
                );

                // Exact 1 kHz-snapped frequency that clicking here will tune to (offset by CW pitch in CW mode)
                let hover_rf = x_to_freq(pos.x);
                let target_dial = (hover_rf as f64 - cw_offset).max(0.0);
                let clean_freq = ((target_dial / 1000.0).round() as u64) * 1000;
                let freq_str = format!("{:.3} MHz", clean_freq as f64 / 1e6);

                // Badge next to cursor line in the frequency scale bar
                let badge_w = 70.0;
                let badge_h = 16.0;
                let badge_x = if pos.x + badge_w + 6.0 <= rect.max.x {
                    pos.x + 5.0
                } else {
                    pos.x - 5.0 - badge_w
                };
                let badge_rect = Rect::from_min_size(
                    Pos2::new(badge_x, scale_rect.center().y - badge_h * 0.5),
                    Vec2::new(badge_w, badge_h),
                );
                painter.rect_filled(badge_rect, 2.0, Color32::from_rgba_unmultiplied(16, 20, 28, 230));
                painter.rect_stroke(badge_rect, 2.0, Stroke::new(1.0f32, Color32::from_rgb(255, 230, 40)));
                painter.text(
                    badge_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    freq_str,
                    egui::FontId::monospace(10.0),
                    Color32::from_rgb(255, 235, 60),
                );
            }
        }

        // 7. Interactive tuning handling
        // Guard against retuning while dragging the scale splitter
        let is_scale_drag = scale_response.dragged() || scale_response.drag_stopped();
        if !is_scale_drag {
            // Single click: Immediately retunes radio LO and demodulator to clicked frequency (snapped to 1 kHz)!
            if response.clicked() {
                if let Some(pos) = response.interact_pointer_pos() {
                    let clicked_rf = x_to_freq(pos.x);
                    let target_dial = (clicked_rf as f64 - cw_offset).max(0.0);
                    let clean_freq = ((target_dial / 1000.0).round() as u64) * 1000;
                    on_lo_changed(clean_freq);
                    on_tune_changed(clean_freq);
                }
            } else if response.dragged() {
                // Dragging: smoothly moves demodulator tune frequency across passband
                if let Some(pos) = response.interact_pointer_pos() {
                    let drag_rf = x_to_freq(pos.x);
                    let target_dial = (drag_rf as f64 - cw_offset).round().max(0.0) as u64;
                    on_tune_changed(target_dial);
                }
            } else if response.drag_stopped() {
                // Releasing drag: sync radio LO to final frequency (clean 1 kHz snap)
                if let Some(pos) = response.interact_pointer_pos() {
                    let final_rf = x_to_freq(pos.x);
                    let target_dial = (final_rf as f64 - cw_offset).max(0.0);
                    let clean_final = ((target_dial / 1000.0).round() as u64) * 1000;
                    on_lo_changed(clean_final);
                    on_tune_changed(clean_final);
                }
            }
        }

        // Mouse wheel for radio VFO tuning (clean 1 kHz step, snapping to exact kHz like 7.204)
        let scroll_delta = ui.input(|i| i.raw_scroll_delta.y);
        if response.hovered() && scroll_delta.abs() > 0.0 {
            let step: i64 = if scroll_delta > 0.0 { 1000 } else { -1000 };
            let rounded_lo = ((lo_freq as f64 / 1000.0).round() as i64) * 1000;
            let new_lo = (rounded_lo + step).max(0) as u64;
            on_lo_changed(new_lo);
            on_tune_changed(new_lo);
        }
    }
}
