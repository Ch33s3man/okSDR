use num_complex::Complex32;
use rustfft::{Fft, FftPlanner};
use std::sync::Arc;
use std::f32::consts::PI;

pub struct SpectrumFft {
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    fft_buffer: Vec<Complex32>,
    output_db: Vec<f32>,
    smooth_db: Vec<f32>,
    size: usize,
    smooth_factor: f32,
}

impl SpectrumFft {
    pub fn new(size: usize, smooth_factor: f32) -> Self {
        let mut planner = FftPlanner::new();
        let fft = planner.plan_fft_forward(size);

        // Precompute Hann window
        let mut window = Vec::with_capacity(size);
        for i in 0..size {
            let w = 0.5 * (1.0 - (2.0 * PI * i as f32 / (size as f32 - 1.0)).cos());
            window.push(w);
        }

        Self {
            fft,
            window,
            fft_buffer: vec![Complex32::new(0.0, 0.0); size],
            output_db: vec![-120.0; size],
            smooth_db: vec![-120.0; size],
            size,
            smooth_factor: smooth_factor.clamp(0.0, 0.99),
        }
    }

    /// Process a block of complex samples (e.g. RF I/Q stream), applies Hann window,
    /// runs FFT, centers zero-frequency (fft-shift), and computes dB magnitude.
    pub fn process_complex(&mut self, samples: &[Complex32]) -> &[f32] {
        let n = self.size.min(samples.len());

        for i in 0..n {
            let w = self.window[i];
            self.fft_buffer[i] = Complex32::new(samples[i].re * w, samples[i].im * w);
        }
        for i in n..self.size {
            self.fft_buffer[i] = Complex32::new(0.0, 0.0);
        }

        self.fft.process(&mut self.fft_buffer);

        let half = self.size / 2;
        let inv_size = 1.0 / (self.size as f32);

        // FFT-shift: indices [half..size] become negative freqs, [0..half] become positive freqs
        for i in 0..self.size {
            let src_idx = if i < half { i + half } else { i - half };
            let val = self.fft_buffer[src_idx];
            let mag = (val.re * val.re + val.im * val.im).sqrt() * inv_size;
            let db = 20.0 * (mag.max(1e-6)).log10();

            // Exponential smoothing
            self.smooth_db[i] = self.smooth_factor * self.smooth_db[i] + (1.0 - self.smooth_factor) * db;
            self.output_db[i] = self.smooth_db[i];
        }

        &self.output_db
    }

    /// Process real audio samples for AF spectrum (0 to Nyquist, e.g. 0 to 24 kHz)
    pub fn process_real(&mut self, samples: &[f32]) -> &[f32] {
        let n = self.size.min(samples.len());

        for i in 0..n {
            let w = self.window[i];
            self.fft_buffer[i] = Complex32::new(samples[i] * w, 0.0);
        }
        for i in n..self.size {
            self.fft_buffer[i] = Complex32::new(0.0, 0.0);
        }

        self.fft.process(&mut self.fft_buffer);

        let half = self.size / 2;
        let inv_size = 1.0 / (self.size as f32);

        for i in 0..half {
            let val = self.fft_buffer[i];
            let mag = (val.re * val.re + val.im * val.im).sqrt() * inv_size;
            let db = 20.0 * (mag.max(1e-6)).log10();

            self.smooth_db[i] = self.smooth_factor * self.smooth_db[i] + (1.0 - self.smooth_factor) * db;
            self.output_db[i] = self.smooth_db[i];
        }

        &self.output_db[0..half]
    }

    pub fn size(&self) -> usize {
        self.size
    }
}
