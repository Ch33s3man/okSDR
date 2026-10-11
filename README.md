# okSDR - for Xiegu G90

A high-performance Software Defined Radio (SDR) receiver, panadapter, and transceiver companion built in Rust for the **Xiegu G90** HF transceiver (Firmware 1.81).

okSDR provides real-time dual waterfalls (RF spectrum and demodulated AF audio), smooth 60 FPS GPU-accelerated rendering, an integrated CW decoder, an audio recording & playback deck, one-click SSB calling frequency band switching, and configurable I/Q audio input and CAT control.

---

## Features

### Dual Waterfall & Spectrum (HDSDR Style)
* **RF Panadapter & Waterfall (±48 kHz / 96 kHz Span):**
  * Centered on the Xiegu G90 VFO frequency in real-time.
  * GPU-accelerated waterfall with selectable color palettes (*Viridis*, *Inferno*, *Turbo*, *Classic SDR*, *High Contrast*).
  * Spectrum floor, dynamic range, and ratio adjustments with peak-hold line.
  * Click anywhere in the passband to tune, Shift-click to retune the radio VFO, or use the mouse wheel to tune in 1 kHz steps.
* **AF Audio Spectrum & Waterfall (0 to 4 kHz):**
  * Zoomed view of demodulated audio showing carrier, voice formants, CW sidetone, and filter skirts.
  * Resizable splitter panel with persistent height adjustment.

### Xiegu G90 Rig Control (CAT)
* **Bi-Directional Frequency & Mode Sync:** Spinning the physical dial on the G90 instantly updates okSDR, and clicking frequencies or modes in okSDR commands the G90 via Hamlib `rigctld`.
* **Live Connection Status:** Real-time CAT status indicator with an automatic reconnection watchdog.
* **Calibrated S-Meter:** Digital bar meter showing accurate signal strength from S1 to S9, +10 to +80 dB over S9, and numeric dBFS readouts.

### Quick SSB Band Switcher & Calling Frequencies
* **9 Dedicated SSB Band Buttons:** Quick-switch between the 9 HF bands authorized for SSB voice: **`160m`**, **`80m`**, **`60m`**, **`40m`**, **`20m`**, **`17m`**, **`15m`**, **`12m`**, and **`10m`** (excludes 30m where voice is prohibited).
* **Direct Calling Frequency Tuning:** Pressing any band button automatically commands the G90 and VFO to the widely recognized national/international SSB calling frequency, selects the correct sideband (LSB on 160m/80m/40m, USB on 60m–10m), and sets appropriate filter bandwidth:
  * **160m:** `1.9100 MHz LSB`
  * **80m:** `3.9850 MHz LSB`
  * **60m:** `5.3305 MHz USB` (Channel 1)
  * **40m:** `7.1850 MHz LSB`
  * **20m:** `14.2850 MHz USB`
  * **17m:** `18.1500 MHz USB`
  * **15m:** `21.3850 MHz USB`
  * **12m:** `24.9500 MHz USB`
  * **10m:** `28.4000 MHz USB`
* **Activity Presets Dropdown:** Instant shortcuts to CW calling frequencies, FT8 digital spots, and voice nets.
* **Per-Band Memory:** Automatically remembers the last tuned frequency across all bands.

### CW Mode & Integrated Morse Decoder
* **Real-Time Morse Decoder:**
  * Auto-scrolling decoded text ticker embedded in the AF waterfall bar, active when operating in CW mode.
  * Automatic WPM speed detection.
  * Real-time `● TONE` detection indicator dot.
  * Pause, resume, and clear controls.
* **Selectable CW Pitch:** Dedicated pitch buttons (600 Hz, 700 Hz, 800 Hz) that center the filter passband.
* **Waterfall Alignment Offset:** Aligns waterfall carrier clicks directly with the CW sidetone so clicking a signal on the waterfall tunes the radio to the exact matching pitch.

### RX Audio Recording Deck
* **Live Waveform Display:** Visualizes received audio in real-time as it is recorded.
* **Deck Controls:** Record, Play, Pause, Stop, and clear recordings.
* **Interactive Scrubbing:** Scrub cursor with mouse wheel scrubbing to review received signals.
* **Automatic WAV Export:** Saves standard 48 kHz uncompressed WAV files into `~/okSDR_Recordings/` with metadata-rich filenames including start timestamp, band, mode, and frequency.
* **Recent Recordings Menu:** Quick recall of recent audio takes for instant playback.

### Hardware Configuration (`⚙ CONFIG`)
* **I/Q Audio Input Selection:** Choose your sound card or stereo Line-In interface from a dropdown list with a refresh button.
* **CAT Serial Port Selection:** Select your G90 serial port (`/dev/serial/by-id/*`, `/dev/ttyUSB*`, `/dev/ttyACM*`, or custom entry) with a **Test CAT** button for immediate verification.
* **I/Q Balance & Squelch (SQL) Calibration:**
  * DC offset notch removal filter.
  * I/Q channel swap (sideband inversion).
  * Amplitude gain balance and phase balance sliders.
  * Squelch threshold (SQL) slider (-130 to 0 dBFS).

### Demodulators & Audio DSP
* **Modes:** **USB**, **LSB**, **CW**, **AM**, **FM**.
* **Filter Passband:** Independent Left and Right filter edge adjustment (50 Hz steps), or quick presets: `2.4k`, `1.8k`, `500Hz`, `Wide`.
* **AGC & Gain:** Multi-speed AGC (`Off`, `Slow`, `Med`, `Fast`) with hang time, plus manual gain adjustment when AGC is off.
* **RF Preamp Gain:** Digital RF preamp slider boosting incoming Line-In audio by 0 to +36 dB.
* **Auto-Save:** User settings, volume, filter edges, and band memories persist across app restarts.

---

## Hardware Connections

1. **Audio I/Q Cable:**
   * Connect a 3.5mm stereo cable from the **Xiegu G90 rear I/Q jack** to your sound card's **stereo Line-In**.
2. **CAT Serial Cable:**
   * Connect the Xiegu G90 CAT cable to a USB port on your PC (detected as `/dev/ttyUSB*` or `/dev/serial/by-id/*`).

---

## Installation & Setup

### Dependencies
Install the required system packages:

```bash
# Debian / Ubuntu / Linux Mint
sudo apt update
sudo apt install -y build-essential libasound2-dev libudev-dev libhamlib-utils git
```

Ensure your user account has access to serial ports:
```bash
sudo usermod -aG dialout $USER
# Log out and back in for permissions to take effect
```

### Building from Source

1. Install Rust via [rustup](https://rustup.rs/):
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   source "$HOME/.cargo/env"
   ```

2. Clone this repository:
   ```bash
   git clone https://github.com/Ch33s3man/okSDR.git
   cd okSDR
   ```

3. Build the release binary:
   ```bash
   cargo build --release
   ```

---

## Launching

### Launcher Script (Recommended)
Use the included `run.sh` script to launch okSDR:

```bash
./run.sh
```

On startup, okSDR presents a **Radio Connection** screen where you can:
* Review or choose your serial port (`/dev/ttyUSB*`, `/dev/serial/by-id/*`, etc.)
* Select the G90 baud rate (19200 default) and CI-V address (`0x70`)
* Click **Test CAT** to safely query the G90's frequency and mode (with DTR/RTS held strictly LOW so the transmitter is never keyed)
* Click **Connect** to initialize Hamlib `rigctld` (Model 3088) with full PTT protection (`dtr_state=OFF,rts_state=OFF,ptt_type=None`)
* Or click **Continue without CAT** to run the SDR in audio/visual mode without rig sync.

### Desktop Shortcut
To create a desktop shortcut:
```bash
cp okSDR.desktop ~/Desktop/
chmod +x ~/Desktop/okSDR.desktop
```
Then double-click the **okSDR** icon on your Desktop.

---

## Operating Shortcuts

| Action | Control / Shortcut |
| :--- | :--- |
| **Tune Frequency (VFO)** | Left-click anywhere on the RF spectrum / waterfall |
| **Retune Radio LO / Center** | Shift + Left-click on the RF spectrum |
| **Fine Frequency Tuning** | Mouse wheel scroll over RF spectrum / waterfall (snaps to 1 kHz) |
| **Adjust Filter Bandwidth** | Drag Left / Right edge sliders or mouse-wheel scroll on slider |
| **Quick Filter Presets** | Click `2.4k`, `1.8k`, `500Hz`, or `Wide` |
| **CW Pitch Adjustment** | Click `600Hz`, `700Hz`, or `800Hz` (CW mode only) |
| **SSB Calling Frequencies** | Click any band button (`160m` through `10m`) |
| **Audio Recording Deck** | Click `[● REC DECK]` to toggle waveform drawer |
| **Scrub Audio Recording** | Drag scrub bar or scroll mouse wheel over recorded waveform |
| **Hardware Configuration** | Click `[⚙ CONFIG]` to select audio input, serial port, or test CAT |

---

## License

okSDR is open-source software licensed under the **GNU General Public License v3.0 (GPL-3.0-or-later)**. See the [LICENSE](LICENSE) file for details.

Developed by **KE2JBD** and **Antigravity**. Contributions and pull requests from the amateur radio community are welcome!
