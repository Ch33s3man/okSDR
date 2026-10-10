use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::state::{RadioMode, SdrState};

pub enum RigCommand {
    SetFreq(u64),
    SetMode(RadioMode),
    SetPtt(bool),
    Reconnect,
}

pub fn list_available_serial_devices() -> Vec<String> {
    let mut list = Vec::new();

    // 1. Check /dev/serial/by-id/
    if let Ok(entries) = std::fs::read_dir("/dev/serial/by-id") {
        for entry in entries.flatten() {
            let path = entry.path().to_string_lossy().to_string();
            if !list.contains(&path) {
                list.push(path);
            }
        }
    }

    // 2. Check /dev/ttyUSB0 .. 9
    for i in 0..10 {
        let p = format!("/dev/ttyUSB{}", i);
        if std::path::Path::new(&p).exists() && !list.contains(&p) {
            list.push(p);
        }
    }

    // 3. Check /dev/ttyACM0 .. 9
    for i in 0..10 {
        let p = format!("/dev/ttyACM{}", i);
        if std::path::Path::new(&p).exists() && !list.contains(&p) {
            list.push(p);
        }
    }

    list
}

pub fn resolve_serial_device(configured: &str) -> Option<String> {
    let trimmed = configured.trim();
    if !trimmed.is_empty() && !trimmed.starts_with("Auto") {
        // Direct absolute path: /dev/ttyUSB0 or /dev/serial/by-id/...
        if std::path::Path::new(trimmed).exists() {
            return Some(trimmed.to_string());
        }
        // Shorthand name in /dev/
        let dev_path = format!("/dev/{}", trimmed);
        if std::path::Path::new(&dev_path).exists() {
            return Some(dev_path);
        }
        // Shorthand name in /dev/serial/by-id/
        let by_id_path = format!("/dev/serial/by-id/{}", trimmed);
        if std::path::Path::new(&by_id_path).exists() {
            return Some(by_id_path);
        }
    }
    find_serial_device()
}

pub fn test_cat_connection(port: &str) -> (bool, String) {
    let resolved = resolve_serial_device(port);
    let dev = match resolved {
        Some(d) => d,
        None => return (false, "No serial device found on system.".to_string()),
    };

    println!("[Rigctld Test] Checking CAT connection for {} (read-only query)...", dev);

    // Pure read-only query over TCP to port 4532: only check frequency and mode.
    // NEVER kill processes or run stty here, which would toggle DTR/RTS and key the transmitter.
    let mut stream = match TcpStream::connect_timeout(
        &"127.0.0.1:4532".parse().unwrap(),
        Duration::from_millis(800),
    ) {
        Ok(s) => s,
        Err(_) => {
            return (false, format!("rigctld service not reachable on port 4532. Re-scan or restart app."));
        }
    };

    let _ = stream.set_read_timeout(Some(Duration::from_millis(600)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(600)));

    if stream.write_all(b"f\n").is_err() || stream.flush().is_err() {
        return (false, format!("Failed querying frequency on {}", dev));
    }

    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return (false, "TCP stream error".to_string()),
    });

    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return (false, format!("No response from G90 on {}", dev));
    }

    let freq = match line.trim().parse::<u64>() {
        Ok(f) if f > 100_000 => f,
        _ => return (false, format!("G90 not responding on {} (check power/cable)", dev)),
    };

    let mhz = freq as f64 / 1_000_000.0;
    let mut mode_str = String::new();

    if stream.write_all(b"m\n").is_ok() && stream.flush().is_ok() {
        let mut mline = String::new();
        let mut wline = String::new();
        if reader.read_line(&mut mline).is_ok() {
            let _ = reader.read_line(&mut wline);
            mode_str = mline.trim().to_string();
        }
    }

    let mode_suffix = if mode_str.is_empty() {
        "".to_string()
    } else {
        format!(" ({})", mode_str)
    };

    (true, format!("✔ G90 Connected: {:.4} MHz{}", mhz, mode_suffix))
}

pub fn find_serial_device() -> Option<String> {
    // 1. Search /dev/serial/by-id/
    if let Ok(entries) = std::fs::read_dir("/dev/serial/by-id") {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Ok(canonical) = std::fs::canonicalize(&path) {
                return Some(canonical.to_string_lossy().to_string());
            }
        }
    }
    // 2. Search /dev/ttyUSB0 .. 9
    for i in 0..10 {
        let p = format!("/dev/ttyUSB{}", i);
        if std::path::Path::new(&p).exists() {
            return Some(p);
        }
    }
    // 3. Search /dev/ttyACM0 .. 9
    for i in 0..10 {
        let p = format!("/dev/ttyACM{}", i);
        if std::path::Path::new(&p).exists() {
            return Some(p);
        }
    }
    None
}

pub fn restart_rigctld_if_needed(serial_dev: &str) {
    println!("[Rigctld] Launching/restarting rigctld on {}...", serial_dev);
    let _ = std::process::Command::new("killall")
        .args(["-9", "rigctld"])
        .status();
    std::thread::sleep(Duration::from_millis(300));

    let _ = std::process::Command::new("stty")
        .args(["-F", serial_dev, "19200", "-hupcl", "-crtscts"])
        .status();

    let _ = std::process::Command::new("rigctld")
        .args([
            "-m", "3089",
            "-r", serial_dev,
            "-s", "19200",
            "--set-conf=civaddr=0x70,dtr_state=OFF,rts_state=OFF,serial_handshake=None,ptt_type=None,cache_timeout=0",
            "-t", "4532",
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();

    std::thread::sleep(Duration::from_millis(800));
}

pub struct RigClient {
    running: Arc<AtomicBool>,
    cmd_tx: crossbeam_channel::Sender<RigCommand>,
}

impl RigClient {
    pub fn new(state_arc: Arc<Mutex<SdrState>>) -> Self {
        let (cmd_tx, cmd_rx) = crossbeam_channel::bounded::<RigCommand>(32);
        let running = Arc::new(AtomicBool::new(true));

        let running_clone = Arc::clone(&running);
        let state_clone = Arc::clone(&state_arc);

        thread::spawn(move || {
            Self::rigctld_worker_loop(state_clone, cmd_rx, running_clone);
        });

        Self { running, cmd_tx }
    }

    pub fn set_freq(&self, freq: u64) {
        let _ = self.cmd_tx.try_send(RigCommand::SetFreq(freq));
    }

    pub fn set_mode(&self, mode: RadioMode) {
        let _ = self.cmd_tx.try_send(RigCommand::SetMode(mode));
    }

    pub fn set_ptt(&self, ptt: bool) {
        let _ = self.cmd_tx.try_send(RigCommand::SetPtt(ptt));
    }

    pub fn reconnect(&self) {
        let _ = self.cmd_tx.try_send(RigCommand::Reconnect);
    }

    fn rigctld_worker_loop(
        state_arc: Arc<Mutex<SdrState>>,
        cmd_rx: crossbeam_channel::Receiver<RigCommand>,
        running: Arc<AtomicBool>,
    ) {
        while running.load(Ordering::Relaxed) {
            let (host, port) = {
                let st = state_arc.lock().unwrap();
                (st.rigctld_host.clone(), st.rigctld_port)
            };

            let addr = format!("{}:{}", host, port);

            // If a serial device exists and port 4532 is not accepting connections, start rigctld
            let configured_dev = {
                let st = state_arc.lock().unwrap();
                st.serial_device.clone()
            };
            if let Some(dev) = resolve_serial_device(&configured_dev) {
                if TcpStream::connect_timeout(
                    &addr.parse().unwrap_or_else(|_| "127.0.0.1:4532".parse().unwrap()),
                    Duration::from_millis(200),
                ).is_err() {
                    restart_rigctld_if_needed(&dev);
                }
            }

            match TcpStream::connect_timeout(
                &addr.parse().unwrap_or_else(|_| "127.0.0.1:4532".parse().unwrap()),
                Duration::from_secs(2),
            ) {
                Ok(mut stream) => {
                    println!("[Rigctld] Connected to {}", addr);
                    let mut last_radio_rx = Instant::now();

                    let _ = stream.set_read_timeout(Some(Duration::from_millis(100)));
                    let _ = stream.set_write_timeout(Some(Duration::from_millis(100)));

                    let mut reader_stream = match stream.try_clone() {
                        Ok(s) => s,
                        Err(_) => break,
                    };
                    let mut reader = BufReader::new(&mut reader_stream);

                    // Immediately query radio's current frequency & mode so app syncs on startup without lag or glitch
                    if stream.write_all(b"f\n").is_ok() {
                        let _ = stream.flush();
                        let mut line = String::new();
                        if reader.read_line(&mut line).is_ok() {
                            if let Ok(freq) = line.trim().parse::<u64>() {
                                if freq > 100_000 {
                                    last_radio_rx = Instant::now();
                                    let mut st = state_arc.lock().unwrap();
                                    st.lo_freq = freq;
                                    st.tune_freq = freq;
                                    st.rigctld_connected = true;
                                    println!("[Rigctld] Initial radio frequency: {} Hz", freq);
                                }
                            }
                        }
                    }

                    if stream.write_all(b"m\n").is_ok() {
                        let _ = stream.flush();
                        let mut mode_line = String::new();
                        let mut width_line = String::new();
                        if reader.read_line(&mut mode_line).is_ok() && reader.read_line(&mut width_line).is_ok() {
                            let mode_str = mode_line.trim().to_uppercase();
                            let parsed_mode = match mode_str.as_str() {
                                "USB" | "PKTUSB" => Some(RadioMode::USB),
                                "LSB" | "PKTLSB" => Some(RadioMode::LSB),
                                "CW" | "CWR" => Some(RadioMode::CW),
                                "AM" => Some(RadioMode::AM),
                                "FM" | "FMN" => Some(RadioMode::FM),
                                _ => None,
                            };
                            if let Some(m) = parsed_mode {
                                last_radio_rx = Instant::now();
                                let mut st = state_arc.lock().unwrap();
                                st.rigctld_connected = true;
                                if st.mode != m {
                                    st.mode = m;
                                    let (f_low, f_high) = m.default_filter_edges();
                                    st.filter_low = f_low;
                                    st.filter_high = f_high;
                                    println!("[Rigctld] Initial radio mode: {:?}", m);
                                }
                            }
                        }
                    }

                    let mut last_poll = Instant::now();
                    let mut last_mode_poll = Instant::now();
                    let mut last_sent_freq: Option<u64> = None;
                    let mut last_mode_sent: Option<(RadioMode, Instant)> = None;

                    while running.load(Ordering::Relaxed) {
                        // 1. Process outgoing commands from SDR -> G90
                        while let Ok(cmd) = cmd_rx.try_recv() {
                            match cmd {
                                RigCommand::SetFreq(f) => {
                                    let msg = format!("F {}\n", f);
                                    if stream.write_all(msg.as_bytes()).is_ok() {
                                        let _ = stream.flush();
                                        last_sent_freq = Some(f);
                                        let mut resp = String::new();
                                        let _ = reader.read_line(&mut resp);
                                    }
                                }
                                RigCommand::SetMode(m) => {
                                    let mode_str = match m {
                                        RadioMode::USB => "USB",
                                        RadioMode::LSB => "LSB",
                                        RadioMode::CW => "CW",
                                        RadioMode::AM => "AM",
                                        RadioMode::FM => "FM",
                                    };
                                    let msg = format!("M {} -1\n", mode_str);
                                    if stream.write_all(msg.as_bytes()).is_ok() {
                                        let _ = stream.flush();
                                        last_mode_sent = Some((m, Instant::now()));
                                        let mut resp = String::new();
                                        let _ = reader.read_line(&mut resp);
                                        println!("[Rigctld] SetMode {} -> {}", mode_str, resp.trim());
                                    }
                                }
                                RigCommand::SetPtt(p) => {
                                    let val = if p { 1 } else { 0 };
                                    let msg = format!("T {}\n", val);
                                    if stream.write_all(msg.as_bytes()).is_ok() {
                                        let _ = stream.flush();
                                        let mut resp = String::new();
                                        let _ = reader.read_line(&mut resp);
                                    }
                                }
                                RigCommand::Reconnect => {
                                    println!("[Rigctld] Reconnect requested. Resetting connection...");
                                    let configured_dev = {
                                        let st = state_arc.lock().unwrap();
                                        st.serial_device.clone()
                                    };
                                    if let Some(dev) = resolve_serial_device(&configured_dev) {
                                        restart_rigctld_if_needed(&dev);
                                    }
                                    break;
                                }
                            }
                        }

                        // 2. Poll frequency from G90 -> SDR every 250ms
                        if last_poll.elapsed() >= Duration::from_millis(250) {
                            last_poll = Instant::now();
                            if stream.write_all(b"f\n").is_err() {
                                break; // Connection lost
                            }
                            let _ = stream.flush();

                            let mut line = String::new();
                            if reader.read_line(&mut line).is_ok() {
                                let trimmed = line.trim();
                                if let Ok(parsed_freq) = trimmed.parse::<u64>() {
                                    if parsed_freq > 100_000 {
                                        last_radio_rx = Instant::now();
                                        {
                                            let mut st = state_arc.lock().unwrap();
                                            if !st.rigctld_connected {
                                                st.rigctld_connected = true;
                                            }
                                        }

                                        let was_just_sent = last_sent_freq.map_or(false, |f| {
                                            (f as i64 - parsed_freq as i64).abs() < 5
                                        });
                                        if !was_just_sent {
                                            let mut st = state_arc.lock().unwrap();
                                            if (st.lo_freq as i64 - parsed_freq as i64).abs() > 2 {
                                                let offset = st.tune_freq as i64 - st.lo_freq as i64;
                                                st.lo_freq = parsed_freq;
                                                st.tune_freq = (parsed_freq as i64 + offset).max(0) as u64;
                                            }
                                        } else {
                                            last_sent_freq = None;
                                        }
                                    }
                                }
                            }
                        }

                        // 3. Poll mode from G90 -> SDR every 500ms
                        if last_mode_poll.elapsed() >= Duration::from_millis(500) {
                            last_mode_poll = Instant::now();
                            if stream.write_all(b"m\n").is_ok() {
                                let _ = stream.flush();
                                let mut mode_line = String::new();
                                let mut width_line = String::new();
                                if reader.read_line(&mut mode_line).is_ok() && reader.read_line(&mut width_line).is_ok() {
                                    let mode_str = mode_line.trim().to_uppercase();
                                    let parsed_mode = match mode_str.as_str() {
                                        "USB" | "PKTUSB" => Some(RadioMode::USB),
                                        "LSB" | "PKTLSB" => Some(RadioMode::LSB),
                                        "CW" | "CWR" => Some(RadioMode::CW),
                                        "AM" => Some(RadioMode::AM),
                                        "FM" | "FMN" => Some(RadioMode::FM),
                                        _ => None,
                                    };

                                    if let Some(m) = parsed_mode {
                                        last_radio_rx = Instant::now();
                                        {
                                            let mut st = state_arc.lock().unwrap();
                                            if !st.rigctld_connected {
                                                st.rigctld_connected = true;
                                            }
                                        }

                                        let in_grace_period = last_mode_sent.as_ref().map_or(false, |(sent_m, sent_t)| {
                                            if sent_t.elapsed() < Duration::from_millis(2000) {
                                                *sent_m != m
                                            } else {
                                                false
                                            }
                                        });

                                        if !in_grace_period {
                                            last_mode_sent = None;
                                            let mut st = state_arc.lock().unwrap();
                                            if st.mode != m {
                                                println!("[Rigctld] Radio mode reported: {:?}", m);
                                                st.mode = m;
                                                let (f_low, f_high) = m.default_filter_edges();
                                                st.filter_low = f_low;
                                                st.filter_high = f_high;
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        // 4. Radio responsiveness watchdog
                        if last_radio_rx.elapsed() > Duration::from_millis(2000) {
                            let mut st = state_arc.lock().unwrap();
                            if st.rigctld_connected {
                                st.rigctld_connected = false;
                                println!("[Rigctld] G90 stopped responding on serial interface.");
                            }
                        }

                        if last_radio_rx.elapsed() > Duration::from_secs(4) {
                            println!("[Rigctld] Radio unresponsive for >4s. Re-scanning serial devices...");
                            let configured_dev = {
                                let st = state_arc.lock().unwrap();
                                st.serial_device.clone()
                            };
                            if let Some(dev) = resolve_serial_device(&configured_dev) {
                                restart_rigctld_if_needed(&dev);
                            }
                            break;
                        }

                        thread::sleep(Duration::from_millis(20));
                    }

                    {
                        let mut st = state_arc.lock().unwrap();
                        st.rigctld_connected = false;
                    }
                    println!("[Rigctld] Disconnected");
                }
                Err(_) => {
                    {
                        let mut st = state_arc.lock().unwrap();
                        st.rigctld_connected = false;
                    }
                    thread::sleep(Duration::from_secs(2));
                }
            }
        }
    }
}

impl Drop for RigClient {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_and_resolve_serial_devices() {
        let devs = list_available_serial_devices();
        // Should return a valid list (might be empty or contain ttyUSB0)
        let resolved = resolve_serial_device("Auto-Detect");
        // Auto-detect resolution either finds the active port or None
        if !devs.is_empty() {
            assert!(resolved.is_some());
        }

        // Non-existent device resolution falls back to auto-detect
        let bogus = resolve_serial_device("/dev/non_existent_device_xyz123");
        assert_eq!(bogus, find_serial_device());
    }

    #[test]
    fn test_cat_test_with_empty_or_bogus_port() {
        let (_ok, msg) = test_cat_connection("/dev/definitely_not_a_real_device_999");
        assert!(!msg.is_empty());
    }
}
