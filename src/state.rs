//! Central registry of persistent host-side artefacts.
//!
//! Every file the emulator keeps between (or during) runs — NVRAM, the
//! Indigo2 motherboard EEPROM, snapshots and their chunk store, the COW
//! overlay and CHD diff sidecar, the jitv2 persistent code cache, the serial
//! log, the bare-metal test-device dump, the crash log and the CI control
//! socket — has its location derived here from a single [`StatePaths`] value.
//!
//! This module is deliberately additive: it is a *seam*, not a migration.
//! [`StatePaths::neutral`] reproduces exactly the paths the tree uses today
//! (relative names in the working directory, the platform's user cache
//! directory for jitv2, the platform CI socket default), so a caller that
//! adopts it sees no behaviour change. [`StatePaths::under`] relocates the
//! whole set beneath one root, which is what parallel per-task instances
//! want. Nothing consumes either constructor yet; existing call sites keep
//! their own derivations until a later change wires them through.
//!
//! Two artefacts cannot be named by a single path today: the COW overlay and
//! the CHD diff resolve either from an environment variable
//! (`IRIS_COW_OVERLAY_DIR`, `IRIS_CHD_DIFF_DIR`) or beside the base image.
//! Those accessors therefore return `Option<PathBuf>`; `None` means "use the
//! existing resolution", and `Some` means "put it under this directory".

use std::path::{Path, PathBuf};

/// A root directory plus accessors for every persistent artefact.
///
/// A `None` root is *neutral*: each accessor returns the exact path the tree
/// derives today. A `Some` root places every artefact beneath it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatePaths {
    root: Option<PathBuf>,
}

impl StatePaths {
    /// The paths the tree uses today, with no common root.
    pub fn neutral() -> Self {
        Self { root: None }
    }

    /// Every artefact beneath `root`.
    pub fn under(root: impl Into<PathBuf>) -> Self {
        Self { root: Some(root.into()) }
    }

    /// The common root, if one was given. `None` for [`StatePaths::neutral`].
    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    /// A bare artefact name, joined to the root when there is one.
    fn at(&self, name: &str) -> PathBuf {
        match &self.root {
            Some(root) => root.join(name),
            None => PathBuf::from(name),
        }
    }

    /// Indy RTC NVRAM. Today: `nvram.bin` in the working directory.
    pub fn nvram(&self) -> PathBuf {
        self.at("nvram.bin")
    }

    /// Indigo2 motherboard EEPROM. Today: `nveeprom.bin`.
    pub fn nveeprom(&self) -> PathBuf {
        self.at("nveeprom.bin")
    }

    /// Snapshot directory. Today: `saves`.
    pub fn snapshots_dir(&self) -> PathBuf {
        self.at("saves")
    }

    /// Content-addressable chunk store. Today: `<snapshots_dir>/.cas`.
    pub fn chunk_store_dir(&self) -> PathBuf {
        self.snapshots_dir().join(".cas")
    }

    /// jitv2 persistent code cache *base* — the directory holding the
    /// per-build subdirectories, not a single build's subtree.
    ///
    /// Neutral resolution mirrors `cpu::jitv2::pcache`: `IRIS_JIT_CACHE_DIR`
    /// if set, otherwise the platform user cache directory with `iris/jitv2`
    /// inside it (`~/Library/Caches`, `%LOCALAPPDATA%`, or
    /// `$XDG_CACHE_HOME`/`~/.cache`). Under a root it is simply `<root>/jitv2`.
    pub fn jit_cache_dir(&self) -> PathBuf {
        match &self.root {
            Some(root) => root.join("jitv2"),
            None => match std::env::var_os("IRIS_JIT_CACHE_DIR") {
                Some(dir) => PathBuf::from(dir),
                None => default_jit_cache_base()
                    .unwrap_or_else(|| PathBuf::from("iris").join("jitv2")),
            },
        }
    }

    /// Serial console capture. There is no default today — `serial_log` is an
    /// `Option<String>` and logging is off unless configured. This name is the
    /// conventional filename a caller should use; `neutral()` returning it does
    /// *not* enable logging by itself.
    pub fn serial_log(&self) -> PathBuf {
        self.at("iris-serial.log")
    }

    /// Bare-metal test-device JSON dump. Today: `iris-testdev-dump.json`.
    pub fn test_device_dump(&self) -> PathBuf {
        self.at("iris-testdev-dump.json")
    }

    /// Crash diagnostics log. Today: `iris-crash.log`.
    pub fn crash_log(&self) -> PathBuf {
        self.at("iris-crash.log")
    }

    /// CI control socket (or `host:port` on Windows). Neutral: the platform
    /// default from `config::default_ci_socket`. Under a root: `<root>/iris.sock`.
    pub fn ci_socket(&self) -> PathBuf {
        match &self.root {
            Some(root) => root.join("iris.sock"),
            None => PathBuf::from(default_ci_socket()),
        }
    }

    /// COW raw-disk overlay directory. Today this resolves via
    /// `IRIS_COW_OVERLAY_DIR`, else beside/under `/tmp`; `None` keeps that
    /// resolution. Under a root: `<root>/overlays`.
    pub fn cow_overlay_dir(&self) -> Option<PathBuf> {
        self.root.as_ref().map(|root| root.join("overlays"))
    }

    /// CHD `.diff.chd` sidecar directory. Today this resolves via
    /// `IRIS_CHD_DIFF_DIR`, else beside the base image; `None` keeps that
    /// resolution. Under a root: `<root>/diffs`.
    pub fn chd_diff_dir(&self) -> Option<PathBuf> {
        self.root.as_ref().map(|root| root.join("diffs"))
    }
}

/// Platform CI socket default, matching `config::default_ci_socket`.
fn default_ci_socket() -> &'static str {
    #[cfg(unix)]
    {
        "/tmp/iris.sock"
    }
    #[cfg(windows)]
    {
        "127.0.0.1:19851"
    }
    #[cfg(not(any(unix, windows)))]
    {
        "/tmp/iris.sock"
    }
}

/// Platform user cache directory plus `iris/jitv2`, mirroring
/// `cpu::jitv2::pcache::default_base` (which is private). `None` when the
/// platform's cache base variable (HOME / LOCALAPPDATA) is unset.
fn default_jit_cache_base() -> Option<PathBuf> {
    let cache = if cfg!(target_os = "macos") {
        PathBuf::from(std::env::var_os("HOME")?).join("Library/Caches")
    } else if cfg!(windows) {
        PathBuf::from(std::env::var_os("LOCALAPPDATA")?)
    } else {
        match std::env::var_os("XDG_CACHE_HOME") {
            Some(dir) if !dir.is_empty() => PathBuf::from(dir),
            _ => PathBuf::from(std::env::var_os("HOME")?).join(".cache"),
        }
    };
    Some(cache.join("iris").join("jitv2"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::MachineConfig;

    #[test]
    fn neutral_matches_todays_literal_paths() {
        let p = StatePaths::neutral();
        assert_eq!(p.root(), None);

        assert_eq!(p.nvram(), PathBuf::from("nvram.bin"));
        assert_eq!(p.nveeprom(), PathBuf::from("nveeprom.bin"));
        assert_eq!(p.snapshots_dir(), PathBuf::from("saves"));
        assert_eq!(p.chunk_store_dir(), PathBuf::from("saves").join(".cas"));
        assert_eq!(p.test_device_dump(), PathBuf::from("iris-testdev-dump.json"));
        assert_eq!(p.crash_log(), PathBuf::from("iris-crash.log"));
        assert_eq!(p.serial_log(), PathBuf::from("iris-serial.log"));

        // The two env-resolved artefacts stay unresolved in neutral mode.
        assert_eq!(p.cow_overlay_dir(), None);
        assert_eq!(p.chd_diff_dir(), None);
    }

    #[test]
    fn neutral_agrees_with_config_defaults() {
        let cfg = MachineConfig::default();
        let p = StatePaths::neutral();
        assert_eq!(p.nvram(), PathBuf::from(&cfg.nvram));
        assert_eq!(p.nveeprom(), PathBuf::from(&cfg.nveeprom));
    }

    #[test]
    fn under_joins_the_same_names() {
        let root = Path::new("/x");
        let p = StatePaths::under("/x");
        assert_eq!(p.root(), Some(root));

        assert_eq!(p.nvram(), root.join("nvram.bin"));
        assert_eq!(p.nveeprom(), root.join("nveeprom.bin"));
        assert_eq!(p.snapshots_dir(), root.join("saves"));
        assert_eq!(p.chunk_store_dir(), root.join("saves").join(".cas"));
        assert_eq!(p.jit_cache_dir(), root.join("jitv2"));
        assert_eq!(p.serial_log(), root.join("iris-serial.log"));
        assert_eq!(p.test_device_dump(), root.join("iris-testdev-dump.json"));
        assert_eq!(p.crash_log(), root.join("iris-crash.log"));
        assert_eq!(p.ci_socket(), root.join("iris.sock"));
        assert_eq!(p.cow_overlay_dir(), Some(root.join("overlays")));
        assert_eq!(p.chd_diff_dir(), Some(root.join("diffs")));
    }

    #[test]
    fn under_accepts_owned_path() {
        let p = StatePaths::under(PathBuf::from("/y"));
        assert_eq!(p.nvram(), Path::new("/y").join("nvram.bin"));
    }

    #[test]
    fn different_roots_yield_disjoint_paths() {
        let a = StatePaths::under("/a");
        let b = StatePaths::under("/b");
        assert_ne!(a.nvram(), b.nvram());
        assert_ne!(a.nveeprom(), b.nveeprom());
        assert_ne!(a.snapshots_dir(), b.snapshots_dir());
        assert_ne!(a.chunk_store_dir(), b.chunk_store_dir());
        assert_ne!(a.jit_cache_dir(), b.jit_cache_dir());
        assert_ne!(a.serial_log(), b.serial_log());
        assert_ne!(a.test_device_dump(), b.test_device_dump());
        assert_ne!(a.crash_log(), b.crash_log());
        assert_ne!(a.ci_socket(), b.ci_socket());
        assert_ne!(a.cow_overlay_dir(), b.cow_overlay_dir());
        assert_ne!(a.chd_diff_dir(), b.chd_diff_dir());
    }

    #[test]
    fn ci_socket_neutral_matches_platform_default() {
        let p = StatePaths::neutral();
        let expected = PathBuf::from(MachineConfig::default().ci_socket);
        assert_eq!(p.ci_socket(), expected);
    }
}
