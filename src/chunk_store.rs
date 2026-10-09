//! Phase 3.1: content-addressable chunk store for snapshot RAM.
//!
//! Each snapshot's RAM banks are split into 64 KB chunks, BLAKE3-hashed,
//! and stored as `saves/.cas/<hex2>/<hex62>` (sharded by the first byte to
//! keep any one directory under a few thousand files). Snapshots reference
//! chunks by hash; identical chunks across snapshots share storage. A
//! workflow that snapshots between every install shares 95–99% of RAM with
//! its parent, so adding a new snapshot costs only the bytes that actually
//! changed.
//!
//! Layout:
//! ```text
//!   saves/.cas/
//!     ab/
//!       cd1234...beef.chunk      ← BLAKE3 hash, hex64, raw 64KB content
//!     cd/
//!       ef9876...cafe.chunk
//! ```
//!
//! On-disk chunks are immutable (CAS). `gc(live_set)` deletes any chunk
//! whose hash isn't referenced by a kept snapshot's manifest — cheap to run
//! and the only way to actually free space (since `delete <name>` only
//! removes the manifest, not the underlying chunks).

use std::collections::HashSet;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Chunk size in bytes. 64 KB is the plan-cited sweet spot — small enough
/// that a few-page write to RAM only dirties one chunk, large enough that
/// per-chunk hashing + filesystem overhead doesn't dominate.
pub const CHUNK_SIZE: usize = 64 * 1024;

const CAS_DIR: &str = ".cas";
const CHUNK_EXT: &str = "chunk";

/// 32-byte BLAKE3 digest.
pub type ChunkHash = [u8; 32];

/// Bit-per-chunk dirty map at [`CHUNK_SIZE`] granularity, covering one RAM bank
/// (`chunks = ceil(bytes / CHUNK_SIZE)`).
///
/// A set bit means the chunk *may* differ from the last save; a clear bit means
/// it is byte-identical to the last save. `save_snapshot` uses this to skip
/// re-hashing and re-storing unchanged chunks on an incremental save.
///
/// Bits are atomic so the CPU thread, the MC-DMA worker and device threads can
/// all mark a write without a lock. [`mark`](Self::mark) tests before it
/// read-modify-writes, so once a chunk is dirty the common case is a plain load
/// and a well-predicted branch rather than a locked OR.
pub struct ChunkDirtyBitmap {
    bits: Box<[AtomicU64]>,
    chunks: usize,
}

impl ChunkDirtyBitmap {
    /// A bitmap over `size_bytes`, with every chunk marked dirty (the state a
    /// fresh bank is in: it has never been saved).
    pub fn new(size_bytes: usize) -> Self {
        let chunks = size_bytes.div_ceil(CHUNK_SIZE);
        let words = chunks.div_ceil(64).max(1);
        let bits: Box<[AtomicU64]> =
            (0..words).map(|_| AtomicU64::new(u64::MAX)).collect();
        // Mask off the tail bits past `chunks` so `flags()` and `clear()` agree.
        if chunks % 64 != 0 {
            let last = words - 1;
            let keep = (1u64 << (chunks % 64)) - 1;
            bits[last].store(keep, Ordering::Relaxed);
        }
        Self { bits, chunks }
    }

    pub fn chunks(&self) -> usize {
        self.chunks
    }

    /// Mark the chunk containing byte offset `off` dirty.
    #[inline(always)]
    pub fn mark(&self, off: usize) {
        let chunk = off / CHUNK_SIZE;
        if chunk >= self.chunks {
            return;
        }
        let word = &self.bits[chunk >> 6];
        let bit = 1u64 << (chunk & 63);
        // Test first: after the first write to a chunk this is a load + branch,
        // not a locked read-modify-write, on every subsequent store.
        if word.load(Ordering::Relaxed) & bit == 0 {
            word.fetch_or(bit, Ordering::Relaxed);
        }
    }

    /// Mark every chunk touching `[off, off+len)` dirty.
    #[inline]
    pub fn mark_range(&self, off: usize, len: usize) {
        if len == 0 {
            return;
        }
        let first = (off / CHUNK_SIZE).min(self.chunks);
        let last = ((off + len - 1) / CHUNK_SIZE).min(self.chunks.saturating_sub(1));
        for chunk in first..=last {
            let word = &self.bits[chunk >> 6];
            let bit = 1u64 << (chunk & 63);
            if word.load(Ordering::Relaxed) & bit == 0 {
                word.fetch_or(bit, Ordering::Relaxed);
            }
        }
    }

    /// Mark every chunk dirty (whole-bank mutation: restore, load, power-on).
    pub fn mark_all(&self) {
        let mut chunk = 0usize;
        while chunk < self.chunks {
            let word_idx = chunk >> 6;
            let bits_in_word = (self.chunks - chunk).min(64);
            let mask = if bits_in_word == 64 { u64::MAX } else { (1u64 << bits_in_word) - 1 };
            self.bits[word_idx].fetch_or(mask, Ordering::Relaxed);
            chunk += 64;
        }
    }

    pub fn is_dirty(&self, chunk: usize) -> bool {
        if chunk >= self.chunks {
            return false;
        }
        self.bits[chunk >> 6].load(Ordering::Relaxed) & (1u64 << (chunk & 63)) != 0
    }

    /// Snapshot the bitmap as one `bool` per chunk, in bank offset order.
    pub fn flags(&self) -> Vec<bool> {
        (0..self.chunks).map(|c| self.is_dirty(c)).collect()
    }

    /// Clear every bit, arming the bitmap for the next save epoch.
    pub fn clear(&self) {
        for word in self.bits.iter() {
            word.store(0, Ordering::Relaxed);
        }
    }
}

pub struct ChunkStore {
    root: PathBuf,
}

impl ChunkStore {
    /// `saves_dir` is e.g. `Path::new("saves")`. The chunk store lives at
    /// `saves_dir/.cas/`.
    pub fn new(saves_dir: impl AsRef<Path>) -> Self {
        Self { root: saves_dir.as_ref().join(CAS_DIR) }
    }

    pub fn root(&self) -> &Path { &self.root }

    /// Hash `data`, write it as `saves/.cas/<hex2>/<hex62>.chunk` if absent,
    /// return the hash. Idempotent — concurrent saves of the same chunk are
    /// safe; the second call is a no-op.
    ///
    /// Crash-safety: chunks are written to a `.tmp` sibling then renamed
    /// (atomic on POSIX), so a partial write never appears under the final
    /// content-addressed name. We deliberately skip per-chunk `fsync` —
    /// 4096 fsyncs per snapshot was costing ~20 s on APFS for the first
    /// save of a 256 MB image. If the process dies mid-save the manifest
    /// (`chunks.bin`) hasn't been written yet, so any complete chunks are
    /// just orphaned bytes that `gc` will sweep later.
    pub fn put(&self, data: &[u8]) -> io::Result<ChunkHash> {
        let hash: ChunkHash = blake3::hash(data).into();
        let path = self.path_for(&hash);
        if path.exists() {
            return Ok(hash);
        }
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("chunk.tmp");
        {
            let mut f = fs::File::create(&tmp)?;
            f.write_all(data)?;
        }
        // Rename is atomic on POSIX. If two threads raced, the loser's
        // rename overwrites the winner's identical content — fine.
        fs::rename(&tmp, &path)?;
        Ok(hash)
    }

    pub fn get(&self, hash: &ChunkHash) -> io::Result<Vec<u8>> {
        let path = self.path_for(hash);
        let mut f = fs::File::open(&path)?;
        let mut data = Vec::with_capacity(CHUNK_SIZE);
        f.read_to_end(&mut data)?;
        Ok(data)
    }

    pub fn has(&self, hash: &ChunkHash) -> bool {
        self.path_for(hash).exists()
    }

    /// Are all `hashes` still present? Lets an incremental save skip the RAM
    /// clone for a bank whose chunks are all clean *and* still on disk after a
    /// possible `gc`.
    pub fn all_present(&self, hashes: &[ChunkHash]) -> bool {
        hashes.iter().all(|h| self.has(h))
    }

    /// Remove any chunk whose hash isn't in `live`. Returns (removed_count,
    /// removed_bytes). Safe to interrupt — chunks not yet visited stay.
    pub fn gc(&self, live: &HashSet<ChunkHash>) -> io::Result<(usize, u64)> {
        if !self.root.is_dir() {
            return Ok((0, 0));
        }
        let mut removed = 0usize;
        let mut bytes_removed = 0u64;
        for shard in fs::read_dir(&self.root)? {
            let shard = shard?;
            if !shard.file_type()?.is_dir() { continue; }
            for chunk in fs::read_dir(shard.path())? {
                let chunk = chunk?;
                let path = chunk.path();
                let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else { continue };
                let Some(hash) = parse_hex62(stem, &shard.file_name().to_string_lossy()) else { continue };
                if !live.contains(&hash) {
                    let size = chunk.metadata().map(|m| m.len()).unwrap_or(0);
                    if fs::remove_file(&path).is_ok() {
                        removed += 1;
                        bytes_removed += size;
                    }
                }
            }
        }
        Ok((removed, bytes_removed))
    }

    /// Total bytes occupied by the chunk store. Useful for `info` reporting.
    pub fn total_size(&self) -> io::Result<u64> {
        if !self.root.is_dir() { return Ok(0); }
        let mut total = 0u64;
        for shard in fs::read_dir(&self.root)? {
            let shard = shard?;
            if !shard.file_type()?.is_dir() { continue; }
            for chunk in fs::read_dir(shard.path())? {
                let chunk = chunk?;
                total += chunk.metadata().map(|m| m.len()).unwrap_or(0);
            }
        }
        Ok(total)
    }

    pub fn path_for(&self, hash: &ChunkHash) -> PathBuf {
        let hex = hex_encode(hash);
        // Shard by first byte: saves/.cas/ab/cd1234...beef.chunk
        let (head, tail) = hex.split_at(2);
        self.root.join(head).join(format!("{}.{}", tail, CHUNK_EXT))
    }
}

fn hex_encode(bytes: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(64);
    for &b in bytes.iter() {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0x0f) as usize] as char);
    }
    s
}

fn parse_hex62(tail: &str, head: &str) -> Option<ChunkHash> {
    if tail.len() != 62 || head.len() != 2 { return None; }
    let mut out = [0u8; 32];
    let mut full = String::with_capacity(64);
    full.push_str(head);
    full.push_str(tail);
    let bytes = full.as_bytes();
    for i in 0..32 {
        out[i] = (hex_nibble(bytes[i * 2])? << 4) | hex_nibble(bytes[i * 2 + 1])?;
    }
    Some(out)
}

fn hex_nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(10 + c - b'a'),
        b'A'..=b'F' => Some(10 + c - b'A'),
        _ => None,
    }
}

/// Walk `words` (host-endian u32) as big-endian byte chunks of `CHUNK_SIZE`,
/// store each chunk via `store.put`, and collect the hashes in order. The
/// final chunk may be smaller if `words.len() * 4` isn't a multiple of
/// `CHUNK_SIZE`. Returns the per-chunk hash list — concat'ing those chunks
/// in order reproduces the bank's BE byte stream exactly.
pub fn put_words_as_chunks(
    store: &ChunkStore,
    words: &[u32],
) -> io::Result<Vec<ChunkHash>> {
    Ok(put_words_as_chunks_selective(store, words, None, &[])?.0)
}

/// Incremental form of [`put_words_as_chunks`].
///
/// `dirty[c]` says whether chunk `c` may have changed since the last save; a
/// clean chunk whose previous hash is present in `prev` is *reused* rather
/// than re-hashed and re-put — this is what makes an incremental save cheap.
/// `prev` is only trusted when it is exactly as long as the current chunk
/// count, so a bank that resized (or a first save) transparently re-hashes.
///
/// A reused hash is only taken if its chunk is still in the store: a `gc`
/// between saves can drop chunks no kept snapshot references, and reusing a
/// hash whose chunk is gone would write a manifest that cannot be loaded.
///
/// Returns `(hashes, chunks_hashed)` — the second element is the
/// instrumentation the incremental-save test counts against.
pub fn put_words_as_chunks_selective(
    store: &ChunkStore,
    words: &[u32],
    prev: Option<&[ChunkHash]>,
    dirty: &[bool],
) -> io::Result<(Vec<ChunkHash>, usize)> {
    let bytes_total = words.len() * 4;
    let chunk_words = CHUNK_SIZE / 4;
    let n_chunks = bytes_total.div_ceil(CHUNK_SIZE);
    let prev = prev.filter(|p| p.len() == n_chunks);
    let dirty_ok = dirty.len() == n_chunks;

    let mut hashes = Vec::with_capacity(n_chunks);
    let mut buf = vec![0u8; CHUNK_SIZE];
    let mut hashed = 0usize;
    let mut i = 0usize;
    for c in 0..n_chunks {
        let take = (words.len() - i).min(chunk_words);
        if let Some(prev) = prev {
            if dirty_ok && !dirty[c] && store.has(&prev[c]) {
                hashes.push(prev[c]);
                i += take;
                continue;
            }
        }
        let bytes_this_chunk = take * 4;
        for (k, &w) in words[i..i + take].iter().enumerate() {
            buf[k * 4..k * 4 + 4].copy_from_slice(&w.to_be_bytes());
        }
        hashes.push(store.put(&buf[..bytes_this_chunk])?);
        hashed += 1;
        i += take;
    }
    Ok((hashes, hashed))
}

/// Inverse of `put_words_as_chunks`. Given a hash list, fetch each chunk
/// and decode BE bytes back into a `Vec<u32>`. Caller is responsible for
/// cross-checking the resulting length against the bank's expected size.
pub fn get_chunks_as_words(
    store: &ChunkStore,
    hashes: &[ChunkHash],
) -> io::Result<Vec<u32>> {
    let mut words = Vec::with_capacity(hashes.len() * (CHUNK_SIZE / 4));
    for h in hashes {
        let bytes = store.get(h)?;
        if bytes.len() % 4 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("chunk size {} not a multiple of 4", bytes.len()),
            ));
        }
        for chunk in bytes.chunks_exact(4) {
            words.push(u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
        }
    }
    Ok(words)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_tmp_dir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let p = std::env::temp_dir().join(format!("iris-cas-{}-{}", tag, nanos));
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn put_get_round_trip() {
        let dir = unique_tmp_dir("rt");
        let store = ChunkStore::new(&dir);
        let data = b"hello world chunk content here";
        let h = store.put(data).unwrap();
        assert!(store.has(&h));
        assert_eq!(store.get(&h).unwrap(), data);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn put_dedupes_identical_content() {
        let dir = unique_tmp_dir("dedupe");
        let store = ChunkStore::new(&dir);
        let data = vec![0xAB; 1024];
        let h1 = store.put(&data).unwrap();
        let h2 = store.put(&data).unwrap();
        assert_eq!(h1, h2);
        // Only one file on disk.
        let mut count = 0;
        for shard in fs::read_dir(&dir.join(".cas")).unwrap() {
            for _ in fs::read_dir(shard.unwrap().path()).unwrap() {
                count += 1;
            }
        }
        assert_eq!(count, 1, "duplicate put should not write twice");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn put_get_words_round_trip() {
        let dir = unique_tmp_dir("words");
        let store = ChunkStore::new(&dir);
        // 33 KB worth of words — exercises the partial-final-chunk path.
        let words: Vec<u32> = (0..33 * 256).map(|i| 0x80000000_u32 ^ (i as u32)).collect();
        let hashes = put_words_as_chunks(&store, &words).unwrap();
        let got = get_chunks_as_words(&store, &hashes).unwrap();
        assert_eq!(got, words);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn put_words_two_banks_share_zero_chunks() {
        // Two all-zero banks should produce the same hashes — same chunk
        // stored once, both bank manifests reference it.
        let dir = unique_tmp_dir("zero");
        let store = ChunkStore::new(&dir);
        let words_a = vec![0u32; CHUNK_SIZE / 4];
        let words_b = vec![0u32; CHUNK_SIZE / 4];
        let h_a = put_words_as_chunks(&store, &words_a).unwrap();
        let h_b = put_words_as_chunks(&store, &words_b).unwrap();
        assert_eq!(h_a, h_b);
        // One physical chunk file.
        let mut count = 0;
        for shard in fs::read_dir(&dir.join(".cas")).unwrap() {
            for _ in fs::read_dir(shard.unwrap().path()).unwrap() {
                count += 1;
            }
        }
        assert_eq!(count, 1, "two zero banks must dedupe to a single chunk");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn gc_removes_unreferenced() {
        let dir = unique_tmp_dir("gc");
        let store = ChunkStore::new(&dir);
        let h_keep = store.put(b"keep me").unwrap();
        let _h_drop = store.put(b"drop me").unwrap();
        let mut live = HashSet::new();
        live.insert(h_keep);
        let (removed, _bytes) = store.gc(&live).unwrap();
        assert_eq!(removed, 1);
        assert!(store.has(&h_keep));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn dirty_bitmap_marks_clears_and_snapshots() {
        // 3.5 chunks -> 4 chunks, exercising the partial final word.
        let bm = ChunkDirtyBitmap::new((CHUNK_SIZE * 3) + CHUNK_SIZE / 2);
        assert_eq!(bm.chunks(), 4);
        assert!(bm.flags().iter().all(|&d| d), "a fresh bank is all-dirty");

        bm.clear();
        assert!(bm.flags().iter().all(|&d| !d));
        bm.mark(CHUNK_SIZE + 5);
        bm.mark_range(CHUNK_SIZE * 2, 10);
        assert_eq!(bm.flags(), vec![false, true, true, false]);

        bm.mark_all();
        assert!(bm.flags().iter().all(|&d| d));

        // Out-of-range marks are inert rather than panicking.
        bm.clear();
        bm.mark(CHUNK_SIZE * 99);
        bm.mark_range(CHUNK_SIZE * 10, 1);
        assert!(bm.flags().iter().all(|&d| !d));
    }

    /// The headline for #48: a second selective save over a bank where exactly
    /// one chunk changed must hash exactly one chunk, reproduce the same hash
    /// list as a full save, and load back to the identical bytes.
    #[test]
    fn selective_save_hashes_only_dirty_chunks_and_loads_identically() {
        let dir = unique_tmp_dir("selective");
        let store = ChunkStore::new(&dir);
        // Three chunks (192 KB) of words.
        let words: Vec<u32> = (0..3 * (CHUNK_SIZE / 4))
            .map(|i| 0x4000_0000u32 ^ (i as u32))
            .collect();

        // First save: no previous manifest, so every chunk is hashed.
        let (first, hashed_all) =
            put_words_as_chunks_selective(&store, &words, None, &[]).unwrap();
        assert_eq!(hashed_all, 3);
        let total_first = store.total_size().unwrap();

        // Touch one word in the middle chunk only.
        let mut words2 = words.clone();
        words2[CHUNK_SIZE / 4 + 7] ^= 0xDEAD_BEEF;

        let dirty = [false, true, false];
        let (second, hashed) =
            put_words_as_chunks_selective(&store, &words2, Some(&first), &dirty).unwrap();
        assert_eq!(hashed, 1, "only the dirty chunk is re-hashed");
        assert_eq!(second[0], first[0], "clean chunk reuses its hash");
        assert_eq!(second[2], first[2], "clean chunk reuses its hash");
        assert_ne!(second[1], first[1], "dirty chunk gets a fresh hash");

        // A full save produces the same hash list the incremental one did.
        let (full, _) = put_words_as_chunks_selective(&store, &words2, None, &[]).unwrap();
        assert_eq!(second, full, "incremental manifest matches a full re-hash");

        // It loads back to the identical bytes.
        let loaded = get_chunks_as_words(&store, &second).unwrap();
        assert_eq!(loaded, words2);
        // Only the one changed chunk was added to the store.
        assert_eq!(
            store.total_size().unwrap(),
            total_first + CHUNK_SIZE as u64,
            "exactly one new chunk written"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// A `gc` between saves can drop a chunk a clean flag would otherwise
    /// reuse; the selective writer must notice and re-hash it rather than
    /// emit a manifest pointing at missing bytes.
    #[test]
    fn selective_save_rehashes_a_chunk_gc_removed() {
        let dir = unique_tmp_dir("selective-gc");
        let store = ChunkStore::new(&dir);
        let words: Vec<u32> = (0..2 * (CHUNK_SIZE / 4))
            .map(|i| 0xA000_0000u32 ^ (i as u32))
            .collect();
        let (first, _) = put_words_as_chunks_selective(&store, &words, None, &[]).unwrap();

        // Simulate gc sweeping the middle chunk.
        fs::remove_file(store.path_for(&first[1])).unwrap();
        assert!(!store.has(&first[1]));

        let (second, hashed) =
            put_words_as_chunks_selective(&store, &words, Some(&first), &[false, false]).unwrap();
        assert_eq!(hashed, 1, "the missing chunk is re-hashed despite being clean");
        assert_eq!(second, first, "re-hash reproduces the same manifest");
        assert_eq!(get_chunks_as_words(&store, &second).unwrap(), words);
        let _ = fs::remove_dir_all(&dir);
    }
}
