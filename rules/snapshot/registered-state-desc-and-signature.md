# Registered state descriptions, schema signature, Verify/Measure

**Keywords:** snapshot,save_state,load_state,StateDesc,signature,verify,measure,codec,state_desc
**Category:** snapshot

# Registered Save-State Codec (#45)

A device may describe its state fields once instead of hand-writing
`save_state`/`load_state`. `Saveable::state_desc()` returns
`Option<StateDesc<'_>>` (`src/state_desc.rs`); when present, the snapshot
orchestrator generates the payload from the description, records a readable
field list + schema signature in `snapshot.toml`, and verifies the incoming
value against the description on load.

## Shape

`StateDesc::new(device, version)` plus chained `.field(name, kind, since,
save, load)` closures and an optional `.after_load(closure)`:

- `save: Fn(&mut toml::map::Map<String, toml::Value>)` — insert the field under
  `name`.
- `load: Fn(&Value) -> Result<(), String>` — receives the **field's value**
  (not the whole state table). A rename/missing field is caught by
  `StateDesc::load`, which calls `verify` (exact field set) before applying.
- `.after_load` carries side effects a hand-written `load_state` used to do
  inline (IOC `update_interrupts`, DS1x86 `base_centiseconds`, WD33C93A
  transient clear, SCC `notify_all`).
- Closures capture `&self`; devices store state behind `Mutex`/atomics. A
  device needing `&mut self` to load (EEPROM's owned array) stays on the
  legacy codec.

## Signature and verification

`StateDesc::signature()` is a stable FNV-1a hash over device name, version and
each field's name + kind + element size + `since_version` (MAME's registry CRC).
It is written per-device into `snapshot.toml` under `[[state]]` and re-checked
on load from the **current code's** description, so a field renamed/added/
removed since the snapshot fails with a precise message. `measure()` is the
signature without serialising (Dolphin's `PointerWrap::Mode::Measure`).

Devices not yet migrated are recorded `registered = false` with a
value-shape signature (`value_signature`, keys + value kinds + array lengths),
which catches truncation/corruption but **not** a code rename.

## Invariants

- `capture_device` (`src/machine.rs`) calls `desc.save()` then `desc.verify()`
  on every save: a save closure that omits or adds a field fails the save.
- Pre-#45 manifests have no `[[state]]` table, so no check runs — they load
  exactly as before. The on-disk `*.bin` postcard format and `SCHEMA_VERSION`
  are unchanged.
- Per-device `version`/`since_version`/`minimum_version` are recorded in the
  manifest; migration closures live in `StateDesc` (#46).

## Per-device versioning and migration (#46)

Each `StateDesc` carries `version` (current) and `minimum_version` (oldest it
can load). Bump `version` when the registered field set changes
incompatibly and register a stepwise closure with
`.migrate(from, |old_value| -> Result<Value, String>)`; registering lowers
`minimum_version` automatically. On load `prepare_device_value`
(`src/machine.rs`) brings the payload to the current version before the
signature check:

- recorded version **older** → chain `from -> from+1 -> … -> version`; a gap
  refuses naming the exact missing step, a broken step fails at the migration,
  and the result is `Verify`d against the current field set before applying;
- recorded version **newer** → refused naming both versions (no forward
  migration);
- recorded version **equal** → the #45 signature check runs as before;
- **no** manifest entry (pre-#45) → load as-is, so old snapshots keep loading.

**Invariant:** `StateDesc::signature()` must *not* fold in `minimum_version`
or the migration registry. #45 builders recorded the signature without them;
adding them would make every pre-#46 snapshot fail the equal-version check.

## Not yet migrated

cpu, hal2, rex3, gr2, mgras, eeprom (the `&mut` case).
