use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, Stream, StreamConfig};
use crossbeam_channel::Sender;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::dsp::DspPipeline;
use crate::state::{RadioMode, SdrState};
use crate::ui::recording_deck::DeckPlayback;

pub struct AudioDeviceInfo {
    pub name: String,
    pub is_default: bool,
    pub is_cubilux: bool,
}

pub struct AudioMessage {
    pub rf_fft: Option<Vec<f32>>,
    pub af_fft: Option<Vec<f32>>,
    pub s_meter: (f32, f32, f32), // (s_units, over_s9_db, dbfs)
    pub cw_chars: Vec<char>,
    pub cw_wpm: u32,
    pub cw_tone: bool,
}

pub struct AudioEngine {
    _capture_stream: Option<Stream>,
    _playback_stream: Option<Stream>,
    running: Arc<AtomicBool>,
}

impl AudioEngine {
    pub fn list_input_devices() -> Vec<AudioDeviceInfo> {
        let host = cpal::default_host();
        let default_name = host.default_input_device().and_then(|d| d.name().ok());
        let mut list = Vec::new();

        if let Ok(devices) = host.input_devices() {
            for dev in devices {
                if let Ok(name) = dev.name() {
                    let is_default = default_name.as_ref() == Some(&name);
                    let is_cubilux = name.to_uppercase().contains("CB5")
                        || name.to_uppercase().contains("CUBILUX");
                    list.push(AudioDeviceInfo {
                        name,
                        is_default,
                        is_cubilux,
                    });
                }
            }
        }
        list
    }

    pub fn list_available_input_devices() -> Vec<String> {
        let host = cpal::default_host();
        let mut list = Vec::new();
        if let Ok(devices) = host.input_devices() {
            for dev in devices {
                if let Ok(name) = dev.name() {
                    if !list.contains(&name) {
                        list.push(name);
                    }
                }
            }
        }
        list
    }

    /// Returns ordered candidate capture devices for Cubilux CB5 Line-In or system fallbacks.
    /// If selected_name is specified (and not Auto), matches are prioritized first.
    pub fn get_input_device_candidates(selected_name: &str) -> Vec<Device> {
        let host = cpal::default_host();
        let mut user_selected = Vec::new();
        let mut dev1_hw = Vec::new();
        let mut dev1_plughw = Vec::new();
        let mut dev1_any = Vec::new();
        let mut cb5_other = Vec::new();
        let mut fallback = Vec::new();

        let want_specific = !selected_name.is_empty() && !selected_name.starts_with("Auto");

        if let Ok(devices) = host.input_devices() {
            for dev in devices {
                if let Ok(name) = dev.name() {
                    if want_specific && (name == selected_name || name.contains(selected_name)) {
                        user_selected.push(dev);
                        continue;
                    }

                    let upper = name.to_uppercase();
                    if upper.contains("CB5") || upper.contains("CUBILUX") {
                        if upper.contains("DEV=1") || upper.contains("#1") {
                            // Target: Hardware Line-In Jack
                            if name.starts_with("hw:") {
                                dev1_hw.push(dev);
                            } else if name.starts_with("plughw:") {
                                dev1_plughw.push(dev);
                            } else {
                                dev1_any.push(dev);
                            }
                        } else {
                            cb5_other.push(dev);
                        }
                    } else {
                        fallback.push(dev);
                    }
                }
            }
        }

        let mut candidates = Vec::new();
        candidates.extend(user_selected);
        candidates.extend(dev1_hw);
        candidates.extend(dev1_plughw);
        candidates.extend(dev1_any);
        candidates.extend(cb5_other);
        if let Some(def) = host.default_input_device() {
            candidates.push(def);
        }
        candidates.extend(fallback);
        candidates
    }

    pub fn start(
        state_arc: Arc<Mutex<SdrState>>,
        ui_tx: Sender<AudioMessage>,
        record_tx: Sender<f32>,
        is_recording: Arc<AtomicBool>,
        deck_playback: DeckPlayback,
    ) -> Result<Self, anyhow::Error> {
        let host = cpal::default_host();
        let configured_device = {
            let st = state_arc.lock().unwrap();
            st.audio_input_device.clone()
        };
        let candidates = Self::get_input_device_candidates(&configured_device);
        if candidates.is_empty() {
            anyhow::bail!("No input audio devices found on system");
        }

        // Bounded channel for passing demodulated audio from capture to playback
        let (audio_tx, audio_rx) = crossbeam_channel::bounded::<f32>(8192);
        let running = Arc::new(AtomicBool::new(true));

        // Try candidate devices until one successfully builds and starts a capture stream
        let mut capture_opt = None;
        let mut in_rate = 96000;
        let mut _in_channels = 2;

        for dev in candidates {
            let dev_name = dev.name().unwrap_or_else(|_| "Unknown".to_string());
            println!("[Audio] Testing capture candidate: {}", dev_name);

            let supported_configs = match dev.supported_input_configs() {
                Ok(cfgs) => cfgs,
                Err(e) => {
                    eprintln!("[Audio] Could not query configs for {}: {}", dev_name, e);
                    continue;
                }
            };

            let mut chosen_in_config = None;
            for cfg in supported_configs {
                if cfg.channels() == 2 {
                    if cfg.min_sample_rate().0 <= 96000 && cfg.max_sample_rate().0 >= 96000 {
                        chosen_in_config = Some(cfg.with_sample_rate(cpal::SampleRate(96000)));
                        break;
                    } else if cfg.min_sample_rate().0 <= 48000 && cfg.max_sample_rate().0 >= 48000 {
                        chosen_in_config = Some(cfg.with_sample_rate(cpal::SampleRate(48000)));
                    }
                }
            }

            let input_config = match chosen_in_config.or_else(|| dev.default_input_config().ok()) {
                Some(cfg) => cfg,
                None => {
                    eprintln!("[Audio] No suitable config found for {}", dev_name);
                    continue;
                }
            };

            let rate = input_config.sample_rate().0;
            let channels = input_config.channels();
            let mut stream_config: StreamConfig = input_config.clone().into();
            if let cpal::SupportedBufferSize::Range { min, max } = input_config.buffer_size() {
                let desired_frames = 512;
                let frames = desired_frames.clamp(*min, *max);
                stream_config.buffer_size = cpal::BufferSize::Fixed(frames);
            }

            let state_capture_f32 = Arc::clone(&state_arc);
            let state_capture_i16 = Arc::clone(&state_arc);
            let ui_tx_f32 = ui_tx.clone();
            let ui_tx_i16 = ui_tx.clone();
            let audio_tx_f32 = audio_tx.clone();
            let audio_tx_i16 = audio_tx.clone();
            let record_tx_f32 = record_tx.clone();
            let record_tx_i16 = record_tx.clone();
            let is_recording_f32 = Arc::clone(&is_recording);
            let is_recording_i16 = Arc::clone(&is_recording);

            let mut dsp_f32 = DspPipeline::new(rate as f32);
            let mut dsp_i16 = DspPipeline::new(rate as f32);

            let err_fn = |err| eprintln!("[Audio] Capture stream error: {}", err);

            let stream_res = match input_config.sample_format() {
                SampleFormat::F32 => dev.build_input_stream(
                    &stream_config,
                    move |data: &[f32], _: &_| {
                        Self::process_input_f32(
                            data,
                            channels,
                            &mut dsp_f32,
                            &state_capture_f32,
                            &audio_tx_f32,
                            &ui_tx_f32,
                            &record_tx_f32,
                            &is_recording_f32,
                        );
                    },
                    err_fn,
                    None,
                ),
                SampleFormat::I16 => dev.build_input_stream(
                    &stream_config,
                    move |data: &[i16], _: &_| {
                        let f32_data: Vec<f32> = data
                            .iter()
                            .map(|&s| s as f32 / 32768.0)
                            .collect();
                        Self::process_input_f32(
                            &f32_data,
                            channels,
                            &mut dsp_i16,
                            &state_capture_i16,
                            &audio_tx_i16,
                            &ui_tx_i16,
                            &record_tx_i16,
                            &is_recording_i16,
                        );
                    },
                    err_fn,
                    None,
                ),
                _ => {
                    eprintln!("[Audio] Unsupported sample format for {}", dev_name);
                    continue;
                }
            };

            match stream_res {
                Ok(stream) => {
                    println!(
                        "[Audio] Selected active capture device: {} ({} Hz, {} channels, format {:?}, buffer {:?})",
                        dev_name, rate, channels, input_config.sample_format(), stream_config.buffer_size
                    );
                    capture_opt = Some(stream);
                    in_rate = rate;
                    _in_channels = channels;
                    break;
                }
                Err(e) => {
                    eprintln!("[Audio] Failed to open stream on {}: {}", dev_name, e);
                }
            }
        }

        let capture_stream = capture_opt.ok_or_else(|| {
            anyhow::anyhow!("Failed to initialize audio capture on any available candidate device")
        })?;

        // Update state with actual sample rate
        {
            let mut st = state_arc.lock().unwrap();
            st.sample_rate = in_rate;
        }

        // Output device candidate search
        let mut out_candidates = Vec::new();
        if let Some(def) = host.default_output_device() {
            out_candidates.push(def);
        }
        if let Ok(devices) = host.output_devices() {
            for dev in devices {
                out_candidates.push(dev);
            }
        }

        let mut playback_opt = None;
        for out_dev in out_candidates {
            let out_name = out_dev.name().unwrap_or_else(|_| "Unknown".to_string());
            let out_config = match out_dev.default_output_config() {
                Ok(c) => c,
                Err(_) => continue,
            };
            let out_rate = out_config.sample_rate().0;
            let out_channels = out_config.channels();
            let mut out_stream_config: StreamConfig = out_config.clone().into();
            if let cpal::SupportedBufferSize::Range { min, max } = out_config.buffer_size() {
                let desired_frames = 512;
                let frames = desired_frames.clamp(*min, *max);
                out_stream_config.buffer_size = cpal::BufferSize::Fixed(frames);
            }

            let audio_rx_f32 = audio_rx.clone();
            let audio_rx_i16 = audio_rx.clone();
            let deck_playback_f32 = deck_playback.clone();
            let deck_playback_i16 = deck_playback.clone();
            let mut phase_f32 = 0.0f64;
            let mut s0_f32 = 0.0f32;
            let mut s1_f32 = 0.0f32;
            let mut phase_i16 = 0.0f64;
            let mut s0_i16 = 0.0f32;
            let mut s1_i16 = 0.0f32;

            let err_fn = |err| eprintln!("[Audio] Playback stream error: {}", err);

            let p_stream_res = match out_config.sample_format() {
                SampleFormat::F32 => out_dev.build_output_stream(
                    &out_stream_config,
                    move |data: &mut [f32], _: &_| {
                        let is_deck_playing = deck_playback_f32.is_playing.load(Ordering::Relaxed);
                        if is_deck_playing {
                            // Drain live audio queue so it doesn't backlog while playing recorded audio
                            while audio_rx_f32.try_recv().is_ok() {}

                            if let Ok(guard) = deck_playback_f32.samples.try_read() {
                                let total = guard.len();
                                let mut pos = deck_playback_f32.play_pos.load(Ordering::Relaxed);
                                let in_rate = deck_playback_f32.sample_rate.load(Ordering::Relaxed) as f64;
                                let ratio = in_rate / out_rate as f64;
                                let is_loop = deck_playback_f32.loop_playback.load(Ordering::Relaxed);

                                for frame in data.chunks_mut(out_channels as usize) {
                                    while phase_f32 >= 1.0 {
                                        phase_f32 -= 1.0;
                                        s0_f32 = s1_f32;
                                        if total > 0 && pos < total {
                                            s1_f32 = guard[pos];
                                            pos += 1;
                                        } else if total > 0 && is_loop {
                                            pos = 0;
                                            s1_f32 = guard[0];
                                        } else {
                                            deck_playback_f32.is_playing.store(false, Ordering::Relaxed);
                                            pos = 0;
                                            s1_f32 = 0.0;
                                        }
                                    }
                                    let sample = s0_f32 + (s1_f32 - s0_f32) * (phase_f32 as f32);
                                    phase_f32 += ratio;

                                    for ch in frame.iter_mut() {
                                        *ch = sample;
                                    }
                                }
                                deck_playback_f32.play_pos.store(pos, Ordering::Relaxed);
                                return;
                            }
                        }

                        // --- Live SDR Low-Latency Playback & Queue Management ---
                        // At 48 kHz: 480 samples = 10 ms, 960 samples = 20 ms, 1920 samples = 40 ms.
                        // 1. Purge stale audio backlog:
                        // If queue exceeds 1920 samples (>40 ms), drain excess down to 960 (~20 ms)
                        // to eliminate the initial startup backlog and keep decode/playback near-realtime.
                        let q_len = audio_rx_f32.len();
                        if q_len > 1920 {
                            let discard = q_len.saturating_sub(960);
                            for _ in 0..discard {
                                let _ = audio_rx_f32.try_recv();
                            }
                        }

                        // 2. Adaptive Clock Drift Tracking:
                        // Modulate resampling speed by ±0.25% (imperceptible to ear) to keep the queue
                        // permanently locked around the 700..1200 sample sweet spot (~15..25 ms).
                        let base_ratio = 48000.0 / out_rate as f64;
                        let cur_q = audio_rx_f32.len();
                        let drift_factor = if cur_q > 1200 {
                            1.0025
                        } else if cur_q < 600 {
                            0.9975
                        } else {
                            1.0
                        };
                        let ratio = base_ratio * drift_factor;

                        for frame in data.chunks_mut(out_channels as usize) {
                            while phase_f32 >= 1.0 {
                                phase_f32 -= 1.0;
                                s0_f32 = s1_f32;
                                s1_f32 = audio_rx_f32.try_recv().unwrap_or(0.0);
                            }
                            let sample = s0_f32 + (s1_f32 - s0_f32) * (phase_f32 as f32);
                            phase_f32 += ratio;

                            for ch in frame.iter_mut() {
                                *ch = sample;
                            }
                        }
                    },
                    err_fn,
                    None,
                ),
                SampleFormat::I16 => out_dev.build_output_stream(
                    &out_stream_config,
                    move |data: &mut [i16], _: &_| {
                        let is_deck_playing = deck_playback_i16.is_playing.load(Ordering::Relaxed);
                        if is_deck_playing {
                            while audio_rx_i16.try_recv().is_ok() {}

                            if let Ok(guard) = deck_playback_i16.samples.try_read() {
                                let total = guard.len();
                                let mut pos = deck_playback_i16.play_pos.load(Ordering::Relaxed);
                                let in_rate = deck_playback_i16.sample_rate.load(Ordering::Relaxed) as f64;
                                let ratio = in_rate / out_rate as f64;
                                let is_loop = deck_playback_i16.loop_playback.load(Ordering::Relaxed);

                                for frame in data.chunks_mut(out_channels as usize) {
                                    while phase_i16 >= 1.0 {
                                        phase_i16 -= 1.0;
                                        s0_i16 = s1_i16;
                                        if total > 0 && pos < total {
                                            s1_i16 = guard[pos];
                                            pos += 1;
                                        } else if total > 0 && is_loop {
                                            pos = 0;
                                            s1_i16 = guard[0];
                                        } else {
                                            deck_playback_i16.is_playing.store(false, Ordering::Relaxed);
                                            pos = 0;
                                            s1_i16 = 0.0;
                                        }
                                    }
                                    let sample = s0_i16 + (s1_i16 - s0_i16) * (phase_i16 as f32);
                                    phase_i16 += ratio;

                                    let s16 = (sample.clamp(-1.0, 1.0) * 32767.0) as i16;
                                    for ch in frame.iter_mut() {
                                        *ch = s16;
                                    }
                                }
                                deck_playback_i16.play_pos.store(pos, Ordering::Relaxed);
                                return;
                            }
                        }

                        // --- Live SDR Low-Latency Playback & Queue Management ---
                        let q_len = audio_rx_i16.len();
                        if q_len > 1920 {
                            let discard = q_len.saturating_sub(960);
                            for _ in 0..discard {
                                let _ = audio_rx_i16.try_recv();
                            }
                        }

                        let base_ratio = 48000.0 / out_rate as f64;
                        let cur_q = audio_rx_i16.len();
                        let drift_factor = if cur_q > 1200 {
                            1.0025
                        } else if cur_q < 600 {
                            0.9975
                        } else {
                            1.0
                        };
                        let ratio = base_ratio * drift_factor;

                        for frame in data.chunks_mut(out_channels as usize) {
                            while phase_i16 >= 1.0 {
                                phase_i16 -= 1.0;
                                s0_i16 = s1_i16;
                                s1_i16 = audio_rx_i16.try_recv().unwrap_or(0.0);
                            }
                            let sample = s0_i16 + (s1_i16 - s0_i16) * (phase_i16 as f32);
                            phase_i16 += ratio;

                            let s16 = (sample.clamp(-1.0, 1.0) * 32767.0) as i16;
                            for ch in frame.iter_mut() {
                                *ch = s16;
                            }
                        }
                    },
                    err_fn,
                    None,
                ),
                _ => continue,
            };

            if let Ok(stream) = p_stream_res {
                if stream.play().is_ok() {
                    println!(
                        "[Audio] Selected active playback device: {} ({} Hz, {} channels, format {:?})",
                        out_name, out_rate, out_channels, out_config.sample_format()
                    );
                    playback_opt = Some(stream);
                    break;
                }
            }
        }

        capture_stream.play()?;

        if playback_opt.is_none() {
            eprintln!("[Audio] Warning: No working playback audio device found. SDR will run with visual FFT only.");
        }

        Ok(Self {
            _capture_stream: Some(capture_stream),
            _playback_stream: playback_opt,
            running,
        })
    }

    #[inline]
    fn process_input_f32(
        data: &[f32],
        channels: u16,
        dsp: &mut DspPipeline,
        state_arc: &Arc<Mutex<SdrState>>,
        audio_tx: &Sender<f32>,
        ui_tx: &Sender<AudioMessage>,
        record_tx: &Sender<f32>,
        is_recording: &Arc<AtomicBool>,
    ) {
        // Fast lock with fallback - never drops buffer!
        let state = match state_arc.lock() {
            Ok(st) => st.clone(),
            Err(e) => e.into_inner().clone(),
        };

        dsp.sync_state(&state);

        let ch = channels as usize;
        let mut last_rf_spec = None;
        let mut last_af_spec = None;

        let preamp_linear = 10.0f32.powf(state.rf_preamp_db / 20.0);

        // Process pairs of stereo samples
        let mut idx = 0;
        while idx + 2 * ch <= data.len() {
            let i0 = data[idx] * preamp_linear;
            let q0 = if ch > 1 { data[idx + 1] * preamp_linear } else { 0.0 };

            let i1 = data[idx + ch] * preamp_linear;
            let q1 = if ch > 1 { data[idx + ch + 1] * preamp_linear } else { 0.0 };

            let (audio_out, rf_spec, af_spec) =
                dsp.process_sample_pair(i0, q0, i1, q1, &state);

            let _ = audio_tx.try_send(audio_out);

            if is_recording.load(Ordering::Relaxed) {
                let _ = record_tx.try_send(audio_out);
            }

            if rf_spec.is_some() {
                last_rf_spec = rf_spec;
            }
            if af_spec.is_some() {
                last_af_spec = af_spec;
            }

            idx += 2 * ch;
        }

        let (cw_chars, cw_tone) = if state.mode == RadioMode::CW {
            (std::mem::take(&mut dsp.cw_chars_buffer), dsp.cw_decoder.is_tone_active)
        } else {
            dsp.cw_chars_buffer.clear();
            (Vec::new(), false)
        };
        let cw_wpm = dsp.cw_decoder.detected_wpm;

        if last_rf_spec.is_some() || last_af_spec.is_some() || !cw_chars.is_empty() {
            let _ = ui_tx.try_send(AudioMessage {
                rf_fft: last_rf_spec,
                af_fft: last_af_spec,
                s_meter: dsp.latest_s_meter,
                cw_chars,
                cw_wpm,
                cw_tone,
            });
        }
    }
}
