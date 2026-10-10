pub mod colormap;
pub mod rf_display;
pub mod af_display;
pub mod cw_display;
pub mod controls;
pub mod recording_deck;
pub mod app;

pub use app::OkSdrApp;

pub fn scrollable_slider(
    ui: &mut egui::Ui,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    step: f32,
    configure: impl FnOnce(egui::Slider<'_>) -> egui::Slider<'_>,
) -> egui::Response {
    let min = *range.start();
    let max = *range.end();
    let slider = configure(egui::Slider::new(value, range));
    let mut resp = ui.add(slider);
    if resp.hovered() {
        let scroll_y = ui.input(|i| i.raw_scroll_delta.y);
        if scroll_y.abs() > 0.0 {
            let notches = (scroll_y / 35.0).round();
            let count = if notches.abs() >= 1.0 {
                notches
            } else {
                scroll_y.signum()
            };
            let new_val = (*value + count * step).clamp(min, max);
            if (new_val - *value).abs() > 1e-6 {
                *value = new_val;
                resp.mark_changed();
            }
        }
    }
    resp
}

