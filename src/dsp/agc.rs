use crate::state::AgcSpeed;

pub struct Agc {
    sample_rate: f32,
    envelope: f32,
    gain: f32,
    target_level: f32,
    attack_alpha: f32,
    decay_fast: f32,
    decay_med: f32,
    decay_slow: f32,
    hang_counter: usize,
    hang_time_samples: usize,
}

impl Agc {
    pub fn new(sample_rate: f32) -> Self {
        // Attack time ~ 2 ms
        let attack_time = 0.002;
        let attack_alpha = 1.0 - (-1.0 / (attack_time * sample_rate)).exp();

        // Decay times
        let decay_fast = 1.0 - (-1.0 / (0.050 * sample_rate)).exp(); // 50ms
        let decay_med = 1.0 - (-1.0 / (0.300 * sample_rate)).exp();  // 300ms
        let decay_slow = 1.0 - (-1.0 / (1.200 * sample_rate)).exp(); // 1200ms

        let hang_time_samples = (0.150 * sample_rate) as usize; // 150ms hang

        Self {
            sample_rate,
            envelope: 0.0001,
            gain: 50.0,
            target_level: 0.35, // Desired output audio RMS level
            attack_alpha,
            decay_fast,
            decay_med,
            decay_slow,
            hang_counter: 0,
            hang_time_samples,
        }
    }

    #[inline]
    pub fn process(&mut self, audio: f32, speed: AgcSpeed, manual_gain: f32) -> f32 {
        if speed == AgcSpeed::Off {
            return audio * manual_gain * 50.0;
        }

        let abs_sample = audio.abs();

        if abs_sample > self.envelope {
            // Fast attack
            self.envelope += self.attack_alpha * (abs_sample - self.envelope);
            self.hang_counter = self.hang_time_samples;
        } else if self.hang_counter > 0 {
            // Hang period
            self.hang_counter -= 1;
        } else {
            // Decay
            let decay_alpha = match speed {
                AgcSpeed::Fast => self.decay_fast,
                AgcSpeed::Med => self.decay_med,
                AgcSpeed::Slow => self.decay_slow,
                AgcSpeed::Off => 0.0,
            };
            self.envelope += decay_alpha * (abs_sample - self.envelope);
        }

        self.envelope = self.envelope.clamp(1e-6, 5.0);

        // Allow up to 5000x gain for weak signals / noise floor to bring to audible level
        let target_gain = (self.target_level / self.envelope).clamp(0.1, 5000.0);
        // Smooth gain change
        self.gain += 0.05 * (target_gain - self.gain);

        audio * self.gain
    }

    /// Calculates S-meter reading from complex baseband signal RMS power.
    /// Returns (s_units: 0.0 to 9.0, over_s9_db: 0.0 to 80.0, dbfs: f32)
    pub fn calculate_s_meter(rms_level: f32) -> (f32, f32, f32) {
        let dbfs = 20.0 * (rms_level.max(1e-7)).log10();
        // Calibrated: S9 (50 uV) is at -35.0 dBFS, so HF noise floor around -70 dBFS reads S2-S3.
        let db_rel_s9 = dbfs - (-35.0);

        if db_rel_s9 <= 0.0 {
            let s_unit = ((db_rel_s9 + 54.0) / 6.0).clamp(0.0, 9.0);
            (s_unit, 0.0, dbfs)
        } else {
            let over_db = db_rel_s9.clamp(0.0, 80.0);
            (9.0, over_db, dbfs)
        }
    }
}
