use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SdrState {
    pub lo_freq: u64,       // Radio Center Frequency (Hz)
    pub tune_freq: u64,     // Demodulated Frequency (Hz)
    pub mode: RadioMode,
    pub filter_low: f32,    // Hz offset from tune frequency
    pub filter_high: f32,   // Hz offset from tune frequency
    pub volume: f32,        // 0.0 to 1.0
    pub squelch_db: f32,    // -130 dB to 0 dB
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
    pub af_panel_height: f32,   // Height of AF bottom panel in pixels (default 150.0)
    pub af_spectrum_ratio: f32, // Ratio of AF display for panadapter vs waterfall (default 0.45)
    pub cw_pitch_hz: f32,       // CW sidetone pitch & waterfall offset in Hz (default 700.0)
    pub recordings_dir: String, // Directory where RX audio recordings are saved
    pub audio_input_device: String, // Preferred audio capture device name or "Auto (Cubilux / Default)"
    pub serial_device: String,      // Preferred CAT serial device path or "Auto-Detect"
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
            volume: 0.7,
            squelch_db: -130.0,
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
            af_panel_height: 150.0,
            af_spectrum_ratio: 0.45,
            cw_pitch_hz: 700.0,
            recordings_dir: default_rec_dir,
            audio_input_device: "Auto (Cubilux / Default)".to_string(),
            serial_device: "Auto-Detect".to_string(),
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
            && (self.volume - other.volume).abs() < 1e-4
            && (self.squelch_db - other.squelch_db).abs() < 1e-4
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
            && (self.af_panel_height - other.af_panel_height).abs() < 1e-4
            && (self.af_spectrum_ratio - other.af_spectrum_ratio).abs() < 1e-4
            && (self.cw_pitch_hz - other.cw_pitch_hz).abs() < 1e-4
            && self.recordings_dir == other.recordings_dir
            && self.audio_input_device == other.audio_input_device
            && self.serial_device == other.serial_device
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
}
