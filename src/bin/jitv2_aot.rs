//! Offline/AOT filler for the jitv2 persistent code cache (`pcache`).
//!
//! The viability spike for #54. The runtime cache is filled only as a side
//! effect of live compilation; this tool fills the *same* cache offline, from
//! a `j2 corpus` capture, so a fresh build can start warm without first
//! running the guest. Decision write-up: `docs/jitv2-aot-cache-spike.md`.
//!
//! Input is one `.pcp` file or a directory of them (`src/cpu/jitv2/pcp_dump.rs`).
//! For each page it re-runs exactly the runtime compile front half — walk the
//! union of the page's `requested` and `compiled` entries through the real
//! `Analyzer`, run the real `compile_region_uncommitted` — then stores the
//! relocation-free machine code through `pcache::fill_blob`, under the
//! emulator binary's build id. No bus, no `Machine`, no `PhysicalCodePage`;
//! the compiled page is not an input the emitter reads, and the code is
//! position-independent unless `IRIS_BAKE_HOOKS`/`IRIS_MEM_HELPERS` are set.
//!
//! ```text
//! jitv2_aot <corpus-file-or-dir> --iris-exe <path-to-iris> --cache-dir <base>
//!           [--cpu r4400|r5000|r10000] [--fr1 0|1|auto] [--opt speed|none]
//!           [--intrun N] [--dry-run] [--build-id <hex>]
//! ```
//!
//! `--cache-dir` is the same base `IRIS_JIT_CACHE_DIR` names; the tool writes
//! `<base>/<build-id>/<fingerprint>/...`, the layout the emulator's own
//! loader reads. The emulator build id is BLAKE3 of the executable named by
//! `--iris-exe`; the tool is a different binary, so it cannot use its own.
//!
//! This does **not** change the emulator's default behaviour. It only writes
//! files the opt-in `[jitv2] cache` path already knows how to read.

use std::path::{Path, PathBuf};

use iris::cpu::jitv2::analyzer::{instrs_linear, Analyzer};
use iris::cpu::jitv2::codegen::Codegen;
use iris::cpu::jitv2::comp::max_instrs_per_compile;
use iris::cpu::jitv2::pcache::{self, Blob};
use iris::cpu::jitv2::pcp_dump::PcpDump;
use iris::cpu::jitv2::{BITMAP_WORDS, ENTRIES_PER_PAGE, PAGE_SIZE};

struct Args {
    input: PathBuf,
    cache_dir: Option<PathBuf>,
    iris_exe: Option<PathBuf>,
    build_id: Option<String>,
    cpu: String,
    fr1: Option<bool>,
    opt_speed: bool,
    intrun: u32,
    dry_run: bool,
}

fn usage() -> ! {
    eprintln!(
        "usage: jitv2_aot <corpus-file-or-dir> --iris-exe <path> --cache-dir <base>\n\
         \x20      [--cpu r4400|r5000|r10000] [--fr1 0|1|auto]\n\
         \x20      [--opt speed|none] [--intrun N] [--dry-run] [--build-id <hex>]"
    );
    std::process::exit(2);
}

fn parse_args() -> Args {
    let mut args = std::env::args().skip(1);
    let input = match args.next() {
        Some(p) => PathBuf::from(p),
        None => usage(),
    };
    let mut a = Args {
        input,
        cache_dir: None,
        iris_exe: None,
        build_id: None,
        cpu: "r4400".into(),
        fr1: None,
        opt_speed: true,
        intrun: 1,
        dry_run: false,
    };
    while let Some(flag) = args.next() {
        let mut val = || args.next().unwrap_or_else(|| usage());
        match flag.as_str() {
            "--cache-dir" => a.cache_dir = Some(PathBuf::from(val())),
            "--iris-exe" => a.iris_exe = Some(PathBuf::from(val())),
            "--build-id" => a.build_id = Some(val()),
            "--cpu" => a.cpu = val(),
            "--fr1" => {
                let v = val();
                a.fr1 = match v.as_str() {
                    "auto" => None,
                    "0" | "off" | "false" => Some(false),
                    "1" | "on" | "true" => Some(true),
                    _ => usage(),
                };
            }
            "--opt" => {
                a.opt_speed = match val().as_str() {
                    "speed" => true,
                    "none" => false,
                    _ => usage(),
                };
            }
            "--intrun" => a.intrun = val().parse().unwrap_or(1),
            "--dry-run" => a.dry_run = true,
            _ => usage(),
        }
    }
    a
}

#[derive(Default)]
struct Stats {
    pages: u64,
    entries: u64,
    compiled: u64,
    walk_declined: u64,
    codegen_declined: u64,
    /// Compiles whose code Cranelift reported relocations for — the one
    /// correctness bar for reuse, and the only failure that would make the
    /// stored set smaller than the compiled set.
    relocation_refused: u64,
    stored: u64,
    bytes: u64,
    failed: u64,
}

fn main() {
    let args = parse_args();

    if !args.dry_run {
        if args.cache_dir.is_none() {
            eprintln!("jitv2_aot: --cache-dir is required unless --dry-run");
            usage();
        }
        if args.build_id.is_none() && args.iris_exe.is_none() {
            eprintln!("jitv2_aot: --iris-exe (or --build-id) is required unless --dry-run");
            usage();
        }
    }

    // `take_last_blob` is only populated while the persistent cache is on.
    // Set it up before the first `pcache::enabled()` call; the directory this
    // creates is unused (blobs go to `--cache-dir`/`<build-id>`), it only
    // satisfies the liveness check.
    std::env::set_var("IRIS_JIT_CACHE", "1");
    if std::env::var_os("IRIS_JIT_CACHE_DIR").is_none() {
        let scratch = args.cache_dir.clone().unwrap_or_else(std::env::temp_dir);
        std::env::set_var("IRIS_JIT_CACHE_DIR", scratch);
    }

    let build_id = if args.dry_run {
        String::new()
    } else if let Some(id) = &args.build_id {
        id.clone()
    } else {
        match pcache::build_id_of_exe(args.iris_exe.as_ref().unwrap()) {
            Some(id) => id,
            None => {
                eprintln!("jitv2_aot: cannot read --iris-exe {}", args.iris_exe.as_ref().unwrap().display());
                std::process::exit(1);
            }
        }
    };

    let files = match collect_pcp_files(&args.input) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("jitv2_aot: {}: {}", args.input.display(), e);
            std::process::exit(1);
        }
    };
    eprintln!("jitv2_aot: {} .pcp file(s), cpu={}, opt={}, intrun={}{}",
        files.len(), args.cpu, if args.opt_speed { "speed" } else { "none" },
        args.intrun, if args.dry_run { ", dry-run" } else { "" });
    if !args.dry_run {
        eprintln!("jitv2_aot: build-id={} cache-dir={}",
            build_id, args.cache_dir.as_ref().unwrap().display());
    }

    // Process-wide codegen config must be set before the Codegen (or an
    // Analyzer) is built, since both read it. These are exactly the knobs
    // `cache_fingerprint` hashes; a production `[jitv2] cache` run uses the
    // defaults this tool mirrors.
    Codegen::set_opt_level_speed(args.opt_speed);
    Codegen::set_interrupt_run(args.intrun);

    let stats = match args.cpu.as_str() {
        "r4400" => run::<iris::cpu::mips_cache_v2::R4400Cache>(&args, &build_id, &files, false),
        "r5000" => run::<iris::cpu::mips_cache_v2::R5000Cache>(&args, &build_id, &files, false),
        "r10000" => run::<iris::cpu::mips_cache_shadow::R10000ShadowCache>(&args, &build_id, &files, true),
        other => {
            eprintln!("jitv2_aot: unknown --cpu '{other}'");
            std::process::exit(2);
        }
    };

    println!(
        "jitv2_aot: pages={} entries={} compiled={} walk_declined={} codegen_declined={} \
         relocation_refused={} stored={} bytes={} io_failed={}",
        stats.pages, stats.entries, stats.compiled, stats.walk_declined,
        stats.codegen_declined, stats.relocation_refused, stats.stored, stats.bytes, stats.failed,
    );
}

fn collect_pcp_files(path: &Path) -> std::io::Result<Vec<PathBuf>> {
    if path.is_file() {
        return Ok(vec![path.to_path_buf()]);
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(path)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "pcp"))
        .collect();
    // Stable order so two runs over one corpus store in the same order.
    files.sort();
    Ok(files)
}

fn run<M: iris::cpu::mips_cache_v2::CpuModel>(
    args: &Args,
    build_id: &str,
    files: &[PathBuf],
    tagless: bool,
) -> Stats {
    // The offline analyzer default is process-wide; a production worker gets
    // its model's `MIPS4` const directly. Pin it here so the walk (and thus
    // the emitted code) is the same one the model's executor would produce.
    iris::cpu::jitv2::isa::set_mips4(M::MIPS4);

    let mut stats = Stats::default();
    let mut analyzer = Analyzer::new();
    let mut codegen = Codegen::new();
    codegen.dc_geometry = iris::cpu::mips_cache_v2::jit_dc_geometry_for_model::<M>(tagless);

    // Same fingerprint the live worker computes for this configuration. A
    // `None` here means the configuration bakes host addresses (helpers or
    // baked hooks) and cannot be cached at all.
    let fp = match codegen.cache_fingerprint(true) {
        Some(fp) => fp,
        None => {
            eprintln!("jitv2_aot: codegen bakes host addresses; refusing to fill a cache");
            std::process::exit(1);
        }
    };

    const RESET_AFTER_BYTES: u64 = 256 * 1024 * 1024;
    let mut since_reset: u64 = 0;
    let max_instrs = max_instrs_per_compile();

    for path in files {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("jitv2_aot: {}: {}", path.display(), e);
                stats.failed += 1;
                continue;
            }
        };
        let dump = match PcpDump::from_bytes(&bytes) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("jitv2_aot: {}: {}", path.display(), e);
                stats.failed += 1;
                continue;
            }
        };
        stats.pages += 1;
        let fr1 = args.fr1.unwrap_or(dump.fr1);

        // Union, not `requested` alone: a requested bit is cleared once a
        // compile covers it, so it reads empty on a settled page. See
        // `rules/jitv2/corpus-capture-from-the-pcp-cache.md`.
        let entry_words: Vec<u16> = (0..ENTRIES_PER_PAGE)
            .filter(|&o| dump.is_requested(o) || dump.is_compiled(o))
            .map(|o| o as u16)
            .collect();
        if entry_words.is_empty() {
            continue;
        }
        let page_base = dump.pfn.wrapping_mul(PAGE_SIZE);

        let (visited, covered, has_fpu) = {
            let instrs = analyzer.walk_multi_entry(&dump.words, &entry_words, page_base, max_instrs);
            let visited: Vec<_> = instrs_linear(instrs).cloned().collect();
            (visited, analyzer.covered().to_vec(), analyzer.has_fpu())
        };
        let covered_set: std::collections::HashSet<u16> = covered.iter().copied().collect();
        let declined = entry_words.iter().filter(|w| !covered_set.contains(w)).count() as u64;
        stats.entries += entry_words.len() as u64;
        stats.walk_declined += declined;
        if covered.is_empty() {
            continue;
        }

        // The words the walk decoded — the same mask the live compile stages
        // for churn avoidance and stores in the blob.
        let mut used = [0u64; BITMAP_WORDS];
        let mut instr_count = 0usize;
        for instr in &visited {
            let w = instr.word as usize;
            used[w >> 6] |= 1u64 << (w & 63);
            instr_count += 1;
        }
        let mut entries = [0u64; BITMAP_WORDS];
        for &o in &covered {
            entries[o as usize >> 6] |= 1u64 << (o % 64);
        }

        let mut instrs_owned = analyzer.instrs_snapshot();
        let func_id = codegen.compile_region_uncommitted(
            &mut instrs_owned, fr1, true, has_fpu, std::ptr::null_mut(),
        );
        match func_id {
            Some(_) => {
                let size = codegen.last_code_size() as u64;
                match codegen.take_last_blob() {
                    Some((code, align)) => {
                        stats.compiled += 1;
                        stats.bytes += code.len() as u64;
                        if !args.dry_run {
                            let ph = pcache::page_hash(&dump.words);
                            let blob = Blob {
                                entries,
                                used,
                                instr_count: instr_count as u32,
                                align,
                                words: Box::new(dump.words),
                                code,
                            };
                            match pcache::fill_blob(
                                args.cache_dir.as_ref().unwrap(), build_id, &fp, &ph, fr1, &blob,
                            ) {
                                Ok(_) => stats.stored += 1,
                                Err(e) => {
                                    eprintln!("jitv2_aot: {}: store failed: {}", path.display(), e);
                                    stats.failed += 1;
                                }
                            }
                        }
                        since_reset += size;
                    }
                    // Compiled, but not storable: a relocation (libcall). The
                    // runtime cache refuses these too; they are the ceiling on
                    // what any offline filler can reuse.
                    None => stats.relocation_refused += 1,
                }
            }
            None => stats.codegen_declined += 1,
        }

        if since_reset >= RESET_AFTER_BYTES {
            // Safety: no `JitFn` this Codegen returned is ever stored or
            // called here; the blob's bytes were copied out already.
            unsafe { codegen.reset() };
            since_reset = 0;
        }
    }
    stats
}
