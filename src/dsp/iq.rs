use num_complex::Complex32;
use crate::state::IqCalibration;

pub struct IqCorrector {
    // DC offset filter states
    last_in_i: f32,
    last_in_q: f32,
    last_out_i: f32,
    last_out_q: f32,
    alpha: f32,
}

impl IqCorrector {
    pub fn new(sample_rate: f32) -> Self {
        // High-pass filter with cutoff around 5 Hz for DC removal
        let fc = 5.0;
        let alpha = 1.0 - (2.0 * std::f32::consts::PI * fc / sample_rate);
        Self {
            last_in_i: 0.0,
            last_in_q: 0.0,
            last_out_i: 0.0,
            last_out_q: 0.0,
            alpha: alpha.clamp(0.99, 0.9999),
        }
    }

    #[inline]
    pub fn process(&mut self, mut i: f32, mut q: f32, cal: &IqCalibration) -> Complex32 {
        if cal.swap_iq {
            std::mem::swap(&mut i, &mut q);
        }

        // DC offset removal
        let (dc_i, dc_q) = if cal.dc_removal {
            let out_i = i - self.last_in_i + self.alpha * self.last_out_i;
            let out_q = q - self.last_in_q + self.alpha * self.last_out_q;

            self.last_in_i = i;
            self.last_in_q = q;
            self.last_out_i = out_i;
            self.last_out_q = out_q;

            (out_i, out_q)
        } else {
            (i, q)
        };

        // Gain balance
        let gain_i = 1.0 + 0.5 * cal.gain_balance;
        let gain_q = 1.0 - 0.5 * cal.gain_balance;

        let corr_i = dc_i * gain_i;
        let pre_q = dc_q * gain_q;

        // Phase balance
        let sin_p = cal.phase_balance.sin();
        let cos_p = cal.phase_balance.cos().max(1e-5);
        let corr_q = (pre_q + corr_i * sin_p) / cos_p;

        Complex32::new(corr_i, corr_q)
    }

    pub fn reset(&mut self) {
        self.last_in_i = 0.0;
        self.last_in_q = 0.0;
        self.last_out_i = 0.0;
        self.last_out_q = 0.0;
    }
}
