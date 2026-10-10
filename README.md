# okSDR - for Xiegu G90 by KE2JBD and Antigravity

A high-performance Software Defined Radio (SDR) receiver and panadapter built natively for Linux Mint 22.3 in Rust. Modeled after HDSDR, it provides real-time dual waterfalls (RF passband and AF demodulated audio), full Xiegu G90 CAT rig control via Hamlib, and direct 96 kHz stereo I/Q capture with the **Cubilux CB5** USB audio interface.

---

## Features

* **Dual Waterfall & Spectrum (Classic HDSDR signature):**
  * **RF Panadapter & Waterfall:** ±48 kHz wide view (96 kHz span) centered on the radio LO with smooth GPU-accelerated waterfall rendering.
  * **AF Audio Spectrum & Waterfall:** 0 to 4 kHz zoomed view of demodulated audio showing carrier, voice formants, and filter skirts.
* **Xiegu G90 Integration (Firmware 1.81):**
  * Auto-discovers and connects to `rigctld` (Hamlib model 3088).
  * Bi-directional frequency tracking: spinning the G90 dial updates the SDR LO in real-time.
  * Click-to-tune: click anywhere in the RF passband to shift demodulation or Shift-click to retune the G90 VFO.
* **Quick HF Band Switcher Bar:**
  * One-click buttons: `160m`, `80m`, `60m`, `40m`, `30m`, `20m`, `17m`, `15m`, `12m`, `10m`.
  * **Per-band memory:** Remembers the last tuned frequency per band.
  * **Auto-mode:** Automatically switches to LSB on 160m/80m/40m and USB on 30m–10m.
  * **Activity Presets:** Instant jump to FT8 calling frequencies, CW calling, and SSB calling spots.
* **Demodulators & DSP:**
  * Modes: **USB**, **LSB**, **CW**, **AM**, **FM**.
  * Complex FIR channel filtering with Blackman windowing and decimation-by-2 to 48 kHz audio rate.
  * Presets: `2.4k`, `1.8k`, `500Hz`, `Wide`.
  * Multi-speed AGC (`Off`, `Slow`, `Med`, `Fast`) with hang time and S-meter calibration (S1–S9, +10 to +60 dB).
* **Universal Hardware & Sound Card Portability (CONFIG Options):**
  * **Audio Line-In Selector:** Choose between auto-detecting the Cubilux CB5 or selecting any sound card or USB audio interface on the system.
  * **CAT Serial Port Selector & Live Tester:** Auto-detects Xiegu DE-19 / CH340 / CP2102 serial ports or allows manual port specification with an interactive **[Test CAT]** button giving instant connection feedback.
  * **DSP & I/Q Calibration:** Fine-tune amplitude gain and phase balance, toggle DC notch removal, or adjust squelch (SQL).
* **RX Audio Recording Deck:**
  * Live audio waveform recorder with scrub cursor, playback deck, and automatic export with timestamp, band, mode, and frequency.

---

## Hardware Hookup Checklist

1. **Audio I/Q Cable:**
   * Connect a 3.5mm stereo cable from the **Xiegu G90 I/Q jack** to the **Cubilux CB5 Line-In jack** (or your PC's stereo Line-In).
2. **CAT Serial Cable:**
   * Plug the G90 CAT cable / Xiegu DE-19 into a USB port on your PC (detected as `/dev/ttyUSB*` or `/dev/serial/by-id/*`).
3. **Radio Settings (Xiegu G90):**
   * Long press **F/FUNC** $\to$ System Menu:
     * Ensure CAT baud rate is **19200**.
     * Make sure Line Out / IQ levels are enabled.

---

## Launching the Application

### Option A: From Desktop
Double-click the **okSDR** icon on your Linux Desktop.

### Option B: From Terminal
```bash
cd ~/Desktop/okSDR
./run.sh
```

`run.sh` will automatically check for serial devices, ensure `rigctld` is communicating with the G90 profile (`-m 3089 -s 19200`), and launch the optimized binary `oksdr`.

---

## License

okSDR is licensed under the **GNU General Public License v3.0 (GPL-3.0-or-later)**. See the [LICENSE](LICENSE) file for details.
