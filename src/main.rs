#![allow(dead_code)]

mod audio;
mod dsp;
mod rig;
mod state;
mod ui;

use crossbeam_channel::bounded;
use eframe::egui;
use std::sync::{Arc, Mutex};

use audio::{AudioEngine, AudioMessage};
use rig::RigClient;
use state::SdrState;
use ui::OkSdrApp;

fn main() -> eframe::Result<()> {
    println!("=== okSDR - for Xiegu G90 by KE2JBD and Antigravity ===");

    // 1. Initialize Shared State (loaded from ~/.config/oksdr/config.json if available)
    let state = Arc::new(Mutex::new(SdrState::load()));

    // 2. Setup Message Channels between Audio/DSP thread and GUI
    let (ui_tx, ui_rx) = bounded::<AudioMessage>(64);

    // Setup RX Audio Recording & Playback
    let is_recording = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (record_tx, record_rx) = bounded::<f32>(96000 * 2);
    let deck_playback = ui::recording_deck::DeckPlayback::default();

    // 3. Start Audio Engine (Capture Cubilux CB5 96kHz I/Q & Playback)
    let audio_engine = match AudioEngine::start(
        Arc::clone(&state),
        ui_tx,
        record_tx,
        Arc::clone(&is_recording),
        deck_playback.clone(),
    ) {
        Ok(engine) => {
            println!("[Main] Audio engine started successfully.");
            Some(engine)
        }
        Err(err) => {
            eprintln!("[Main] Warning: Failed to start audio engine: {}. SDR will run in visual-only mode.", err);
            None
        }
    };

    // 4. Start Hamlib rigctld Client
    let rig_client = Some(RigClient::new(Arc::clone(&state)));

    // 5. Initialize RX Audio Recording Deck
    let recording_deck = ui::recording_deck::RecordingDeck::new(
        is_recording,
        record_rx,
        deck_playback,
    );

    // 6. Setup native GUI window
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("okSDR - for Xiegu G90 by KE2JBD and Antigravity")
            .with_inner_size([1120.0, 760.0])
            .with_min_inner_size([800.0, 550.0]),
        ..Default::default()
    };

    eframe::run_native(
        "okSDR - for Xiegu G90 by KE2JBD and Antigravity",
        native_options,
        Box::new(|_cc| {
            Ok(Box::new(OkSdrApp::new(
                state,
                ui_rx,
                audio_engine,
                rig_client,
                recording_deck,
            )))
        }),
    )
}
