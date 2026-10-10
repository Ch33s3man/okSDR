use num_complex::Complex32;
use std::f32::consts::PI;

pub struct ComplexFirFilter {
    taps: Vec<Complex32>,
    buffer: Vec<Complex32>,
    buf_idx: usize,
    num_taps: usize,
    sample_rate: f32,
}

impl ComplexFirFilter {
    pub fn new(num_taps: usize, sample_rate: f32) -> Self {
        // Ensure odd number of taps for symmetric FIR
        let taps_count = if num_taps % 2 == 0 { num_taps + 1 } else { num_taps };
        let mut filter = Self {
            taps: vec![Complex32::new(0.0, 0.0); taps_count],
            buffer: vec![Complex32::new(0.0, 0.0); taps_count],
            buf_idx: 0,
            num_taps: taps_count,
            sample_rate,
        };
        // Default to 2.4 kHz USB filter
        filter.design_bandpass(200.0, 2800.0);
        filter
    }

    /// Designs a complex bandpass filter with low_cut and high_cut in Hz relative to carrier.
    pub fn design_bandpass(&mut self, low_cut: f32, high_cut: f32) {
        let (f_min, f_max) = if low_cut <= high_cut {
            (low_cut, high_cut)
        } else {
            (high_cut, low_cut)
        };

        let bw = (f_max - f_min).max(100.0);
        let f_center = (f_min + f_max) * 0.5;
        let fc_lp = bw * 0.5; // lowpass cutoff

        let m = (self.num_taps - 1) as f32;
        let half_m = m * 0.5;
        let fs = self.sample_rate;

        let mut sum_real = 0.0f32;

        for i in 0..self.num_taps {
            let n = i as f32 - half_m;

            // Sinc function
            let sinc = if n.abs() < 1e-6 {
                2.0 * fc_lp / fs
            } else {
                (2.0 * PI * fc_lp * n / fs).sin() / (PI * n)
            };

            // Blackman window
            let a0 = 0.42;
            let a1 = 0.5;
            let a2 = 0.08;
            let window = a0 - a1 * (2.0 * PI * i as f32 / m).cos() + a2 * (4.0 * PI * i as f32 / m).cos();

            let lp_val = sinc * window;
            sum_real += lp_val;

            // Shift lowpass filter to f_center
            let theta = 2.0 * PI * f_center * n / fs;
            let shift = Complex32::new(theta.cos(), theta.sin());

            self.taps[i] = shift * lp_val;
        }

        // Normalize gain so passband gain is 1.0
        if sum_real > 1e-6 {
            let inv_sum = 1.0 / sum_real;
            for tap in &mut self.taps {
                *tap *= inv_sum;
            }
        }
    }

    #[inline]
    pub fn push_sample(&mut self, sample: Complex32) {
        self.buffer[self.buf_idx] = sample;
        self.buf_idx = (self.buf_idx + 1) % self.num_taps;
    }

    #[inline]
    pub fn filter_current(&self) -> Complex32 {
        let mut acc = Complex32::new(0.0, 0.0);
        let n = self.num_taps;
        let idx = self.buf_idx;

        // Multiply-accumulate oldest to newest
        for i in 0..n {
            let buf_pos = (idx + i) % n;
            acc += self.buffer[buf_pos] * self.taps[n - 1 - i];
        }

        acc
    }

    /// Processes 2 input samples at 96 kHz and returns 1 decimated sample at 48 kHz.
    #[inline]
    pub fn decimate2(&mut self, s0: Complex32, s1: Complex32) -> Complex32 {
        self.push_sample(s0);
        self.push_sample(s1);
        self.filter_current()
    }
}
