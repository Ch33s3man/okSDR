use num_complex::Complex32;
use crate::state::RadioMode;

pub struct Demodulator {
    prev_sample: Complex32,
    am_dc_level: f32,
}

impl Demodulator {
    pub fn new() -> Self {
        Self {
            prev_sample: Complex32::new(1.0, 0.0),
            am_dc_level: 0.0,
        }
    }

    #[inline]
    pub fn demodulate(&mut self, sample: Complex32, mode: RadioMode) -> f32 {
        match mode {
            RadioMode::USB | RadioMode::LSB | RadioMode::CW => {
                // For SSB and CW, the complex FIR bandpass filter already placed the desired
                // sideband or tone in the passband. The audio output is twice the real component.
                2.0 * sample.re
            }
            RadioMode::AM => {
                // Envelope detector: magnitude of complex signal minus DC carrier
                let envelope = (sample.re * sample.re + sample.im * sample.im).sqrt();
                // Slow DC tracking filter for carrier removal (alpha = 0.999)
                self.am_dc_level = 0.999 * self.am_dc_level + 0.001 * envelope;
                envelope - self.am_dc_level
            }
            RadioMode::FM => {
                // FM discriminator: phase difference between consecutive samples
                let prod = sample * self.prev_sample.conj();
                self.prev_sample = sample;
                prod.im.atan2(prod.re) * 0.5
            }
        }
    }

    pub fn reset(&mut self) {
        self.prev_sample = Complex32::new(1.0, 0.0);
        self.am_dc_level = 0.0;
    }
}
