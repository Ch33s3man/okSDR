//! PTT-safe, low-level serial access for the Xiegu G90.
//!
//! WHY THIS EXISTS
//! ---------------
//! The G90 / DE-19 (CH340) interface keys the transmitter whenever DTR or RTS is asserted.
//! Linux *always* raises DTR + RTS when a tty is opened. If nothing lowers them again they
//! stay high. The old launcher ran `stty -F <dev> 19200 -hupcl`, which opened the port
//! (raising DTR/RTS) and then closed it with HUPCL disabled, leaving both lines HIGH and the
//! G90 stuck in TX until `rigctld` happened to open the port and clear them. On a system
//! without Hamlib installed nothing ever cleared them.
//!
//! Every function in this module therefore:
//!   * opens the port and IMMEDIATELY drops DTR + RTS (TIOCMBIC),
//!   * disables all hardware/software flow control,
//!   * keeps HUPCL set so the kernel drops DTR/RTS on close,
//!   * never sends anything except read-only CI-V queries (read frequency / read mode).

use std::ffi::CString;
use std::io;
use std::os::unix::io::RawFd;
use std::time::{Duration, Instant};

/// Default CI-V address of the Xiegu G90.
pub const G90_DEFAULT_CIV: u8 = 0x70;
/// CI-V address used by the controller (PC). 0xE0 is the conventional value.
pub const CONTROLLER_CIV: u8 = 0xE0;
/// Baud rates selectable on the G90 (menu: CI-V baud).
pub const G90_BAUD_RATES: [u32; 6] = [4800, 9600, 19200, 38400, 57600, 115200];
/// The G90 factory default CAT baud rate.
pub const G90_DEFAULT_BAUD: u32 = 19200;

fn baud_constant(baud: u32) -> io::Result<libc::speed_t> {
    Ok(match baud {
        1200 => libc::B1200,
        2400 => libc::B2400,
        4800 => libc::B4800,
        9600 => libc::B9600,
        19200 => libc::B19200,
        38400 => libc::B38400,
        57600 => libc::B57600,
        115200 => libc::B115200,
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("Unsupported baud rate {}", baud),
            ))
        }
    })
}

/// Turn a raw OS error into a message a ham can act on.
pub fn describe_open_error(path: &str, e: &io::Error) -> String {
    match e.raw_os_error() {
        Some(libc::ENOENT) => format!("{} does not exist. Is the CAT cable plugged in?", path),
        Some(libc::EACCES) | Some(libc::EPERM) => format!(
            "Permission denied opening {}. Add yourself to the 'dialout' group \
             (sudo usermod -aG dialout $USER) and log out/in.",
            path
        ),
        Some(libc::EBUSY) => format!("{} is busy (another program has it open exclusively).", path),
        _ => format!("Could not open {}: {}", path, e),
    }
}

/// A raw serial port that is guaranteed to hold DTR and RTS LOW for its whole lifetime.
pub struct G90Port {
    fd: RawFd,
}

impl G90Port {
    /// Open `path` at `baud`, 8N1, raw, no flow control, DTR + RTS forced LOW.
    pub fn open(path: &str, baud: u32) -> io::Result<Self> {
        let c_path = CString::new(path)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid device path"))?;

        // O_NONBLOCK: don't wait for carrier detect. O_NOCTTY: never become our controlling tty.
        let fd = unsafe {
            libc::open(
                c_path.as_ptr(),
                libc::O_RDWR | libc::O_NOCTTY | libc::O_NONBLOCK | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let port = G90Port { fd };

        // The kernel just raised DTR/RTS on open. Drop them before doing anything else.
        port.set_dtr_rts_low()?;
        port.configure(baud)?;
        // Some USB-serial drivers re-assert lines on a termios change; drop them again.
        port.set_dtr_rts_low()?;
        unsafe {
            libc::tcflush(port.fd, libc::TCIOFLUSH);
        }
        Ok(port)
    }

    /// (Re)configure line settings. Safe to call repeatedly (used by the baud scanner).
    pub fn configure(&self, baud: u32) -> io::Result<()> {
        let speed = baud_constant(baud)?;
        let mut t: libc::termios = unsafe { std::mem::zeroed() };
        if unsafe { libc::tcgetattr(self.fd, &mut t) } != 0 {
            return Err(io::Error::last_os_error());
        }
        unsafe { libc::cfmakeraw(&mut t) };
        // 8 data bits, 1 stop bit, no parity, receiver on, ignore modem control lines.
        t.c_cflag &= !(libc::CSIZE | libc::PARENB | libc::CSTOPB | libc::CRTSCTS);
        t.c_cflag |= libc::CS8 | libc::CLOCAL | libc::CREAD;
        // HUPCL ON: when the port is closed the kernel drops DTR/RTS (= RX for the G90).
        t.c_cflag |= libc::HUPCL;
        t.c_iflag &= !(libc::IXON | libc::IXOFF | libc::IXANY);
        t.c_cc[libc::VMIN] = 0;
        t.c_cc[libc::VTIME] = 0;
        unsafe {
            libc::cfsetispeed(&mut t, speed);
            libc::cfsetospeed(&mut t, speed);
        }
        if unsafe { libc::tcsetattr(self.fd, libc::TCSANOW, &t) } != 0 {
            return Err(io::Error::last_os_error());
        }
        self.set_dtr_rts_low()?;
        unsafe {
            libc::tcflush(self.fd, libc::TCIOFLUSH);
        }
        Ok(())
    }

    /// Clear DTR and RTS. For the G90 both must ALWAYS be low (high = transmit).
    pub fn set_dtr_rts_low(&self) -> io::Result<()> {
        let bits: libc::c_int = libc::TIOCM_DTR | libc::TIOCM_RTS;
        if unsafe { libc::ioctl(self.fd, libc::TIOCMBIC, &bits) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// Read back the actual modem-control line state: (dtr_high, rts_high).
    pub fn modem_lines(&self) -> io::Result<(bool, bool)> {
        let mut bits: libc::c_int = 0;
        if unsafe { libc::ioctl(self.fd, libc::TIOCMGET, &mut bits) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok((bits & libc::TIOCM_DTR != 0, bits & libc::TIOCM_RTS != 0))
    }

    fn write_all(&self, mut data: &[u8], timeout: Duration) -> io::Result<()> {
        let deadline = Instant::now() + timeout;
        while !data.is_empty() {
            let n = unsafe { libc::write(self.fd, data.as_ptr() as *const libc::c_void, data.len()) };
            if n > 0 {
                data = &data[n as usize..];
                continue;
            }
            let err = io::Error::last_os_error();
            if n < 0 && err.kind() != io::ErrorKind::WouldBlock && err.kind() != io::ErrorKind::Interrupted {
                return Err(err);
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(io::ErrorKind::TimedOut, "serial write timed out"));
            }
            self.poll(libc::POLLOUT, Duration::from_millis(20))?;
        }
        unsafe {
            libc::tcdrain(self.fd);
        }
        Ok(())
    }

    fn poll(&self, events: libc::c_short, timeout: Duration) -> io::Result<bool> {
        let mut pfd = libc::pollfd { fd: self.fd, events, revents: 0 };
        let r = unsafe { libc::poll(&mut pfd, 1, timeout.as_millis().min(i32::MAX as u128) as libc::c_int) };
        if r < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                return Ok(false);
            }
            return Err(err);
        }
        if pfd.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "serial device disconnected"));
        }
        Ok(r > 0)
    }

    /// Read whatever arrives until `deadline`, or until `done(buf)` returns true.
    fn read_until(&self, deadline: Instant, mut done: impl FnMut(&[u8]) -> bool) -> io::Result<Vec<u8>> {
        let mut buf = Vec::with_capacity(64);
        let mut chunk = [0u8; 64];
        loop {
            let now = Instant::now();
            if now >= deadline {
                return Ok(buf);
            }
            if !self.poll(libc::POLLIN, deadline - now)? {
                continue;
            }
            let n = unsafe { libc::read(self.fd, chunk.as_mut_ptr() as *mut libc::c_void, chunk.len()) };
            if n > 0 {
                buf.extend_from_slice(&chunk[..n as usize]);
                if done(&buf) {
                    return Ok(buf);
                }
            } else if n == 0 {
                // No data (VMIN=0). Keep waiting until deadline.
                std::thread::sleep(Duration::from_millis(5));
            } else {
                let err = io::Error::last_os_error();
                if err.kind() != io::ErrorKind::WouldBlock && err.kind() != io::ErrorKind::Interrupted {
                    return Err(err);
                }
            }
        }
    }

    /// Send a read-only CI-V query and return `(responder_address, payload)` of the reply.
    /// The command echo (G90 CI-V bus echo) is ignored automatically.
    pub fn civ_query(&self, civ: u8, cmd: u8, timeout: Duration) -> io::Result<Option<(u8, Vec<u8>)>> {
        unsafe {
            libc::tcflush(self.fd, libc::TCIFLUSH);
        }
        let frame = [0xFE, 0xFE, civ, CONTROLLER_CIV, cmd, 0xFD];
        self.write_all(&frame, Duration::from_millis(300))?;

        let deadline = Instant::now() + timeout;
        let mut found: Option<(u8, Vec<u8>)> = None;
        self.read_until(deadline, |buf| {
            for f in split_civ_frames(buf) {
                // A reply is addressed TO the controller, FROM the radio, for the same command.
                if f.len() >= 3 && f[0] == CONTROLLER_CIV && f[2] == cmd {
                    found = Some((f[1], f[3..].to_vec()));
                    return true;
                }
            }
            false
        })?;
        Ok(found)
    }
}

impl Drop for G90Port {
    fn drop(&mut self) {
        let _ = self.set_dtr_rts_low();
        unsafe {
            libc::close(self.fd);
        }
    }
}

/// Split a byte stream into CI-V frame bodies (bytes between `FE FE` and `FD`).
pub fn split_civ_frames(buf: &[u8]) -> Vec<&[u8]> {
    let mut frames = Vec::new();
    let mut i = 0;
    while i + 1 < buf.len() {
        if buf[i] == 0xFE && buf[i + 1] == 0xFE {
            let mut start = i + 2;
            // Tolerate extra preamble bytes.
            while start < buf.len() && buf[start] == 0xFE {
                start += 1;
            }
            if let Some(end_rel) = buf[start..].iter().position(|&b| b == 0xFD) {
                frames.push(&buf[start..start + end_rel]);
                i = start + end_rel + 1;
                continue;
            } else {
                break;
            }
        }
        i += 1;
    }
    frames
}

/// Decode an Icom-style 5-byte little-endian BCD frequency.
pub fn decode_bcd_freq(data: &[u8]) -> Option<u64> {
    if data.len() < 4 {
        return None;
    }
    let mut freq: u64 = 0;
    let mut mult: u64 = 1;
    for &b in data.iter().take(5) {
        let lo = (b & 0x0F) as u64;
        let hi = (b >> 4) as u64;
        if lo > 9 || hi > 9 {
            return None;
        }
        freq += (hi * 10 + lo) * mult;
        mult *= 100;
    }
    Some(freq)
}

pub fn civ_mode_name(code: u8) -> &'static str {
    match code {
        0x00 => "LSB",
        0x01 => "USB",
        0x02 => "AM",
        0x03 => "CW",
        0x04 => "RTTY",
        0x05 => "FM",
        0x07 => "CW-R",
        0x08 => "RTTY-R",
        _ => "?",
    }
}

/// Result of a direct, read-only CI-V probe of the G90.
#[derive(Debug, Clone, Default)]
pub struct ProbeReport {
    pub ok: bool,
    pub device: String,
    pub baud: u32,
    pub freq_hz: Option<u64>,
    pub mode: Option<String>,
    pub responder_civ: Option<u8>,
    /// Line state read back from the driver after the probe: (DTR high, RTS high).
    pub lines: Option<(bool, bool)>,
    pub message: String,
}

impl ProbeReport {
    fn fail(device: &str, baud: u32, message: String) -> Self {
        Self { ok: false, device: device.to_string(), baud, message, ..Default::default() }
    }
}

fn probe_open_port(port: &G90Port, device: &str, baud: u32, civ: u8) -> ProbeReport {
    let mut report = ProbeReport { device: device.to_string(), baud, ..Default::default() };
    report.lines = port.modem_lines().ok();

    match port.civ_query(civ, 0x03, Duration::from_millis(700)) {
        Ok(Some((from, data))) => {
            report.responder_civ = Some(from);
            report.freq_hz = decode_bcd_freq(&data);
        }
        Ok(None) => {}
        Err(e) => {
            report.message = format!("Serial I/O error on {}: {}", device, e);
            return report;
        }
    }

    match report.freq_hz {
        Some(f) if f > 100_000 => {
            if let Ok(Some((_, data))) = port.civ_query(civ, 0x04, Duration::from_millis(500)) {
                if let Some(&m) = data.first() {
                    report.mode = Some(civ_mode_name(m).to_string());
                }
            }
            report.lines = port.modem_lines().ok().or(report.lines);
            report.ok = true;
            report.message = format!(
                "G90 answered: {:.4} MHz{} @ {} baud",
                f as f64 / 1e6,
                report.mode.as_ref().map(|m| format!(" {}", m)).unwrap_or_default(),
                baud
            );
        }
        _ => {
            report.message = format!(
                "No CI-V reply from address 0x{:02X} on {} @ {} baud. Check that the G90 is \
                 powered on, the CAT cable is connected, and the baud rate / CI-V address \
                 match the radio's menu.",
                civ, device, baud
            );
        }
    }
    report
}

/// Open the port with DTR/RTS low and ask the G90 for its frequency + mode. Read-only;
/// never touches PTT and never needs Hamlib.
pub fn probe_g90(device: &str, baud: u32, civ: u8) -> ProbeReport {
    let port = match G90Port::open(device, baud) {
        Ok(p) => p,
        Err(e) => return ProbeReport::fail(device, baud, describe_open_error(device, &e)),
    };
    probe_open_port(&port, device, baud, civ)
}

/// Try every G90 baud rate (preferred one first) using a SINGLE open of the port, so the
/// kernel's open-time DTR/RTS pulse only happens once.
pub fn scan_g90_baud(device: &str, preferred: u32, civ: u8) -> ProbeReport {
    let port = match G90Port::open(device, preferred) {
        Ok(p) => p,
        Err(e) => return ProbeReport::fail(device, preferred, describe_open_error(device, &e)),
    };
    let mut order = vec![preferred];
    order.extend(G90_BAUD_RATES.iter().copied().filter(|&b| b != preferred));

    let mut last = ProbeReport::fail(device, preferred, "No baud rates tried".into());
    for baud in order {
        if let Err(e) = port.configure(baud) {
            last = ProbeReport::fail(device, baud, format!("Could not set {} baud: {}", baud, e));
            continue;
        }
        std::thread::sleep(Duration::from_millis(50));
        let r = probe_open_port(&port, device, baud, civ);
        if r.ok {
            return r;
        }
        last = r;
    }
    last.message = format!(
        "G90 did not answer at any baud rate ({}) on {} with CI-V address 0x{:02X}.",
        G90_BAUD_RATES.iter().map(|b| b.to_string()).collect::<Vec<_>>().join(", "),
        device,
        civ
    );
    last
}

/// Emergency un-key: open the port, force DTR + RTS low, close (HUPCL keeps them low).
pub fn force_lines_low(device: &str) -> Result<String, String> {
    match G90Port::open(device, G90_DEFAULT_BAUD) {
        Ok(port) => {
            let lines = port.modem_lines().ok();
            drop(port);
            Ok(match lines {
                Some((false, false)) => format!("DTR and RTS forced LOW on {}.", device),
                Some((d, r)) => format!(
                    "Requested LOW on {} but driver reports DTR={} RTS={}.",
                    device,
                    if d { "HIGH" } else { "LOW" },
                    if r { "HIGH" } else { "LOW" }
                ),
                None => format!("DTR and RTS cleared on {}.", device),
            })
        }
        Err(e) => Err(describe_open_error(device, &e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bcd_decoding() {
        // 7.185000 MHz -> 00 50 18 07 00
        assert_eq!(decode_bcd_freq(&[0x00, 0x50, 0x18, 0x07, 0x00]), Some(7_185_000));
        // 14.285000 MHz -> 00 50 28 14 00
        assert_eq!(decode_bcd_freq(&[0x00, 0x50, 0x28, 0x14, 0x00]), Some(14_285_000));
        assert_eq!(decode_bcd_freq(&[0xFA, 0x00, 0x00, 0x00, 0x00]), None);
    }

    #[test]
    fn frame_splitting_skips_echo() {
        // Echo of our query followed by the radio's reply.
        let stream = [
            0xFE, 0xFE, 0x70, 0xE0, 0x03, 0xFD, // echo
            0xFE, 0xFE, 0xE0, 0x70, 0x03, 0x00, 0x50, 0x18, 0x07, 0x00, 0xFD, // reply
        ];
        let frames = split_civ_frames(&stream);
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0], &[0x70, 0xE0, 0x03]);
        assert_eq!(frames[1][0], CONTROLLER_CIV);
        assert_eq!(decode_bcd_freq(&frames[1][3..]), Some(7_185_000));
    }

    #[test]
    fn probe_missing_device_reports_error() {
        let r = probe_g90("/dev/definitely_not_a_real_device_999", 19200, G90_DEFAULT_CIV);
        assert!(!r.ok);
        assert!(r.message.contains("does not exist"));
    }
}
