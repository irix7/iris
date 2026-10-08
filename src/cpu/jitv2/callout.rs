//! JIT→Rust callout clobber masks (#36).
//!
//! Every compiled region reaches back into Rust through a handful of
//! `call_indirect` callouts (memory access, FPU conversion, interpreter
//! fallback, exception delivery, …). Those calls are opaque to Cranelift: it
//! has no way to know which guest GPRs/FPRs the callee reads or writes, so it
//! must conservatively assume the call clobbers everything it might care
//! about. This module gives each callout a declared **read mask** and
//! **write mask** of guest GPRs/FPRs, and codegen threads the declared set
//! through a single emission helper so it can decide spill/forwarding work
//! from it.
//!
//! This is the plumbing only. [`Callout::clobbers`] returns
//! [`CalloutClobbers::CONSERVATIVE`] for every hook — every GPR and FPR,
//! read and written — which reproduces the pre-mask behaviour exactly (no
//! spill/forward decision can ever drop a register the callee might touch).
//! The per-hook fill is the follow-up ticket; when it lands, only
//! [`Callout::clobbers`] needs to change.
//!
//! This mirrors QEMU's `TCG_CALL_NO_READ_GLOBALS`/`TCG_CALL_NO_WRITE_GLOBALS`
//! design; see `docs/research/b02-dbt-granularity-caches.md` (transferable
//! technique #1) for the rationale.

/// Number of guest GPRs (`$zero`..`$ra`) addressable by a mask bit.
pub const GPR_COUNT: u32 = 32;
/// Number of guest FPRs addressable by a mask bit. FPRs are indexed by the
/// architectural register number, independently of the FR packing mode: a
/// 32-bit access to an odd register in FR=0 touches the same architectural
/// register a 64-bit access would.
pub const FPR_COUNT: u32 = 32;

/// The guest GPR/FPR read and write sets a single JIT→Rust callout declares.
///
/// Bit `n` of `gpr_read`/`gpr_write` means guest `$n` (GPR) is read/written;
/// bit `n` of `fpr_read`/`fpr_write` means guest `$f{n}` (FPR) is
/// read/written. A mask word is exactly 32 bits, one per architectural
/// register, so no guest register is ever silently out of range.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CalloutClobbers {
    pub gpr_read: u32,
    pub gpr_write: u32,
    pub fpr_read: u32,
    pub fpr_write: u32,
}

impl CalloutClobbers {
    /// Declares nothing: the callee touches no guest GPR or FPR. Sound only
    /// for a hook that genuinely has no guest-register side effects.
    pub const NONE: Self = Self {
        gpr_read: 0,
        gpr_write: 0,
        fpr_read: 0,
        fpr_write: 0,
    };

    /// The default every callout ships in #36: every GPR and FPR, both read
    /// and written. This is the same assumption Cranelift makes for an
    /// opaque call, so applying it changes nothing about emitted code — it
    /// exists so a narrower mask can be dropped in per hook without touching
    /// the emission plumbing.
    pub const CONSERVATIVE: Self = Self {
        gpr_read: u32::MAX,
        gpr_write: u32::MAX,
        fpr_read: u32::MAX,
        fpr_write: u32::MAX,
    };

    /// `true` iff this mask claims every register in both directions.
    pub const fn is_conservative(self) -> bool {
        self.gpr_read == u32::MAX
            && self.gpr_write == u32::MAX
            && self.fpr_read == u32::MAX
            && self.fpr_write == u32::MAX
    }

    /// `true` iff the callout may read guest GPR `reg`.
    pub const fn reads_gpr(self, reg: u32) -> bool {
        reg < GPR_COUNT && (self.gpr_read & (1u32 << reg)) != 0
    }

    /// `true` iff the callout may write guest GPR `reg`.
    pub const fn writes_gpr(self, reg: u32) -> bool {
        reg < GPR_COUNT && (self.gpr_write & (1u32 << reg)) != 0
    }

    /// `true` iff the callout may read guest FPR `reg`.
    pub const fn reads_fpr(self, reg: u32) -> bool {
        reg < FPR_COUNT && (self.fpr_read & (1u32 << reg)) != 0
    }

    /// `true` iff the callout may write guest FPR `reg`.
    pub const fn writes_fpr(self, reg: u32) -> bool {
        reg < FPR_COUNT && (self.fpr_write & (1u32 << reg)) != 0
    }

    /// Union of two clobber sets — how codegen accumulates a region's total
    /// callout footprint from the individual hooks.
    pub const fn union(self, other: Self) -> Self {
        Self {
            gpr_read: self.gpr_read | other.gpr_read,
            gpr_write: self.gpr_write | other.gpr_write,
            fpr_read: self.fpr_read | other.fpr_read,
            fpr_write: self.fpr_write | other.fpr_write,
        }
    }

    /// In-place [`Self::union`], the form codegen uses while walking a region.
    pub fn union_assign(&mut self, other: Self) {
        self.gpr_read |= other.gpr_read;
        self.gpr_write |= other.gpr_write;
        self.fpr_read |= other.fpr_read;
        self.fpr_write |= other.fpr_write;
    }

    /// Collapse read and write into the single set codegen cares about when
    /// deciding spills: a register the callee may *read* must be materialised
    /// (written back) before the call, and one it may *write* must be
    /// invalidated (reloaded) after. Both directions force the same
    /// "don't carry a forwarded value across this call" decision.
    pub const fn spill_plan(self) -> SpillPlan {
        SpillPlan {
            gpr: self.gpr_read | self.gpr_write,
            fpr: self.fpr_read | self.fpr_write,
        }
    }
}

/// The set of guest registers a callout — or a whole region's worth of
/// callouts — forces codegen to spill/forward around.
///
/// Derived from [`CalloutClobbers`] via [`CalloutClobbers::spill_plan`]; kept
/// as its own type so the emission site can consult "must this register be
/// spilled?" without re-deriving the read/write split.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SpillPlan {
    /// GPRs that must be spilled to / reloaded from memory around the call.
    pub gpr: u32,
    /// FPRs that must be spilled to / reloaded from memory around the call.
    pub fpr: u32,
}

impl SpillPlan {
    /// Nothing to spill — the callout touches no guest register.
    pub const NONE: Self = Self { gpr: 0, fpr: 0 };

    /// The conservative plan: every guest register must be spilled.
    pub const ALL: Self = Self {
        gpr: u32::MAX,
        fpr: u32::MAX,
    };

    /// `true` iff GPR `reg` must be spilled/reloaded around the callout.
    pub const fn spills_gpr(self, reg: u32) -> bool {
        reg < GPR_COUNT && (self.gpr & (1u32 << reg)) != 0
    }

    /// `true` iff FPR `reg` must be spilled/reloaded around the callout.
    pub const fn spills_fpr(self, reg: u32) -> bool {
        reg < FPR_COUNT && (self.fpr & (1u32 << reg)) != 0
    }

    /// `true` iff every GPR is spilled — the #36 conservative default, and
    /// the signal that no forwarding can be recovered at this call.
    pub const fn spills_all_gprs(self) -> bool {
        self.gpr == u32::MAX
    }
}

/// Every JIT→Rust callout a compiled region can emit.
///
/// One variant per hooked function pointer on [`MipsCore`], so the #37 fill
/// can assign each hook its true access set in exactly one place.
/// [`Callout::ALL`] is the exhaustive list, asserted against by the tests.
///
/// [`MipsCore`]: crate::cpu::mips_core::MipsCore
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Callout {
    /// `read{8,16,32,64}_fn` / `read*_sext_fn`: writes the loaded value
    /// straight to the destination GPR (or the scratch slot).
    MemoryRead,
    /// `write{8,16,32,64}_fn`: consumes a value, writes guest memory.
    MemoryWrite,
    /// The shared out-of-line memory helpers (`build_mem_helper`), whose
    /// guard + callout body covers one (kind, size, extend) combination.
    MemoryHelper,
    /// `write64_masked_fn` — SWL/SWR/SDL/SDR.
    MemoryWriteMasked,
    /// `kill_entry_fn` — un-publishes a stale FR-mismatch entry.
    KillEntry,
    /// `interp_fallback_fn` — runs one instruction through the interpreter.
    InterpFallback,
    /// `fetch_verify_fn` (`fetchverify` builds only).
    FetchVerify,
    /// `dev_trace_bp_fn` (`developer` builds only).
    DevTrace,
    /// `lockstep_step_fn` (`jitv2_lockstep` builds only).
    LockstepStep,
    /// `lockstep_compare_fn` (`jitv2_lockstep` builds only).
    LockstepCompare,
    /// `handle_exception_at_fn` — the shared exception-raise call.
    HandleException,
    /// `fpu_set_mode_fn` — updates the host rounding mode.
    FpuSetMode,
    /// `fpu_cvt_to_int_fn`.
    FpuCvtToInt,
    /// `fpu_cvt_int_to_float_fn`.
    FpuCvtIntToFloat,
    /// `fpu_cvt_d_to_s_fn`.
    FpuCvtDToS,
}

impl Callout {
    /// Every callout, so a test can assert the #36 default is conservative
    /// across the whole set without one being forgotten.
    pub const ALL: [Callout; 15] = [
        Callout::MemoryRead,
        Callout::MemoryWrite,
        Callout::MemoryHelper,
        Callout::MemoryWriteMasked,
        Callout::KillEntry,
        Callout::InterpFallback,
        Callout::FetchVerify,
        Callout::DevTrace,
        Callout::LockstepStep,
        Callout::LockstepCompare,
        Callout::HandleException,
        Callout::FpuSetMode,
        Callout::FpuCvtToInt,
        Callout::FpuCvtIntToFloat,
        Callout::FpuCvtDToS,
    ];

    /// A stable name for diagnostics and tests.
    pub const fn name(self) -> &'static str {
        match self {
            Callout::MemoryRead => "memory_read",
            Callout::MemoryWrite => "memory_write",
            Callout::MemoryHelper => "memory_helper",
            Callout::MemoryWriteMasked => "memory_write_masked",
            Callout::KillEntry => "kill_entry",
            Callout::InterpFallback => "interp_fallback",
            Callout::FetchVerify => "fetch_verify",
            Callout::DevTrace => "dev_trace",
            Callout::LockstepStep => "lockstep_step",
            Callout::LockstepCompare => "lockstep_compare",
            Callout::HandleException => "handle_exception",
            Callout::FpuSetMode => "fpu_set_mode",
            Callout::FpuCvtToInt => "fpu_cvt_to_int",
            Callout::FpuCvtIntToFloat => "fpu_cvt_int_to_float",
            Callout::FpuCvtDToS => "fpu_cvt_d_to_s",
        }
    }

    /// The read/write masks this callout declares.
    ///
    /// #36 ships [`CalloutClobbers::CONSERVATIVE`] for every hook:
    /// behaviour and emitted code are unchanged. #37 replaces the matching
    /// arm here with the hook's true access set; every emission site reads
    /// this one function, so nothing else moves.
    pub const fn clobbers(self) -> CalloutClobbers {
        CalloutClobbers::CONSERVATIVE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conservative_default_is_all_read_and_written() {
        let c = CalloutClobbers::CONSERVATIVE;
        assert!(c.is_conservative());
        assert_eq!(c.gpr_read, u32::MAX);
        assert_eq!(c.gpr_write, u32::MAX);
        assert_eq!(c.fpr_read, u32::MAX);
        assert_eq!(c.fpr_write, u32::MAX);
        for reg in 0..GPR_COUNT {
            assert!(c.reads_gpr(reg), "conservative must read gpr{reg}");
            assert!(c.writes_gpr(reg), "conservative must write gpr{reg}");
        }
        for reg in 0..FPR_COUNT {
            assert!(c.reads_fpr(reg), "conservative must read f{reg}");
            assert!(c.writes_fpr(reg), "conservative must write f{reg}");
        }
        // The spill plan that codegen consumes is likewise total.
        assert!(c.spill_plan().spills_all_gprs());
        assert_eq!(c.spill_plan(), SpillPlan::ALL);
    }

    #[test]
    fn every_callout_ships_the_conservative_default() {
        for callout in Callout::ALL {
            assert_eq!(
                callout.clobbers(),
                CalloutClobbers::CONSERVATIVE,
                "{} must default to all-clobbered until #37 fills its mask",
                callout.name()
            );
        }
    }

    #[test]
    fn union_accumulates_regions_across_callouts() {
        let mut acc = CalloutClobbers::NONE;
        assert!(!acc.is_conservative());
        for callout in Callout::ALL {
            acc.union_assign(callout.clobbers());
        }
        assert!(
            acc.is_conservative(),
            "uniting every callout must reproduce the conservative default"
        );
    }

    #[test]
    fn narrow_masks_select_only_the_declared_registers() {
        // A hand-built non-conservative mask (what #37 will produce): a load
        // callout that writes only r4 and reads nothing.
        let c = CalloutClobbers {
            gpr_read: 0,
            gpr_write: 1 << 4,
            fpr_read: 0,
            fpr_write: 0,
        };
        assert!(!c.is_conservative());
        assert!(c.writes_gpr(4));
        assert!(!c.writes_gpr(5));
        assert!(!c.reads_gpr(4));

        let plan = c.spill_plan();
        assert!(plan.spills_gpr(4), "a written register must be spilled/reloaded");
        assert!(!plan.spills_gpr(5), "an untouched register must not be spilled");
        assert!(!plan.spills_all_gprs());
        assert!(!plan.spills_fpr(0));
    }

    #[test]
    fn out_of_range_register_numbers_are_never_reported() {
        let c = CalloutClobbers::CONSERVATIVE;
        assert!(!c.reads_gpr(GPR_COUNT));
        assert!(!c.writes_gpr(u32::MAX));
        assert!(!c.reads_fpr(FPR_COUNT));
        assert!(!c.writes_fpr(1 << 20));
        // A mask with the high bit set still does not name a real >31 reg.
        let all = CalloutClobbers::CONSERVATIVE.spill_plan();
        assert!(!all.spills_gpr(32));
        assert!(!all.spills_fpr(32));
    }
}
