//! One live `iris` per instance state directory.
//!
//! Every CLI start takes an exclusive, non-blocking advisory lock on
//! `<state-dir>/iris.lock` (`iris.lock` in the working directory in
//! single-instance mode) before constructing the Machine. The *lock*, not the
//! PID written inside the file, is authoritative: the kernel releases it on
//! process death however that happens (crash, `SIGKILL`, power loss), so a
//! stale file can never keep an instance out forever. The record inside it
//! exists so a refused start can name the owner, and so `--replace` can check
//! that the PID it is about to signal is still the process that wrote the
//! record rather than an unrelated process that reused the PID.
//!
//! Startup also probes the listeners the instance is about to bind (monitor,
//! serial A/B outside `--ci`, the CI socket and the optional GDB stub) so a
//! held port is a clear error before any disk or NVRAM is touched.
//!
//! # Cleanup
//!
//! [`InstanceLock`]'s `Drop` removes the CI socket and lock file and releases
//! the lock. Most CLI exits go through `std::process::exit`, which skips
//! destructors, so the binary calls [`cleanup_for_exit`] at those points. On
//! Unix, [`install_signal_cleanup`] also unlinks both paths from a
//! `SIGINT`/`SIGTERM`/`SIGHUP` handler, then re-raises the signal. That handler
//! is deliberately async-signal-safe and **does not stop the guest**: NVRAM is
//! saved by `Machine::stop`, which takes locks and must never run in a signal
//! handler. Quit through the monitor or `iris-ci quit` for a clean shutdown;
//! `--replace` prefers that route and only sends SIGTERM to a validated owner
//! that has no CI socket.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::ci::SocketState;
use crate::config::{ci_socket_is_tcp, MachineConfig};
use crate::monitor::DEFAULT_PORT;
use crate::state::StatePaths;

/// Name of the ownership file inside an instance state directory.
pub const LOCK_FILE: &str = "iris.lock";

/// How long `--replace` waits for the old owner to release the lock and its
/// CI endpoint.
const REPLACE_TIMEOUT: Duration = Duration::from_secs(10);
const REPLACE_POLL: Duration = Duration::from_millis(50);
/// How long endpoint probes retry a busy port. A replaced instance releases
/// its listeners within this window; an unrelated holder will not.
const PROBE_RETRY: Duration = Duration::from_secs(2);

// ---------------------------------------------------------------------------
// Owner record
// ---------------------------------------------------------------------------

/// The contents of `iris.lock`, written by the process that holds the lock.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstanceRecord {
    /// Process holding the lock.
    pub pid: u32,
    /// Fleet instance id when one was derived.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance: Option<u32>,
    /// State directory, as configured (relative or absolute).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_dir: Option<String>,
    /// CI socket (Unix path or `host:port`) when `--ci` is active. `None`
    /// otherwise: a configured-but-unused default must not be quit by mistake.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ci_socket: Option<String>,
    /// Monitor port this instance binds.
    pub monitor_port: u16,
    /// Wall-clock start time, for humans reading the file.
    pub started_unix: u64,
    /// Platform process start marker. Compared before signalling a PID, so a
    /// recycled PID is refused rather than stopped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_marker: Option<String>,
    /// Executable path, for diagnostics only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exe: Option<String>,
}

impl InstanceRecord {
    /// The record for this process under `cfg`.
    pub fn current(cfg: &MachineConfig) -> Self {
        Self {
            pid: std::process::id(),
            instance: cfg.instance_id,
            state_dir: cfg.state_dir.clone(),
            ci_socket: cfg.ci.then(|| cfg.ci_socket.clone()),
            monitor_port: cfg.monitor_port.unwrap_or(DEFAULT_PORT),
            started_unix: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            start_marker: process_start_marker(std::process::id()),
            exe: std::env::current_exe()
                .ok()
                .map(|p| p.display().to_string()),
        }
    }

    /// One line naming the owner, for a refused-start message.
    pub fn describe(&self) -> String {
        let mut out = format!("pid {}", self.pid);
        if let Some(id) = self.instance {
            out.push_str(&format!(" (instance {id})"));
        }
        if let Some(exe) = self.exe.as_deref() {
            if let Some(name) = Path::new(exe).file_name() {
                out.push_str(&format!(", {}", name.to_string_lossy()));
            }
        }
        if let Some(dir) = self.state_dir.as_deref() {
            out.push_str(&format!(", state dir {dir}"));
        }
        if let Some(socket) = self.ci_socket.as_deref() {
            out.push_str(&format!(", CI socket {socket}"));
        }
        out
    }
}

/// The lock file for `cfg`: beside the derived state, or in the working
/// directory in single-instance mode.
pub fn lock_path(cfg: &MachineConfig) -> PathBuf {
    match cfg.state_dir.as_deref() {
        Some(dir) => StatePaths::under(dir).instance_lock(),
        None => StatePaths::neutral().instance_lock(),
    }
}

// ---------------------------------------------------------------------------
// Lock errors
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum LockError {
    /// Another live process holds the lock and `--replace` was not given.
    AlreadyRunning {
        path: PathBuf,
        owner: Option<InstanceRecord>,
    },
    /// `--replace` would have had to stop something it could not identify.
    ReplaceRefused { path: PathBuf, reason: String },
    /// Creating, reading or writing the lock file failed.
    Io { path: PathBuf, error: String },
}

impl std::fmt::Display for LockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LockError::AlreadyRunning { path, owner } => {
                write!(
                    f,
                    "another iris instance is already using this state (lock {})",
                    path.display()
                )?;
                match owner {
                    Some(o) => write!(f, ": {}.", o.describe())?,
                    None => write!(f, "; its owner record is unreadable.")?,
                }
                write!(
                    f,
                    " Use a different --instance or --state-dir, or --replace to stop it."
                )
            }
            LockError::ReplaceRefused { path, reason } => {
                write!(
                    f,
                    "cannot replace the instance holding {}: {reason}",
                    path.display()
                )
            }
            LockError::Io { path, error } => {
                write!(f, "instance lock {}: {error}", path.display())
            }
        }
    }
}

impl std::error::Error for LockError {}

fn io_error(path: &Path, error: impl std::fmt::Display) -> LockError {
    LockError::Io {
        path: path.to_path_buf(),
        error: error.to_string(),
    }
}

// ---------------------------------------------------------------------------
// The lock guard
// ---------------------------------------------------------------------------

/// Exclusive ownership of one instance state directory.
#[derive(Debug)]
pub struct InstanceLock {
    path: PathBuf,
    file: Option<File>,
    /// Unix CI socket to remove on cleanup. `None` for TCP or when CI is off.
    socket: Option<PathBuf>,
    replaced: Option<u32>,
}

impl InstanceLock {
    /// Take the lock, or explain who has it.
    ///
    /// With `replace`, a live owner is asked to quit through its CI socket
    /// first (a clean `Machine::stop`, so NVRAM is saved). Only when that is
    /// not possible is its recorded PID signalled, and only after its start
    /// marker has been verified. An unreadable record or a recycled PID is
    /// refused, never guessed at.
    pub fn acquire(
        path: impl AsRef<Path>,
        record: InstanceRecord,
        replace: bool,
    ) -> Result<Self, LockError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|e| io_error(&path, e))?;
            }
        }

        let replaced_until = Instant::now() + REPLACE_TIMEOUT;
        let mut replaced = None;
        let mut attempts = 0u32;
        loop {
            let mut file = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .open(&path)
                .map_err(|e| io_error(&path, e))?;
            match file.try_lock() {
                Ok(()) => {
                    // The previous owner may have unlinked the path between our
                    // open and lock. We would then hold a lock on an unlinked
                    // inode while a third process creates a fresh lock file.
                    // Start again against the current path.
                    if !same_file(&file, &path) {
                        attempts += 1;
                        if attempts > 16 {
                            return Err(LockError::ReplaceRefused {
                                path,
                                reason: "the lock file kept being replaced while acquiring it"
                                    .to_string(),
                            });
                        }
                        continue;
                    }
                    write_record(&mut file, &record).map_err(|e| io_error(&path, e))?;
                    let socket = record
                        .ci_socket
                        .as_deref()
                        .filter(|p| !ci_socket_is_tcp(p))
                        .map(PathBuf::from);
                    return Ok(Self {
                        path,
                        file: Some(file),
                        socket,
                        replaced,
                    });
                }
                Err(TryLockError::WouldBlock) => {
                    let owner = read_record(&path);
                    if !replace {
                        return Err(LockError::AlreadyRunning { path, owner });
                    }
                    if replaced.is_none() {
                        let owner = owner.as_ref().ok_or_else(|| LockError::ReplaceRefused {
                            path: path.clone(),
                            reason: "the lock is held but its owner record is unreadable; \
                                     refusing to guess which process to stop"
                                .to_string(),
                        })?;
                        if owner.pid == std::process::id() {
                            return Err(LockError::ReplaceRefused {
                                path: path.clone(),
                                reason: "the lock names this process; refusing to signal myself"
                                    .to_string(),
                            });
                        }
                        replace_owner(&path, owner)?;
                        replaced = Some(owner.pid);
                    }
                    if Instant::now() >= replaced_until {
                        return Err(LockError::ReplaceRefused {
                            path,
                            reason: format!(
                                "the owner did not release it within {} s",
                                REPLACE_TIMEOUT.as_secs()
                            ),
                        });
                    }
                    std::thread::sleep(REPLACE_POLL);
                }
                Err(TryLockError::Error(e)) => return Err(io_error(&path, e)),
            }
        }
    }

    /// The lock file being held.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// PID of the instance `--replace` stopped, when it stopped one.
    pub fn replaced_pid(&self) -> Option<u32> {
        self.replaced
    }

    /// Remove the CI socket and lock file, then release the lock. Idempotent.
    pub fn cleanup(&mut self) {
        if let Some(socket) = self.socket.take() {
            let _ = std::fs::remove_file(socket);
        }
        if let Some(file) = self.file.take() {
            // Unlink before unlocking: a waiter that opened this inode while we
            // held the lock then fails its same-file check and retries against
            // a fresh path, rather than acquiring a lock on an unlinked inode.
            #[cfg(unix)]
            let _ = std::fs::remove_file(&self.path);
            // Windows cannot check that the path still names the held file
            // without unstable APIs, so keep the inode and clear its contents.
            // The OS releases the lock when the handle closes.
            #[cfg(not(unix))]
            let _ = file.set_len(0);
            drop(file);
        }
    }
}

impl Drop for InstanceLock {
    fn drop(&mut self) {
        self.cleanup();
    }
}

fn write_record(file: &mut File, record: &InstanceRecord) -> std::io::Result<()> {
    file.seek(SeekFrom::Start(0))?;
    file.set_len(0)?;
    let json = serde_json::to_vec(record)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    file.write_all(&json)?;
    file.write_all(b"\n")?;
    file.flush()
}

fn read_record(path: &Path) -> Option<InstanceRecord> {
    let mut text = String::new();
    File::open(path).ok()?.read_to_string(&mut text).ok()?;
    serde_json::from_str(text.trim()).ok()
}

#[cfg(unix)]
fn same_file(file: &File, path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (file.metadata(), std::fs::metadata(path)) {
        (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino(),
        _ => false,
    }
}

#[cfg(windows)]
fn same_file(_file: &File, _path: &Path) -> bool {
    // Windows cleanup truncates the lock instead of unlinking it (see
    // `InstanceLock::cleanup`), so the path always names the same file and
    // there is no unlinking race to detect.
    true
}

#[cfg(not(any(unix, windows)))]
fn same_file(_file: &File, _path: &Path) -> bool {
    true
}

// ---------------------------------------------------------------------------
// Replacing a live owner
// ---------------------------------------------------------------------------

fn replace_owner(lock_path: &Path, owner: &InstanceRecord) -> Result<(), LockError> {
    // The CI socket is the only route that runs Machine::stop and therefore
    // saves NVRAM. Use it when it answers as the recorded process.
    if let Some(socket) = owner.ci_socket.as_deref() {
        match crate::ci::probe_socket(socket) {
            SocketState::Live { pid: Some(pid) } if pid == owner.pid => {
                return crate::ci::request_quit(socket).map_err(|e| LockError::ReplaceRefused {
                    path: lock_path.to_path_buf(),
                    reason: format!(
                        "asked pid {} to quit over {} but the request failed: {e}",
                        owner.pid, socket
                    ),
                });
            }
            SocketState::Live { pid: Some(other) } => {
                return Err(LockError::ReplaceRefused {
                    path: lock_path.to_path_buf(),
                    reason: format!(
                        "the CI socket {socket} belongs to pid {other}, not the recorded \
                         owner pid {}; refusing to stop an unrelated instance",
                        owner.pid
                    ),
                });
            }
            SocketState::Live { pid: None } => {
                // A server that does not report its PID cannot be the process
                // that wrote this record (this build always reports it), so
                // fall through to the validated signal path.
            }
            SocketState::Foreign => {
                return Err(LockError::ReplaceRefused {
                    path: lock_path.to_path_buf(),
                    reason: format!(
                        "the CI socket {socket} is held by a process that does not speak the \
                         iris CI protocol; refusing to touch it"
                    ),
                });
            }
            SocketState::Free => {}
        }
    }
    signal_owner(lock_path, owner)
}

fn signal_owner(lock_path: &Path, owner: &InstanceRecord) -> Result<(), LockError> {
    if !process_alive(owner.pid) {
        // It died between the failed try_lock and here; the lock is about to
        // be released and the wait loop will pick it up.
        return Ok(());
    }
    match (&owner.start_marker, process_start_marker(owner.pid)) {
        (Some(want), Some(got)) if want == &got => {}
        (Some(_), Some(_)) => {
            return Err(LockError::ReplaceRefused {
                path: lock_path.to_path_buf(),
                reason: format!(
                    "pid {} now belongs to a different process (its start time changed); \
                     refusing to signal a recycled PID. Remove the stale lock only if you \
                     are sure no iris is running",
                    owner.pid
                ),
            });
        }
        _ => {
            return Err(LockError::ReplaceRefused {
                path: lock_path.to_path_buf(),
                reason: format!(
                    "cannot verify that pid {} is still the instance that wrote the lock; \
                     quit it with `iris-ci quit`, or remove the lock by hand once it is gone",
                    owner.pid
                ),
            });
        }
    }

    #[cfg(unix)]
    {
        let rc = unsafe { libc::kill(owner.pid as libc::pid_t, libc::SIGTERM) };
        if rc == 0 {
            eprintln!(
                "iris: stopped pid {} with SIGTERM (it has no live CI socket, so the guest was \
                 not shut down cleanly and unsaved NVRAM is lost)",
                owner.pid
            );
            return Ok(());
        }
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::ESRCH) {
            return Ok(());
        }
        Err(LockError::ReplaceRefused {
            path: lock_path.to_path_buf(),
            reason: format!("could not signal pid {}: {err}", owner.pid),
        })
    }
    #[cfg(not(unix))]
    {
        let _ = lock_path;
        Err(LockError::ReplaceRefused {
            path: lock_path.to_path_buf(),
            reason: if cfg!(windows) {
                format!(
                    "on Windows --replace can only stop an instance that answers on its CI \
                     socket; quit pid {} with `iris-ci quit` first",
                    owner.pid
                )
            } else {
                format!(
                    "cannot signal pid {} on this platform; quit it with `iris-ci quit` first",
                    owner.pid
                )
            },
        })
    }
}

// ---------------------------------------------------------------------------
// Process identity
// ---------------------------------------------------------------------------

/// Whether `pid` names a live process. Only used to decide whether there is
/// anything to signal; the lock itself is the authority.
fn process_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    #[cfg(unix)]
    {
        let rc = unsafe { libc::kill(pid as libc::pid_t, 0) };
        rc == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{
            GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        // STILL_ACTIVE from <winbase.h>; a process that has exited reports its
        // exit code instead.
        const STILL_ACTIVE: u32 = 259;
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() {
                return false;
            }
            let mut code = 0u32;
            let ok = GetExitCodeProcess(handle, &mut code);
            CloseHandle(handle);
            ok != 0 && code == STILL_ACTIVE
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        false
    }
}

/// A stable marker for when `pid` started, used to detect PID reuse.
///
/// Linux reads the start time from `/proc/<pid>/stat`, macOS asks
/// `proc_pidinfo`, and Windows reads the creation `FILETIME`. `None` means the
/// platform cannot answer; `--replace` then refuses to signal rather than
/// guess.
fn process_start_marker(pid: u32) -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        // Field 2 (comm) may contain spaces and parentheses; everything after
        // the last ')' starts at field 3 (state). starttime is field 22, i.e.
        // index 19 in that remainder.
        let rest = stat.rsplit_once(')')?.1;
        let ticks = rest.split_whitespace().nth(19)?;
        Some(format!("linux:{ticks}"))
    }
    #[cfg(target_os = "macos")]
    {
        let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
        let n = unsafe {
            libc::proc_pidinfo(
                pid as libc::c_int,
                libc::PROC_PIDTBSDINFO,
                0,
                &mut info as *mut _ as *mut libc::c_void,
                size,
            )
        };
        if n <= 0 {
            return None;
        }
        Some(format!(
            "darwin:{}.{}",
            info.pbi_start_tvsec, info.pbi_start_tvusec
        ))
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{CloseHandle, FILETIME};
        use windows_sys::Win32::System::Threading::{
            GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() {
                return None;
            }
            let mut creation = FILETIME {
                dwLowDateTime: 0,
                dwHighDateTime: 0,
            };
            let mut exit = FILETIME {
                dwLowDateTime: 0,
                dwHighDateTime: 0,
            };
            let mut kernel = FILETIME {
                dwLowDateTime: 0,
                dwHighDateTime: 0,
            };
            let mut user = FILETIME {
                dwLowDateTime: 0,
                dwHighDateTime: 0,
            };
            let ok = GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user);
            CloseHandle(handle);
            if ok == 0 {
                return None;
            }
            let ticks =
                (u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime);
            Some(format!("windows:{ticks}"))
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = pid;
        None
    }
}

// ---------------------------------------------------------------------------
// Exit cleanup
// ---------------------------------------------------------------------------

struct ExitCleanup {
    lock: PathBuf,
    socket: Option<PathBuf>,
}

static EXIT_CLEANUP: Mutex<Option<ExitCleanup>> = Mutex::new(None);

/// Register the paths [`cleanup_for_exit`] should remove. Called once, after
/// the lock is taken and before any path that can call `std::process::exit`.
pub fn register_exit_cleanup(lock: &Path, ci_socket: Option<&str>) {
    *EXIT_CLEANUP.lock() = Some(ExitCleanup {
        lock: lock.to_path_buf(),
        socket: ci_socket
            .filter(|p| !ci_socket_is_tcp(p))
            .map(PathBuf::from),
    });
}

/// Remove the registered CI socket and lock file. Safe to call more than once,
/// and from `std::process::exit` paths where `Drop` will not run.
pub fn cleanup_for_exit() {
    let Some(cleanup) = EXIT_CLEANUP.lock().take() else {
        return;
    };
    if let Some(socket) = cleanup.socket {
        let _ = std::fs::remove_file(socket);
    }
    #[cfg(unix)]
    let _ = std::fs::remove_file(cleanup.lock);
    #[cfg(not(unix))]
    if let Ok(file) = OpenOptions::new().write(true).open(&cleanup.lock) {
        let _ = file.set_len(0);
    }
}

/// Install an async-signal-safe cleanup handler for `SIGINT`, `SIGTERM` and
/// `SIGHUP`. The handler unlinks the CI socket and lock file, then re-raises
/// the signal with its default disposition, so the exit status still reports
/// the signal. It intentionally does not touch the Machine: `Machine::stop`
/// takes locks and allocates, which is not safe in a signal handler.
#[cfg(unix)]
pub fn install_signal_cleanup(lock: &Path, ci_socket: Option<&str>) {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    use std::ptr;
    use std::sync::atomic::{AtomicPtr, Ordering};

    static LOCK_PATH: AtomicPtr<libc::c_char> = AtomicPtr::new(ptr::null_mut());
    static SOCKET_PATH: AtomicPtr<libc::c_char> = AtomicPtr::new(ptr::null_mut());

    fn leak(path: &Path) -> *mut libc::c_char {
        match CString::new(path.as_os_str().as_bytes()) {
            Ok(s) => s.into_raw(),
            Err(_) => ptr::null_mut(),
        }
    }

    LOCK_PATH.store(leak(lock), Ordering::SeqCst);
    let socket = ci_socket
        .filter(|p| !ci_socket_is_tcp(p))
        .map(|p| leak(Path::new(p)))
        .unwrap_or(ptr::null_mut());
    SOCKET_PATH.store(socket, Ordering::SeqCst);

    unsafe extern "C" fn handler(sig: libc::c_int) {
        let lock = LOCK_PATH.load(Ordering::SeqCst);
        if !lock.is_null() {
            libc::unlink(lock);
        }
        let socket = SOCKET_PATH.load(Ordering::SeqCst);
        if !socket.is_null() {
            libc::unlink(socket);
        }
        // `SA_RESETHAND` restored the default action on entry. The signal is
        // still blocked inside the handler, so raise it as pending; it fires
        // with its normal status as soon as this returns.
        libc::raise(sig);
    }

    unsafe {
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = handler as *const () as usize;
        sa.sa_flags = libc::SA_RESETHAND;
        libc::sigemptyset(&mut sa.sa_mask);
        for sig in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
            libc::sigaction(sig, &sa, ptr::null_mut());
        }
    }
}

#[cfg(not(unix))]
pub fn install_signal_cleanup(_lock: &Path, _ci_socket: Option<&str>) {}

// ---------------------------------------------------------------------------
// Listener ownership
// ---------------------------------------------------------------------------

/// Verify that every listener this start will bind is free, before the Machine
/// opens NVRAM or disks.
///
/// The CI socket is resolved first: `--replace` may have to ask its owner to
/// quit, and that releases the monitor/serial ports the old instance held. A
/// stale socket file is removed, a live iris is refused (or asked to quit with
/// `--replace`), and a non-iris listener is never touched.
///
/// The remaining listeners are probed by binding and dropping the port. In
/// `--ci` mode the serial TCP ports are never bound (the in-process CI serial
/// backend replaces them), so they are not checked.
pub fn check_endpoints(cfg: &MachineConfig) -> Result<(), String> {
    let deadline = Instant::now() + PROBE_RETRY;
    if cfg.ci {
        check_ci_socket(&cfg.ci_socket, cfg.replace, deadline)?;
    }
    probe_tcp(
        "monitor",
        &format!("127.0.0.1:{}", cfg.monitor_port.unwrap_or(DEFAULT_PORT)),
        deadline,
    )?;
    if !cfg.ci {
        let [a, b] = cfg.serial_ports();
        probe_tcp("serial channel A", &format!("127.0.0.1:{a}"), deadline)?;
        probe_tcp("serial channel B", &format!("127.0.0.1:{b}"), deadline)?;
    }
    if let Some(port) = cfg.gdb_port {
        probe_tcp("GDB stub", &format!("127.0.0.1:{port}"), deadline)?;
    }
    Ok(())
}

fn probe_tcp(what: &str, addr: &str, deadline: Instant) -> Result<(), String> {
    loop {
        match std::net::TcpListener::bind(addr) {
            Ok(listener) => {
                drop(listener);
                return Ok(());
            }
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse && Instant::now() < deadline => {
                std::thread::sleep(REPLACE_POLL);
            }
            Err(e) => {
                let hint = if e.kind() == std::io::ErrorKind::AddrInUse {
                    " (another iris or another program is using it; give this instance different \
                     ports, or stop the other program)"
                } else {
                    ""
                };
                return Err(format!("{what} cannot bind {addr}: {e}{hint}"));
            }
        }
    }
}

fn check_ci_socket(path: &str, replace: bool, deadline: Instant) -> Result<(), String> {
    let tcp = ci_socket_is_tcp(path);
    let addr = crate::config::ci_socket_tcp_addr(path);
    match crate::ci::probe_socket(path) {
        SocketState::Free => {
            if !tcp {
                // A dead socket file is ours to replace. This mirrors the check
                // the CI server itself makes at bind time.
                if Path::new(path).exists() {
                    std::fs::remove_file(path)
                        .map_err(|e| format!("cannot remove stale CI socket {path}: {e}"))?;
                }
            } else {
                probe_tcp("CI socket", &addr, deadline)?;
            }
            Ok(())
        }
        SocketState::Live { pid } => {
            if !replace {
                let owner = pid.map(|p| format!(" (pid {p})")).unwrap_or_default();
                return Err(format!(
                    "CI socket {path} is live{owner}; use --replace to ask it to quit, or pick \
                     a different --ci-socket"
                ));
            }
            crate::ci::request_quit(path)?;
            wait_until_socket_free(path)?;
            if tcp {
                probe_tcp("CI socket", &addr, deadline)?;
            } else {
                let _ = std::fs::remove_file(path);
            }
            Ok(())
        }
        SocketState::Foreign => Err(format!(
            "CI socket {path} is held by a process that does not speak the iris CI protocol; \
             refusing to touch it"
        )),
    }
}

fn wait_until_socket_free(path: &str) -> Result<(), String> {
    let deadline = Instant::now() + REPLACE_TIMEOUT;
    loop {
        if crate::ci::probe_socket(path) == SocketState::Free {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "CI socket {path} did not close within {} s",
                REPLACE_TIMEOUT.as_secs()
            ));
        }
        std::thread::sleep(REPLACE_POLL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("iris-instance-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn record(pid: u32) -> InstanceRecord {
        InstanceRecord {
            pid,
            instance: None,
            state_dir: None,
            ci_socket: None,
            monitor_port: 0,
            started_unix: 0,
            start_marker: None,
            exe: None,
        }
    }

    #[test]
    fn a_second_lock_is_refused_and_names_the_owner() {
        let dir = temp_dir("second");
        let path = dir.join(LOCK_FILE);
        let first = InstanceLock::acquire(&path, record(4242), false).unwrap();
        let err = InstanceLock::acquire(&path, record(std::process::id()), false).unwrap_err();
        match &err {
            LockError::AlreadyRunning {
                owner: Some(owner), ..
            } => {
                assert_eq!(owner.pid, 4242);
            }
            other => panic!("expected AlreadyRunning with an owner record, got {other:?}"),
        }
        assert!(err.to_string().contains("4242"), "{err}");
        drop(first);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_stale_lock_file_is_reclaimed() {
        let dir = temp_dir("stale");
        let path = dir.join(LOCK_FILE);
        std::fs::write(&path, serde_json::to_vec(&record(999_999)).unwrap()).unwrap();
        let lock = InstanceLock::acquire(&path, record(std::process::id()), false)
            .expect("an unlocked leftover file is stale, not live");
        let on_disk = read_record(&path).expect("our record was written");
        assert_eq!(on_disk.pid, std::process::id());
        assert_eq!(on_disk, record(std::process::id()));
        drop(lock);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn cleanup_removes_the_lock_and_is_idempotent() {
        let dir = temp_dir("cleanup");
        let path = dir.join(LOCK_FILE);
        let mut lock = InstanceLock::acquire(&path, record(std::process::id()), false).unwrap();
        assert!(path.exists());
        lock.cleanup();
        assert!(!path.exists());
        lock.cleanup();
        assert!(!path.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_lock_path_follows_the_state_dir() {
        let mut cfg = MachineConfig::default();
        cfg.scsi.clear();
        assert_eq!(lock_path(&cfg), PathBuf::from("iris.lock"));
        cfg.state_dir = Some("some/state".into());
        assert_eq!(lock_path(&cfg), PathBuf::from("some/state/iris.lock"));
    }

    #[test]
    fn the_record_names_the_instance_and_state_dir() {
        let mut cfg = MachineConfig::default();
        cfg.scsi.clear();
        cfg.ci = true;
        cfg.instance_id = Some(7);
        cfg.state_dir = Some("/tmp/iris-instance-7".into());
        cfg.ci_socket = "/tmp/iris-instance-7/iris.sock".into();
        let rec = InstanceRecord::current(&cfg);
        assert_eq!(rec.pid, std::process::id());
        assert_eq!(rec.instance, Some(7));
        assert_eq!(rec.state_dir.as_deref(), Some("/tmp/iris-instance-7"));
        assert_eq!(
            rec.ci_socket.as_deref(),
            Some("/tmp/iris-instance-7/iris.sock")
        );
        assert!(rec.describe().contains("instance 7"), "{}", rec.describe());
    }

    #[test]
    fn a_record_without_ci_does_not_name_the_unused_socket() {
        let mut cfg = MachineConfig::default();
        cfg.scsi.clear();
        let rec = InstanceRecord::current(&cfg);
        assert_eq!(
            rec.ci_socket, None,
            "a configured default must not be quit by --replace"
        );
    }

    #[test]
    fn check_endpoints_reports_a_busy_port() {
        let dir = temp_dir("busy");
        let busy = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = busy.local_addr().unwrap().port();

        let mut cfg = MachineConfig::default();
        cfg.scsi.clear();
        cfg.ci = true;
        cfg.ci_socket = dir.join("iris.sock").display().to_string();
        cfg.monitor_port = Some(port);
        let err = check_endpoints(&cfg).expect_err("the monitor port is held");
        assert!(err.contains("monitor"), "{err}");
        assert!(err.contains(&port.to_string()), "{err}");

        drop(busy);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn check_endpoints_accepts_free_ports_and_a_stale_socket() {
        let dir = temp_dir("free");
        let sock = dir.join("iris.sock");
        std::fs::write(&sock, b"not a socket").unwrap();

        let mut cfg = MachineConfig::default();
        cfg.scsi.clear();
        cfg.ci = true;
        cfg.ci_socket = sock.display().to_string();
        cfg.monitor_port = Some(0);
        check_endpoints(&cfg).expect("an ephemeral monitor port and a stale file are fine");
        assert!(!sock.exists(), "the stale socket file is removed");

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn replace_refuses_a_recycled_pid() {
        use std::process::{Command, Stdio};
        let dir = temp_dir("recycled");
        let path = dir.join(LOCK_FILE);

        // Hold the lock here, but write a record for a live child whose start
        // marker deliberately does not match. --replace must refuse rather
        // than signal an unrelated process.
        let holder = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&path)
            .unwrap();
        holder.try_lock().unwrap();
        let mut child = Command::new("sleep")
            .arg("30")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut owner = record(child.id());
        owner.start_marker = Some("not-the-child-s-start-marker".to_string());
        write_record(&mut holder.try_clone().unwrap(), &owner).unwrap();

        let err = InstanceLock::acquire(&path, record(std::process::id()), true)
            .expect_err("a recycled PID must be refused");
        assert!(
            err.to_string().contains("different process"),
            "unexpected error: {err}"
        );
        assert!(
            child.try_wait().unwrap().is_none(),
            "the child must still be alive"
        );

        child.kill().unwrap();
        child.wait().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_start_marker_is_stable_for_a_live_process() {
        let a = process_start_marker(std::process::id()).expect("Linux reports /proc start time");
        let b = process_start_marker(std::process::id()).unwrap();
        assert_eq!(a, b);
        assert!(process_alive(std::process::id()));
    }
}
