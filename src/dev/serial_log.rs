//! The `--serial-log` file sink.
//!
//! Both serial backends that capture guest output — the TCP tee used by an
//! ordinary run and the in-process CI console — share one [`SerialLog`].
//! Keeping the buffering and formatting here means a line becomes visible at
//! the same moment in both modes, and that the raw transcript (timestamps
//! off) is byte-for-byte what the guest emitted.
//!
//! Flush policy: a completed line (one ending in `\n`) is flushed as soon as
//! it is written; a partial line is flushed after at most
//! [`PARTIAL_LINE_FLUSH`] by a small flusher thread, because a prompt may be
//! the last thing the guest writes for a long time. A byte-at-a-time flush
//! was deliberately not used — it made the writer syscall-bound on the SCC TX
//! thread.

use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::Path;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;

use crate::config::{format_unix_utc_iso_micros, SerialLogTimestamps};
use crate::cpu::mips_core::{CyclesPtr, NS_PER_GUEST_CYCLE};

/// Longest a partial line may stay buffered before a `tail -f` sees it.
pub const PARTIAL_LINE_FLUSH: Duration = Duration::from_millis(100);

/// Append-only guest-console capture with optional per-line timestamps.
pub struct SerialLog {
    mode: SerialLogTimestamps,
    /// Address of the CPU's live `Hot::cycles`, once the CPU exists.
    /// Null until then, and read as 0.
    cycles: AtomicPtr<u64>,
    inner: Mutex<Inner>,
}

struct Inner {
    out: BufWriter<File>,
    /// True when the next byte starts a new line and must be stamped.
    at_line_start: bool,
    /// True when bytes have been written since the last flush.
    dirty: bool,
    last_flush: Instant,
}

impl SerialLog {
    /// Open `path` for appending, creating it if needed. The file is shared
    /// by every clone of the returned handle.
    ///
    /// A small flusher thread makes a partial line (a prompt, a progress bar)
    /// visible even when the guest goes quiet mid-line and never writes again.
    /// It holds only a `Weak` reference, so it goes away with the log.
    pub fn open(path: &Path, mode: SerialLogTimestamps) -> io::Result<Arc<Self>> {
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        let log = Arc::new(Self {
            mode,
            cycles: AtomicPtr::new(std::ptr::null_mut()),
            inner: Mutex::new(Inner {
                out: BufWriter::new(file),
                at_line_start: true,
                dirty: false,
                last_flush: Instant::now(),
            }),
        });
        let weak = Arc::downgrade(&log);
        let _ = std::thread::Builder::new()
            .name("serial-log-flush".to_string())
            .spawn(move || loop {
                std::thread::sleep(Duration::from_millis(50));
                let Some(log) = weak.upgrade() else { break };
                log.inner.lock().flush_if_stale();
            });
        Ok(log)
    }

    /// Wire the live guest clock. Called once the CPU is inside its final
    /// allocation; until then guest stamps read zero.
    pub fn set_guest_cycles(&self, cycles: CyclesPtr) {
        self.cycles
            .store(cycles.raw() as *mut u64, Ordering::Release);
    }

    /// Stop reading the CPU's cycle counter (used before that CPU is freed).
    pub fn clear_guest_cycles(&self) {
        self.cycles.store(std::ptr::null_mut(), Ordering::Release);
    }

    /// Append guest output. With timestamps on, every line start gets a
    /// prefix; the guest's own bytes (including `\r`) are otherwise passed
    /// through unchanged.
    pub fn write(&self, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        let mut inner = self.inner.lock();

        if self.mode == SerialLogTimestamps::Off {
            let _ = inner.out.write_all(data);
            inner.dirty = true;
            if data.contains(&b'\n') {
                inner.flush();
            } else {
                inner.flush_if_stale();
            }
            return;
        }

        let mut ended_line = false;
        for &byte in data {
            if inner.at_line_start {
                let prefix = self.line_prefix();
                let _ = inner.out.write_all(prefix.as_bytes());
                inner.at_line_start = false;
            }
            let _ = inner.out.write_all(&[byte]);
            if byte == b'\n' {
                inner.at_line_start = true;
                ended_line = true;
            }
        }

        inner.dirty = true;
        if ended_line {
            inner.flush();
        } else {
            inner.flush_if_stale();
        }
    }

    /// Push buffered bytes to the OS. Called at machine stop so a graceful
    /// exit cannot lose the last partial line.
    pub fn flush(&self) {
        self.inner.lock().flush();
    }

    fn guest_cycles(&self) -> u64 {
        let ptr = self.cycles.load(Ordering::Acquire);
        if ptr.is_null() {
            0
        } else {
            CyclesPtr::new(ptr).get()
        }
    }

    fn line_prefix(&self) -> String {
        match self.mode {
            SerialLogTimestamps::Off => String::new(),
            SerialLogTimestamps::Guest => {
                format!("[{}] ", format_guest_stamp(self.guest_cycles()))
            }
            SerialLogTimestamps::Wall => format!("[{}] ", format_wall_stamp()),
            SerialLogTimestamps::Both => format!(
                "[{}] [{}] ",
                format_guest_stamp(self.guest_cycles()),
                format_wall_stamp()
            ),
        }
    }
}

impl Drop for SerialLog {
    fn drop(&mut self) {
        self.inner.lock().flush();
    }
}

impl Inner {
    fn flush(&mut self) {
        let _ = self.out.flush();
        self.last_flush = Instant::now();
        self.dirty = false;
    }

    /// Flush a line that has stopped arriving mid-way (a prompt, a progress
    /// bar) without waiting for its newline or for process exit.
    fn flush_if_stale(&mut self) {
        if self.dirty && self.last_flush.elapsed() >= PARTIAL_LINE_FLUSH {
            self.flush();
        }
    }
}

/// `   12.345678`: seconds and microseconds, right-aligned to a stable width.
///
/// Guest time is the emulator's synthetic time base — retired cycles at
/// `NS_PER_GUEST_CYCLE` (10 ns) each, the same clock CP0 Count derives from —
/// not wall time. It advances only while the guest executes, so a slow or
/// paused host shows as a gap-free guest timeline.
fn format_guest_stamp(cycles: u64) -> String {
    let micros = (cycles as u128 * NS_PER_GUEST_CYCLE as u128 / 1_000) as u64;
    format!("{:>5}.{:06}", micros / 1_000_000, micros % 1_000_000)
}

fn format_wall_stamp() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format_unix_utc_iso_micros(now.as_secs() as i64, now.subsec_micros())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::AtomicU64;
    use std::sync::mpsc;
    use std::thread;

    fn temp_path(tag: &str) -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        std::env::temp_dir().join(format!(
            "iris-serial-log-{}-{}-{}.log",
            std::process::id(),
            tag,
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn raw_mode_is_a_byte_for_byte_transcript() {
        let path = temp_path("raw");
        let text = b"\x00\xffbinary\r\nline two\nno newline at end";
        let log = SerialLog::open(&path, SerialLogTimestamps::Off).unwrap();
        for byte in text {
            log.write(&[*byte]);
        }
        log.flush();
        assert_eq!(std::fs::read(&path).unwrap(), text);
        drop(log);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_partial_line_flushes_without_another_write() {
        let path = temp_path("partial");
        let log = SerialLog::open(&path, SerialLogTimestamps::Off).unwrap();
        log.write(b"login: ");
        assert_eq!(std::fs::read(&path).unwrap(), b"", "not stale yet");

        // Pretend the prompt arrived more than one interval ago. A prompt has
        // no newline, and the guest may write nothing else; the flusher thread
        // is what makes it visible to `tail -f`.
        {
            let mut inner = log.inner.lock();
            inner.last_flush = Instant::now() - PARTIAL_LINE_FLUSH;
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut got = Vec::new();
        while Instant::now() < deadline {
            got = std::fs::read(&path).unwrap_or_default();
            if got == b"login: " {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(got, b"login: ");

        drop(log);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn guest_stamps_prefix_each_line_once() {
        let path = temp_path("guest");
        let clock = 1_234_567_800u64; // 12.345678 s at 10 ns/cycle.
        let log = SerialLog::open(&path, SerialLogTimestamps::Guest).unwrap();
        log.set_guest_cycles(CyclesPtr::new(&clock as *const u64));
        for byte in b"first\r\nsecond\n\nthird" {
            log.write(&[*byte]);
        }
        log.flush();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "[   12.345678] first\r\n\
             [   12.345678] second\n\
             [   12.345678] \n\
             [   12.345678] third"
        );
        drop(log);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_carriage_return_does_not_start_a_new_stamp() {
        let path = temp_path("cr");
        let clock = 0u64;
        let log = SerialLog::open(&path, SerialLogTimestamps::Both).unwrap();
        log.set_guest_cycles(CyclesPtr::new(&clock as *const u64));
        for byte in b"progress 1\rprogress 2\n" {
            log.write(&[*byte]);
        }
        log.flush();
        let got = std::fs::read_to_string(&path).unwrap();
        assert_eq!(got.matches("[    0.000000] ").count(), 1, "{got:?}");
        assert!(got.contains("progress 1\rprogress 2\n"), "{got:?}");
        drop(log);
        let _ = std::fs::remove_file(path);
    }

    /// The point of the line flush: a reader in another thread must see a
    /// completed line while the writer thread is still alive and the file is
    /// still open. Without the newline flush the bytes sit in the BufWriter
    /// until the log is dropped, which is exactly the #105 bug.
    #[test]
    fn a_completed_line_is_visible_to_another_thread_without_closing_the_log() {
        let path = temp_path("prompt");
        let log = SerialLog::open(&path, SerialLogTimestamps::Off).unwrap();
        let (line_written_tx, line_written_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();

        let writer = log.clone();
        let thread = thread::spawn(move || {
            for byte in b"kernel: up\n" {
                writer.write(&[*byte]);
            }
            line_written_tx.send(()).unwrap();
            // Stay alive, and keep the log open, until the reader is done.
            let _ = release_rx.recv_timeout(Duration::from_secs(10));
        });

        line_written_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("writer signalled the line");

        let deadline = Instant::now() + Duration::from_secs(2);
        let mut got = Vec::new();
        while Instant::now() < deadline {
            got = std::fs::read(&path).unwrap_or_default();
            if got.ends_with(b"kernel: up\n") {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }

        let _ = release_tx.send(());
        thread.join().unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(
            got, b"kernel: up\n",
            "a completed line was not readable while the writer was alive"
        );
    }

    #[test]
    fn guest_stamp_has_a_stable_width_and_microsecond_precision() {
        assert_eq!(format_guest_stamp(0), "    0.000000");
        assert_eq!(format_guest_stamp(100_000_000), "    1.000000");
        assert_eq!(format_guest_stamp(1_234_567_800), "   12.345678");
        assert_eq!(format_guest_stamp(12_345_678_900), "  123.456789");
    }
}
