# The instance lock is the file lock, not the PID file

**Symptom.** A second `iris` in the same state directory either refuses to
start forever after a crash, or silently steals a stale CI socket and attaches
to the wrong instance; the usual recovery is `pkill` plus deleting the socket by
hand.

**Rule.** `iris.lock` is owned by a non-blocking exclusive advisory lock
(`File::try_lock`). Use the lock to decide liveness, never the PID in the file:
the kernel releases the lock on every death, including `SIGKILL`, while a PID
can be recycled and a file can outlive its writer. The JSON record inside the
lock is diagnostic and a `--replace` safety check, not the authority.

- `--replace` may stop only the recorded owner. Ask its CI socket to `quit`
  first: that path runs `Machine::stop`, so NVRAM and the Indigo2 EEPROM are
  saved. `SIGTERM` is the fallback only after the recorded `start_marker`
  (Linux `/proc/<pid>/stat` field 22, macOS `proc_pidinfo`, Windows process
  creation `FILETIME`) matches the current process's marker. Without a marker,
  refuse rather than signal.
- On Unix, cleanup unlinks the lock path before releasing the lock, and a
  waiter re-checks `dev`+`ino` after acquiring. Otherwise a waiter can lock the
  unlinked inode while a third process creates a fresh path — two live owners.
  On Windows the file is truncated in place instead: the std metadata methods
  for file index are unstable, so the inode is never unlinked.
- The SIGINT/SIGTERM/SIGHUP handler only `unlink`s the CI socket and lock and
  re-raises the signal. It must not call `Machine::stop`: that takes locks,
  allocates and writes NVRAM, none of which is async-signal-safe. A forced kill
  therefore loses unsaved NVRAM; say so in the docs and prefer `iris-ci quit`.
- The lock is taken in `main` before `Machine::new` and endpoint preflight, so a
  refused start touches no disk or NVRAM. Every explicit `std::process::exit`
  in the CLI path calls `instance::cleanup_for_exit`, because `exit` skips
  `Drop`. Embedders (`iris-gui`) never register the global, so the call is a
  no-op there.

**Tests.** `cargo test -p iris --test instance_lifecycle` covers start → quit →
restart, a refused second start, `--replace` over the CI socket, a stale socket
and lock, `SIGTERM` cleanup and a busy monitor port. Unit tests in
`src/instance.rs` cover the record, stale reclamation, an idempotent cleanup
and the recycled-PID refusal.
