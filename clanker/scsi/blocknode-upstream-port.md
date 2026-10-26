# BlockNode is upstream; keep `ScsiDevice::open` when the fork syncs

**Status:** upstream PR techomancer/iris#337 open 2026-10-26 (one commit,
`55f8ff8`, all CI green). The fork still has the older shape.

The upstream port keeps `ScsiDevice::open` as the single target constructor,
which preserves atomchild411's #265 "any controller can open" seam. The
WD33C93A and AIC-7880 therefore need no construction change, and the O2's AIC
keeps full CHD/COW support without the fork's raw-only shim.

The fork's current tree differs: it inlined node construction into
`wd33c93a` and added the raw-only AIC shim. When upstream #337 lands and is
merged into the fork:

- keep the shared `ScsiDevice::open`, delete the AIC raw-only shim;
- re-apply the fork's advisory-lock work (`ab0be92`, #90) on the ported
  `RawNode` — it is deliberately **not** in the upstream PR;
- the upstream port leaves `cow_disk::CowDisk` as a standalone test-covered
  type after SCSI stops using it; remove it only in a separate cleanup, not
  as part of the merge;
- do not reintroduce `DiskBackend`.

Tests of record: 16 `block_node` tests (the fork's 17th exercised `flock` and
stays fork-only with the advisory-lock node), 5 SCSI tests including
#279/#283, and `cargo test --workspace` (1366 default, 1848 `jitv2`).
