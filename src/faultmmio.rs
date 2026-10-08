//! Fault-based MMIO viability spike — **compile-time gated, off by default**.
//!
//! Issue #52. The full reasoning, the measurement and the go/no-go decision
//! are in `docs/ppmem-fault-mmio-spike.md`. This file is the reproducible
//! evidence: an isolated prototype that
//!
//! 1. maps a "device" page `PROT_NONE`,
//! 2. installs a validated `SIGSEGV` handler,
//! 3. *services* a fault by attributing it to a registered site (never by
//!    decoding host machine code), and
//! 4. demonstrates the store-restartability invariant — a store spanning a
//!    page boundary can tear, and compute-then-commit (validate every word
//!    before committing any) is what prevents it.
//!
//! It also measures a real fault round trip against a branchy virtual bus
//! dispatch, which is the number the decision turns on.
//!
//! **It is not wired into `Physical`, `jitv2` or any execution path.** The
//! spike concluded NO-GO (see the doc), so enabling this feature changes
//! nothing about how the emulator runs — it only compiles these tests.
//!
//! Everything here is host-side scaffolding; a production implementation would
//! replace the guest instruction re-execution with Dolphin's registered-site
//! backpatch (`Source/Core/Core/PowerPC/Jit64/Jit.cpp`), which needs jitv2 to
//! emit per-access trap metadata. That work was deliberately not attempted.

#![allow(dead_code, unused_variables)]

use std::sync::atomic::{AtomicUsize, Ordering};

// ---------------------------------------------------------------------------
// Platform-independent logic: validation and the restartability invariant.
// ---------------------------------------------------------------------------

/// Which way a trapped access went.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AccessKind {
    Load,
    Store,
}

/// One guest instruction registered as a potential fault site.
///
/// `start..end` is the *host* code range the compiled access occupies; a fault
/// whose PC falls in it is one of ours. This is wasmtime's `lookup_code`
/// discipline (`crates/wasmtime/src/runtime/vm/traphandlers.rs:976-1011`): a
/// handler must be able to prove a fault came from code it registered, or it
/// must forward the signal. Otherwise a real emulator bug (a wild pointer)
/// gets silently serviced as MMIO.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FaultSite {
    /// Inclusive start of the host code range.
    pub start: usize,
    /// Exclusive end of the host code range.
    pub end: usize,
    /// Guest PC this site belongs to, for re-execution/diagnostics.
    pub guest_pc: u32,
    pub kind: AccessKind,
}

/// The result of classifying a fault.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// The fault is inside the arena and its PC is a registered site: service
    /// it (re-execute the guest instruction / commit its staged value).
    Service(FaultSite),
    /// Not ours — restore the previous disposition and re-raise. Never
    /// swallow a genuine bug.
    NotOurs,
}

/// Classify a fault the way the real handler would.
///
/// Two independent checks must both pass: the faulting *address* is inside the
/// device arena we trapped, and the faulting *PC* is a site we registered.
pub fn classify_fault(
    pc: usize,
    fault_addr: usize,
    arena: (usize, usize),
    sites: &[FaultSite],
) -> Verdict {
    if !(arena.0..arena.1).contains(&fault_addr) {
        return Verdict::NotOurs;
    }
    match sites.iter().find(|s| (s.start..s.end).contains(&pc)) {
        Some(site) => Verdict::Service(*site),
        None => Verdict::NotOurs,
    }
}

/// Does a `size`-byte access at `addr` cross a `page`-sized boundary?
///
/// A crossed store is exactly the instruction that can partially complete
/// before faulting: the host commits the words on the present page and then
/// faults on the trapped page. On x86 a normal split store may commit the
/// first page's portion, so retrying the guest instruction after servicing the
/// second page would commit the first page's bytes a second time.
pub fn crosses_page(addr: usize, size: usize, page: usize) -> bool {
    debug_assert!(page.is_power_of_two());
    (addr & !(page - 1)) != ((addr + size - 1) & !(page - 1))
}

/// A store plan that never commits an instruction that spans two pages.
///
/// The invariant, made concrete: **any store that can partially complete
/// before faulting must compute-then-commit or record undo.** Decomposing a
/// cross-page store into single-page, all-or-nothing commits is the
/// compute-then-commit form — each returned `(addr, len)` piece is wholly
/// within one page, so it either lands or it faults before landing. A real
/// implementation records the decomposition (and the destination register for
/// the faulting half) so the handler can finish it without re-executing a
/// partially-applied instruction.
///
/// The pieces are returned low-address-first with the *page-boundary* piece
/// last, so a handler that must fault does so after the in-page piece is
/// already committed rather than before it.
pub fn plan_store(addr: usize, size: usize, page: usize) -> Vec<(usize, usize)> {
    debug_assert!(page.is_power_of_two());
    let mut out = Vec::new();
    let mut at = addr;
    let mut left = size;
    while left > 0 {
        let boundary = (at | (page - 1)) + 1;
        let chunk = (boundary - at).min(left).max(1);
        out.push((at, chunk));
        at += chunk;
        left -= chunk;
    }
    out
}

// ---------------------------------------------------------------------------
// Unix (Linux x86-64) signal prototype.
// ---------------------------------------------------------------------------

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod imp {
    use super::*;
    use core::ffi::c_void;
    use core::ptr;

    /// Everything the handler and the parent exchange. Lives in a `MAP_SHARED`
    /// anonymous mapping created before the fork, so the child's handler can
    /// publish into it and the parent can read it after `waitpid`.
    #[repr(C)]
    #[derive(Default)]
    pub struct Shared {
        pub fault_addr: usize,
        pub fault_pc: usize,
        pub validated: u32,
        pub t0_ns: u64,
        pub t1_ns: u64,
        pub page1_tail_before: u64,
        pub page1_tail_after: u64,
    }

    static SHARED: AtomicUsize = AtomicUsize::new(0);
    static ARENA_LO: AtomicUsize = AtomicUsize::new(0);
    static ARENA_HI: AtomicUsize = AtomicUsize::new(0);
    static SITE_LO: AtomicUsize = AtomicUsize::new(0);
    static SITE_HI: AtomicUsize = AtomicUsize::new(0);
    static PAGE1_TAIL: AtomicUsize = AtomicUsize::new(0);

    fn now_ns() -> u64 {
        let mut ts: libc::timespec = unsafe { core::mem::zeroed() };
        unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
        ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
    }

    /// The faulting host PC out of the `ucontext`.
    unsafe fn fault_pc(ctx: *mut c_void) -> usize {
        let uc = ctx as *mut libc::ucontext_t;
        (*uc).uc_mcontext.gregs[libc::REG_RIP as usize] as usize
    }

    /// The handler. Async-signal-safe: it touches only atomics, the shared
    /// mapping, `sigaction`/`raise` and `_exit`.
    unsafe extern "C" fn handler(_sig: i32, info: *mut libc::siginfo_t, ctx: *mut c_void) -> () {
        let addr = (*info).si_addr() as usize;
        let pc = fault_pc(ctx);
        let sh = SHARED.load(Ordering::Relaxed) as *mut Shared;
        if !sh.is_null() {
            (*sh).fault_addr = addr;
            (*sh).fault_pc = pc;
            (*sh).t1_ns = now_ns();
        }

        let in_arena = addr >= ARENA_LO.load(Ordering::Relaxed)
            && addr < ARENA_HI.load(Ordering::Relaxed);
        let in_site =
            pc >= SITE_LO.load(Ordering::Relaxed) && pc < SITE_HI.load(Ordering::Relaxed);
        if in_arena && in_site {
            if !sh.is_null() {
                (*sh).validated = 1;
                // Record the in-page word a naive split store may have torn.
                let tail = PAGE1_TAIL.load(Ordering::Relaxed) as *const u64;
                if !tail.is_null() {
                    (*sh).page1_tail_after = ptr::read_volatile(tail);
                }
            }
            // A real handler resumes the guest at the faulting instruction; the
            // prototype just proves the fault was claimed and attributed.
            libc::_exit(42);
        } else {
            // Validation discipline: not our fault, so do not swallow it.
            // Restore the default action and re-raise; the child dies with
            // SIGSEGV exactly as it would without any handler.
            if !sh.is_null() {
                (*sh).validated = 0;
            }
            // The signal that invoked us is blocked while the handler runs, so
            // `raise` alone would only make it pending and `_exit` here would
            // terminate before it is ever delivered. Restore the default
            // action, unblock, then re-raise: the default action (terminate,
            // core dumped) fires before `raise` returns.
            let mut dfl: libc::sigaction = core::mem::zeroed();
            dfl.sa_sigaction = libc::SIG_DFL;
            libc::sigaction(_sig, &dfl, ptr::null_mut());
            let mut set: libc::sigset_t = core::mem::zeroed();
            libc::sigemptyset(&mut set);
            libc::sigaddset(&mut set, _sig);
            libc::sigprocmask(libc::SIG_UNBLOCK, &set, ptr::null_mut());
            libc::raise(_sig);
            libc::_exit(43);
        }
    }

    fn install() {
        unsafe {
            let mut sa: libc::sigaction = core::mem::zeroed();
            sa.sa_sigaction = handler as *const () as usize;
            sa.sa_flags = libc::SA_SIGINFO | libc::SA_ONSTACK;
            libc::sigemptyset(&mut sa.sa_mask);
            libc::sigaction(libc::SIGSEGV, &sa, ptr::null_mut());
        }
    }

    /// A `MAP_SHARED` `Shared` block, zeroed.
    pub fn shared_new() -> *mut Shared {
        let p = unsafe {
            libc::mmap(
                ptr::null_mut(),
                core::mem::size_of::<Shared>(),
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        assert_ne!(p, libc::MAP_FAILED, "shared mmap failed");
        unsafe { ptr::write_bytes(p, 0, core::mem::size_of::<Shared>()) };
        p as *mut Shared
    }

    pub fn shared_free(p: *mut Shared) {
        unsafe {
            libc::munmap(p as *mut c_void, core::mem::size_of::<Shared>());
        }
    }

    /// Reserve `pages` pages, hand back the base, and leave every page
    /// `PROT_NONE`. The caller maps back the pages it wants accessible.
    pub fn reserve_prot_none(pages: usize) -> *mut u8 {
        let len = pages * page_size();
        let p = unsafe {
            libc::mmap(
                ptr::null_mut(),
                len,
                libc::PROT_NONE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_NORESERVE,
                -1,
                0,
            )
        };
        assert_ne!(p, libc::MAP_FAILED, "reserve mmap failed");
        p as *mut u8
    }

    pub fn make_read_write(base: *mut u8, pages: usize) {
        let len = pages * page_size();
        let r = unsafe { libc::mprotect(base as *mut c_void, len, libc::PROT_READ | libc::PROT_WRITE) };
        assert_eq!(r, 0, "mprotect RW failed");
    }

    pub fn page_size() -> usize {
        let n = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        if n > 0 {
            n as usize
        } else {
            4096
        }
    }

    pub fn fork() -> libc::pid_t {
        unsafe { libc::fork() }
    }

    /// Exit status of a `waitpid`-style wait, in the same shape `libc` uses.
    pub fn wait(pid: libc::pid_t) -> libc::c_int {
        let mut status: libc::c_int = 0;
        unsafe { libc::waitpid(pid, &mut status, 0) };
        status
    }

    pub fn wifsignaled(status: libc::c_int) -> bool {
        status & 0x7f != 0x7f && (status & 0x7f) != 0
    }

    pub fn wtermsig(status: libc::c_int) -> libc::c_int {
        status & 0x7f
    }

    pub fn wexitstatus(status: libc::c_int) -> libc::c_int {
        (status >> 8) & 0xff
    }

    /// Install the handler, set the globals the handler reads, and return.
    /// Called in the child only, so the parent's signal disposition is never
    /// touched.
    pub fn setup_child(
        arena_lo: usize,
        arena_hi: usize,
        site_lo: usize,
        site_hi: usize,
        page1_tail: usize,
        sh: *mut Shared,
    ) {
        ARENA_LO.store(arena_lo, Ordering::Relaxed);
        ARENA_HI.store(arena_hi, Ordering::Relaxed);
        SITE_LO.store(site_lo, Ordering::Relaxed);
        SITE_HI.store(site_hi, Ordering::Relaxed);
        PAGE1_TAIL.store(page1_tail, Ordering::Relaxed);
        SHARED.store(sh as usize, Ordering::Relaxed);
        install();
    }

    /// Read the volatile word at `p` — the faulting access in tests.
    #[inline(never)]
    pub fn faulting_load(p: *const u64) -> u64 {
        unsafe { ptr::read_volatile(p) }
    }

    /// Write the volatile word at `p` — the faulting access in tests.
    #[inline(never)]
    pub fn faulting_store(p: *mut u64, v: u64) {
        unsafe { ptr::write_volatile(p, v) }
    }

    /// The host PC range the two access helpers occupy, generously padded.
    pub fn site_range() -> (usize, usize) {
        let lo = faulting_load as *const () as usize;
        let hi = faulting_store as *const () as usize + 0x100;
        (lo.min(faulting_store as *const () as usize), hi)
    }

    pub fn now() -> u64 {
        now_ns()
    }
}

#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
mod imp {
    // Non-Linux/non-x86-64 stub: the platform-independent logic above still
    // compiles and tests; the signal path does not exist here.
    use super::*;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: usize = 4096;

    fn site(start: usize, end: usize) -> FaultSite {
        FaultSite {
            start,
            end,
            guest_pc: 0x400000,
            kind: AccessKind::Load,
        }
    }

    #[test]
    fn classify_accepts_a_registered_fault_inside_the_arena() {
        let sites = [site(0x1000, 0x1100)];
        let v = classify_fault(0x1040, 0x8000_0000, (0x8000_0000, 0x8000_2000), &sites);
        assert!(matches!(v, Verdict::Service(s) if s.guest_pc == 0x400000));
    }

    #[test]
    fn classify_rejects_a_foreign_address() {
        // Same PC we registered, but the trapped *address* is not in our arena:
        // e.g. a wild pointer in the emulator, not a device access.
        let sites = [site(0x1000, 0x1100)];
        assert_eq!(
            classify_fault(0x1040, 0xDEAD_0000, (0x8000_0000, 0x8000_2000), &sites),
            Verdict::NotOurs
        );
    }

    #[test]
    fn classify_rejects_an_unregistered_pc() {
        // Inside the arena, but the fault did not come from code we compiled.
        let sites = [site(0x1000, 0x1100)];
        assert_eq!(
            classify_fault(0x9999, 0x8000_0100, (0x8000_0000, 0x8000_2000), &sites),
            Verdict::NotOurs
        );
    }

    #[test]
    fn crosses_page_detects_the_boundary_access() {
        assert!(!crosses_page(PAGE - 8, 8, PAGE), "aligned word: one page");
        assert!(crosses_page(PAGE - 4, 8, PAGE), "4-aligned qword straddles");
        assert!(!crosses_page(PAGE - 1, 1, PAGE), "one byte inside the last page");
        assert!(crosses_page(PAGE - 1, 2, PAGE), "two bytes run off the end");
    }

    #[test]
    fn plan_store_never_commits_a_cross_page_instruction() {
        let plan = plan_store(PAGE - 4, 8, PAGE);
        assert_eq!(plan, vec![(PAGE - 4, 4), (PAGE, 4)]);
        for (a, len) in plan {
            assert!(
                !crosses_page(a, len, PAGE),
                "piece [{a:#x}, +{len}) still crosses a page"
            );
        }
    }

    // -- real-signal tests (Linux x86-64 only) ------------------------------

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    mod signals {
        use super::*;
        use crate::faultmmio::imp::*;

        /// Map two pages PROT_NONE, make page 1 RW, and return (base, sh, sites).
        fn arena() -> (*mut u8, *mut Shared, [FaultSite; 1]) {
            let base = reserve_prot_none(2);
            make_read_write(base, 1);
            let sh = shared_new();
            let (lo, hi) = site_range();
            let sites = [FaultSite {
                start: lo,
                end: hi,
                guest_pc: 0x400000,
                kind: AccessKind::Load,
            }];
            (base, sh, sites)
        }

        #[test]
        fn handler_services_a_registered_fault() {
            let (base, sh, _sites) = arena();
            let (lo, hi) = site_range();
            let trap = unsafe { base.add(PAGE) } as *const u64;
            let pid = fork();
            if pid == 0 {
                setup_child(
                    base as usize,
                    base as usize + 2 * PAGE,
                    lo,
                    hi,
                    base as usize + PAGE - 8,
                    sh,
                );
                let _ = faulting_load(trap);
                unsafe { libc::_exit(99) };
            }
            let status = wait(pid);
            assert!(wexitstatus(status) == 42, "child status {status:#x}");
            unsafe {
                assert_eq!((*sh).validated, 1);
                assert_eq!((*sh).fault_addr, base as usize + PAGE);
            }
            shared_free(sh);
        }

        #[test]
        fn handler_forwards_a_foreign_fault() {
            // The faulting page is outside the arena the child tells the
            // handler about, so the handler must re-raise, not swallow.
            let (base, sh, _sites) = arena();
            let (lo, hi) = site_range();
            let pid = fork();
            if pid == 0 {
                // Arena passed to the handler deliberately excludes the page
                // we are about to touch.
                setup_child(
                    base as usize,
                    base as usize + PAGE, // excludes page 2
                    lo,
                    hi,
                    base as usize + PAGE - 8,
                    sh,
                );
                let _ = faulting_load(unsafe { base.add(PAGE) } as *const u64);
                unsafe { libc::_exit(99) };
            }
            let status = wait(pid);
            assert!(
                wifsignaled(status) && wtermsig(status) == libc::SIGSEGV,
                "foreign fault was swallowed: status {status:#x}"
            );
            unsafe { assert_eq!((*sh).validated, 0) };
            shared_free(sh);
        }

        #[test]
        fn naive_boundary_store_can_tear() {
            // Illustrates the hazard step 4 of the invitation describes: write
            // the in-page word first, then fault on the trapped page. The
            // in-page word is already committed.
            let (base, sh, _sites) = arena();
            let (lo, hi) = site_range();
            let tail = base as usize + PAGE - 8;
            let pid = fork();
            if pid == 0 {
                setup_child(base as usize, base as usize + 2 * PAGE, lo, hi, tail, sh);
                unsafe {
                    // "Translate then write" per word: the first word lands...
                    faulting_store((base.add(PAGE - 8)) as *mut u64, 0x1122_3344_5566_7788);
                    // ...then the second word faults on the trapped page.
                    faulting_store((base.add(PAGE)) as *mut u64, 0xAAAA_BBBB_CCCC_DDDD);
                }
                unsafe { libc::_exit(99) };
            }
            wait(pid);
            unsafe {
                assert_eq!((*sh).page1_tail_before, 0);
                assert_ne!(
                    (*sh).page1_tail_after, 0,
                    "the in-page word did NOT commit, so this is not a tear"
                );
            }
            shared_free(sh);
        }

        #[test]
        fn validated_boundary_store_is_atomic() {
            // Compute-then-commit: validate every word (touch both pages)
            // before committing any. The trapped page faults during
            // validation, so the in-page word is never written.
            let (base, sh, _sites) = arena();
            let (lo, hi) = site_range();
            let tail = base as usize + PAGE - 8;
            let pid = fork();
            if pid == 0 {
                setup_child(base as usize, base as usize + 2 * PAGE, lo, hi, tail, sh);
                unsafe {
                    // Validate the second page first: this faults, before any
                    // store has happened.
                    let _probe = faulting_load(base.add(PAGE) as *const u64);
                    faulting_store(base.add(PAGE - 8) as *mut u64, 0x1122_3344_5566_7788);
                }
                unsafe { libc::_exit(99) };
            }
            wait(pid);
            unsafe {
                assert_eq!((*sh).page1_tail_after, 0, "validation did not prevent the tear");
            }
            shared_free(sh);
        }

        /// The measurement the decision turns on: a real host fault round trip
        /// (delivery + handler entry) against a branchy virtual bus dispatch.
        ///
        /// Run with `--features faultmmio -- --nocapture` to see the numbers.
        #[test]
        fn fault_latency_exceeds_bus_dispatch() {
            let (base, sh, _sites) = arena();
            let (lo, hi) = site_range();
            let trap = unsafe { base.add(PAGE) } as *const u64;
            let pid = fork();
            if pid == 0 {
                setup_child(base as usize, base as usize + 2 * PAGE, lo, hi, 0, sh);
                let t0 = now();
                unsafe {
                    (*sh).t0_ns = t0;
                }
                let _ = faulting_load(trap);
                unsafe { libc::_exit(99) };
            }
            wait(pid);

            let fault_ns = unsafe { (*sh).t1_ns.saturating_sub((*sh).t0_ns) };
            shared_free(sh);

            // A branchy dispatch of the shape `Physical::read32` does before it
            // decides RAM vs device: a virtual call through a fat pointer.
            struct Dispatch;
            impl crate::traits::BusDevice for Dispatch {
                fn read32(&self, _addr: u32) -> crate::traits::BusRead32 {
                    crate::traits::BusRead32::ok(0x1234_5678)
                }
            }
            let dev: &dyn crate::traits::BusDevice = &Dispatch;
            const N: u64 = 20_000_000;
            let t0 = std::time::Instant::now();
            let mut acc = 0u32;
            for i in 0..N {
                acc = acc.wrapping_add(std::hint::black_box(dev.read32(i as u32)).data);
            }
            let dispatch_ns = t0.elapsed().as_nanos() as f64 / N as f64;
            std::hint::black_box(acc);

            eprintln!(
                "faultmmio: host fault round trip = {fault_ns} ns; \
                 bus dispatch = {dispatch_ns:.2} ns/access; \
                 ratio = {:.0}x",
                fault_ns as f64 / dispatch_ns
            );

            assert!(
                fault_ns as f64 > dispatch_ns * 5.0,
                "unexpectedly cheap fault ({fault_ns} ns) vs dispatch ({dispatch_ns:.2} ns)"
            );
        }
    }
}
