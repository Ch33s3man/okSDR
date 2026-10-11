use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RadioMode {
    USB,
    LSB,
    CW,
    AM,
    FM,
}

impl RadioMode {
    pub fn name(&self) -> &'static str {
        match self {
            RadioMode::USB => "USB",
            RadioMode::LSB => "LSB",
            RadioMode::CW => "CW",
            RadioMode::AM => "AM",
            RadioMode::FM => "FM",
        }
    }

    pub fn default_filter_edges(&self) -> (f32, f32) {
        match self {
            RadioMode::USB => (100.0, 2500.0),
            RadioMode::LSB => (-2500.0, -100.0),
            RadioMode::CW => (450.0, 950.0),
            RadioMode::AM => (-3000.0, 3000.0),
            RadioMode::FM => (-6000.0, 6000.0),
        }
    }

    pub fn default_filter_edges_with_pitch(&self, cw_pitch: f32) -> (f32, f32) {
        match self {
            RadioMode::CW => {
                let pitch = cw_pitch.clamp(300.0, 1500.0);
                ((pitch - 250.0).max(50.0), pitch + 250.0)
            }
            _ => self.default_filter_edges(),
        }
    }

    pub fn is_ssb(&self) -> bool {
        matches!(self, RadioMode::USB | RadioMode::LSB)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgcSpeed {
    Off,
    Slow,
    Med,
    Fast,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Band {
    M160,
    M80,
    M60,
    M40,
    M30,
    M20,
    M17,
    M15,
    M12,
    M10,
}

impl Band {
    pub const ALL: [Band; 10] = [
        Band::M160,
        Band::M80,
        Band::M60,
        Band::M40,
        Band::M30,
        Band::M20,
        Band::M17,
        Band::M15,
        Band::M12,
        Band::M10,
    ];

    /// Bands authorized for SSB phone operation (excludes 30m which prohibits voice)
    pub const SSB_BANDS: [Band; 9] = [
        Band::M160,
        Band::M80,
        Band::M60,
        Band::M40,
        Band::M20,
        Band::M17,
        Band::M15,
        Band::M12,
        Band::M10,
    ];

    pub fn name(&self) -> &'static str {
        match self {
            Band::M160 => "160m",
            Band::M80 => "80m",
            Band::M60 => "60m",
            Band::M40 => "40m",
            Band::M30 => "30m",
            Band::M20 => "20m",
            Band::M17 => "17m",
            Band::M15 => "15m",
            Band::M12 => "12m",
            Band::M10 => "10m",
        }
    }

    /// Widely agreed upon SSB calling frequency for each band
    pub fn ssb_calling_freq(&self) -> u64 {
        match self {
            Band::M160 => 1_910_000, // 1.910 MHz LSB (US Calling / QRP)
            Band::M80 => 3_985_000,  // 3.985 MHz LSB (75m Phone Calling)
            Band::M60 => 5_330_500,  // 5.3305 MHz USB dial (Channel 1 Calling)
            Band::M40 => 7_185_000,  // 7.185 MHz LSB (40m Phone Calling)
            Band::M30 => 10_136_000, // 30m no SSB (FT8 / Digital only)
            Band::M20 => 14_285_000, // 14.285 MHz USB (20m Phone Calling)
            Band::M17 => 18_150_000, // 18.150 MHz USB (17m Phone Calling)
            Band::M15 => 21_385_000, // 21.385 MHz USB (15m Phone Calling)
            Band::M12 => 24_950_000, // 24.950 MHz USB (12m Phone Calling)
            Band::M10 => 28_400_000, // 28.400 MHz USB (10m Phone Calling)
        }
    }

    pub fn default_freq(&self) -> u64 {
        self.ssb_calling_freq()
    }

    pub fn default_mode(&self) -> RadioMode {
        match self {
            Band::M160 | Band::M80 | Band::M40 => RadioMode::LSB,
            _ => RadioMode::USB,
        }
    }

    pub fn from_freq(freq: u64) -> Option<Band> {
        match freq {
            1_800_000..=2_000_000 => Some(Band::M160),
            3_500_000..=4_000_000 => Some(Band::M80),
            5_250_000..=5_450_000 => Some(Band::M60),
            7_000_000..=7_300_000 => Some(Band::M40),
            10_100_000..=10_150_000 => Some(Band::M30),
            14_000_000..=14_350_000 => Some(Band::M20),
            18_068_000..=18_168_000 => Some(Band::M17),
            21_000_000..=21_450_000 => Some(Band::M15),
            24_890_000..=24_990_000 => Some(Band::M12),
            28_000_000..=29_700_000 => Some(Band::M10),
            _ => None,
        }
    }
}

pub struct ActivitySpot {
    pub name: &'static str,
    pub freq: u64,
    pub mode: RadioMode,
}

pub const ACTIVITY_SPOTS: &[ActivitySpot] = &[
    ActivitySpot { name: "160m LSB Call", freq: 1_910_000, mode: RadioMode::LSB },
    ActivitySpot { name: "80m CW Call", freq: 3_560_000, mode: RadioMode::CW },
    ActivitySpot { name: "80m LSB Call", freq: 3_985_000, mode: RadioMode::LSB },
    ActivitySpot { name: "60m Ch1 USB", freq: 5_330_500, mode: RadioMode::USB },
    ActivitySpot { name: "40m CW Call", freq: 7_030_000, mode: RadioMode::CW },
    ActivitySpot { name: "40m LSB Call", freq: 7_185_000, mode: RadioMode::LSB },
    ActivitySpot { name: "20m CW Call", freq: 14_060_000, mode: RadioMode::CW },
    ActivitySpot { name: "20m USB Call", freq: 14_285_000, mode: RadioMode::USB },
    ActivitySpot { name: "17m USB Call", freq: 18_150_000, mode: RadioMode::USB },
    ActivitySpot { name: "15m CW Call", freq: 21_060_000, mode: RadioMode::CW },
    ActivitySpot { name: "15m USB Call", freq: 21_385_000, mode: RadioMode::USB },
    ActivitySpot { name: "12m USB Call", freq: 24_950_000, mode: RadioMode::USB },
    ActivitySpot { name: "10m CW Call", freq: 28_060_000, mode: RadioMode::CW },
    ActivitySpot { name: "10m USB Call", freq: 28_400_000, mode: RadioMode::USB },
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IqCalibration {
    pub gain_balance: f32, // -0.2 to +0.2 (1.0 = equal)
    pub phase_balance: f32, // in radians (-0.2 to +0.2)
    pub swap_iq: bool,
    pub dc_removal: bool,
}

impl Default for IqCalibration {
    fn default() -> Self {
        Self {
            gain_balance: 0.0,
            phase_balance: 0.0,
            swap_iq: true, // Xiegu G90 rear 3.5mm I/Q outputs Q on Tip and I on Ring
            dc_removal: true,
        }
    }
}

/// Runtime status of the CAT link (never saved to disk).
#[derive(Debug, Clone, PartialEq, Default)]
pub enum RigStatus {
    /// No CAT connection requested (startup, or user chose SDR-only).
    #[default]
    Idle,
    /// Connection in progress; the string describes the current step.
    Connecting(String),
    /// rigctld is running and the G90 is answering frequency queries.
    Connected,
    /// A connection attempt failed; the string explains why.
    Failed(String),
    /// A working connection stopped responding.
    Lost(String),
}

/// Xiegu G90 Hamlib model number (`rigctl -l | grep G90`).
pub const HAMLIB_MODEL_G90: u32 = 3088;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NoiseControls {
    pub nr_enabled: bool,
    pub nr_level: f32, // 0.0 to 1.0 (default 0.5)
    pub nb_enabled: bool,
    pub nb_threshold: f32, // 1.5 to 8.0 (default 3.5)
    pub anf_enabled: bool, // Automatic Notch Filter
    pub squelch_enabled: bool,
}

impl Default for NoiseControls {
    fn default() -> Self {
        Self {
            nr_enabled: false,
            nr_level: 0.5,
            nb_enabled: false,
            nb_threshold: 3.5,
            anf_enabled: false,
            squelch_enabled: false,
        }
    }
}

pub fn default_mode_filters() -> HashMap<RadioMode, (f32, f32)> {
    let mut m = HashMap::new();
    m.insert(RadioMode::USB, RadioMode::USB.default_filter_edges());
    m.insert(RadioMode::LSB, RadioMode::LSB.default_filter_edges());
    m.insert(RadioMode::CW, RadioMode::CW.default_filter_edges());
    m.insert(RadioMode::AM, RadioMode::AM.default_filter_edges());
    m.insert(RadioMode::FM, RadioMode::FM.default_filter_edges());
    m
}

fn compare_mode_filters(
    a: &HashMap<RadioMode, (f32, f32)>,
    b: &HashMap<RadioMode, (f32, f32)>,
) -> bool {
    for m in &[RadioMode::USB, RadioMode::LSB, RadioMode::CW, RadioMode::AM, RadioMode::FM] {
        match (a.get(m), b.get(m)) {
            (Some((a_low, a_high)), Some((b_low, b_high))) => {
                if (a_low - b_low).abs() >= 1e-4 || (a_high - b_high).abs() >= 1e-4 {
                    return false;
                }
            }
            (None, None) => {}
            _ => return false,
        }
    }
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SdrState {
    pub lo_freq: u64,       // Radio Center Frequency (Hz)
    pub tune_freq: u64,     // Demodulated Frequency (Hz)
    pub mode: RadioMode,
    pub filter_low: f32,    // Hz offset from tune frequency
    pub filter_high: f32,   // Hz offset from tune frequency
    #[serde(default = "default_mode_filters")]
    pub mode_filters: HashMap<RadioMode, (f32, f32)>, // Saved filter edges per mode (low, high)
    pub volume: f32,        // 0.0 to 1.0
    pub squelch_db: f32,    // -130 dB to 0 dB
    pub noise: NoiseControls,
    pub agc_speed: AgcSpeed,
    pub agc_gain: f32,      // Manual gain factor if AGC Off
    pub iq_cal: IqCalibration,
    pub band_memories: HashMap<Band, u64>,
    pub rigctld_host: String,
    pub rigctld_port: u16,
    #[serde(skip)]
    pub rigctld_connected: bool,
    pub sample_rate: u32,   // typically 96000
    pub rf_preamp_db: f32,  // Digital RF preamp gain (-10.0 to +36.0 dB, default +18.0 dB)
    pub rf_waterfall_floor_db: f32, // Waterfall floor (-150 to -40 dB, default -120 dB)
    pub rf_waterfall_range_db: f32, // Waterfall range (10 to 80 dB, default 50 dB)
    pub rf_spectrum_ratio: f32, // Ratio of RF display for panadapter vs waterfall (default 0.45)
    pub rf_display_span_hz: f32, // Display frequency span/width in Hz (default 96000.0, or 0.0 for max)
    pub show_af_display: bool,  // Whether bottom AF spectrum & waterfall is visible (default true)
    pub af_panel_height: f32,   // Height of AF bottom panel in pixels (default 150.0)
    pub af_spectrum_ratio: f32, // Ratio of AF display for panadapter vs waterfall (default 0.45)
    pub cw_pitch_hz: f32,       // CW sidetone pitch & waterfall offset in Hz (default 700.0)
    pub recordings_dir: String, // Directory where RX audio recordings are saved
    pub audio_input_device: String, // Preferred audio capture device name or "Auto (Cubilux / Default)"
    pub serial_device: String,      // Preferred CAT serial device path or "Auto-Detect"
    pub cat_baud: u32,              // G90 CI-V baud rate (factory default 19200)
    pub civ_address: u8,            // G90 CI-V address (factory default 0x70)
    pub hamlib_model: u32,          // Hamlib rig model (3088 = Xiegu G90)
    pub cat_auto_connect: bool,     // Connect automatically on startup (dialog still shown on failure)
    #[serde(skip)]
    pub rig_status: RigStatus,
}

impl Default for SdrState {
    fn default() -> Self {
        let default_band = Band::M40;
        let mut band_memories = HashMap::new();
        for b in &Band::ALL {
            band_memories.insert(*b, b.default_freq());
        }

        let lo_freq = default_band.default_freq();
        let mode = default_band.default_mode();
        let (f_low, f_high) = mode.default_filter_edges();

        let mut mode_filters = default_mode_filters();
        mode_filters.insert(mode, (f_low, f_high));

        let default_rec_dir = if let Ok(home) = std::env::var("HOME") {
            format!("{}/okSDR_Recordings", home)
        } else {
            "recordings".to_string()
        };

        Self {
            lo_freq,
            tune_freq: lo_freq,
            mode,
            filter_low: f_low,
            filter_high: f_high,
            mode_filters,
            volume: 0.7,
            squelch_db: -130.0,
            noise: NoiseControls::default(),
            agc_speed: AgcSpeed::Med,
            agc_gain: 1.0,
            iq_cal: IqCalibration::default(),
            band_memories,
            rigctld_host: "127.0.0.1".to_string(),
            rigctld_port: 4532,
            rigctld_connected: false,
            sample_rate: 96000,
            rf_preamp_db: 18.0,
            rf_waterfall_floor_db: -120.0,
            rf_waterfall_range_db: 50.0,
            rf_spectrum_ratio: 0.45,
            rf_display_span_hz: 96000.0,
            show_af_display: true,
            af_panel_height: 150.0,
            af_spectrum_ratio: 0.45,
            cw_pitch_hz: 700.0,
            recordings_dir: default_rec_dir,
            audio_input_device: "Auto (Cubilux / Default)".to_string(),
            serial_device: "Auto-Detect".to_string(),
            cat_baud: 19200,
            civ_address: 0x70,
            hamlib_model: HAMLIB_MODEL_G90,
            cat_auto_connect: false,
            rig_status: RigStatus::Idle,
        }
    }
}

impl PartialEq for SdrState {
    fn eq(&self, other: &Self) -> bool {
        self.lo_freq == other.lo_freq
            && self.tune_freq == other.tune_freq
            && self.mode == other.mode
            && (self.filter_low - other.filter_low).abs() < 1e-4
            && (self.filter_high - other.filter_high).abs() < 1e-4
            && compare_mode_filters(&self.mode_filters, &other.mode_filters)
            && (self.volume - other.volume).abs() < 1e-4
            && (self.squelch_db - other.squelch_db).abs() < 1e-4
            && self.noise == other.noise
            && self.agc_speed == other.agc_speed
            && (self.agc_gain - other.agc_gain).abs() < 1e-4
            && self.iq_cal == other.iq_cal
            && self.band_memories == other.band_memories
            && self.rigctld_host == other.rigctld_host
            && self.rigctld_port == other.rigctld_port
            && self.sample_rate == other.sample_rate
            && (self.rf_preamp_db - other.rf_preamp_db).abs() < 1e-4
            && (self.rf_waterfall_floor_db - other.rf_waterfall_floor_db).abs() < 1e-4
            && (self.rf_waterfall_range_db - other.rf_waterfall_range_db).abs() < 1e-4
            && (self.rf_spectrum_ratio - other.rf_spectrum_ratio).abs() < 1e-4
            && (self.rf_display_span_hz - other.rf_display_span_hz).abs() < 1e-4
            && self.show_af_display == other.show_af_display
            && (self.af_panel_height - other.af_panel_height).abs() < 1e-4
            && (self.af_spectrum_ratio - other.af_spectrum_ratio).abs() < 1e-4
            && (self.cw_pitch_hz - other.cw_pitch_hz).abs() < 1e-4
            && self.recordings_dir == other.recordings_dir
            && self.audio_input_device == other.audio_input_device
            && self.serial_device == other.serial_device
            && self.cat_baud == other.cat_baud
            && self.civ_address == other.civ_address
            && self.hamlib_model == other.hamlib_model
            && self.cat_auto_connect == other.cat_auto_connect
    }
}

impl SdrState {
    pub fn config_path() -> std::path::PathBuf {
        if let Ok(home) = std::env::var("HOME") {
            let dir = std::path::PathBuf::from(home).join(".config").join("oksdr");
            if std::fs::create_dir_all(&dir).is_ok() {
                return dir.join("config.json");
            }
        }
        std::path::PathBuf::from("config.json")
    }

    pub fn load() -> Self {
        let home = std::env::var("HOME").ok();
        let new_config = home.as_ref().map(|h| std::path::PathBuf::from(h).join(".config").join("oksdr").join("config.json"));
        let old_config = home.as_ref().map(|h| std::path::PathBuf::from(h).join(".config").join("custom_sdr").join("config.json"));

        let candidates = [
            new_config,
            old_config,
            Some(std::path::PathBuf::from("config.json")),
        ];

        for opt_path in candidates {
            if let Some(path) = opt_path {
                if path.exists() {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        if let Ok(mut state) = serde_json::from_str::<Self>(&content) {
                            println!("[Config] Loaded previous session settings from {}", path.display());
                            state.rigctld_connected = false;
                            state.rig_status = RigStatus::Idle;
                            return state;
                        } else {
                            eprintln!("[Config] Warning: Failed to parse {}, trying next candidate.", path.display());
                        }
                    }
                }
            }
        }
        println!("[Config] Initialized with defaults (no previous config found).");
        Self::default()
    }

    pub fn save(&self) {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            if let Err(e) = std::fs::write(&path, &json) {
                eprintln!("[Config] Notice: Could not write to {}, saving to local config.json: {}", path.display(), e);
                let fallback = std::path::PathBuf::from("config.json");
                let _ = std::fs::write(&fallback, &json);
            } else {
                println!("[Config] Saved session settings to {}", path.display());
            }
        }
    }

    /// Get saved or default filter edges (low, high) for a specific mode.
    pub fn get_filter_for_mode(&self, mode: RadioMode) -> (f32, f32) {
        self.mode_filters
            .get(&mode)
            .copied()
            .unwrap_or_else(|| mode.default_filter_edges_with_pitch(self.cw_pitch_hz))
    }

    /// Store filter edges for a specific mode. If that mode is currently active,
    /// also updates filter_low and filter_high immediately.
    pub fn set_filter_for_mode(&mut self, mode: RadioMode, low: f32, high: f32) {
        self.mode_filters.insert(mode, (low, high));
        if self.mode == mode {
            self.filter_low = low;
            self.filter_high = high;
        }
    }

    /// Reset filter settings for the specified mode to standard default and apply if active.
    pub fn reset_filter_for_mode(&mut self, mode: RadioMode) -> (f32, f32) {
        let def = mode.default_filter_edges_with_pitch(self.cw_pitch_hz);
        self.mode_filters.insert(mode, def);
        if self.mode == mode {
            self.filter_low = def.0;
            self.filter_high = def.1;
        }
        def
    }

    /// Switch active radio mode: saves active filter edges to previous mode,
    /// updates mode, and recalls saved (or default) filter edges for the new mode.
    pub fn switch_mode(&mut self, new_mode: RadioMode) {
        if self.mode != new_mode {
            self.mode_filters.insert(self.mode, (self.filter_low, self.filter_high));
            self.mode = new_mode;
            let (low, high) = self.get_filter_for_mode(new_mode);
            self.filter_low = low;
            self.filter_high = high;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_swap_iq_default_true() {
        let cal = IqCalibration::default();
        assert!(cal.swap_iq, "Xiegu G90 requires swap_iq true by default");
    }

    #[test]
    fn test_vfo_snapping_1khz() {
        let lo_freq: u64 = 7_204_500;
        let step: i64 = 1000;
        let rounded_lo = ((lo_freq as f64 / 1000.0).round() as i64) * 1000;
        let new_lo = (rounded_lo + step).max(0) as u64;
        assert_eq!(new_lo, 7_206_000);

        let lo_clean: u64 = 7_204_000;
        let step_dn: i64 = -1000;
        let rounded_dn = ((lo_clean as f64 / 1000.0).round() as i64) * 1000;
        let new_dn = (rounded_dn + step_dn).max(0) as u64;
        assert_eq!(new_dn, 7_203_000);
    }

    #[test]
    fn test_mode_filter_edges() {
        // USB should have positive filter edges
        let (usb_low, usb_high) = RadioMode::USB.default_filter_edges();
        assert!(usb_low >= 0.0);
        assert!(usb_high > usb_low);
        assert_eq!(usb_high - usb_low, 2400.0);

        // LSB should have negative filter edges
        let (lsb_low, lsb_high) = RadioMode::LSB.default_filter_edges();
        assert!(lsb_high <= 0.0);
        assert!(lsb_high > lsb_low);
        assert_eq!(lsb_high - lsb_low, 2400.0);

        // CW should be centered around CW pitch
        let (cw_low, cw_high) = RadioMode::CW.default_filter_edges();
        assert!(cw_high > cw_low);
        assert_eq!(cw_high - cw_low, 500.0);
    }

    #[test]
    fn test_state_serialization_roundtrip() {
        let mut original = SdrState::default();
        original.lo_freq = 14_225_000;
        original.tune_freq = 14_225_000;
        original.mode = RadioMode::USB;
        original.volume = 0.85;
        original.squelch_db = -65.0;
        original.rf_preamp_db = 24.0;
        original.rf_waterfall_floor_db = -110.0;
        original.rf_waterfall_range_db = 65.0;
        original.rf_spectrum_ratio = 0.55;
        original.af_panel_height = 180.0;
        original.af_spectrum_ratio = 0.40;
        original.filter_low = 200.0;
        original.filter_high = 2800.0;
        original.cw_pitch_hz = 700.0;
        original.rf_display_span_hz = 48000.0;
        original.show_af_display = false;
        original.iq_cal.gain_balance = 0.05;
        original.iq_cal.phase_balance = -0.02;

        let json = serde_json::to_string(&original).expect("Serialization failed");
        let restored: SdrState = serde_json::from_str(&json).expect("Deserialization failed");

        assert_eq!(original, restored);
    }

    #[test]
    fn test_save_load_file_roundtrip() {
        let temp_dir = std::env::temp_dir().join("oksdr_test");
        let _ = std::fs::create_dir_all(&temp_dir);
        let test_file = temp_dir.join("test_config.json");

        let mut state = SdrState::default();
        state.lo_freq = 7_150_000;
        state.volume = 0.62;
        state.rf_waterfall_floor_db = -105.0;

        let json = serde_json::to_string_pretty(&state).unwrap();
        std::fs::write(&test_file, json).unwrap();

        let loaded_json = std::fs::read_to_string(&test_file).unwrap();
        let loaded_state: SdrState = serde_json::from_str(&loaded_json).unwrap();

        assert_eq!(state, loaded_state);
        let _ = std::fs::remove_file(test_file);
        let _ = std::fs::remove_dir(temp_dir);
    }

    #[test]
    fn test_cw_pitch_offset_tuning() {
        let mut state = SdrState::default();
        state.mode = RadioMode::CW;
        state.cw_pitch_hz = 700.0;
        state.filter_low = 450.0;
        state.filter_high = 950.0;

        let cw_offset = (state.filter_low as f64 + state.filter_high as f64) * 0.5;
        assert_eq!(cw_offset, 700.0);

        // Station is transmitting at 7.037 MHz dial (RF carrier is at 7,037,700 Hz)
        let rf_carrier = 7_037_700.0;
        let dial_freq = rf_carrier - cw_offset;
        let clean_freq = ((dial_freq / 1000.0).round() as u64) * 1000;
        assert_eq!(clean_freq, 7_037_000, "Clean freq in CW mode must be 7.037 MHz, not 7.038 MHz");

        // When tuned to 7,037,000, passband center should match RF carrier exactly
        let passband_center = clean_freq as f64 + cw_offset;
        assert_eq!(passband_center, rf_carrier);
    }

    #[test]
    fn test_ssb_bands_and_calling_freqs() {
        // 30m must NOT be in SSB_BANDS
        assert!(!Band::SSB_BANDS.contains(&Band::M30));
        assert_eq!(Band::SSB_BANDS.len(), 9);

        // Verify calling frequencies are in phone segments with correct sideband
        assert_eq!(Band::M160.ssb_calling_freq(), 1_910_000);
        assert_eq!(Band::M160.default_mode(), RadioMode::LSB);

        assert_eq!(Band::M80.ssb_calling_freq(), 3_985_000);
        assert_eq!(Band::M80.default_mode(), RadioMode::LSB);

        assert_eq!(Band::M60.ssb_calling_freq(), 5_330_500);
        assert_eq!(Band::M60.default_mode(), RadioMode::USB);

        assert_eq!(Band::M40.ssb_calling_freq(), 7_185_000);
        assert_eq!(Band::M40.default_mode(), RadioMode::LSB);

        assert_eq!(Band::M20.ssb_calling_freq(), 14_285_000);
        assert_eq!(Band::M20.default_mode(), RadioMode::USB);

        assert_eq!(Band::M10.ssb_calling_freq(), 28_400_000);
        assert_eq!(Band::M10.default_mode(), RadioMode::USB);
    }

    #[test]
    fn test_af_display_and_span_defaults() {
        let state = SdrState::default();
        assert!(state.show_af_display);
        assert_eq!(state.rf_display_span_hz, 96000.0);

        // Visible window math test
        let lo_freq = 14_070_000f64;
        let sample_rate = 96_000.0f64;
        let baseband_half = sample_rate * 0.5;
        let baseband_min = lo_freq - baseband_half;
        let baseband_max = lo_freq + baseband_half;

        // When zoomed to 20 kHz span and tuned to 14.074 MHz
        let cur_span = 20_000.0f64;
        let tune_freq = 14_074_000f64;
        let half_span = cur_span * 0.5;
        let center_f = tune_freq.clamp(baseband_min + half_span, baseband_max - half_span);
        let f_min = center_f - half_span;
        let f_max = center_f + half_span;

        assert_eq!(f_max - f_min, cur_span);
        assert!(f_min >= baseband_min);
        assert!(f_max <= baseband_max);
    }

    #[test]
    fn test_per_mode_filter_settings_and_reset() {
        let mut state = SdrState::default();

        // 1. Initial defaults
        assert_eq!(state.get_filter_for_mode(RadioMode::USB), (100.0, 2500.0));
        assert_eq!(state.get_filter_for_mode(RadioMode::LSB), (-2500.0, -100.0));
        assert_eq!(state.get_filter_for_mode(RadioMode::CW), (450.0, 950.0));

        // 2. Switch to CW mode and mess with CW filter
        state.switch_mode(RadioMode::CW);
        assert_eq!(state.mode, RadioMode::CW);
        assert_eq!(state.filter_low, 450.0);
        assert_eq!(state.filter_high, 950.0);

        // User customizes CW filter (e.g. 550 to 850 Hz)
        state.set_filter_for_mode(RadioMode::CW, 550.0, 850.0);
        assert_eq!(state.filter_low, 550.0);
        assert_eq!(state.filter_high, 850.0);

        // 3. Switch to LSB mode: must recall LSB filter (-2500, -100), NOT remain in CW positive frequencies!
        state.switch_mode(RadioMode::LSB);
        assert_eq!(state.mode, RadioMode::LSB);
        assert_eq!(state.filter_low, -2500.0);
        assert_eq!(state.filter_high, -100.0);

        // User customizes LSB filter (e.g. -2800 to -200 Hz)
        state.set_filter_for_mode(RadioMode::LSB, -2800.0, -200.0);
        assert_eq!(state.filter_low, -2800.0);
        assert_eq!(state.filter_high, -200.0);

        // 4. Switch back to CW mode: custom CW filter must be recalled!
        state.switch_mode(RadioMode::CW);
        assert_eq!(state.mode, RadioMode::CW);
        assert_eq!(state.filter_low, 550.0);
        assert_eq!(state.filter_high, 850.0);

        // 5. Switch back to LSB mode: custom LSB filter must be recalled!
        state.switch_mode(RadioMode::LSB);
        assert_eq!(state.mode, RadioMode::LSB);
        assert_eq!(state.filter_low, -2800.0);
        assert_eq!(state.filter_high, -200.0);

        // 6. Test Reset button functionality: resets active LSB mode back to clean defaults
        let reset_edges = state.reset_filter_for_mode(RadioMode::LSB);
        assert_eq!(reset_edges, (-2500.0, -100.0));
        assert_eq!(state.filter_low, -2500.0);
        assert_eq!(state.filter_high, -100.0);

        // 7. Verify serialization roundtrip preserves mode_filters
        let json = serde_json::to_string(&state).unwrap();
        let loaded: SdrState = serde_json::from_str(&json).unwrap();
        assert_eq!(state, loaded);
        assert_eq!(loaded.get_filter_for_mode(RadioMode::CW), (550.0, 850.0));
        assert_eq!(loaded.get_filter_for_mode(RadioMode::LSB), (-2500.0, -100.0));
    }
}
