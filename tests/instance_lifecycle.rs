//! Instance lifecycle: start, refuse a second run, replace, quit, restart, and
//! clean up after a signal.
//!
//! These spawn the real `iris` binary in a private working directory (the
//! single-instance state directory) and drive it over its CI socket. No disk
//! image or IRIX media is needed: the test configuration attaches no SCSI
//! device and the CPU is never started.
//!
//! Unix-only because the socket helpers use `UnixStream`; the lock and cleanup
//! code itself is platform-neutral. Run with:
//!
//! ```sh
//! cargo test -p iris --test instance_lifecycle
//! ```

#![cfg(unix)]

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const START_TIMEOUT: Duration = Duration::from_secs(60);
const EXIT_TIMEOUT: Duration = Duration::from_secs(30);

fn iris_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_iris"))
}

fn config_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test/fleet/machine.toml")
}

fn temp_root(name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let root = std::env::temp_dir().join(format!(
        "iris-lifecycle-{}-{stamp}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

/// Owns the test's working directory and removes it after every `Instance`
/// (and any `Child`) has been dropped.
struct TestDir(PathBuf);

impl TestDir {
    fn new(name: &str) -> Self {
        Self(temp_root(name))
    }
}

impl std::ops::Deref for TestDir {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

impl AsRef<Path> for TestDir {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl AsRef<std::ffi::OsStr> for TestDir {
    fn as_ref(&self) -> &std::ffi::OsStr {
        self.0.as_os_str()
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A spawned iris that never outlives the test, even when an assertion panics.
struct Instance {
    child: Child,
    root: PathBuf,
    socket: PathBuf,
    stderr: PathBuf,
}
impl Instance {
    fn start(root: &Path, name: &str, monitor_port: u16, replace: bool) -> Self {
        let socket = root.join(format!("{name}.sock"));
        Self::start_with_socket(root, &socket, name, monitor_port, replace)
    }

    fn start_with_socket(
        root: &Path,
        socket: &Path,
        name: &str,
        monitor_port: u16,
        replace: bool,
    ) -> Self {
        let stderr = root.join(format!("{name}.stderr"));
        let out = std::fs::File::create(root.join(format!("{name}.stdout"))).unwrap();
        let err = std::fs::File::create(&stderr).unwrap();
        let mut cmd = Command::new(iris_bin());
        cmd.current_dir(root)
            .arg("--config")
            .arg(config_path())
            .arg("--ci")
            .arg("--ci-socket")
            .arg(socket)
            .arg("--monitor-port")
            .arg(monitor_port.to_string())
            .stdin(Stdio::null())
            .stdout(out)
            .stderr(err);
        if replace {
            cmd.arg("--replace");
        }
        let child = cmd.spawn().expect("spawn iris");
        Self {
            child,
            root: root.to_path_buf(),
            socket: socket.to_path_buf(),
            stderr,
        }
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }

    fn lock_path(&self) -> PathBuf {
        self.root.join("iris.lock")
    }

    fn wait_ready(&mut self) {
        let deadline = Instant::now() + START_TIMEOUT;
        loop {
            if let Some(pid) = ping(&self.socket) {
                if pid == self.pid() {
                    return;
                }
                // A replacement can still see the previous owner answering
                // until it has finished quitting. Keep waiting for our own PID.
            }
            if let Some(status) = self.child.try_wait().expect("try_wait") {
                panic!(
                    "iris exited before its CI socket answered: {status}\n{}",
                    self.stderr_text()
                );
            }
            assert!(
                Instant::now() < deadline,
                "CI socket never answered as pid {}\n{}",
                self.pid(),
                self.stderr_text()
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn stderr_text(&self) -> String {
        std::fs::read_to_string(&self.stderr).unwrap_or_default()
    }

    fn quit(&mut self) {
        send(&self.socket, "quit").expect("send quit");
        let status = self.wait_exit();
        assert!(
            status.success(),
            "quit should exit cleanly, got {status}\n{}",
            self.stderr_text()
        );
    }

    fn wait_exit(&mut self) -> std::process::ExitStatus {
        let deadline = Instant::now() + EXIT_TIMEOUT;
        loop {
            if let Some(status) = self.child.try_wait().expect("try_wait") {
                return status;
            }
            assert!(Instant::now() < deadline, "iris did not exit in time");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

/// Send one CI request, returning the response line. The connection is opened
/// fresh each time, so a missing socket is an error rather than a hang.
fn request(socket: &Path, cmd: &str) -> std::io::Result<String> {
    let mut stream = UnixStream::connect(socket)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let line = format!("{{\"cmd\":\"{cmd}\",\"args\":{{}}}}\n");
    stream.write_all(line.as_bytes())?;
    stream.flush()?;
    let mut reader = BufReader::new(stream);
    let mut response = String::new();
    reader.read_line(&mut response)?;
    Ok(response)
}

fn send(socket: &Path, cmd: &str) -> std::io::Result<String> {
    request(socket, cmd)
}

/// `Some(pid)` when an iris CI server answers on `socket`.
fn ping(socket: &Path) -> Option<u32> {
    let line = request(socket, "ping").ok()?;
    let value: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    if value.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        return None;
    }
    value
        .get("data")
        .and_then(|d| d.get("pid"))
        .and_then(|p| p.as_u64())
        .map(|p| p as u32)
}

fn wait_gone(socket: &Path, lock: &Path) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while socket.exists() || lock.exists() {
        assert!(
            Instant::now() < deadline,
            "cleanup left {} or {} behind",
            socket.display(),
            lock.display()
        );
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[test]
fn start_quit_restart_cleans_up() {
    let root = TestDir::new("restart");
    {
        let mut first = Instance::start(&root, "first", 0, false);
        first.wait_ready();
        assert!(
            first.lock_path().exists(),
            "the lock file is created at startup"
        );
        let record: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(first.lock_path()).unwrap()).unwrap();
        assert_eq!(record["pid"].as_u64(), Some(u64::from(first.pid())));
        first.quit();
        wait_gone(&first.socket, &first.lock_path());
    }
    {
        // A second start in the same directory must not be blocked by the
        // clean exit's (now removed) socket or lock.
        let mut second = Instance::start(&root, "second", 0, false);
        second.wait_ready();
        second.quit();
        wait_gone(&second.socket, &second.lock_path());
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_live_instance_is_refused_without_replace() {
    let root = TestDir::new("refused");
    let mut first = Instance::start(&root, "first", 0, false);
    first.wait_ready();

    let stderr = root.join("second.stderr");
    let status = Command::new(iris_bin())
        .current_dir(&root)
        .arg("--config")
        .arg(config_path())
        .arg("--ci")
        .arg("--ci-socket")
        .arg(root.join("second.sock"))
        .arg("--monitor-port")
        .arg("0")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(&stderr).unwrap())
        .status()
        .expect("run a second iris");
    assert!(!status.success(), "a second instance must not start");
    let text = std::fs::read_to_string(&stderr).unwrap();
    assert!(text.contains("already using this state"), "{text}");
    assert!(text.contains(&first.pid().to_string()), "{text}");

    assert_eq!(
        ping(&first.socket),
        Some(first.pid()),
        "the first instance must survive"
    );
    first.quit();
    wait_gone(&first.socket, &first.lock_path());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn replace_stops_the_recorded_owner() {
    let root = TestDir::new("replace");
    let mut first = Instance::start(&root, "first", 0, false);
    first.wait_ready();
    let first_pid = first.pid();

    // Same state directory, same socket and monitor port: only --replace can
    // make this start.
    let mut replacement = Instance::start(&root, "first", 0, true);
    replacement.wait_ready();
    assert_ne!(
        replacement.pid(),
        first_pid,
        "the replacement is a new process"
    );
    assert_eq!(ping(&first.socket), Some(replacement.pid()));

    let status = first.wait_exit();
    assert!(
        status.success(),
        "the replaced instance should quit cleanly, got {status}"
    );
    assert!(
        replacement
            .stderr_text()
            .contains(&format!("replaced instance pid {first_pid}")),
        "{}",
        replacement.stderr_text()
    );

    replacement.quit();
    wait_gone(&replacement.socket, &replacement.lock_path());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn replace_stops_a_live_socket_without_a_lock() {
    // The OS test-helper shape: an older/foreign iris owns a CI socket and its
    // monitor port, but no lock in *our* state directory. --replace must quit
    // the socket it was pointed at, then find the ports free.
    let first_root = TestDir::new("legacy-first");
    let second_root = TestDir::new("legacy-second");
    let mut first = Instance::start(&first_root, "first", 0, false);
    first.wait_ready();
    let first_pid = first.pid();

    let mut second = Instance::start_with_socket(&second_root, &first.socket, "second", 0, true);
    second.wait_ready();
    assert_ne!(second.pid(), first_pid);
    let status = first.wait_exit();
    assert!(
        status.success(),
        "the CI socket owner should quit cleanly, got {status}"
    );

    second.quit();
    wait_gone(&second.socket, &second.lock_path());
    let _ = std::fs::remove_dir_all(&first_root.0);
    let _ = std::fs::remove_dir_all(&second_root.0);
}

#[test]
fn a_stale_socket_and_lock_are_reclaimed() {
    let root = TestDir::new("stale");
    let socket = root.join("first.sock");
    let lock = root.join("iris.lock");
    // A dead socket file (no listener) and a lock record whose PID is long
    // gone: exactly what an interrupted run leaves behind.
    std::fs::write(&socket, b"stale").unwrap();
    std::fs::write(&lock, r#"{"pid":999999,"monitor_port":0,"started_unix":0}"#).unwrap();

    let mut instance = Instance::start(&root, "first", 0, false);
    instance.wait_ready();
    let record: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&lock).unwrap()).unwrap();
    assert_eq!(record["pid"].as_u64(), Some(u64::from(instance.pid())));

    instance.quit();
    wait_gone(&socket, &lock);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn sigterm_removes_the_socket_and_lock() {
    let root = TestDir::new("sigterm");
    let mut instance = Instance::start(&root, "first", 0, false);
    instance.wait_ready();
    let pid = instance.pid();

    let status = Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .status()
        .expect("send SIGTERM");
    assert!(status.success());
    let exit = instance.wait_exit();
    assert!(
        !exit.success(),
        "SIGTERM must not look like a clean exit: {exit}"
    );
    wait_gone(&instance.socket, &instance.lock_path());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_busy_monitor_port_is_reported_before_booting() {
    let root = TestDir::new("busy-port");
    let busy = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = busy.local_addr().unwrap().port();

    let stderr = root.join("busy.stderr");
    let status = Command::new(iris_bin())
        .current_dir(&root)
        .arg("--config")
        .arg(config_path())
        .arg("--ci")
        .arg("--ci-socket")
        .arg(root.join("busy.sock"))
        .arg("--monitor-port")
        .arg(port.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(&stderr).unwrap())
        .status()
        .expect("run iris");
    assert!(!status.success(), "a held monitor port must fail startup");
    let text = std::fs::read_to_string(&stderr).unwrap();
    assert!(text.contains("monitor"), "{text}");
    assert!(text.contains(&port.to_string()), "{text}");
    assert!(
        !root.join("iris.lock").exists(),
        "the failed start must release its lock"
    );
    let _ = std::fs::remove_dir_all(&root);
}
