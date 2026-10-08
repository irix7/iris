//! Record/replay journal keyed on guest cycles (issue #47).
//!
//! Guest time became reproducible when CP0 Count was made cycle-derived (#42)
//! and the ordered guest-time timer queue landed (#43): an interrupt from the
//! Compare deadline now fires at a deterministic `hot.cycles` value, and idle
//! parking advances the same cycle counter. That makes a *journal* of the
//! inputs a run consumed — how many cycles it retired, when an external
//! interrupt line went pending, when it took an exception, and where its
//! checkpoints were — enough to re-drive the same run and assert it byte-for-
//! byte.
//!
//! The shape follows QEMU's `replay-internal.c` / `replay.c`
//! (`docs/research/b06-timers-interrupts-determinism.md` §R4): a flat stream of
//! *instruction deltas* plus *interrupt/exception events*, fenced by
//! *checkpoints* at reset and snapshot boundaries. Deltas are measured in
//! retired guest cycles (`hot.cycles`), the same base Count and every timer
//! deadline derive from, so the log is independent of host speed.
//!
//! Deliberately not a second determinism mechanism. The replay pass compares
//! its own observed event stream against the recording (which is the same
//! `assert_eq!`-friendly shape the snapshot determinism validator uses), and
//! the *final* architectural state is checked with the existing
//! [`crate::cpu::mips_exec::CpuStateDigest`] via `Machine::cpu_state_digest`.
//!
//! Off unless asked for. The CPU carries no journal state by default; the
//! driver methods take the journal by reference, so a normal run pays nothing
//! and cannot be perturbed by a recording that was never started.

use std::io::{self, BufRead, BufReader, BufWriter, Write};

/// Cause.IP-position mask for the externally-driven interrupt lines. Mirrors
/// `mips_exec::EXT_INT_MASK` (IP2..IP7); IP0/IP1 are software interrupts that
/// never appear in `Hot::interrupts`.
const EXT_INT_MASK: u32 = crate::cpu::mips_core::CAUSE_IP_MASK;

/// One entry in the guest-cycle journal.
///
/// `Delta` is the only entry that advances the log's cycle cursor; every other
/// entry happens at the cursor's current value. `Checkpoint` is absolute
/// because it re-anchors the cursor at a reset or snapshot boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A checkpoint at a reset or snapshot boundary. `cycle` is the guest
    /// cycle the checkpoint state is anchored to.
    Checkpoint { tag: String, cycle: u64 },
    /// `cycles` retired since the previous entry.
    Delta { cycles: u64 },
    /// External interrupt lines became pending. `bits` is the raw
    /// Cause.IP-position word exactly as `MipsCore::hot_interrupts` held it.
    Interrupt { cycle: u64, bits: u32 },
    /// The guest took an exception. `code` is Cause.ExcCode.
    Exception { cycle: u64, code: u32 },
}

impl Event {
    /// The guest cycle this entry is stamped with, if it carries one.
    pub fn cycle(&self) -> Option<u64> {
        match self {
            Event::Checkpoint { cycle, .. }
            | Event::Interrupt { cycle, .. }
            | Event::Exception { cycle, .. } => Some(*cycle),
            Event::Delta { .. } => None,
        }
    }
}

/// A recorded (or replayed) run: an ordered event stream plus the guest cycle
/// the run ended on.
///
/// Event cycles are stored **relative to the opening checkpoint**, not the
/// process-wide `hot.cycles` value. Two runs restored from the same snapshot
/// start from different absolute cycle counts (cycles are not part of a
/// snapshot), but the same relative timeline, so a replay can compare streams
/// directly. `base_cycle` is the absolute cycle the opening checkpoint was
/// taken at; it is live-recording bookkeeping and deliberately excluded from
/// `PartialEq`/serialisation.
#[derive(Debug, Clone, Default)]
pub struct Journal {
    events: Vec<Event>,
    /// Guest cycle at the end of the run — the last checkpoint position plus
    /// the sum of every delta after it.
    end_cycle: u64,
    base_cycle: Option<u64>,
}

impl PartialEq for Journal {
    fn eq(&self, other: &Self) -> bool {
        self.events == other.events && self.end_cycle == other.end_cycle
    }
}

impl Eq for Journal {}

impl Journal {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn events(&self) -> &[Event] {
        &self.events
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn end_cycle(&self) -> u64 {
        self.end_cycle
    }

    /// Map an absolute `hot.cycles` value onto this journal's relative
    /// timeline. The first checkpoint fixes the base; before any checkpoint
    /// the first stamped event does.
    fn relative(&mut self, cycle: u64) -> u64 {
        let base = *self.base_cycle.get_or_insert(cycle);
        cycle.wrapping_sub(base)
    }

    /// Append a checkpoint fence and re-anchor the cycle cursor. The first
    /// checkpoint defines the timeline's origin; later checkpoints record
    /// their position relative to it.
    pub fn checkpoint(&mut self, tag: impl Into<String>, cycle: u64) {
        let rel = self.relative(cycle);
        self.events.push(Event::Checkpoint { tag: tag.into(), cycle: rel });
        self.end_cycle = rel;
    }

    /// Advance the cursor by `cycles` retired guest cycles.
    pub fn delta(&mut self, cycles: u64) {
        if cycles != 0 {
            self.events.push(Event::Delta { cycles });
            self.end_cycle = self.end_cycle.wrapping_add(cycles);
        }
    }

    /// Record an external interrupt-line change at the current cursor.
    pub fn interrupt(&mut self, cycle: u64, bits: u32) {
        let rel = self.relative(cycle);
        self.events.push(Event::Interrupt { cycle: rel, bits });
        self.end_cycle = rel;
    }

    /// Record an exception taken at the current cursor.
    pub fn exception(&mut self, cycle: u64, code: u32) {
        let rel = self.relative(cycle);
        self.events.push(Event::Exception { cycle: rel, code });
        self.end_cycle = rel;
    }

    /// Count of exception entries — a cheap CI assertion of "this run took
    /// the interrupts it was supposed to".
    pub fn exceptions(&self) -> usize {
        self.events.iter().filter(|e| matches!(e, Event::Exception { .. })).count()
    }

    /// Count of interrupt entries.
    pub fn interrupts(&self) -> usize {
        self.events.iter().filter(|e| matches!(e, Event::Interrupt { .. })).count()
    }

    /// Human-readable one-line summary for CI output.
    pub fn summary(&self) -> String {
        format!(
            "{} entries, {} interrupt(s), {} exception(s), end cycle {}",
            self.events.len(),
            self.interrupts(),
            self.exceptions(),
            self.end_cycle,
        )
    }

    /// Serialise to a line-oriented text format. One event per line, so a
    /// journal diffs cleanly with `diff(1)` and survives a partial trailing
    /// write by simply stopping at the last complete line.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for e in &self.events {
            match e {
                Event::Checkpoint { tag, cycle } => {
                    out.push_str(&format!("checkpoint {} {}\n", sanitize_tag(tag), cycle));
                }
                Event::Delta { cycles } => out.push_str(&format!("delta {}\n", cycles)),
                Event::Interrupt { cycle, bits } => {
                    out.push_str(&format!("interrupt {} {}\n", cycle, bits));
                }
                Event::Exception { cycle, code } => {
                    out.push_str(&format!("exception {} {}\n", cycle, code));
                }
            }
        }
        out
    }

    /// Parse the format produced by [`Journal::to_text`].
    pub fn from_text(text: &str) -> Result<Self, String> {
        let mut j = Journal::new();
        for (lineno, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let mut it = line.split_whitespace();
            let kind = it.next().unwrap_or("");
            let err = |what: &str| format!("journal line {}: {}", lineno + 1, what);
            match kind {
                "checkpoint" => {
                    let tag = it.next().ok_or_else(|| err("checkpoint missing tag"))?;
                    let cycle = parse_u64(it.next(), &err)?;
                    j.checkpoint(tag, cycle);
                }
                "delta" => {
                    let cycles = parse_u64(it.next(), &err)?;
                    j.delta(cycles);
                }
                "interrupt" => {
                    let cycle = parse_u64(it.next(), &err)?;
                    let bits = parse_u32(it.next(), &err)?;
                    j.interrupt(cycle, bits);
                }
                "exception" => {
                    let cycle = parse_u64(it.next(), &err)?;
                    let code = parse_u32(it.next(), &err)?;
                    j.exception(cycle, code);
                }
                other => return Err(err(&format!("unknown entry '{}'", other))),
            }
        }
        Ok(j)
    }

    /// Write the journal to `path`.
    pub fn save(&self, path: &std::path::Path) -> io::Result<()> {
        let f = std::fs::File::create(path)?;
        let mut w = BufWriter::new(f);
        w.write_all(self.to_text().as_bytes())?;
        w.flush()
    }

    /// Read a journal from `path`.
    pub fn load(path: &std::path::Path) -> io::Result<Self> {
        let f = BufReader::new(std::fs::File::open(path)?);
        let mut text = String::new();
        for line in f.lines() {
            text.push_str(&line?);
            text.push('\n');
        }
        Journal::from_text(&text).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    /// First index at which `self` and `other` differ, if any. Returns the
    /// index and owned copies of both entries for a readable CI error, so the
    /// caller is free to drop either journal immediately.
    pub fn first_divergence(&self, other: &Journal) -> Option<(usize, Option<Event>, Option<Event>)> {
        let n = self.events.len().max(other.events.len());
        (0..n).find_map(|i| {
            let a = self.events.get(i);
            let b = other.events.get(i);
            if a != b {
                Some((i, a.cloned(), b.cloned()))
            } else {
                None
            }
        })
    }

    pub fn matches(&self, other: &Journal) -> bool {
        self == other
    }
}

fn sanitize_tag(tag: &str) -> String {
    tag.chars().map(|c| if c.is_whitespace() { '_' } else { c }).collect()
}

fn parse_u64(tok: Option<&str>, err: &dyn Fn(&str) -> String) -> Result<u64, String> {
    tok.ok_or_else(|| err("expected a cycle value"))?
        .parse::<u64>()
        .map_err(|_| err("cycle value is not a u64"))
}

fn parse_u32(tok: Option<&str>, err: &dyn Fn(&str) -> String) -> Result<u32, String> {
    tok.ok_or_else(|| err("expected a numeric value"))?
        .parse::<u32>()
        .map_err(|_| err("value is not a u32"))
}

/// The outcome of re-driving a recording.
#[derive(Debug)]
pub struct ReplayReport {
    /// The event stream the replay pass actually produced.
    pub observed: Journal,
    /// True when `observed` is identical to the recording.
    pub matches: bool,
    /// The recording's entry count, final cycle, and the replay's, for CI.
    pub recorded_len: usize,
    pub recorded_end_cycle: u64,
    pub replayed_end_cycle: u64,
    /// First (index, recorded, observed) difference when the streams diverge.
    /// `None` means they matched.
    pub first_divergence: Option<(usize, Option<Event>, Option<Event>)>,
}

impl ReplayReport {
    /// Build a report by diffing a replay's observed stream against the
    /// recording it was meant to reproduce.
    pub fn compare(recording: &Journal, observed: Journal) -> Self {
        let first_divergence = recording.first_divergence(&observed);
        Self {
            matches: first_divergence.is_none(),
            recorded_len: recording.len(),
            recorded_end_cycle: recording.end_cycle(),
            replayed_end_cycle: observed.end_cycle(),
            first_divergence,
            observed,
        }
    }

    pub fn summary(&self) -> String {
        if self.matches {
            return format!(
                "replay matches recording ({} entries, end cycle {})",
                self.observed.len(),
                self.observed.end_cycle(),
            );
        }
        let mut s = format!(
            "replay DIVERGED (recording {} entries/end {}, replay {} entries/end {})",
            self.recorded_len,
            self.recorded_end_cycle,
            self.observed.len(),
            self.replayed_end_cycle,
        );
        if let Some((i, a, b)) = &self.first_divergence {
            s.push_str(&format!("\n  first difference at #{}", i));
            match (a, b) {
                (Some(a), Some(b)) => s.push_str(&format!("\n    recorded: {:?}\n    replayed: {:?}", a, b)),
                (Some(a), None) => s.push_str(&format!("\n    recorded: {:?}\n    replayed: <missing>", a)),
                (None, Some(b)) => s.push_str(&format!("\n    recorded: <missing>\n    replayed: {:?}", b)),
                (None, None) => {}
            }
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_text() {
        let mut j = Journal::new();
        j.checkpoint("reset", 0);
        j.delta(7);
        j.interrupt(7, 0x0400);
        j.delta(3);
        j.exception(10, 0);
        j.delta(5);

        let text = j.to_text();
        let back = Journal::from_text(&text).expect("must parse");
        assert_eq!(back, j);
        assert_eq!(back.end_cycle(), 15);
        assert_eq!(back.interrupts(), 1);
        assert_eq!(back.exceptions(), 1);
    }

    #[test]
    fn malformed_text_is_rejected() {
        assert!(Journal::from_text("nonsense 1 2\n").is_err());
        assert!(Journal::from_text("delta notanumber\n").is_err());
        assert!(Journal::from_text("interrupt 1\n").is_err());
    }

    #[test]
    fn first_divergence_reports_index_and_missing_tail() {
        let mut a = Journal::new();
        a.delta(1);
        a.delta(2);
        let mut b = Journal::new();
        b.delta(1);
        b.delta(9);
        let (i, _, _) = a.first_divergence(&b).unwrap();
        assert_eq!(i, 1);

        let mut c = Journal::new();
        c.delta(1);
        let (i, _, missing) = a.first_divergence(&c).unwrap();
        assert_eq!(i, 1);
        assert!(missing.is_none());
    }

    #[test]
    fn checkpoint_reanchors_cursor() {
        // The first checkpoint fixes the origin; a later one is recorded
        // relative to it, so the timeline survives a snapshot restore that
        // starts from a different absolute cycle.
        let mut j = Journal::new();
        j.checkpoint("reset", 1_000);
        assert_eq!(j.end_cycle(), 0);
        j.delta(100);
        assert_eq!(j.end_cycle(), 100);
        j.checkpoint("snap", 1_500);
        assert_eq!(j.end_cycle(), 500);
        j.delta(20);
        assert_eq!(j.end_cycle(), 520);
    }

    #[test]
    fn base_cycle_is_not_part_of_equality_or_the_text_format() {
        let mut a = Journal::new();
        a.checkpoint("reset", 1_000);
        a.delta(5);
        let mut b = Journal::new();
        b.checkpoint("reset", 9_999); // different absolute base
        b.delta(5);
        assert_eq!(a, b, "relative timelines must compare equal");
        assert_eq!(a.to_text(), b.to_text());
    }
}
