# The `Workstation` trait's optional overrides are load-bearing

**Symptom.** `iris --load-elf` reports the trait's `NOT_HERE` error, and the CI
socket's `save`/`restore`/`rollback`, `nvram save`, disc commands, scratch
commands and NAT queries all fail the same way, while the same operations work
through the monitor or in-process.

**Cause.** `Workstation` gives the optional facilities (`load_elf`,
`save_snapshot`, `load_snapshot`, `ci_restore`, `ci_rollback`, `save_nvram`,
`scratch_path`, `disc_changer`, `remount_cdrom_guest`,
`pending_chd_sync_count`, `sync_chd_disks`, `nat_control`, `nat_expected`)
defaults that return `NOT_HERE`. `impl Workstation for Machine` must override
every one. A merge resolution dropped the block after `as_any_mut`, so the
inherent methods still worked for direct calls while every call through
`dyn Workstation` (the CI socket, `iris-bench --iris`) hit the defaults.

**Rule.** When a merge touches `impl Workstation for Machine`, diff the override
list against `src/workstation.rs`, not just the methods that conflict. The
regression test `machine::controller_lifetime_tests::workstation_trait_forwards_load_elf`
calls the trait method (not the inherent one) so this cannot silently return.
