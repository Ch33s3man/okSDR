use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::state::{RadioMode, RigStatus, SdrState};

/// Everything needed to start Hamlib for the G90. DTR/RTS are NOT configurable: for the
/// Xiegu G90 they must always be LOW (high = transmit), so they are hard-coded OFF below.
#[derive(Debug, Clone, PartialEq)]
pub struct RigConfig {
    pub serial_device: String,
    pub baud: u32,
    pub civ_address: u8,
    pub hamlib_model: u32,
    pub host: String,
    pub port: u16,
}

impl RigConfig {
    /// Build a config from saved settings. Returns Err if no serial device can be resolved.
    pub fn from_state(st: &SdrState) -> Result<Self, String> {
        let dev = resolve_serial_device(&st.serial_device).ok_or_else(|| {
            "No USB serial device found (/dev/ttyUSB*, /dev/ttyACM*, /dev/serial/by-id/*). \
             Is the G90 CAT cable plugged in?"
                .to_string()
        })?;
        Ok(Self {
            serial_device: dev,
            baud: st.cat_baud,
            civ_address: st.civ_address,
            hamlib_model: st.hamlib_model,
            host: st.rigctld_host.clone(),
            port: st.rigctld_port,
        })
    }

    /// The exact `--set-conf` string passed to rigctld.
    pub fn hamlib_conf(&self) -> String {
        format!(
            "civaddr=0x{:02x},dtr_state=OFF,rts_state=OFF,serial_handshake=None,ptt_type=None",
            self.civ_address
        )
    }

    pub fn rigctld_args(&self) -> Vec<String> {
        vec![
            "-m".into(),
            self.hamlib_model.to_string(),
            "-r".into(),
            self.serial_device.clone(),
            "-s".into(),
            self.baud.to_string(),
            format!("--set-conf={}", self.hamlib_conf()),
            "-T".into(),
            self.host.clone(),
            "-t".into(),
            self.port.to_string(),
        ]
    }

    pub fn command_line(&self) -> String {
        format!("rigctld {}", self.rigctld_args().join(" "))
    }
}

pub enum RigCommand {
    SetFreq(u64),
    SetMode(RadioMode),
    SetPtt(bool),
    Connect(RigConfig),
    Disconnect,
}

pub fn list_available_serial_devices() -> Vec<String> {
    let mut list = Vec::new();

    // 1. Check /dev/serial/by-id/ (stable names, preferred)
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

// ---------------------------------------------------------------------------------------
// Hamlib discovery
// ---------------------------------------------------------------------------------------

/// What we know about the local Hamlib installation.
#[derive(Debug, Clone, Default)]
pub struct HamlibInfo {
    pub rigctld_path: Option<PathBuf>,
    pub version: Option<String>,
    /// Xiegu models reported by `rigctl -l`: (model number, description)
    pub xiegu_models: Vec<(u32, String)>,
}

pub const HAMLIB_INSTALL_HINT: &str = "sudo apt install libhamlib-utils";

pub fn find_in_path(bin: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .chain(["/usr/bin", "/usr/local/bin"].iter().map(PathBuf::from))
        .map(|d| d.join(bin))
        .find(|p| p.is_file())
}

pub fn detect_hamlib() -> HamlibInfo {
    let mut info = HamlibInfo { rigctld_path: find_in_path("rigctld"), ..Default::default() };
    if let Some(p) = &info.rigctld_path {
        if let Ok(out) = Command::new(p).arg("--version").stdin(Stdio::null()).output() {
            let s = String::from_utf8_lossy(&out.stdout);
            info.version = s.lines().next().map(|l| l.trim().to_string());
        }
    }
    if let Some(rigctl) = find_in_path("rigctl") {
        if let Ok(out) = Command::new(rigctl).arg("-l").stdin(Stdio::null()).output() {
            for line in String::from_utf8_lossy(&out.stdout).lines() {
                if !line.to_lowercase().contains("xiegu") {
                    continue;
                }
                let mut parts = line.split_whitespace();
                if let Some(Ok(num)) = parts.next().map(|n| n.parse::<u32>()) {
                    let desc: Vec<&str> = parts.take_while(|w| !w.chars().next().map_or(false, |c| c.is_ascii_digit())).collect();
                    info.xiegu_models.push((num, desc.join(" ")));
                }
            }
        }
    }
    info
}

fn rigctld_log_path() -> PathBuf {
    SdrState::config_path().with_file_name("rigctld.log")
}

fn log_tail(max_lines: usize) -> String {
    let mut s = String::new();
    if let Ok(mut f) = std::fs::File::open(rigctld_log_path()) {
        let _ = f.read_to_string(&mut s);
    }
    let lines: Vec<&str> = s.lines().filter(|l| !l.trim().is_empty()).collect();
    let start = lines.len().saturating_sub(max_lines);
    lines[start..].join("\n")
}

/// Stop any stale rigctld left listening on OUR TCP port (e.g. from the old run.sh, which
/// started a persistent `nohup rigctld`). Other rigctld instances are left alone.
fn kill_stale_rigctld(port: u16) {
    let _ = Command::new("pkill")
        .args(["-f", &format!("rigctld.* -t {}( |$)", port)])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    thread::sleep(Duration::from_millis(300));
}

fn spawn_rigctld(cfg: &RigConfig) -> Result<Child, String> {
    let bin = find_in_path("rigctld").ok_or_else(|| {
        format!("Hamlib 'rigctld' is not installed.\nInstall it with:  {}", HAMLIB_INSTALL_HINT)
    })?;

    kill_stale_rigctld(cfg.port);

    let log = std::fs::File::create(rigctld_log_path())
        .map_err(|e| format!("Cannot create rigctld log: {}", e))?;
    let log_err = log.try_clone().map_err(|e| e.to_string())?;

    println!("[Rigctld] Starting: {}", cfg.command_line());
    Command::new(bin)
        .args(cfg.rigctld_args())
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err))
        .spawn()
        .map_err(|e| format!("Failed to start rigctld: {}", e))
}

fn stop_child(child: &mut Option<Child>) {
    if let Some(mut c) = child.take() {
        println!("[Rigctld] Stopping rigctld (pid {})", c.id());
        // SIGTERM first so Hamlib closes the port cleanly (lines stay LOW), then SIGKILL.
        unsafe {
            libc::kill(c.id() as libc::pid_t, libc::SIGTERM);
        }
        let deadline = Instant::now() + Duration::from_millis(800);
        while Instant::now() < deadline {
            if let Ok(Some(_)) = c.try_wait() {
                return;
            }
            thread::sleep(Duration::from_millis(40));
        }
        let _ = c.kill();
        let _ = c.wait();
    }
}

fn parse_mode(s: &str) -> Option<RadioMode> {
    match s.trim().to_uppercase().as_str() {
        "USB" | "PKTUSB" => Some(RadioMode::USB),
        "LSB" | "PKTLSB" => Some(RadioMode::LSB),
        "CW" | "CWR" => Some(RadioMode::CW),
        "AM" => Some(RadioMode::AM),
        "FM" | "FMN" => Some(RadioMode::FM),
        _ => None,
    }
}

fn set_status(state: &Arc<Mutex<SdrState>>, status: RigStatus) {
    let mut st = state.lock().unwrap();
    st.rigctld_connected = status == RigStatus::Connected;
    st.rig_status = status;
}

/// Why a connected session ended.
enum SessionEnd {
    Shutdown,
    Disconnect,
    Reconnect(RigConfig),
    Lost(String),
}

pub struct RigClient {
    running: Arc<AtomicBool>,
    cmd_tx: crossbeam_channel::Sender<RigCommand>,
    worker: Option<thread::JoinHandle<()>>,
}

impl RigClient {
    /// Creates the CAT worker. It stays IDLE (never touches the serial port) until
    /// `connect()` is called from the connection dialog.
    pub fn new(state_arc: Arc<Mutex<SdrState>>) -> Self {
        let (cmd_tx, cmd_rx) = crossbeam_channel::bounded::<RigCommand>(32);
        let running = Arc::new(AtomicBool::new(true));
        let running_clone = Arc::clone(&running);

        let worker = thread::spawn(move || {
            Self::worker_loop(state_arc, cmd_rx, running_clone);
        });

        Self { running, cmd_tx, worker: Some(worker) }
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

    pub fn connect(&self, cfg: RigConfig) {
        let _ = self.cmd_tx.send(RigCommand::Connect(cfg));
    }

    pub fn disconnect(&self) {
        let _ = self.cmd_tx.send(RigCommand::Disconnect);
    }

    fn worker_loop(
        state: Arc<Mutex<SdrState>>,
        cmd_rx: crossbeam_channel::Receiver<RigCommand>,
        running: Arc<AtomicBool>,
    ) {
        let mut child: Option<Child> = None;
        let mut pending: Option<RigConfig> = None;

        while running.load(Ordering::Relaxed) {
            // ---- Idle: wait for a Connect request. Never touch the port on our own. ----
            let cfg = match pending.take() {
                Some(c) => c,
                None => match cmd_rx.recv_timeout(Duration::from_millis(200)) {
                    Ok(RigCommand::Connect(c)) => c,
                    Ok(RigCommand::Disconnect) => {
                        stop_child(&mut child);
                        set_status(&state, RigStatus::Idle);
                        continue;
                    }
                    Ok(_) => continue, // freq/mode/ptt ignored while not connected
                    Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
                    Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
                },
            };

            stop_child(&mut child);
            set_status(&state, RigStatus::Connecting("Starting Hamlib rigctld…".into()));

            match spawn_rigctld(&cfg) {
                Ok(c) => child = Some(c),
                Err(msg) => {
                    set_status(&state, RigStatus::Failed(msg));
                    continue;
                }
            }

            set_status(&state, RigStatus::Connecting(format!("Waiting for G90 on {}…", cfg.serial_device)));
            let stream = match Self::wait_for_radio(&cfg, &mut child, &state, Duration::from_secs(8)) {
                Ok(s) => s,
                Err(msg) => {
                    stop_child(&mut child);
                    set_status(&state, RigStatus::Failed(msg));
                    continue;
                }
            };

            set_status(&state, RigStatus::Connected);
            println!("[Rigctld] G90 CAT connected via {}", cfg.serial_device);

            match Self::session(stream, &state, &cmd_rx, &running, &mut child) {
                SessionEnd::Shutdown => break,
                SessionEnd::Disconnect => {
                    stop_child(&mut child);
                    set_status(&state, RigStatus::Idle);
                }
                SessionEnd::Reconnect(c) => {
                    pending = Some(c);
                }
                SessionEnd::Lost(msg) => {
                    println!("[Rigctld] {}", msg);
                    stop_child(&mut child);
                    set_status(&state, RigStatus::Lost(msg));
                }
            }
        }
        stop_child(&mut child);
    }

    /// Wait for rigctld to accept TCP connections AND for the G90 to answer a frequency query.
    fn wait_for_radio(
        cfg: &RigConfig,
        child: &mut Option<Child>,
        state: &Arc<Mutex<SdrState>>,
        timeout: Duration,
    ) -> Result<TcpStream, String> {
        let addr = format!("{}:{}", cfg.host, cfg.port);
        let sock_addr = addr
            .parse()
            .map_err(|_| format!("Invalid rigctld address {}", addr))?;
        let deadline = Instant::now() + timeout;
        let mut last_err = String::from("rigctld did not start listening");

        while Instant::now() < deadline {
            if let Some(c) = child.as_mut() {
                if let Ok(Some(status)) = c.try_wait() {
                    let tail = log_tail(6);
                    return Err(format!(
                        "rigctld exited immediately ({}).\n{}",
                        status,
                        if tail.is_empty() { "Check model number / serial device.".into() } else { tail }
                    ));
                }
            }

            if let Ok(mut stream) = TcpStream::connect_timeout(&sock_addr, Duration::from_millis(300)) {
                let _ = stream.set_read_timeout(Some(Duration::from_millis(1500)));
                let _ = stream.set_write_timeout(Some(Duration::from_millis(300)));
                if stream.write_all(b"f\n").is_ok() {
                    let _ = stream.flush();
                    let mut reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);
                    let mut line = String::new();
                    if reader.read_line(&mut line).is_ok() {
                        if let Ok(freq) = line.trim().parse::<u64>() {
                            if freq > 100_000 {
                                Self::initial_sync(&mut stream, &mut reader, freq, state);
                                return Ok(stream);
                            }
                        }
                        last_err = format!(
                            "rigctld is running but the G90 did not answer (reply: '{}'). \
                             Check baud rate and CI-V address.",
                            line.trim()
                        );
                    } else {
                        last_err = "rigctld is running but the G90 did not answer the frequency query.".into();
                    }
                }
            }
            thread::sleep(Duration::from_millis(300));
        }
        Err(last_err)
    }

    fn initial_sync(stream: &mut TcpStream, reader: &mut BufReader<TcpStream>, freq: u64, state: &Arc<Mutex<SdrState>>) {
        {
            let mut st = state.lock().unwrap();
            st.lo_freq = freq;
            st.tune_freq = freq;
            println!("[Rigctld] Initial radio frequency: {} Hz", freq);
        }
        if stream.write_all(b"m\n").is_ok() {
            let _ = stream.flush();
            let mut mode_line = String::new();
            let mut width_line = String::new();
            if reader.read_line(&mut mode_line).is_ok() && reader.read_line(&mut width_line).is_ok() {
                if let Some(m) = parse_mode(&mode_line) {
                    let mut st = state.lock().unwrap();
                    if st.mode != m {
                        st.switch_mode(m);
                        println!("[Rigctld] Initial radio mode: {:?}", m);
                    }
                }
            }
        }
    }

    fn session(
        mut stream: TcpStream,
        state_arc: &Arc<Mutex<SdrState>>,
        cmd_rx: &crossbeam_channel::Receiver<RigCommand>,
        running: &Arc<AtomicBool>,
        child: &mut Option<Child>,
    ) -> SessionEnd {
        let _ = stream.set_read_timeout(Some(Duration::from_millis(100)));
        let _ = stream.set_write_timeout(Some(Duration::from_millis(100)));

        let mut reader_stream = match stream.try_clone() {
            Ok(s) => s,
            Err(e) => return SessionEnd::Lost(format!("TCP error: {}", e)),
        };
        let mut reader = BufReader::new(&mut reader_stream);

        let mut last_radio_rx = Instant::now();
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
                        let mode_str = m.name();
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
                        // Note: rigctld runs with ptt_type=None, so this is a no-op safety-wise.
                        let msg = format!("T {}\n", if p { 1 } else { 0 });
                        if stream.write_all(msg.as_bytes()).is_ok() {
                            let _ = stream.flush();
                            let mut resp = String::new();
                            let _ = reader.read_line(&mut resp);
                        }
                    }
                    RigCommand::Connect(cfg) => return SessionEnd::Reconnect(cfg),
                    RigCommand::Disconnect => return SessionEnd::Disconnect,
                }
            }

            // 2. Poll frequency from G90 -> SDR every 250ms
            if last_poll.elapsed() >= Duration::from_millis(250) {
                last_poll = Instant::now();
                if stream.write_all(b"f\n").is_err() {
                    return SessionEnd::Lost("Lost TCP connection to rigctld.".into());
                }
                let _ = stream.flush();

                let mut line = String::new();
                if reader.read_line(&mut line).is_ok() {
                    if let Ok(parsed_freq) = line.trim().parse::<u64>() {
                        if parsed_freq > 100_000 {
                            last_radio_rx = Instant::now();
                            {
                                let mut st = state_arc.lock().unwrap();
                                if !st.rigctld_connected {
                                    st.rigctld_connected = true;
                                    st.rig_status = RigStatus::Connected;
                                }
                            }

                            let was_just_sent = last_sent_freq
                                .map_or(false, |f| (f as i64 - parsed_freq as i64).abs() < 5);
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
                        if let Some(m) = parse_mode(&mode_line) {
                            last_radio_rx = Instant::now();

                            let in_grace_period = last_mode_sent.as_ref().map_or(false, |(sent_m, sent_t)| {
                                sent_t.elapsed() < Duration::from_millis(2000) && *sent_m != m
                            });

                            if !in_grace_period {
                                last_mode_sent = None;
                                let mut st = state_arc.lock().unwrap();
                                if st.mode != m {
                                    println!("[Rigctld] Radio mode reported: {:?}", m);
                                    st.switch_mode(m);
                                }
                            }
                        }
                    }
                }
            }

            // 4. rigctld process health
            if let Some(c) = child.as_mut() {
                if let Ok(Some(status)) = c.try_wait() {
                    child.take();
                    return SessionEnd::Lost(format!("rigctld exited unexpectedly ({}).\n{}", status, log_tail(4)));
                }
            }

            // 5. Radio responsiveness watchdog. We do NOT reopen the serial port automatically
            //    (every open pulses DTR/RTS); instead the user is prompted to reconnect.
            if last_radio_rx.elapsed() > Duration::from_millis(2000) {
                let mut st = state_arc.lock().unwrap();
                if st.rigctld_connected {
                    st.rigctld_connected = false;
                    st.rig_status = RigStatus::Connecting("G90 not responding…".into());
                    println!("[Rigctld] G90 stopped responding on serial interface.");
                }
            }
            if last_radio_rx.elapsed() > Duration::from_secs(6) {
                return SessionEnd::Lost(
                    "The G90 stopped answering frequency queries for 6 seconds \
                     (radio powered off or cable unplugged?)."
                        .into(),
                );
            }

            thread::sleep(Duration::from_millis(20));
        }
        SessionEnd::Shutdown
    }
}

impl Drop for RigClient {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        // Join so the worker can stop rigctld cleanly (serial port closed, lines LOW).
        if let Some(h) = self.worker.take() {
            let _ = h.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_and_resolve_serial_devices() {
        let devs = list_available_serial_devices();
        let resolved = resolve_serial_device("Auto-Detect");
        if !devs.is_empty() {
            assert!(resolved.is_some());
        }
        // Non-existent device resolution falls back to auto-detect
        let bogus = resolve_serial_device("/dev/non_existent_device_xyz123");
        assert_eq!(bogus, find_serial_device());
    }

    #[test]
    fn test_rigctld_args_force_lines_low() {
        let cfg = RigConfig {
            serial_device: "/dev/ttyUSB0".into(),
            baud: 19200,
            civ_address: 0x70,
            hamlib_model: crate::state::HAMLIB_MODEL_G90,
            host: "127.0.0.1".into(),
            port: 4532,
        };
        let conf = cfg.hamlib_conf();
        assert!(conf.contains("dtr_state=OFF"));
        assert!(conf.contains("rts_state=OFF"));
        assert!(conf.contains("ptt_type=None"));
        assert!(conf.contains("civaddr=0x70"));
        assert!(cfg.command_line().contains("-m 3088"));
    }
}
