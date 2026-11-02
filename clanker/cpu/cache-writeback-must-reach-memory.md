# A CACHE writeback must reach memory, not stop in L2

**Kind:** rule

**Symptom.** A guest `WRITE(6)` returns success but the sector on the host
image is unchanged. Reading back through SCSI returns the old payload; a
scratch volume stayed all-`0xFF`. The IRIX 7 epoch store wrote five sectors,
reported `epoch-store: stored`, and the restarted node still fell back to the
seed.

**Cause.** `cache 0x15` (Hit_Writeback_Invalidate, PD) copied the dirty L1-D
line into the modelled L2 and stopped. The L2 line stayed dirty. The HPC3 DMA
engine reads physical memory through `BusDevice`, not through the cache, so it
fetched the descriptor and bounce buffer from memory as they were *before* the
guest filled them. In the IRIX 7 case the frame still held the PROM's RAM-test
pattern: each descriptor word read back as the complement of the address of
that word (`~0xA8B15000` at `0x08B15000`), which sent the DMA to a wild address.
The MC answered with its bus-error fill (`0xFF`), so the SCSI write wrote
`0xFF` and the guest saw a successful command.

**Fix.** `l1d_writeback_op` now calls `writeback_l1d_line(eidx, true)`, so a
CACHE writeback pushes the L2 line to memory and leaves it clean. Invalidation
paths keep `cascade = false`: an invalidate must not drop an L2 line that
another L1 sub-line has left dirty.

**How to recognise it.** A descriptor or data buffer read by DMA contains data
that looks like an old test pattern or unrelated memory, while the CPU reads
its own writes correctly. `MC: CPU Error at ...` for an address that is not
anywhere near the buffer is the follow-on. Check the guest's sequence: a store
to a cached alias, `cache 0x15`, then a device reads the physical address.

**Test.** `cpu::mips_cache_v2::tests::cache_op_hit_wbinv_l1d` asserts memory
holds the data immediately after `Hit_Writeback_Invalidate`, and
`index_writeback_invalidate_pushes_through_to_memory` pins the index-op path.
The two-node IRIX 7 rig is the end-to-end check: the root logs
`epoch-store: stored`, the scratch file holds the record, and the restarted
member logs `epoch-store: restored`.
