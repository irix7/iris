//! Registered save-state codec.
//!
//! A device describes its own state fields once — name, logical kind,
//! `since_version`, and a read/write closure — and the payload, the readable
//! field list and the schema signature are all generated from that description.
//!
//! This is the IRIS analogue of QEMU's `VMStateDescription` / MAME's
//! `save_item` registry. A device returns a [`StateDesc`] from
//! [`Saveable::state_desc`](crate::traits::Saveable::state_desc); the snapshot
//! orchestrator then:
//!
//! * serialises the payload by walking the registered fields
//!   ([`StateDesc::save`]) — the same description drives load
//!   ([`StateDesc::load`]);
//! * writes a human-readable field list into `snapshot.toml` as the analogue of
//!   QEMU's VM Description ([`StateDesc::field_list`]);
//! * records a schema signature — a stable hash over the registered field
//!   names, kinds and versions (MAME's registry CRC) ([`StateDesc::signature`]);
//! * and on load checks the incoming value against the description
//!   ([`StateDesc::verify`]) before applying it, so a missing or renamed field
//!   fails rather than loading silently.
//!
//! `Measure` is the read-only half of the round-trip (Dolphin's
//! `PointerWrap::Mode`): it computes the schema signature — and, from a value,
//! a shape signature — without touching the device.

use std::borrow::Cow;
use toml::Value;

/// A `toml::Value` table — what a [`StateField`]'s save closure writes into.
pub type TomlMap = toml::map::Map<String, Value>;

/// Logical type of a registered field. `element_size` is the element size in
/// bytes (0 for variable-length aggregates); both the readable field list and
/// the schema signature are derived from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    Bool,
    U8,
    U16,
    U32,
    U64,
    I64,
    F64,
    Str,
    U8Array,
    U16Array,
    U32Array,
    U64Array,
    Table,
    Array,
}

impl FieldKind {
    /// Element size in bytes; 0 for aggregates whose size is not fixed.
    pub fn element_size(self) -> u32 {
        match self {
            FieldKind::Bool | FieldKind::U8 => 1,
            FieldKind::U16 => 2,
            FieldKind::U32 => 4,
            FieldKind::U64 | FieldKind::I64 | FieldKind::F64 => 8,
            FieldKind::Str
            | FieldKind::U8Array
            | FieldKind::U16Array
            | FieldKind::U32Array
            | FieldKind::U64Array
            | FieldKind::Table
            | FieldKind::Array => 0,
        }
    }

    /// Stable short tag used in the manifest field list and the signature.
    pub fn tag(self) -> &'static str {
        match self {
            FieldKind::Bool => "bool",
            FieldKind::U8 => "u8",
            FieldKind::U16 => "u16",
            FieldKind::U32 => "u32",
            FieldKind::U64 => "u64",
            FieldKind::I64 => "i64",
            FieldKind::F64 => "f64",
            FieldKind::Str => "str",
            FieldKind::U8Array => "u8array",
            FieldKind::U16Array => "u16array",
            FieldKind::U32Array => "u32array",
            FieldKind::U64Array => "u64array",
            FieldKind::Table => "table",
            FieldKind::Array => "array",
        }
    }

    /// Parse a [`FieldKind::tag`] back. Used by the manifest reader.
    pub fn from_tag(s: &str) -> Option<Self> {
        Some(match s {
            "bool" => FieldKind::Bool,
            "u8" => FieldKind::U8,
            "u16" => FieldKind::U16,
            "u32" => FieldKind::U32,
            "u64" => FieldKind::U64,
            "i64" => FieldKind::I64,
            "f64" => FieldKind::F64,
            "str" => FieldKind::Str,
            "u8array" => FieldKind::U8Array,
            "u16array" => FieldKind::U16Array,
            "u32array" => FieldKind::U32Array,
            "u64array" => FieldKind::U64Array,
            "table" => FieldKind::Table,
            "array" => FieldKind::Array,
            _ => return None,
        })
    }
}

/// Readable description of one registered field. Copied into the snapshot
/// manifest; contains no closures, so it is cheap to clone and compare.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldInfo {
    pub name: String,
    pub kind: FieldKind,
    /// First `StateDesc` version in which this field exists (0 = since the
    /// description was introduced).
    pub since_version: u32,
}

impl FieldInfo {
    /// Encode as `name:kind:since` for the manifest field list.
    pub fn encode(&self) -> String {
        format!("{}:{}:{}", self.name, self.kind.tag(), self.since_version)
    }

    /// Decode a [`FieldInfo::encode`] string.
    pub fn decode(s: &str) -> Option<Self> {
        let mut it = s.splitn(3, ':');
        let name = it.next()?.to_string();
        let kind = FieldKind::from_tag(it.next()?)?;
        let since_version = it.next().and_then(|v| v.parse().ok()).unwrap_or(0);
        Some(FieldInfo { name, kind, since_version })
    }
}

type SaveFn<'a> = Box<dyn Fn(&mut TomlMap) + 'a>;
type LoadFn<'a> = Box<dyn Fn(&Value) -> Result<(), String> + 'a>;
/// Transforms a payload captured at one version into the next version's shape.
type MigrateFn<'a> = Box<dyn Fn(&Value) -> Result<Value, String> + 'a>;

/// One registered field: metadata plus the closures that read and write it
/// against the borrowing device. The closures capture `&device`; devices with
/// interior mutability (atomics, `Mutex`) mutate through it on load.
pub struct StateField<'a> {
    pub name: Cow<'static, str>,
    pub kind: FieldKind,
    pub since_version: u32,
    save: SaveFn<'a>,
    load: LoadFn<'a>,
}

/// A device's registered state description. See the module docs.
pub struct StateDesc<'a> {
    /// Device name (matches the snapshot file base, e.g. `"cpu"`).
    pub device: Cow<'static, str>,
    /// Current schema version for this device. Bump this when the registered
    /// field set changes incompatibly and register a [`StateDesc::migrate`]
    /// closure for every old version you want to keep loading.
    pub version: u32,
    /// Oldest version this description can load, directly or through a chain of
    /// registered migrations. Defaults to `version`; [`StateDesc::migrate`]
    /// lowers it automatically.
    pub minimum_version: u32,
    fields: Vec<StateField<'a>>,
    migrations: Vec<(u32, MigrateFn<'a>)>,
    after_load: Option<Box<dyn Fn() -> Result<(), String> + 'a>>,
}

impl<'a> StateDesc<'a> {
    pub fn new(device: impl Into<Cow<'static, str>>, version: u32) -> Self {
        Self {
            device: device.into(),
            version,
            minimum_version: version,
            fields: Vec::new(),
            migrations: Vec::new(),
            after_load: None,
        }
    }

    /// Register a field. `save` writes the field's current value into the
    /// output table (under `name`); `load` applies the field's value. `kind`
    /// is logical metadata for the field list and signature.
    pub fn field(
        mut self,
        name: impl Into<Cow<'static, str>>,
        kind: FieldKind,
        since_version: u32,
        save: impl Fn(&mut TomlMap) + 'a,
        load: impl Fn(&Value) -> Result<(), String> + 'a,
    ) -> Self {
        self.fields.push(StateField {
            name: name.into(),
            kind,
            since_version,
            save: Box::new(save),
            load: Box::new(load),
        });
        self
    }

    /// Register a post-load hook, run once after every field has been applied.
    /// For side effects a device's `load_state` used to perform inline, such as
    /// re-deriving cascade bits or clearing transient state.
    pub fn after_load(mut self, f: impl Fn() -> Result<(), String> + 'a) -> Self {
        self.after_load = Some(Box::new(f));
        self
    }

    /// Register a migration from `from_version` to `from_version + 1`. The
    /// closure receives the device's payload as captured at `from_version` and
    /// returns it in the `from_version + 1` shape. Loading chains migrations
    /// from the snapshot's version up to [`StateDesc::version`]; a gap is a
    /// precise refusal rather than a silent misload. Registering a migration
    /// also lowers [`StateDesc::minimum_version`] to include `from_version`.
    pub fn migrate(
        mut self,
        from_version: u32,
        f: impl Fn(&Value) -> Result<Value, String> + 'a,
    ) -> Self {
        self.minimum_version = self.minimum_version.min(from_version);
        self.migrations.push((from_version, Box::new(f)));
        self
    }

    /// Migrate a payload captured at `from` up to the current [`version`].
    ///
    /// * `from == version` → the value is cloned unchanged.
    /// * `from > version` → refused: a snapshot from a newer build cannot be
    ///   interpreted (we don't know its future layout).
    /// * `from < minimum_version` → refused: older than any registered chain.
    /// * a missing step in the chain → refused, naming the exact versions.
    ///
    /// The migrated value is verified against the current field set before it
    /// is returned, so a broken migration fails at the migration, not midway
    /// through applying fields.
    ///
    /// [`version`]: StateDesc::version
    pub fn migrate_value(&self, from: u32, value: &Value) -> Result<Value, String> {
        if from == self.version {
            return Ok(value.clone());
        }
        if from > self.version {
            return Err(format!(
                "device '{}' snapshot state version {} is newer than this build's version {} \
                 — refusing rather than misinterpreting a future layout",
                self.device, from, self.version
            ));
        }
        if from < self.minimum_version {
            return Err(format!(
                "device '{}' snapshot state version {} is older than the minimum supported \
                 version {} (no migration chain reaches it)",
                self.device, from, self.minimum_version
            ));
        }
        let mut v = value.clone();
        let mut cur = from;
        while cur < self.version {
            let step = self
                .migrations
                .iter()
                .find(|(f, _)| *f == cur)
                .ok_or_else(|| {
                    format!(
                        "device '{}' has no migration registered from state version {} to {}",
                        self.device,
                        cur,
                        cur + 1
                    )
                })?;
            v = (step.1)(&v).map_err(|e| {
                format!(
                    "device '{}' migration {} -> {} failed: {}",
                    self.device,
                    cur,
                    cur + 1,
                    e
                )
            })?;
            cur += 1;
        }
        self.verify(&v)?;
        Ok(v)
    }

    /// Serialise the descriptor's fields into a fresh table — the generated
    /// payload (the analogue of QEMU walking the `VMStateField` array).
    pub fn save(&self) -> Value {
        let mut t = TomlMap::new();
        for f in &self.fields {
            (f.save)(&mut t);
        }
        Value::Table(t)
    }

    /// Verify `v` against the registered field set, then apply every field.
    /// Proof: missing or renamed fields fail here rather than loading silent
    /// defaults.
    pub fn load(&self, v: &Value) -> Result<(), String> {
        self.verify(v)?;
        for f in &self.fields {
            if let Some(fv) = v.get(f.name.as_ref()) {
                (f.load)(fv).map_err(|e| format!("{}: field '{}': {}", self.device, f.name, e))?;
            }
        }
        if let Some(after) = &self.after_load {
            after()?;
        }
        Ok(())
    }

    /// Verify that `v` carries exactly the registered fields — nothing
    /// missing (renamed away) and nothing unknown (added without registering).
    pub fn verify(&self, v: &Value) -> Result<(), String> {
        let tbl = v
            .as_table()
            .ok_or_else(|| format!("{}: state is not a table", self.device))?;
        for f in &self.fields {
            if !tbl.contains_key(f.name.as_ref()) {
                return Err(format!(
                    "{}: missing field '{}' — renamed or dropped since the snapshot was taken",
                    self.device, f.name
                ));
            }
        }
        for k in tbl.keys() {
            if !self.fields.iter().any(|f| f.name.as_ref() == k) {
                return Err(format!(
                    "{}: unknown field '{}' — added or renamed without registering it",
                    self.device, k
                ));
            }
        }
        Ok(())
    }

    /// Readable field list for the manifest (QEMU's VM Description analogue).
    pub fn field_list(&self) -> Vec<FieldInfo> {
        self.fields
            .iter()
            .map(|f| FieldInfo {
                name: f.name.to_string(),
                kind: f.kind,
                since_version: f.since_version,
            })
            .collect()
    }

    /// Schema signature: a stable hash over the device name, version and each
    /// registered field's name + kind size + `since_version`. MAME's registry
    /// CRC, so a code change to the registered schema fails a load instead of
    /// mis-assigning fields. `Measure` is this same computation without
    /// serialising.
    pub fn signature(&self) -> u64 {
        let mut h = FNV_OFFSET;
        fnv_mix(&mut h, self.device.as_bytes());
        fnv_mix(&mut h, &self.version.to_le_bytes());
        for f in &self.fields {
            fnv_mix(&mut h, f.name.as_bytes());
            fnv_mix(&mut h, f.kind.tag().as_bytes());
            fnv_mix(&mut h, &f.kind.element_size().to_le_bytes());
            fnv_mix(&mut h, &f.since_version.to_le_bytes());
        }
        h
    }

    /// Dolphin's `PointerWrap::Mode::Measure` — the signature without
    /// touching the device. Here it is exactly [`StateDesc::signature`].
    pub fn measure(&self) -> u64 {
        self.signature()
    }
}

// ---- value-shape signatures (legacy / unregistered devices) ----

/// Signature over the *shape* of a value tree (keys + value kinds + array
/// lengths, not the values). Used for devices that have not been migrated to a
/// `StateDesc`: stable across saves of the same schema, so a truncated or
/// corrupt on-disk value fails to match the signature recorded when it was
/// saved. A code rename is not caught for unregistered devices — that requires
/// a registered description.
pub fn value_signature(device: &str, v: &Value) -> u64 {
    let mut h = FNV_OFFSET;
    fnv_mix(&mut h, device.as_bytes());
    hash_value_shape(&mut h, v);
    h
}

fn hash_value_shape(h: &mut u64, v: &Value) {
    match v {
        Value::Boolean(b) => {
            fnv_mix(h, b"bool");
            fnv_mix(h, &[*b as u8]);
        }
        Value::Integer(_) => fnv_mix(h, b"int"),
        Value::Float(_) => fnv_mix(h, b"float"),
        Value::String(_) => fnv_mix(h, b"str"),
        Value::Datetime(_) => fnv_mix(h, b"datetime"),
        Value::Array(a) => {
            fnv_mix(h, b"array");
            fnv_mix(h, &(a.len() as u64).to_le_bytes());
            for e in a {
                hash_value_shape(h, e);
            }
        }
        Value::Table(t) => {
            fnv_mix(h, b"table");
            fnv_mix(h, &(t.len() as u64).to_le_bytes());
            for (k, val) in t {
                fnv_mix(h, k.as_bytes());
                hash_value_shape(h, val);
            }
        }
    }
}

/// Best-effort readable field list for an unregistered device, inferred from
/// the top-level keys of its saved value. Not a substitute for a registered
/// description — it is recorded in the manifest for inspection only.
pub fn value_field_list(v: &Value) -> Vec<FieldInfo> {
    let mut out = Vec::new();
    if let Some(tbl) = v.as_table() {
        for (k, val) in tbl {
            out.push(FieldInfo {
                name: k.clone(),
                kind: infer_kind(val),
                since_version: 0,
            });
        }
    }
    out
}

fn infer_kind(v: &Value) -> FieldKind {
    match v {
        Value::Boolean(_) => FieldKind::Bool,
        Value::Integer(_) => FieldKind::I64,
        Value::Float(_) => FieldKind::F64,
        Value::String(_) => FieldKind::Str,
        Value::Datetime(_) => FieldKind::Str,
        Value::Array(_) => FieldKind::Array,
        Value::Table(_) => FieldKind::Table,
    }
}

// ---- stable FNV-1a 64 hash ----

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv_mix(h: &mut u64, bytes: &[u8]) {
    for &b in bytes {
        *h ^= b as u64;
        *h = h.wrapping_mul(FNV_PRIME);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_info_encodes_and_decodes() {
        let f = FieldInfo { name: "ch0".into(), kind: FieldKind::Table, since_version: 3 };
        assert_eq!(f, FieldInfo::decode(&f.encode()).unwrap());
        assert!(FieldInfo::decode("bogus").is_none());
        assert!(FieldInfo::decode("x:notakind:1").is_none());
    }

    #[test]
    fn signature_is_stable_and_sensitive_to_the_schema() {
        let make = |name: &'static str, kind: FieldKind| {
            StateDesc::new("d", 1).field("x", kind, 1, |_| {}, |_| Ok(()))
        };
        let a = make("d", FieldKind::U32).signature();
        assert_eq!(a, make("d", FieldKind::U32).signature(), "same schema, same signature");
        assert_ne!(a, make("d", FieldKind::U64).signature(), "kind size feeds the signature");
    }

    #[test]
    fn verify_rejects_missing_and_unknown_fields() {
        let desc = StateDesc::new("d", 1)
            .field("a", FieldKind::U8, 1, |_| {}, |_| Ok(()))
            .field("b", FieldKind::U8, 1, |_| {}, |_| Ok(()));

        let mut missing = TomlMap::new();
        missing.insert("a".into(), Value::Integer(1));
        assert!(desc.verify(&Value::Table(missing)).unwrap_err().contains("missing field 'b'"));

        let mut unknown = TomlMap::new();
        unknown.insert("a".into(), Value::Integer(1));
        unknown.insert("b".into(), Value::Integer(1));
        unknown.insert("c".into(), Value::Integer(1));
        assert!(desc.verify(&Value::Table(unknown)).unwrap_err().contains("unknown field 'c'"));

        let mut exact = TomlMap::new();
        exact.insert("a".into(), Value::Integer(1));
        exact.insert("b".into(), Value::Integer(2));
        assert!(desc.verify(&Value::Table(exact)).is_ok());
    }

    #[test]
    fn generated_payload_round_trips_through_the_description() {
        // The generated save feeds the same description's load, exercising the
        // Verify path in between.
        let cell = std::sync::atomic::AtomicU32::new(0);
        let desc = StateDesc::new("d", 1).field(
            "v",
            FieldKind::U32,
            1,
            |t| {
                t.insert("v".into(), Value::Integer(7));
            },
            |v| {
                let _ = v.as_integer().ok_or("not an int")?;
                Ok(())
            },
        );
        let v = desc.save();
        desc.load(&v).expect("load");
        let _ = cell; // keep the closure-capture example minimal
    }

    #[test]
    fn value_signature_is_stable_and_shape_sensitive() {
        let mut a = TomlMap::new();
        a.insert("x".into(), Value::Integer(1));
        let mut b = TomlMap::new();
        b.insert("x".into(), Value::Integer(999));
        let va = Value::Table(a.clone());
        let vb = Value::Table(b);
        assert_eq!(value_signature("d", &va), value_signature("d", &va));
        assert_eq!(value_signature("d", &va), value_signature("d", &vb), "values don't change the shape");

        let mut c = TomlMap::new();
        c.insert("x".into(), Value::Integer(1));
        c.insert("y".into(), Value::Integer(2));
        assert_ne!(value_signature("d", &va), value_signature("d", &Value::Table(c)));
    }

    // ---- per-device versioning + migration registry (#46) ----

    /// A description at version 3 that added `b` in v2 and `c` in v3, with a
    /// stepwise migration registered for each jump. `a` exists since v1.
    fn migrating_desc() -> StateDesc<'static> {
        StateDesc::new("mig", 3)
            .field("a", FieldKind::U32, 1, |_| {}, |_| Ok(()))
            .field("b", FieldKind::U32, 2, |_| {}, |_| Ok(()))
            .field("c", FieldKind::U32, 3, |_| {}, |_| Ok(()))
            .migrate(1, |v| {
                let mut t = v.as_table().cloned().unwrap_or_default();
                t.insert("b".into(), Value::Integer(0));
                Ok(Value::Table(t))
            })
            .migrate(2, |v| {
                let mut t = v.as_table().cloned().unwrap_or_default();
                t.insert("c".into(), Value::Integer(0));
                Ok(Value::Table(t))
            })
    }

    fn old_value() -> Value {
        let mut t = TomlMap::new();
        t.insert("a".into(), Value::Integer(7));
        Value::Table(t)
    }

    #[test]
    fn old_version_state_migrates_to_current() {
        let desc = migrating_desc();
        // Registering migrations lowered the minimum supported version.
        assert_eq!(desc.minimum_version, 1);
        assert_eq!(desc.version, 3);

        let migrated = desc.migrate_value(1, &old_value()).expect("v1 migrates");
        // The migration chain added the fields introduced after v1.
        assert_eq!(migrated.get("a").and_then(|v| v.as_integer()), Some(7));
        assert_eq!(migrated.get("b").and_then(|v| v.as_integer()), Some(0));
        assert_eq!(migrated.get("c").and_then(|v| v.as_integer()), Some(0));
        // The migrated value now matches the current field set.
        desc.verify(&migrated).expect("migrated value matches current schema");

        // A v2 payload skips straight to v3 through the single remaining step.
        let mut v2 = TomlMap::new();
        v2.insert("a".into(), Value::Integer(1));
        v2.insert("b".into(), Value::Integer(2));
        let migrated = desc.migrate_value(2, &Value::Table(v2)).expect("v2 migrates");
        assert_eq!(migrated.get("b").and_then(|v| v.as_integer()), Some(2));
        assert_eq!(migrated.get("c").and_then(|v| v.as_integer()), Some(0));
    }

    #[test]
    fn current_version_state_passes_through_unchanged() {
        let desc = migrating_desc();
        let v = desc.save();
        assert_eq!(desc.migrate_value(3, &v).unwrap(), v);
    }

    #[test]
    fn newer_version_refuses_with_a_precise_reason() {
        let desc = migrating_desc();
        let err = desc.migrate_value(4, &old_value()).unwrap_err();
        assert!(err.contains("version 4"), "names the incoming version: {err}");
        assert!(err.contains("newer than this build's version 3"), "names the current version: {err}");
        assert!(err.contains("refusing"), "says it refuses rather than misloading: {err}");
    }

    #[test]
    fn missing_migration_step_refuses_with_the_exact_versions() {
        // v1 -> v2 registered, but no v2 -> v3 step: pulling a v1 payload up to
        // the current v3 must refuse, not stop halfway.
        let desc = StateDesc::new("gap", 3)
            .field("a", FieldKind::U32, 1, |_| {}, |_| Ok(()))
            .field("b", FieldKind::U32, 2, |_| {}, |_| Ok(()))
            .field("c", FieldKind::U32, 3, |_| {}, |_| Ok(()))
            .migrate(1, |v| {
                let mut t = v.as_table().cloned().unwrap_or_default();
                t.insert("b".into(), Value::Integer(0));
                Ok(Value::Table(t))
            });
        let err = desc.migrate_value(1, &old_value()).unwrap_err();
        assert!(err.contains("no migration registered from state version 2 to 3"), "{err}");
    }

    #[test]
    fn version_below_the_chain_refuses_clearly() {
        let desc = migrating_desc(); // minimum_version is 1
        let err = desc.migrate_value(0, &old_value()).unwrap_err();
        assert!(err.contains("version 0"), "{err}");
        assert!(err.contains("minimum supported version 1"), "{err}");
    }
}
