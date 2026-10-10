use egui::{Align2, Color32, FontId, Pos2, RichText, Sense, Stroke, Ui, Vec2};
use std::collections::VecDeque;
use std::time::Instant;

struct DecodedCharEntry {
    char: char,
    anchor_distance: f32,
}

pub struct CwDisplay {
    chars: VecDeque<DecodedCharEntry>,
    total_distance: f32,
    last_update: Instant,
    pub current_wpm: u32,
    pub is_tone_active: bool,
    last_tone_time: Instant,
    char_spacing: f32,
}

impl CwDisplay {
    pub fn new() -> Self {
        Self {
            chars: VecDeque::with_capacity(128),
            total_distance: 0.0,
            last_update: Instant::now(),
            current_wpm: 20,
            is_tone_active: false,
            last_tone_time: Instant::now(),
            char_spacing: 8.5,
        }
    }

    /// Push a newly decoded character from the Morse decoder into the scrolling ticker
    pub fn push_char(&mut self, ch: char) {
        let min_anchor = self
            .chars
            .back()
            .map_or(self.total_distance, |last| last.anchor_distance + self.char_spacing);
        let anchor = min_anchor.max(self.total_distance);
        self.chars.push_back(DecodedCharEntry {
            char: ch,
            anchor_distance: anchor,
        });

        // Limit maximum buffer size to avoid unbounded memory
        if self.chars.len() > 300 {
            self.chars.pop_front();
        }
    }

    /// Update detected WPM and audio tone detection status from audio DSP
    pub fn update_state(&mut self, wpm: u32, is_tone: bool) {
        if wpm >= 6 && wpm <= 50 {
            self.current_wpm = wpm;
        }
        self.is_tone_active = is_tone;
        if is_tone {
            self.last_tone_time = Instant::now();
        }
    }

    /// Clear decoded message buffer
    pub fn clear(&mut self) {
        self.chars.clear();
        self.total_distance = 0.0;
    }

    /// Render CW decoder status and scrolling text ticker on the right side of the AF bar
    pub fn show(&mut self, ui: &mut Ui) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_update).as_secs_f32().min(0.1);
        self.last_update = now;

        // Scroll speed in pixels per second matching current WPM
        // (PARIS standard: 5 characters per word -> chars_per_sec = WPM * 5 / 60)
        let wpm_f = self.current_wpm as f32;
        let px_per_sec = (wpm_f / 12.0) * self.char_spacing;

        // Scroll ticker whenever there is text on screen or recent tone activity
        if !self.chars.is_empty() {
            self.total_distance += px_per_sec * dt;
        }

        ui.separator();

        // 1. Tone Activity Indicator LED
        let led_col = if self.is_tone_active {
            Color32::from_rgb(255, 230, 40) // Bright glowing yellow when CW tone is sounding
        } else if self.last_tone_time.elapsed().as_secs_f32() < 0.5 {
            Color32::from_rgb(180, 160, 30) // Soft decay
        } else {
            Color32::from_gray(50) // Idle dark gray
        };
        ui.label(RichText::new("●").size(14.0).color(led_col))
            .on_hover_text("CW Tone Key-Down Activity Indicator");

        // 2. Decoder Label & WPM Readout Badge
        let wpm_text = format!("CW: {} WPM", self.current_wpm);

        let wpm_label = ui.add(
            egui::Label::new(
                RichText::new(wpm_text)
                    .size(11.0)
                    .monospace()
                    .strong()
                    .color(Color32::from_rgb(255, 200, 50)),
            )
            .sense(Sense::click()),
        )
        .on_hover_text("Morse speed (auto-evaluates every 20s; scroll wheel adjusts manually)");

        // Allow scrolling over WPM label to nudge speed
        if wpm_label.hovered() {
            let scroll = ui.input(|i| i.raw_scroll_delta.y);
            if scroll.abs() > 0.0 {
                let step: i32 = if scroll > 0.0 { 1 } else { -1 };
                self.current_wpm = ((self.current_wpm as i32 + step).clamp(6, 50)) as u32;
            }
        }

        // 3. Clear button
        if ui
            .button(RichText::new("⌫").size(10.0))
            .on_hover_text("Clear CW decoder buffer")
            .clicked()
        {
            self.clear();
        }

        // 4. Scrolling Morse Text Ticker Ribbon
        let avail_width = ui.available_size().x.max(60.0);
        let ticker_height = 18.0;
        let (rect, _) = ui.allocate_exact_size(
            Vec2::new(avail_width, ticker_height),
            Sense::hover(),
        );

        let painter = ui.painter_at(rect);

        // Dark ribbon background with subtle border
        painter.rect_filled(rect, 3.0, Color32::from_rgb(12, 16, 24));
        painter.rect_stroke(rect, 3.0, Stroke::new(1.0f32, Color32::from_gray(55)));

        // Clip painter so characters cleanly scroll in from the right and disappear on the left
        let clipped_painter = painter.with_clip_rect(rect);

        if self.chars.is_empty() {
            // Idle placeholder
            clipped_painter.text(
                Pos2::new(rect.min.x + 8.0, rect.center().y),
                Align2::LEFT_CENTER,
                "Waiting for CW signal...",
                FontId::monospace(10.0),
                Color32::from_gray(90),
            );
        } else {
            // Render characters scrolling from right to left
            for entry in &self.chars {
                let x = rect.max.x - 12.0 - (self.total_distance - entry.anchor_distance);
                if x >= rect.min.x - 20.0 && x <= rect.max.x + 20.0 {
                    clipped_painter.text(
                        Pos2::new(x, rect.center().y),
                        Align2::LEFT_CENTER,
                        entry.char.to_string(),
                        FontId::monospace(12.0),
                        Color32::from_rgb(80, 255, 120), // Classic high-visibility phosphor green
                    );
                }
            }

            // Prune characters that have completely scrolled past the left edge
            while let Some(front) = self.chars.front() {
                let x = rect.max.x - 12.0 - (self.total_distance - front.anchor_distance);
                if x < rect.min.x - 40.0 {
                    self.chars.pop_front();
                } else {
                    break;
                }
            }
        }
    }
}
