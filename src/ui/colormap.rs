use egui::Color32;

/// Map normalized value (0.0 to 1.0) to an SDR heat colormap
#[inline]
pub fn sdr_colormap(t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);

    let (r, g, b) = if t < 0.2 {
        // Deep blue to bright blue: (0, 0, 40) -> (0, 50, 200)
        let f = t / 0.2;
        (0.0, 50.0 * f, 40.0 + 160.0 * f)
    } else if t < 0.45 {
        // Blue to cyan: (0, 50, 200) -> (0, 230, 230)
        let f = (t - 0.2) / 0.25;
        (0.0, 50.0 + 180.0 * f, 200.0 + 30.0 * f)
    } else if t < 0.7 {
        // Cyan to green/yellow: (0, 230, 230) -> (220, 240, 0)
        let f = (t - 0.45) / 0.25;
        (220.0 * f, 230.0 + 10.0 * f, 230.0 * (1.0 - f))
    } else if t < 0.9 {
        // Yellow to orange/red: (220, 240, 0) -> (255, 30, 0)
        let f = (t - 0.7) / 0.2;
        (220.0 + 35.0 * f, 240.0 * (1.0 - f) + 30.0 * f, 0.0)
    } else {
        // Red to white: (255, 30, 0) -> (255, 255, 255)
        let f = (t - 0.9) / 0.1;
        (255.0, 30.0 + 225.0 * f, 255.0 * f)
    };

    Color32::from_rgb(r as u8, g as u8, b as u8)
}
