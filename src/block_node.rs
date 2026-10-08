//! Layered block-node interface for the disk backends.
//!
//! This module introduces a single block-device abstraction — [`BlockNode`] —
//! that sits **beside** the closed [`crate::scsi::DiskBackend`] enum. Nothing in
//! the emulator is wired onto it yet: the call sites migrate in batches
//! (ticket #50) and the old enum is deleted once no caller remains (ticket
//! #51). Behaviour is therefore unchanged by construction.
//!
//! The shape follows QEMU's block graph: every image is a node with an optional
//! `backing` node, and the one query that makes copy-on-write consistent across
//! formats is [`BlockNode::block_status`] — "does this layer hold these bytes,
//! and if not, where do I read them from?". Commit / flatten / dirty-count are
//! intended to become derived operations over this graph (the contract lands in
//! #51), rather than the four bespoke sidecars the enum carries today.
//!
//! Nodes are byte-addressed and operate on byte ranges. The raw and overlay
//! layers use 512-byte sectors internally; reads and writes may be unaligned,
//! and a partial-sector write copies the rest of the sector up through the
//! backing chain first (read-modify-write), exactly as a real overlay must.

use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::chd_disk::ChdHd;

/// Sector granularity shared by the raw and overlay layers. Matches the SCSI
/// disk sector size and [`crate::cow_disk`]'s private constant.
pub const SECTOR_BYTES: usize = 512;
/// [`SECTOR_BYTES`] as a `u64`, for offset arithmetic.
pub const SECTOR_SIZE: u64 = SECTOR_BYTES as u64;

/// What one layer knows about the allocation of a byte range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockState {
    /// The range is allocated in this layer and holds real data.
    Data,
    /// The range is not held by this layer and there is no backing to fall
    /// through to, so it reads as zeroes.
    Zero,
    /// The range is not held by this layer; read it from [`BlockNode::backing`]
    /// at [`BlockStatus::backing_offset`].
    Allocated,
}

/// The answer to a [`BlockNode::block_status`] query.
///
/// A status describes a homogeneous run beginning at the queried offset, so the
/// answer always makes sense even when the requested range straddles an
/// allocation boundary: `len` is the number of bytes from the query offset for
/// which this `state` holds. Callers that need a whole range covered iterate
/// `offset += status.len` until they have consumed it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockStatus {
    /// The allocation state of the run.
    pub state: BlockState,
    /// For [`BlockState::Allocated`], the byte offset to read from in the
    /// backing node. `None` for `Data` and `Zero`.
    pub backing_offset: Option<u64>,
    /// Bytes from the query offset for which `state` holds. Never zero for a
    /// non-empty query.
    pub len: u64,
}

impl BlockStatus {
    /// A run of real data held by this layer.
    pub const fn data(len: u64) -> Self {
        Self { state: BlockState::Data, backing_offset: None, len }
    }

    /// A run that reads as zeroes because no layer holds it.
    pub const fn zero(len: u64) -> Self {
        Self { state: BlockState::Zero, backing_offset: None, len }
    }

    /// A run served by the backing node at `backing_offset`.
    pub const fn allocated(backing_offset: u64, len: u64) -> Self {
        Self { state: BlockState::Allocated, backing_offset: Some(backing_offset), len }
    }
}

/// A byte-addressed node in a layered block graph.
///
/// Implementations are the raw image, the copy-on-write overlay and the CHD, each
/// re-expressing what one arm of [`crate::scsi::DiskBackend`] does today. A node
/// owns its storage; it is `Send` (the SCSI worker already moves its backend
/// across threads) but not `Sync`.
pub trait BlockNode: Send {
    /// Capacity in bytes.
    fn size(&self) -> u64;

    /// Read exactly `buf.len()` bytes starting at byte `offset`.
    fn read(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<()>;

    /// Write exactly `buf.len()` bytes starting at byte `offset`.
    fn write(&mut self, offset: u64, buf: &[u8]) -> io::Result<()>;

    /// Push buffered writes out to durable storage.
    fn flush(&mut self) -> io::Result<()>;

    /// Discard `len` bytes at `offset`, releasing the layer's claim on them.
    /// A later read falls through to the backing node (or zeroes, if none).
    fn discard(&mut self, offset: u64, len: u64) -> io::Result<()>;

    /// Report the allocation state of `[offset, offset + len)`.
    fn block_status(&self, offset: u64, len: u64) -> io::Result<BlockStatus>;

    /// The node that serves ranges this layer does not hold, if any.
    fn backing(&self) -> Option<&dyn BlockNode>;
}

fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, msg.to_string())
}

/// Check that `[offset, offset + len)` lies within a node of `size` bytes.
fn check_range(size: u64, offset: u64, len: u64) -> io::Result<()> {
    let end = offset
        .checked_add(len)
        .ok_or_else(|| invalid("block range overflows u64"))?;
    if end > size {
        return Err(invalid("block range past end of node"));
    }
    Ok(())
}

/// The sector range `[first, last)` covering `[offset, offset + len)`.
fn sectors_for(offset: u64, len: u64) -> (u64, u64) {
    let first = offset / SECTOR_SIZE;
    let end = offset + len;
    let last = (end + SECTOR_SIZE - 1) / SECTOR_SIZE;
    (first, last)
}

/// A plain raw image file: every byte is real data held by this layer.
pub struct RawNode {
    file: File,
    size: u64,
}

impl RawNode {
    /// Open `path` read-write and use its current length as the capacity.
    pub fn open(path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        let size = file.metadata()?.len();
        Ok(Self { file, size })
    }
}

impl BlockNode for RawNode {
    fn size(&self) -> u64 {
        self.size
    }

    fn read(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        check_range(self.size, offset, buf.len() as u64)?;
        self.file.seek(SeekFrom::Start(offset))?;
        self.file.read_exact(buf)
    }

    fn write(&mut self, offset: u64, buf: &[u8]) -> io::Result<()> {
        check_range(self.size, offset, buf.len() as u64)?;
        self.file.seek(SeekFrom::Start(offset))?;
        self.file.write_all(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.sync_all()
    }

    fn discard(&mut self, offset: u64, len: u64) -> io::Result<()> {
        check_range(self.size, offset, len)?;
        // Raw images do not track allocation, so a discard is a zero-fill: the
        // bytes are still "data" to this layer, they just read as zeroes.
        let zeros = vec![0u8; len as usize];
        self.write(offset, &zeros)
    }

    fn block_status(&self, offset: u64, len: u64) -> io::Result<BlockStatus> {
        check_range(self.size, offset, len)?;
        Ok(BlockStatus::data(len))
    }

    fn backing(&self) -> Option<&dyn BlockNode> {
        None
    }
}

/// A copy-on-write overlay: writes are redirected to a sparse overlay file and
/// unheld ranges read through to the backing node.
pub struct CowNode {
    backing: Option<Box<dyn BlockNode>>,
    overlay: File,
    overlay_path: PathBuf,
    dirty: HashSet<u64>,
    size: u64,
}

impl CowNode {
    /// Open (creating if needed) an overlay at `overlay_path` presenting `size`
    /// bytes over `backing`. The overlay starts with no held sectors, so every
    /// range reads through to the backing until first written.
    pub fn new(
        backing: Option<Box<dyn BlockNode>>,
        overlay_path: impl AsRef<Path>,
        size: u64,
    ) -> io::Result<Self> {
        let overlay_path = overlay_path.as_ref().to_path_buf();
        let overlay = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&overlay_path)?;
        Ok(Self { backing, overlay, overlay_path, dirty: HashSet::new(), size })
    }

    /// Number of sectors currently held by the overlay.
    pub fn dirty_count(&self) -> usize {
        self.dirty.len()
    }

    /// Path of the overlay file (for diagnostics and, later, commit/export).
    pub fn overlay_path(&self) -> &Path {
        &self.overlay_path
    }

    /// Read one whole sector: the overlay if it holds it, else the backing, else
    /// zeroes.
    fn read_sector(&mut self, lba: u64) -> io::Result<[u8; SECTOR_BYTES]> {
        let mut buf = [0u8; SECTOR_BYTES];
        if self.dirty.contains(&lba) {
            self.overlay.seek(SeekFrom::Start(lba * SECTOR_SIZE))?;
            self.overlay.read_exact(&mut buf)?;
        } else if let Some(backing) = self.backing.as_mut() {
            backing.read(lba * SECTOR_SIZE, &mut buf)?;
        }
        Ok(buf)
    }

    /// The state of one sector, ignoring the requested range.
    fn sector_state(&self, lba: u64) -> BlockState {
        if self.dirty.contains(&lba) {
            BlockState::Data
        } else if self.backing.is_some() {
            BlockState::Allocated
        } else {
            BlockState::Zero
        }
    }
}

impl BlockNode for CowNode {
    fn size(&self) -> u64 {
        self.size
    }

    fn read(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        check_range(self.size, offset, buf.len() as u64)?;
        if buf.is_empty() {
            return Ok(());
        }
        let end = offset + buf.len() as u64;
        let (first, last) = sectors_for(offset, buf.len() as u64);
        for lba in first..last {
            let sector = self.read_sector(lba)?;
            let s_off = lba * SECTOR_SIZE;
            let start = offset.max(s_off);
            let stop = end.min(s_off + SECTOR_SIZE);
            let src = (start - s_off) as usize;
            let dst = (start - offset) as usize;
            let n = (stop - start) as usize;
            buf[dst..dst + n].copy_from_slice(&sector[src..src + n]);
        }
        Ok(())
    }

    fn write(&mut self, offset: u64, buf: &[u8]) -> io::Result<()> {
        check_range(self.size, offset, buf.len() as u64)?;
        if buf.is_empty() {
            return Ok(());
        }
        let (first, last) = sectors_for(offset, buf.len() as u64);
        for lba in first..last {
            // Copy-on-write does read-modify-write: pull the current sector up
            // through the chain so the untouched bytes survive, then hand the
            // whole sector to the overlay.
            let mut sector = self.read_sector(lba)?;
            let s_off = lba * SECTOR_SIZE;
            let start = offset.max(s_off);
            let stop = (offset + buf.len() as u64).min(s_off + SECTOR_SIZE);
            let src = (start - offset) as usize;
            let dst = (start - s_off) as usize;
            let n = (stop - start) as usize;
            sector[dst..dst + n].copy_from_slice(&buf[src..src + n]);

            self.overlay.seek(SeekFrom::Start(lba * SECTOR_SIZE))?;
            self.overlay.write_all(&sector)?;
            self.dirty.insert(lba);
        }
        Ok(())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.overlay.sync_all()?;
        if let Some(backing) = self.backing.as_mut() {
            backing.flush()?;
        }
        Ok(())
    }

    fn discard(&mut self, offset: u64, len: u64) -> io::Result<()> {
        check_range(self.size, offset, len)?;
        if len == 0 {
            return Ok(());
        }
        let (first, last) = sectors_for(offset, len);
        for lba in first..last {
            self.dirty.remove(&lba);
        }
        Ok(())
    }

    fn block_status(&self, offset: u64, len: u64) -> io::Result<BlockStatus> {
        check_range(self.size, offset, len)?;
        if len == 0 {
            return Ok(BlockStatus::data(0));
        }
        let (first, _) = sectors_for(offset, len);
        let state = self.sector_state(first);
        // Extend the run while the next sector has the same state and still
        // overlaps the requested range.
        let end = offset + len;
        let mut lba = first;
        loop {
            let next = lba + 1;
            if next * SECTOR_SIZE >= end || self.sector_state(next) != state {
                break;
            }
            lba = next;
        }
        let run_end = ((lba + 1) * SECTOR_SIZE).min(end);
        let covered = run_end - offset;
        Ok(match state {
            BlockState::Data => BlockStatus::data(covered),
            BlockState::Zero => BlockStatus::zero(covered),
            BlockState::Allocated => BlockStatus::allocated(offset, covered),
        })
    }

    fn backing(&self) -> Option<&dyn BlockNode> {
        self.backing.as_deref()
    }
}

/// A hard-disk CHD. The underlying [`ChdHd`] already presents the merged
/// parent + diff view, so this layer holds every byte it reports — there is no
/// backing node.
pub struct ChdNode {
    hd: ChdHd,
    sector_size: u64,
}

// `ChdHd` wraps a MAME `chd_file` with a raw pointer and is `!Send`; it is only
// ever owned from the SCSI worker thread, so transferring ownership across
// threads is safe, exactly as `chd_disk` reasons for its own handle.
unsafe impl Send for ChdNode {}

impl ChdNode {
    /// Open a hard-disk CHD. `cow` is the per-disk copy-on-write toggle, threaded
    /// straight through to [`ChdHd::open`].
    pub fn open(path: &str, cow: bool) -> io::Result<Self> {
        let hd = ChdHd::open(path, cow)?;
        let sector_size = u64::from(hd.sector_size());
        Ok(Self { hd, sector_size })
    }
}

impl BlockNode for ChdNode {
    fn size(&self) -> u64 {
        self.hd.size()
    }

    fn read(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        let ss = self.sector_size;
        if offset % ss != 0 || (buf.len() as u64) % ss != 0 {
            return Err(invalid("CHD reads must be sector-aligned"));
        }
        check_range(self.hd.size(), offset, buf.len() as u64)?;
        let data = self.hd.read_blocks(offset / ss, buf.len() / (ss as usize), ss)?;
        buf.copy_from_slice(&data);
        Ok(())
    }

    fn write(&mut self, offset: u64, buf: &[u8]) -> io::Result<()> {
        let ss = self.sector_size;
        if offset % ss != 0 || (buf.len() as u64) % ss != 0 {
            return Err(invalid("CHD writes must be sector-aligned"));
        }
        check_range(self.hd.size(), offset, buf.len() as u64)?;
        self.hd.write_sectors(offset / ss, buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        // CHD sector writes go straight to the image (or its diff); there is no
        // user-space write-back cache to drain.
        Ok(())
    }

    fn discard(&mut self, offset: u64, len: u64) -> io::Result<()> {
        check_range(self.hd.size(), offset, len)?;
        let zeros = vec![0u8; len as usize];
        self.write(offset, &zeros)
    }

    fn block_status(&self, offset: u64, len: u64) -> io::Result<BlockStatus> {
        check_range(self.hd.size(), offset, len)?;
        Ok(BlockStatus::data(len))
    }

    fn backing(&self) -> Option<&dyn BlockNode> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn tmp_path(tag: &str) -> PathBuf {
        static N: AtomicU64 = AtomicU64::new(0);
        std::env::temp_dir().join(format!(
            "iris-blocknode-{}-{}-{}",
            tag,
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn raw_filled(byte: u8, len: usize) -> (RawNode, PathBuf) {
        let path = tmp_path("raw");
        std::fs::write(&path, vec![byte; len]).unwrap();
        (RawNode::open(&path).unwrap(), path)
    }

    #[test]
    fn raw_roundtrips_and_reports_data() {
        let (mut node, path) = raw_filled(0x00, 4 * SECTOR_BYTES);

        node.write(SECTOR_SIZE, &[0x11u8; SECTOR_BYTES]).unwrap();
        let mut got = [0u8; SECTOR_BYTES];
        node.read(SECTOR_SIZE, &mut got).unwrap();
        assert_eq!(got, [0x11u8; SECTOR_BYTES]);

        let status = node.block_status(SECTOR_SIZE, SECTOR_SIZE).unwrap();
        assert_eq!(status.state, BlockState::Data);
        assert_eq!(status.backing_offset, None);
        assert_eq!(status.len, SECTOR_SIZE);
        assert!(node.backing().is_none(), "a raw image has no backing");

        node.discard(SECTOR_SIZE, SECTOR_SIZE).unwrap();
        node.read(SECTOR_SIZE, &mut got).unwrap();
        assert_eq!(got, [0x00u8; SECTOR_BYTES], "discard zero-fills a raw node");

        node.flush().unwrap();
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn raw_rejects_ranges_past_the_end() {
        let (mut node, path) = raw_filled(0, 2 * SECTOR_BYTES);
        let mut buf = [0u8; SECTOR_BYTES];
        assert!(node.read(SECTOR_SIZE, &mut buf).is_ok());
        assert!(node.read(2 * SECTOR_SIZE, &mut buf).is_err());
        assert!(node.block_status(SECTOR_SIZE, SECTOR_SIZE + 1).is_err());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn cow_unwritten_range_reads_through_to_backing() {
        let (backing, backing_path) = raw_filled(0xAA, 4 * SECTOR_BYTES);
        let overlay_path = tmp_path("cow-overlay");
        let mut node =
            CowNode::new(Some(Box::new(backing)), &overlay_path, 4 * SECTOR_SIZE).unwrap();

        assert!(node.backing().is_some(), "the overlay sits on a backing node");
        assert_eq!(node.dirty_count(), 0);

        // An unwritten range reads through to the backing and reports that it is
        // allocated somewhere below this layer.
        let mut got = [0u8; SECTOR_BYTES];
        node.read(SECTOR_SIZE, &mut got).unwrap();
        assert_eq!(got, [0xAAu8; SECTOR_BYTES]);
        let status = node.block_status(SECTOR_SIZE, SECTOR_SIZE).unwrap();
        assert_eq!(status.state, BlockState::Allocated);
        assert_eq!(status.backing_offset, Some(SECTOR_SIZE));
        assert_eq!(status.len, SECTOR_SIZE);

        let _ = std::fs::remove_file(backing_path);
        let _ = std::fs::remove_file(overlay_path);
    }

    #[test]
    fn cow_written_range_reads_overlay_and_reports_data() {
        let (backing, backing_path) = raw_filled(0xAA, 4 * SECTOR_BYTES);
        let overlay_path = tmp_path("cow-overlay");
        let mut node =
            CowNode::new(Some(Box::new(backing)), &overlay_path, 4 * SECTOR_SIZE).unwrap();

        node.write(SECTOR_SIZE, &[0x55u8; SECTOR_BYTES]).unwrap();
        assert_eq!(node.dirty_count(), 1);

        let mut got = [0u8; SECTOR_BYTES];
        node.read(SECTOR_SIZE, &mut got).unwrap();
        assert_eq!(got, [0x55u8; SECTOR_BYTES], "the overlay shadows the backing");
        let status = node.block_status(SECTOR_SIZE, SECTOR_SIZE).unwrap();
        assert_eq!(status.state, BlockState::Data);
        assert_eq!(status.backing_offset, None);

        // A neighbouring range is still served by the backing.
        node.read(2 * SECTOR_SIZE, &mut got).unwrap();
        assert_eq!(got, [0xAAu8; SECTOR_BYTES]);

        let _ = std::fs::remove_file(backing_path);
        let _ = std::fs::remove_file(overlay_path);
    }

    #[test]
    fn cow_discard_releases_the_overlay_range() {
        let (backing, backing_path) = raw_filled(0xAA, 4 * SECTOR_BYTES);
        let overlay_path = tmp_path("cow-overlay");
        let mut node =
            CowNode::new(Some(Box::new(backing)), &overlay_path, 4 * SECTOR_SIZE).unwrap();

        node.write(SECTOR_SIZE, &[0x55u8; SECTOR_BYTES]).unwrap();
        node.discard(SECTOR_SIZE, SECTOR_SIZE).unwrap();
        assert_eq!(node.dirty_count(), 0);

        let mut got = [0u8; SECTOR_BYTES];
        node.read(SECTOR_SIZE, &mut got).unwrap();
        assert_eq!(got, [0xAAu8; SECTOR_BYTES], "discard falls back to the backing");
        let status = node.block_status(SECTOR_SIZE, SECTOR_SIZE).unwrap();
        assert_eq!(status.state, BlockState::Allocated);
        assert_eq!(status.backing_offset, Some(SECTOR_SIZE));

        let _ = std::fs::remove_file(backing_path);
        let _ = std::fs::remove_file(overlay_path);
    }

    #[test]
    fn cow_without_backing_reports_zero_and_zero_fills() {
        let overlay_path = tmp_path("cow-nobacking");
        let mut node = CowNode::new(None, &overlay_path, 2 * SECTOR_SIZE).unwrap();
        assert!(node.backing().is_none());

        let mut got = [0xFFu8; SECTOR_BYTES];
        node.read(0, &mut got).unwrap();
        assert_eq!(got, [0x00u8; SECTOR_BYTES], "no backing and no overlay reads zeroes");
        let status = node.block_status(0, SECTOR_SIZE).unwrap();
        assert_eq!(status.state, BlockState::Zero);
        assert_eq!(status.backing_offset, None);

        node.write(0, &[0x33u8; SECTOR_BYTES]).unwrap();
        assert_eq!(node.block_status(0, SECTOR_SIZE).unwrap().state, BlockState::Data);

        node.discard(0, SECTOR_SIZE).unwrap();
        assert_eq!(node.block_status(0, SECTOR_SIZE).unwrap().state, BlockState::Zero);

        let _ = std::fs::remove_file(overlay_path);
    }

    #[test]
    fn cow_range_straddling_allocated_and_data_reports_first_run() {
        let (backing, backing_path) = raw_filled(0xAA, 4 * SECTOR_BYTES);
        let overlay_path = tmp_path("cow-overlay");
        let mut node =
            CowNode::new(Some(Box::new(backing)), &overlay_path, 4 * SECTOR_SIZE).unwrap();

        // Sector 1 lives in the overlay; sectors 0 and 2 do not.
        node.write(SECTOR_SIZE, &[0x55u8; SECTOR_BYTES]).unwrap();

        let mut got = [0u8; 3 * SECTOR_BYTES];
        node.read(0, &mut got).unwrap();
        assert_eq!(&got[0..SECTOR_BYTES], &[0xAAu8; SECTOR_BYTES]);
        assert_eq!(&got[SECTOR_BYTES..2 * SECTOR_BYTES], &[0x55u8; SECTOR_BYTES]);
        assert_eq!(&got[2 * SECTOR_BYTES..], &[0xAAu8; SECTOR_BYTES]);

        let first = node.block_status(0, 3 * SECTOR_SIZE).unwrap();
        assert_eq!(first.state, BlockState::Allocated);
        assert_eq!(first.len, SECTOR_SIZE, "the run stops at the overlay boundary");

        // Stepping by `status.len` reaches the held sector.
        let second = node.block_status(first.len, 2 * SECTOR_SIZE).unwrap();
        assert_eq!(second.state, BlockState::Data);
        assert_eq!(second.len, SECTOR_SIZE);

        let _ = std::fs::remove_file(backing_path);
        let _ = std::fs::remove_file(overlay_path);
    }

    #[test]
    fn cow_partial_sector_write_preserves_neighbours() {
        let (backing, backing_path) = raw_filled(0xAA, 2 * SECTOR_BYTES);
        let overlay_path = tmp_path("cow-overlay");
        let mut node =
            CowNode::new(Some(Box::new(backing)), &overlay_path, 2 * SECTOR_SIZE).unwrap();

        // Overwrite 100 bytes in the middle of sector 0.
        node.write(100, &[0x11u8; 100]).unwrap();
        let mut got = [0u8; SECTOR_BYTES];
        node.read(0, &mut got).unwrap();
        assert_eq!(&got[0..100], &[0xAAu8; 100]);
        assert_eq!(&got[100..200], &[0x11u8; 100]);
        assert_eq!(&got[200..], &[0xAAu8; SECTOR_BYTES - 200]);

        let _ = std::fs::remove_file(backing_path);
        let _ = std::fs::remove_file(overlay_path);
    }

    #[test]
    fn chd_node_reads_writes_and_reports_data() {
        use libchdman_rs::hd::{create_from_reader, HdCreateOptions};
        use libchdman_rs::CHD_CODEC_ZLIB;
        use std::io::Cursor;

        let base = tmp_path("chd").with_extension("chd");
        // MAME's v5 compression map asserts on a logical size smaller than one
        // hunk, so use the same 256 KiB the chd_disk tests use.
        let logical = 256 * 1024u64;
        create_from_reader(
            Cursor::new(vec![0xABu8; logical as usize]),
            &base,
            HdCreateOptions {
                logical_size: logical,
                hunk_size: 4096,
                unit_size: SECTOR_BYTES as u32,
                codecs: [CHD_CODEC_ZLIB, 0, 0, 0],
                geometry: None,
                ident: None,
            },
            &mut |_| {},
            &|| false,
        )
        .unwrap();

        let mut node = ChdNode::open(base.to_str().unwrap(), false).unwrap();
        assert_eq!(node.size(), logical);
        assert!(node.backing().is_none(), "a CHD presents the merged view itself");

        let mut got = [0u8; SECTOR_BYTES];
        node.read(SECTOR_SIZE, &mut got).unwrap();
        assert_eq!(got, [0xABu8; SECTOR_BYTES]);
        assert_eq!(
            node.block_status(SECTOR_SIZE, SECTOR_SIZE).unwrap().state,
            BlockState::Data
        );

        node.write(SECTOR_SIZE, &[0x5Au8; SECTOR_BYTES]).unwrap();
        node.read(SECTOR_SIZE, &mut got).unwrap();
        assert_eq!(got, [0x5Au8; SECTOR_BYTES]);
        node.flush().unwrap();

        drop(node);
        let _ = std::fs::remove_file(&base);
        let _ = std::fs::remove_file(crate::chd_disk::diff_path_for(&base));
    }
}
