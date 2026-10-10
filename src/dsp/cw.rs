use std::f32::consts::PI;

pub const WPM_UPDATE_INTERVAL_MS: u32 = 20_000; // 20 seconds (15-30s window)

pub struct CwDecoder {
    sample_rate: f32,

    // 2nd-order Biquad Bandpass Filter (Direct Form I)
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,

    // 48:1 Decimator accumulator (down to 1 kHz)
    accum_abs: f32,
    accum_count: usize,

    // Envelope and adaptive thresholds (at 1 kHz)
    pub envelope: f32,
    pub noise_floor: f32,
    pub signal_peak: f32,
    pub key_down: bool,
    pub is_tone_active: bool,
    pub current_state_duration_ms: u32,

    // Morse timing: stable locked active speed
    pub active_wpm: u32,
    pub active_dot_len_ms: f32,
    pub detected_wpm: u32,

    // 15-30 second WPM statistical accumulator
    eval_timer_ms: u32,
    wpm_accum_dits_sum: f32,
    wpm_accum_dits_count: u32,
    wpm_accum_dahs_sum: f32,
    wpm_accum_dahs_count: u32,

    // Morse symbol assembly and dispatch flags
    current_symbol: String,
    symbol_dispatched: bool,
    word_space_dispatched: bool,
}

impl CwDecoder {
    pub fn new(sample_rate: f32) -> Self {
        let initial_wpm = 20;
        let initial_dot_ms = 1200.0 / initial_wpm as f32; // 60.0 ms

        let mut decoder = Self {
            sample_rate,
            b0: 0.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,

            accum_abs: 0.0,
            accum_count: 0,

            envelope: 0.0,
            noise_floor: 0.005,
            signal_peak: 0.05,
            key_down: false,
            is_tone_active: false,
            current_state_duration_ms: 0,

            active_wpm: initial_wpm,
            active_dot_len_ms: initial_dot_ms,
            detected_wpm: initial_wpm,

            eval_timer_ms: 0,
            wpm_accum_dits_sum: 0.0,
            wpm_accum_dits_count: 0,
            wpm_accum_dahs_sum: 0.0,
            wpm_accum_dahs_count: 0,

            current_symbol: String::with_capacity(8),
            symbol_dispatched: false,
            word_space_dispatched: false,
        };

        decoder.update_filter_freq(700.0);
        decoder
    }

    /// Set active WPM manually or from configuration
    pub fn set_wpm(&mut self, wpm: u32) {
        let clamped = wpm.clamp(6, 50);
        self.active_wpm = clamped;
        self.active_dot_len_ms = 1200.0 / clamped as f32;
        self.detected_wpm = clamped;
        self.wpm_accum_dits_sum = 0.0;
        self.wpm_accum_dits_count = 0;
        self.wpm_accum_dahs_sum = 0.0;
        self.wpm_accum_dahs_count = 0;
        self.eval_timer_ms = 0;
    }

    /// Reset decoder transient state when disabled or switching modes
    pub fn reset(&mut self) {
        self.accum_abs = 0.0;
        self.accum_count = 0;
        self.envelope = 0.0;
        self.key_down = false;
        self.is_tone_active = false;
        self.current_state_duration_ms = 0;
        self.current_symbol.clear();
        self.symbol_dispatched = false;
        self.word_space_dispatched = false;
        self.wpm_accum_dits_sum = 0.0;
        self.wpm_accum_dits_count = 0;
        self.wpm_accum_dahs_sum = 0.0;
        self.wpm_accum_dahs_count = 0;
        self.eval_timer_ms = 0;
    }

    /// Recompute Biquad Bandpass filter coefficients for target CW pitch (e.g. 700 Hz)
    pub fn update_filter_freq(&mut self, pitch_hz: f32) {
        let f0 = pitch_hz.clamp(300.0, 1500.0);
        let q = 4.0f32; // Q = 4.0 gives ~175 Hz bandwidth at 700 Hz
        let w0 = 2.0 * PI * f0 / self.sample_rate;
        let alpha = w0.sin() / (2.0 * q);

        let b0 = alpha;
        let b1 = 0.0;
        let b2 = -alpha;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * w0.cos();
        let a2 = 1.0 - alpha;

        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    /// Process a single demodulated audio sample (48 kHz).
    /// Returns Some(char) when a letter or space is decoded.
    #[inline]
    pub fn process_sample(&mut self, x: f32) -> Option<char> {
        // Direct Form I Biquad bandpass filter
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1 - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;

        // Decimate 48:1 (down to 1 kHz / 1 ms ticks)
        self.accum_abs += y.abs();
        self.accum_count += 1;

        if self.accum_count >= 48 {
            let env_ms = self.accum_abs / 48.0;
            self.accum_abs = 0.0;
            self.accum_count = 0;
            self.process_1ms_tick(env_ms)
        } else {
            None
        }
    }

    /// 1 ms tick processing
    pub fn process_1ms_tick(&mut self, env_ms: f32) -> Option<char> {
        // Leaky integration of envelope (tau ~ 8 ms)
        self.envelope = 0.88 * self.envelope + 0.12 * env_ms;

        // Adaptive tracking of noise floor and signal peak
        if self.envelope < self.noise_floor {
            self.noise_floor = 0.995 * self.noise_floor + 0.005 * self.envelope;
        } else {
            self.noise_floor = (self.noise_floor * 1.0003).min(0.5);
        }

        if self.envelope > self.signal_peak {
            self.signal_peak = 0.95 * self.signal_peak + 0.05 * self.envelope;
        } else {
            self.signal_peak = (self.signal_peak * 0.9995).max(self.noise_floor * 1.4 + 0.005);
        }

        let dynamic_range = self.signal_peak - self.noise_floor;
        let is_signal_valid = dynamic_range > 0.008 && self.signal_peak > self.noise_floor * 1.3;

        let th_high = self.noise_floor + 0.45 * dynamic_range;
        let th_low = self.noise_floor + 0.25 * dynamic_range;

        self.current_state_duration_ms = self.current_state_duration_ms.saturating_add(1);

        // 1. Advance the 15-30 second WPM evaluation window
        self.eval_timer_ms = self.eval_timer_ms.saturating_add(1);
        if self.eval_timer_ms >= WPM_UPDATE_INTERVAL_MS {
            self.eval_timer_ms = 0;
            let total_marks = self.wpm_accum_dits_count + self.wpm_accum_dahs_count;
            if total_marks >= 8 {
                let dits_norm_sum = self.wpm_accum_dits_sum + (self.wpm_accum_dahs_sum / 3.0);
                let measured_dot_ms = dits_norm_sum / (total_marks as f32);
                if measured_dot_ms >= 25.0 && measured_dot_ms <= 180.0 {
                    let candidate_wpm = (1200.0 / measured_dot_ms).round().clamp(6.0, 50.0) as u32;
                    self.active_wpm = candidate_wpm;
                    self.active_dot_len_ms = 1200.0 / self.active_wpm as f32;
                    self.detected_wpm = self.active_wpm;
                }
            }
            // Reset statistical accumulators for the next 20s evaluation window
            self.wpm_accum_dits_sum = 0.0;
            self.wpm_accum_dits_count = 0;
            self.wpm_accum_dahs_sum = 0.0;
            self.wpm_accum_dahs_count = 0;
        }

        let mut decoded_char = None;

        // 2. Mark / Space transition state machine
        if is_signal_valid {
            if !self.key_down {
                if self.envelope > th_high && self.current_state_duration_ms >= 8 {
                    // Transition to key-down (start of mark)
                    self.key_down = true;
                    self.is_tone_active = true;
                    self.current_state_duration_ms = 0;
                    self.symbol_dispatched = false;
                    self.word_space_dispatched = false;
                }
            } else {
                if self.envelope < th_low && self.current_state_duration_ms >= 8 {
                    // Transition to key-up: mark ended
                    self.key_down = false;
                    self.is_tone_active = false;
                    let mark_dur = self.current_state_duration_ms;
                    self.current_state_duration_ms = 0;

                    let dot_len = self.active_dot_len_ms;
                    let min_mark = (0.30 * dot_len).max(12.0) as u32;
                    let max_mark = (5.0 * dot_len) as u32;

                    if mark_dur >= min_mark && mark_dur <= max_mark {
                        let split = 2.0 * dot_len;
                        if (mark_dur as f32) < split {
                            // Dit '.'
                            self.current_symbol.push('.');
                            self.wpm_accum_dits_sum += mark_dur as f32;
                            self.wpm_accum_dits_count += 1;
                        } else {
                            // Dah '-'
                            self.current_symbol.push('-');
                            self.wpm_accum_dahs_sum += mark_dur as f32;
                            self.wpm_accum_dahs_count += 1;
                        }
                    }
                }
            }
        } else {
            // Signal invalid / pure noise: release key if stuck
            if self.key_down && self.current_state_duration_ms >= 20 {
                self.key_down = false;
                self.is_tone_active = false;
                self.current_state_duration_ms = 0;
            }
        }

        // 3. When key is up, check for character pause and word pause using locked active_dot_len_ms
        if !self.key_down {
            let char_space_thresh = (2.0 * self.active_dot_len_ms).round() as u32;
            let word_space_thresh = (5.0 * self.active_dot_len_ms).round() as u32;

            if !self.current_symbol.is_empty()
                && self.current_state_duration_ms >= char_space_thresh
                && !self.symbol_dispatched
            {
                let ch = decode_morse(&self.current_symbol);
                self.current_symbol.clear();
                self.symbol_dispatched = true;
                decoded_char = Some(ch);
            } else if self.current_state_duration_ms >= word_space_thresh
                && !self.word_space_dispatched
            {
                self.word_space_dispatched = true;
                decoded_char = Some(' ');
            }
        }

        decoded_char
    }

    /// Helper for feeding mark/space durations directly (e.g. for testing)
    #[cfg(test)]
    pub fn feed_mark_space(&mut self, mark_ms: u32, space_ms: u32) -> Vec<char> {
        let mut out = Vec::new();
        for _ in 0..mark_ms {
            if let Some(c) = self.process_1ms_tick(0.2) {
                out.push(c);
            }
        }
        for _ in 0..space_ms {
            if let Some(c) = self.process_1ms_tick(0.001) {
                out.push(c);
            }
        }
        out
    }
}

pub fn decode_morse(code: &str) -> char {
    match code {
        ".-" => 'A',
        "-..." => 'B',
        "-.-." => 'C',
        "-.." => 'D',
        "." => 'E',
        "..-." => 'F',
        "--." => 'G',
        "...." => 'H',
        ".." => 'I',
        ".---" => 'J',
        "-.-" => 'K',
        ".-.." => 'L',
        "--" => 'M',
        "-." => 'N',
        "---" => 'O',
        ".--." => 'P',
        "--.-" => 'Q',
        ".-." => 'R',
        "..." => 'S',
        "-" => 'T',
        "..-" => 'U',
        "...-" => 'V',
        ".--" => 'W',
        "-..-" => 'X',
        "-.--" => 'Y',
        "--.." => 'Z',
        "-----" => '0',
        ".----" => '1',
        "..---" => '2',
        "...--" => '3',
        "....-" => '4',
        "....." => '5',
        "-...." => '6',
        "--..." => '7',
        "---.." => '8',
        "----." => '9',
        ".-.-.-" => '.',
        "--..--" => ',',
        "..--.." => '?',
        "-..-." => '/',
        "-...-" => '=',
        "-....-" => '-',
        ".--.-." => '@',
        "---..." => ':',
        ".-.-." => '+',
        "-.-.--" => '!',
        "...-.-" => '*', // <SK>
        "-.--." => '(',  // <KN>
        _ => '?',
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_morse_alphabet_decoding() {
        assert_eq!(decode_morse(".-"), 'A');
        assert_eq!(decode_morse("-..."), 'B');
        assert_eq!(decode_morse("-.-."), 'C');
        assert_eq!(decode_morse("..."), 'S');
        assert_eq!(decode_morse("---"), 'O');
        assert_eq!(decode_morse(".----"), '1');
        assert_eq!(decode_morse("?"), '?');
    }

    #[test]
    fn test_cw_decoder_sequence() {
        let mut decoder = CwDecoder::new(48000.0);
        decoder.set_wpm(20); // 20 WPM (T_dit = 60ms)
        decoder.noise_floor = 0.005;
        decoder.signal_peak = 0.25;

        // Send 'E' (single dit 60ms, then space 160ms)
        let chars = decoder.feed_mark_space(60, 160);
        assert_eq!(chars, vec!['E']);

        // Send 'A' (dit 60ms, element space 60ms, dah 180ms, char space 160ms)
        let mut a_chars = Vec::new();
        a_chars.extend(decoder.feed_mark_space(60, 60));
        a_chars.extend(decoder.feed_mark_space(180, 160));
        assert_eq!(a_chars, vec!['A']);

        // Verify active WPM remained constant at 20 WPM during the sequence
        assert_eq!(decoder.active_wpm, 20);
    }

    #[test]
    fn test_wpm_evaluation_window_20s() {
        let mut decoder = CwDecoder::new(48000.0);
        decoder.set_wpm(20); // Starts at 20 WPM
        decoder.noise_floor = 0.005;
        decoder.signal_peak = 0.25;

        // Send a series of 15 WPM dits (80ms) and dahs (240ms)
        for _ in 0..6 {
            decoder.feed_mark_space(80, 80);  // 15 WPM Dit
            decoder.feed_mark_space(240, 80); // 15 WPM Dah
        }

        // Before 20 seconds expires, active_wpm is still 20!
        assert_eq!(decoder.active_wpm, 20);

        // Advance remainder of 20 seconds (20,000 ms)
        for _ in 0..19_000 {
            decoder.process_1ms_tick(0.002);
        }

        // After the 20s window, WPM evaluates to ~15 WPM (within ±1 WPM due to envelope rise time)
        assert!((decoder.active_wpm as i32 - 15).abs() <= 1);
    }
}
