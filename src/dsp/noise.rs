use num_complex::Complex32;

/// Impulse Noise Blanker (NB)
///
/// Detects fast impulsive spikes (e.g., automotive ignition pulses, electric fences,
/// powerline arcing) on the raw 96 kHz baseband I/Q stream and blanks them before
/// they can ring the narrow IF filtering stages.
pub struct NoiseBlanker {
    avg_mag: f32,
    blank_counter: usize,
    last_clean: Complex32,
}

impl NoiseBlanker {
    pub fn new() -> Self {
        Self {
            avg_mag: 0.001,
            blank_counter: 0,
            last_clean: Complex32::new(0.0, 0.0),
        }
    }

    pub fn process(&mut self, sample: Complex32, threshold_mult: f32) -> Complex32 {
        let mag = (sample.re * sample.re + sample.im * sample.im).sqrt();

        // Exponential moving average tracking of the ambient background envelope
        self.avg_mag += (mag - self.avg_mag) * 0.002;
        let threshold = (self.avg_mag * threshold_mult).max(0.002);

        if mag > threshold && threshold_mult > 0.5 {
            // Impulse detected! Blank for ~3 samples (~31 us at 96 kHz)
            self.blank_counter = 3;
        }

        if self.blank_counter > 0 {
            self.blank_counter -= 1;
            // Suppress the pulse spike
            self.last_clean * 0.05
        } else {
            self.last_clean = sample;
            sample
        }
    }

    pub fn reset(&mut self) {
        self.avg_mag = 0.001;
        self.blank_counter = 0;
        self.last_clean = Complex32::new(0.0, 0.0);
    }
}

/// Adaptive LMS Noise Reduction (NR)
///
/// Uses a Normalized Least Mean Squares (NLMS) adaptive FIR filter with a decorrelation
/// delay line. Correlated components (speech formants, CW tones) are passed while
/// uncorrelated broadband noise (white noise, atmospheric hiss) is attenuated.
pub struct LmsNoiseReduction {
    weights: Vec<f32>,
    delay_line: Vec<f32>,
    delay_idx: usize,
    taps: usize,
    delay: usize,
}

impl LmsNoiseReduction {
    pub fn new() -> Self {
        let taps = 48;
        let delay = 32;
        Self {
            weights: vec![0.0; taps],
            delay_line: vec![0.0; taps + delay + 1],
            delay_idx: 0,
            taps,
            delay,
        }
    }

    pub fn process(&mut self, input: f32, level: f32) -> f32 {
        let n_buf = self.delay_line.len();
        self.delay_line[self.delay_idx] = input;

        let mut y = 0.0f32;
        let mut power = 0.0001f32;

        for k in 0..self.taps {
            let idx = (self.delay_idx + n_buf - self.delay - k) % n_buf;
            let d_sample = self.delay_line[idx];
            y += self.weights[k] * d_sample;
            power += d_sample * d_sample;
        }

        let err = input - y;

        // NLMS adaptation step
        let mu = (0.015 * level.clamp(0.05, 1.0)) / power;
        for k in 0..self.taps {
            let idx = (self.delay_idx + n_buf - self.delay - k) % n_buf;
            let d_sample = self.delay_line[idx];
            self.weights[k] = (self.weights[k] + mu * err * d_sample).clamp(-2.0, 2.0);
        }

        self.delay_idx = (self.delay_idx + 1) % n_buf;

        // Smooth blend with original signal (retains natural audio timbre)
        let alpha = level.clamp(0.0, 1.0) * 0.85;
        input * (1.0 - alpha) + y * alpha
    }

    pub fn reset(&mut self) {
        self.weights.fill(0.0);
        self.delay_line.fill(0.0);
        self.delay_idx = 0;
    }
}

/// Automatic Notch Filter (ANF)
///
/// An adaptive LMS notch filter that automatically detects and suppresses continuous
/// carrier heterodynes and steady whistles (e.g. tuner carriers) without affecting voice.
pub struct AutoNotchFilter {
    weights: Vec<f32>,
    delay_line: Vec<f32>,
    delay_idx: usize,
    taps: usize,
    delay: usize,
}

impl AutoNotchFilter {
    pub fn new() -> Self {
        let taps = 32;
        let delay = 8;
        Self {
            weights: vec![0.0; taps],
            delay_line: vec![0.0; taps + delay + 1],
            delay_idx: 0,
            taps,
            delay,
        }
    }

    pub fn process(&mut self, input: f32) -> f32 {
        let n_buf = self.delay_line.len();
        self.delay_line[self.delay_idx] = input;

        let mut y = 0.0f32;
        let mut power = 0.0001f32;

        for k in 0..self.taps {
            let idx = (self.delay_idx + n_buf - self.delay - k) % n_buf;
            let d_sample = self.delay_line[idx];
            y += self.weights[k] * d_sample;
            power += d_sample * d_sample;
        }

        let notched = input - y;

        let mu = 0.02 / power;
        for k in 0..self.taps {
            let idx = (self.delay_idx + n_buf - self.delay - k) % n_buf;
            let d_sample = self.delay_line[idx];
            self.weights[k] = (self.weights[k] + mu * notched * d_sample).clamp(-2.0, 2.0);
        }

        self.delay_idx = (self.delay_idx + 1) % n_buf;
        notched
    }

    pub fn reset(&mut self) {
        self.weights.fill(0.0);
        self.delay_line.fill(0.0);
        self.delay_idx = 0;
    }
}

/// Smooth Audio Squelch Gate
///
/// Smoothly ramps gain up/down over ~10-15 ms to eliminate clicks when gating.
pub struct AudioSquelch {
    gain: f32,
}

impl AudioSquelch {
    pub fn new() -> Self {
        Self { gain: 1.0 }
    }

    pub fn process(&mut self, input: f32, current_dbfs: f32, threshold_dbfs: f32) -> f32 {
        let is_open = current_dbfs >= threshold_dbfs;
        let target = if is_open { 1.0 } else { 0.0 };
        // Smooth gain slew rate (~10 ms at 48 kHz)
        self.gain += (target - self.gain) * 0.005;

        input * self.gain
    }

    pub fn reset(&mut self) {
        self.gain = 1.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_noise_blanker_attenuates_spike() {
        let mut nb = NoiseBlanker::new();
        // Feed normal samples
        for _ in 0..500 {
            nb.process(Complex32::new(0.05, 0.05), 3.0);
        }
        // Sudden 20x spike
        let spike = nb.process(Complex32::new(1.0, 1.0), 3.0);
        let spike_mag = (spike.re * spike.re + spike.im * spike.im).sqrt();
        assert!(spike_mag < 0.2, "Spike was not blanked! Mag: {}", spike_mag);
    }

    #[test]
    fn test_lms_nr_stability() {
        let mut nr = LmsNoiseReduction::new();
        for i in 0..1000 {
            let sample = ((i as f32 * 0.1).sin()) * 0.2;
            let out = nr.process(sample, 0.8);
            assert!(!out.is_nan());
            assert!(!out.is_infinite());
        }
    }

    #[test]
    fn test_auto_notch_suppression() {
        let mut anf = AutoNotchFilter::new();
        let freq = 1000.0f32; // 1 kHz tone at 48 kHz sample rate
        let sample_rate = 48000.0f32;
        let mut last_amp = 1.0;

        for i in 0..3000 {
            let phase = 2.0 * std::f32::consts::PI * freq * (i as f32) / sample_rate;
            let tone = phase.sin() * 0.5;
            let notched = anf.process(tone);
            if i > 2500 {
                last_amp = notched.abs().max(last_amp * 0.99);
            }
        }

        assert!(last_amp < 0.25, "Auto notch failed to suppress 1 kHz tone! Amp: {}", last_amp);
    }

    #[test]
    fn test_squelch_gating() {
        let mut sql = AudioSquelch::new();
        // Below threshold: should ramp down towards 0
        for _ in 0..1500 {
            let _ = sql.process(0.5, -90.0, -70.0);
        }
        let out = sql.process(0.5, -90.0, -70.0);
        assert!(out.abs() < 0.01, "Squelch failed to mute: {}", out);

        // Above threshold: should ramp back up
        for _ in 0..1500 {
            let _ = sql.process(0.5, -60.0, -70.0);
        }
        let out = sql.process(0.5, -60.0, -70.0);
        assert!(out > 0.45, "Squelch failed to open: {}", out);
    }
}
