pub mod iq;
pub mod nco;
pub mod filter;
pub mod demod;
pub mod agc;
pub mod fft;
pub mod cw;

use num_complex::Complex32;
use crate::state::{RadioMode, SdrState};
use iq::IqCorrector;
use nco::Nco;
use filter::ComplexFirFilter;
use demod::Demodulator;
use agc::Agc;
use fft::SpectrumFft;
use cw::CwDecoder;

pub const RF_FFT_SIZE: usize = 2048;
pub const AF_FFT_SIZE: usize = 1024;

pub struct DspPipeline {
    pub iq_corrector: IqCorrector,
    pub nco: Nco,
    pub fir_filter: ComplexFirFilter,
    pub demodulator: Demodulator,
    pub agc: Agc,
    pub rf_fft: SpectrumFft,
    pub af_fft: SpectrumFft,
    pub cw_decoder: CwDecoder,
    pub cw_chars_buffer: Vec<char>,

    // Buffers for FFTs
    rf_fft_accumulator: Vec<Complex32>,
    af_fft_accumulator: Vec<f32>,

    // Current filter parameters tracking
    last_filter_low: f32,
    last_filter_high: f32,
    last_mode: Option<RadioMode>,

    // S-meter and power levels
    pub latest_s_meter: (f32, f32, f32), // (s_units, over_s9_db, dbfs)
    rms_accumulator: f32,
    rms_count: usize,
}

impl DspPipeline {
    pub fn new(input_sample_rate: f32) -> Self {
        let decimate_rate = input_sample_rate / 2.0; // 48 kHz
        Self {
            iq_corrector: IqCorrector::new(input_sample_rate),
            nco: Nco::new(input_sample_rate),
            fir_filter: ComplexFirFilter::new(127, input_sample_rate),
            demodulator: Demodulator::new(),
            agc: Agc::new(decimate_rate),
            rf_fft: SpectrumFft::new(RF_FFT_SIZE, 0.6),
            af_fft: SpectrumFft::new(AF_FFT_SIZE, 0.5),
            cw_decoder: CwDecoder::new(decimate_rate),
            cw_chars_buffer: Vec::new(),

            rf_fft_accumulator: Vec::with_capacity(RF_FFT_SIZE),
            af_fft_accumulator: Vec::with_capacity(AF_FFT_SIZE),

            last_filter_low: 0.0,
            last_filter_high: 0.0,
            last_mode: None,

            latest_s_meter: (0.0, 0.0, -120.0),
            rms_accumulator: 0.0,
            rms_count: 0,
        }
    }

    /// Update NCO and filter edges if frequencies or modes have changed
    pub fn sync_state(&mut self, state: &SdrState) {
        let freq_offset = state.tune_freq as f32 - state.lo_freq as f32;
        self.nco.set_freq_offset(freq_offset);

        if (state.filter_low - self.last_filter_low).abs() > 1.0
            || (state.filter_high - self.last_filter_high).abs() > 1.0
        {
            self.fir_filter.design_bandpass(state.filter_low, state.filter_high);
            self.last_filter_low = state.filter_low;
            self.last_filter_high = state.filter_high;

            let cw_pitch = ((state.filter_low.abs() + state.filter_high.abs()) * 0.5).clamp(300.0, 1500.0);
            self.cw_decoder.update_filter_freq(cw_pitch);
        }

        // If switched away from CW mode, flush decoder state and output buffer
        if self.last_mode != Some(state.mode) {
            if state.mode != RadioMode::CW {
                self.cw_decoder.reset();
                self.cw_chars_buffer.clear();
            }
            self.last_mode = Some(state.mode);
        }
    }

    /// Process a pair of interleaved stereo I/Q input samples [i0, q0, i1, q1] at 96 kHz.
    /// Returns:
    /// - An optional demodulated audio sample (f32) at 48 kHz (due to decimate-by-2).
    /// - An optional RF spectrum update if an FFT block was filled.
    /// - An optional AF spectrum update if an AF FFT block was filled.
    pub fn process_sample_pair(
        &mut self,
        i0: f32,
        q0: f32,
        i1: f32,
        q1: f32,
        state: &SdrState,
    ) -> (f32, Option<Vec<f32>>, Option<Vec<f32>>) {
        // Step 1: I/Q correction
        let c0 = self.iq_corrector.process(i0, q0, &state.iq_cal);
        let c1 = self.iq_corrector.process(i1, q1, &state.iq_cal);

        // Step 2: Accumulate for RF FFT
        let mut rf_spectrum = None;
        self.rf_fft_accumulator.push(c0);
        self.rf_fft_accumulator.push(c1);
        if self.rf_fft_accumulator.len() >= RF_FFT_SIZE {
            let spec = self.rf_fft.process_complex(&self.rf_fft_accumulator);
            rf_spectrum = Some(spec.to_vec());
            self.rf_fft_accumulator.clear();
        }

        // Step 3: NCO Frequency shift
        let shifted0 = self.nco.mix(c0);
        let shifted1 = self.nco.mix(c1);

        // Step 4: Complex FIR filter and decimate-by-2 (down to 48 kHz)
        let filtered = self.fir_filter.decimate2(shifted0, shifted1);

        // Step 5: S-meter power calculation on filtered baseband
        let mag_sq = filtered.re * filtered.re + filtered.im * filtered.im;
        self.rms_accumulator += mag_sq;
        self.rms_count += 1;
        if self.rms_count >= 1024 {
            let rms = (self.rms_accumulator / self.rms_count as f32).sqrt();
            self.latest_s_meter = Agc::calculate_s_meter(rms);
            self.rms_accumulator = 0.0;
            self.rms_count = 0;
        }

        // Squelch check
        let is_squelched = self.latest_s_meter.2 < state.squelch_db;

        // Step 6: Demodulation
        let raw_audio = if is_squelched {
            0.0
        } else {
            self.demodulator.demodulate(filtered, state.mode)
        };

        // Step 7: AGC
        let agc_audio = self.agc.process(raw_audio, state.agc_speed, state.agc_gain);
        let final_audio = (agc_audio * state.volume).clamp(-1.0, 1.0);

        // Step 8: AF FFT Accumulation (Audio spectrum 0 - 24 kHz, interest in 0 - 4 kHz)
        let mut af_spectrum = None;
        self.af_fft_accumulator.push(final_audio);
        if self.af_fft_accumulator.len() >= AF_FFT_SIZE {
            let spec = self.af_fft.process_real(&self.af_fft_accumulator);
            af_spectrum = Some(spec.to_vec());
            self.af_fft_accumulator.clear();
        }

        // Step 9: CW Morse Decoding on demodulated audio (only enabled in CW mode)
        if state.mode == RadioMode::CW {
            if let Some(ch) = self.cw_decoder.process_sample(final_audio) {
                self.cw_chars_buffer.push(ch);
            }
        }

        (final_audio, rf_spectrum, af_spectrum)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cw_gated_by_mode() {
        let mut dsp = DspPipeline::new(96000.0);
        let mut state = SdrState::default();

        // 1. In USB mode (default 40m is LSB, let's set USB)
        state.mode = RadioMode::USB;
        dsp.sync_state(&state);

        // Process samples with strong tone that would trigger CW
        for _ in 0..200 {
            dsp.process_sample_pair(0.5, 0.5, 0.5, 0.5, &state);
        }
        assert!(dsp.cw_chars_buffer.is_empty(), "CW chars buffer should be empty in USB mode");

        // 2. Switch to CW mode
        state.mode = RadioMode::CW;
        dsp.sync_state(&state);
        assert_eq!(dsp.last_mode, Some(RadioMode::CW));

        // 3. Switch back to LSB mode
        state.mode = RadioMode::LSB;
        dsp.sync_state(&state);
        assert_eq!(dsp.last_mode, Some(RadioMode::LSB));
        assert!(dsp.cw_chars_buffer.is_empty());
    }
}
