use num_complex::Complex32;
use std::f32::consts::TAU;

pub struct Nco {
    phase: f32,
    phase_inc: f32,
    sample_rate: f32,
}

impl Nco {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            phase: 0.0,
            phase_inc: 0.0,
            sample_rate,
        }
    }

    pub fn set_freq_offset(&mut self, freq_offset: f32) {
        // Shift negative offset to translate desired frequency to 0 Hz baseband
        self.phase_inc = (-freq_offset / self.sample_rate) * TAU;
    }

    #[inline]
    pub fn step(&mut self) -> Complex32 {
        let (sin, cos) = self.phase.sin_cos();
        self.phase += self.phase_inc;
        if self.phase > TAU {
            self.phase -= TAU;
        } else if self.phase < -TAU {
            self.phase += TAU;
        }
        Complex32::new(cos, sin)
    }

    #[inline]
    pub fn mix(&mut self, sample: Complex32) -> Complex32 {
        sample * self.step()
    }
}
