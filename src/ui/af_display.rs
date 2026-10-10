use egui::{
    Color32, ColorImage, ImageData, Pos2, Rect, Sense, Stroke,
    TextureHandle, TextureOptions, Ui, Vec2,
};
use std::sync::{Arc, Mutex};

use crate::dsp::AF_FFT_SIZE;
use crate::state::SdrState;
use crate::ui::colormap::sdr_colormap;

const AF_WF_WIDTH: usize = 512;
const AF_WF_HEIGHT: usize = 120;

pub struct AfDisplay {
    texture: Option<TextureHandle>,
    waterfall_pixels: Vec<Color32>,
    latest_spectrum: Vec<f32>,
    waterfall_floor_db: f32,
    waterfall_range_db: f32,
    spectrum_min_db: f32,
    spectrum_max_db: f32,
    new_data_ready: bool,
}

impl AfDisplay {
    pub fn new() -> Self {
        let size = AF_WF_WIDTH * AF_WF_HEIGHT;
        let waterfall_pixels = vec![Color32::from_rgb(0, 0, 40); size];

        Self {
            texture: None,
            waterfall_pixels,
            latest_spectrum: vec![-120.0; AF_FFT_SIZE / 2],
            waterfall_floor_db: -90.0,
            waterfall_range_db: 60.0,
            spectrum_min_db: -100.0,
            spectrum_max_db: 0.0,
            new_data_ready: false,
        }
    }

    pub fn push_spectrum(&mut self, spectrum: Vec<f32>) {
        if spectrum.is_empty() {
            return;
        }

        // We only care about audio frequencies 0 to 4000 Hz.
        // If sample rate is 48 kHz, Nyquist is 24 kHz. 4000 / 24000 = 1/6th of bins.
        let max_bin = (spectrum.len() * 4000 / 24000).max(16);
        let audio_slice = &spectrum[..max_bin.min(spectrum.len())];

        // Shift rows down
        let row_len = AF_WF_WIDTH;
        self.waterfall_pixels.copy_within(0..((AF_WF_HEIGHT - 1) * row_len), row_len);

        // Render top row
        let slice_len = audio_slice.len();
        for x in 0..AF_WF_WIDTH {
            let idx = (x * slice_len) / AF_WF_WIDTH;
            let db = audio_slice[idx];
            let norm = ((db - self.waterfall_floor_db) / self.waterfall_range_db).clamp(0.0, 1.0);
            self.waterfall_pixels[x] = sdr_colormap(norm);
        }

        self.latest_spectrum = audio_slice.to_vec();
        self.new_data_ready = true;
    }

    pub fn show(&mut self, ui: &mut Ui, state_arc: &Arc<Mutex<SdrState>>) {
        let (filter_low, filter_high) = {
            let st = state_arc.lock().unwrap();
            (st.filter_low.abs(), st.filter_high.abs())
        };

        let texture = self.texture.get_or_insert_with(|| {
            ui.ctx().load_texture(
                "af_waterfall",
                ImageData::Color(Arc::new(ColorImage {
                    size: [AF_WF_WIDTH, AF_WF_HEIGHT],
                    pixels: self.waterfall_pixels.clone(),
                })),
                TextureOptions::LINEAR,
            )
        });

        if self.new_data_ready {
            texture.set(
                ImageData::Color(Arc::new(ColorImage {
                    size: [AF_WF_WIDTH, AF_WF_HEIGHT],
                    pixels: self.waterfall_pixels.clone(),
                })),
                TextureOptions::LINEAR,
            );
            self.new_data_ready = false;
        }

        let af_spectrum_ratio = {
            let st = state_arc.lock().unwrap();
            st.af_spectrum_ratio.clamp(0.10, 0.90)
        };

        let available_size = ui.available_size();
        let total_height = available_size.y.max(60.0);
        let scale_height = 18.0;
        let usable_height = (total_height - scale_height).max(30.0);
        let spectrum_height = (usable_height * af_spectrum_ratio).clamp(15.0, usable_height - 15.0);
        let waterfall_height = usable_height - spectrum_height;

        let (rect, _) = ui.allocate_exact_size(
            Vec2::new(available_size.x, total_height),
            Sense::hover(),
        );

        let painter = ui.painter_at(rect);

        let spec_rect = Rect::from_min_size(rect.min, Vec2::new(rect.width(), spectrum_height));
        let scale_rect = Rect::from_min_size(
            Pos2::new(rect.min.x, spec_rect.max.y),
            Vec2::new(rect.width(), scale_height),
        );
        let wf_rect = Rect::from_min_size(
            Pos2::new(rect.min.x, scale_rect.max.y),
            Vec2::new(rect.width(), waterfall_height),
        );

        // Splitter interaction on the AF frequency scale bar between panadapter and waterfall
        let scale_id = ui.make_persistent_id("af_scale_splitter");
        let scale_response = ui.interact(scale_rect, scale_id, Sense::click_and_drag());
        if scale_response.hovered() || scale_response.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
        }
        if scale_response.dragged() {
            let dy = scale_response.drag_delta().y;
            if dy.abs() > 0.0 {
                let mut st = state_arc.lock().unwrap();
                let new_ratio = (st.af_spectrum_ratio + dy / usable_height).clamp(0.10, 0.90);
                st.af_spectrum_ratio = new_ratio;
            }
        }

        // Backgrounds & Scale splitter borders
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
            Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );

        // AF Spectrum line
        let spec_len = self.latest_spectrum.len();
        if spec_len > 1 {
            let mut points = Vec::with_capacity(spec_len);
            for (i, &db) in self.latest_spectrum.iter().enumerate() {
                let x = spec_rect.min.x + (i as f32 / spec_len as f32) * spec_rect.width();
                let y_norm = 1.0 - ((db - self.spectrum_min_db) / (self.spectrum_max_db - self.spectrum_min_db)).clamp(0.0, 1.0);
                let y = spec_rect.min.y + y_norm * spec_rect.height();
                points.push(Pos2::new(x, y));
            }
            painter.add(egui::Shape::line(
                points,
                Stroke::new(1.2f32, Color32::from_rgb(100, 180, 255)),
            ));
        }

        // AF Frequency Scale (0 to 4000 Hz)
        let max_hz = 4000.0;
        let step_hz = 500.0;
        let mut hz = 0.0;
        while hz <= max_hz {
            let norm = hz / max_hz;
            let x = scale_rect.min.x + norm * scale_rect.width();
            painter.line_segment(
                [Pos2::new(x, scale_rect.min.y), Pos2::new(x, scale_rect.min.y + 4.0)],
                Stroke::new(1.0f32, Color32::from_gray(180)),
            );
            painter.text(
                Pos2::new(x, scale_rect.min.y + 10.0),
                egui::Align2::CENTER_CENTER,
                format!("{:.0}", hz),
                egui::FontId::monospace(9.0),
                Color32::from_gray(200),
            );
            hz += step_hz;
        }

        // Draw filter cut boundaries
        let f_min = filter_low.min(filter_high);
        let f_max = filter_low.max(filter_high);
        let min_x = scale_rect.min.x + (f_min / max_hz).clamp(0.0, 1.0) * scale_rect.width();
        let max_x = scale_rect.min.x + (f_max / max_hz).clamp(0.0, 1.0) * scale_rect.width();

        painter.line_segment(
            [Pos2::new(min_x, rect.min.y), Pos2::new(min_x, rect.max.y)],
            Stroke::new(1.0f32, Color32::from_rgb(255, 120, 40)),
        );
        painter.line_segment(
            [Pos2::new(max_x, rect.min.y), Pos2::new(max_x, rect.max.y)],
            Stroke::new(1.0f32, Color32::from_rgb(255, 120, 40)),
        );
    }
}
