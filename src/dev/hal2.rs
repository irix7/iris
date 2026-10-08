use std::sync::Arc;
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use crate::config::{AudioBackendKind, AudioConfig, ResamplerKind};
use crate::devlog::LogModule;
use std::time::{Duration, Instant};
use std::io::Write;
use crate::traits::{BusRead8, BusRead16, BusRead32, BusRead64, BUS_OK, BUS_ERR, Device, DmaClient, Saveable};
use crate::snapshot::{get_field, hex_u16, u16_slice_to_toml, load_u16_slice,
                      u32_slice_to_toml, load_u32_slice, toml_u16};
use crate::hptimer::{TimerManager, TimerId, TimerReturn};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rtrb::{RingBuffer, Producer};

// HAL2 Register Offsets (relative to 0x1FBD8000)
pub const HAL2_ISR: u32 = 0x10; // Interrupt Status Register
pub const HAL2_REV: u32 = 0x20; // Revision

pub const HAL2_IAR: u32 = 0x30; // Indirect Address Register
pub const HAL2_IDR0: u32 = 0x40; // Indirect Data Register 0
pub const HAL2_IDR1: u32 = 0x50; // Indirect Data Register 1
pub const HAL2_IDR2: u32 = 0x60; // Indirect Data Register 2
pub const HAL2_IDR3: u32 = 0x70; // Indirect Data Register 3

pub mod isr {
    pub const TSTATUS: u8       = 0x01; // r  transaction busy
    pub const USTATUS: u8       = 0x02; // r  utime armed
    pub const CODEC_MODE: u8    = 0x04; // rw 0=indigo, 1=quad
    pub const GLOBAL_RESET_N: u8 = 0x08; // rw 0=reset entire chip
    pub const CODEC_RESET_N: u8  = 0x10; // rw 0=reset codec/synth only
}

pub mod iar {
    pub const WR: u8 = 0x80;
    pub const RD: u8 = 0x40;
    pub const ADDR_MASK: u8 = 0x0F;
}

// Indirect registers
pub const HAL2_I_STATUS: u8 = 0x00;
pub const HAL2_I_CONTROL: u8 = 0x01;
pub const HAL2_I_PBUS_CH1: u8 = 0x02;
pub const HAL2_I_PBUS_CH2: u8 = 0x03;
pub const HAL2_I_PBUS_CH3: u8 = 0x04;
pub const HAL2_I_PBUS_CH4: u8 = 0x05;
pub const HAL2_I_DMA_END_CH1: u8 = 0x06;
pub const HAL2_I_DMA_END_CH2: u8 = 0x07;
pub const HAL2_I_DMA_END_CH3: u8 = 0x08;
pub const HAL2_I_DMA_END_CH4: u8 = 0x09;
pub const HAL2_I_DMA_DRV_CH1: u8 = 0x0A;
pub const HAL2_I_DMA_DRV_CH2: u8 = 0x0B;
pub const HAL2_I_DMA_DRV_CH3: u8 = 0x0C;
pub const HAL2_I_DMA_DRV_CH4: u8 = 0x0D;
pub const HAL2_I_AES_RX: u8 = 0x0E;
pub const HAL2_I_AES_TX: u8 = 0x0F;

pub mod control {
    pub const DEC_RESET_N: u16 = 0x0001;
    pub const INC_RESET_N: u16 = 0x0002;
    pub const SYN_RESET_N: u16 = 0x0004;
    pub const AES_RX_RESET_N: u16 = 0x0008;
    pub const AES_TX_RESET_N: u16 = 0x0010;
    pub const DAC_RESET_N: u16 = 0x0020;
    pub const ADC_RESET_N: u16 = 0x0040;
    pub const UTO_RESET_N: u16 = 0x0080;
}

// IAR field masks
// Bit 7 (0x0080) selects read vs write; NOT bit 15.
// type = bits 15:12, num = bits 11:8, access_sel = bit 7, param = bits 3:2
const IAR_ACCESS_READ: u16  = 0x0080;   // bit 7: 1=read, 0=write
const IAR_TYPE_MASK: u16    = 0xF000;
const IAR_NUM_MASK: u16     = 0x0F00;
const IAR_PARAM_MASK: u16   = 0x000C;   // bits 3:2

// IAR type field values (bits 15:12)
const IAR_TYPE_DMA: u16        = 0x1000; // codec / AES / synth DMA control
const IAR_TYPE_BRES: u16       = 0x2000; // Bresenham clock generators
const IAR_TYPE_GLOBAL_DMA: u16 = 0x9000; // global DMA enable/drive/endian/relay

// IAR num field values for IAR_TYPE_DMA (bits 11:8)
const IAR_NUM_AES_RX: u16  = 0x0200;
const IAR_NUM_AES_TX: u16  = 0x0300;
const IAR_NUM_CODECA: u16  = 0x0400;
const IAR_NUM_CODECB: u16  = 0x0500;

// IAR param values (from bits 3:2 of the IAR word)
// param 0 = relay/endian/special, param 1 = ctrl1, param 2 = ctrl2, param 3 = drive
const IAR_PARAM_0: u16 = 0x00;
const IAR_PARAM_1: u16 = 0x04;   // HAL2_*_CTRL1_W = ...04
const IAR_PARAM_2: u16 = 0x08;   // HAL2_*_CTRL2_W = ...08
const IAR_PARAM_3: u16 = 0x0C;

// DMA enable register bits (HAL2_DMA_ENABLE_W)
const DMA_EN_AES_RX: u16 = 0x02;
const DMA_EN_AES_TX: u16 = 0x04;
const DMA_EN_CODECA: u16 = 0x08;
const DMA_EN_CODECB: u16 = 0x10;

// Codec CTRL1 bitfield positions
const CTRL1_CHAN_MASK:  u16 = 0x0007; // bits 2:0 – HPC3 DMA channel
const CTRL1_CLOCK_SHIFT: u32 = 3;    // bits 4:3 – CLKID: the BRES generator
                                     // NUMBER, 1..3 (0 = none). Not an index.
const CTRL1_CLOCK_MASK:  u16 = 0x0003;
const CTRL1_MODE_SHIFT:  u32 = 8;    // bits 9:8 – channel mode
const CTRL1_MODE_MASK:   u16 = 0x0003;

// Channel mode values (CTRL1 bits 9:8)
const MODE_MONO:   usize = 1;
const MODE_STEREO: usize = 2;
const MODE_QUAD:   usize = 3;

// Default pre-buffer: accumulate this many ms of audio samples before pushing to
// the ring (overridable via `[audio] prebuf_ms`; `AudioConfig::default` uses the
// same value). This gives the CPU time to fill its circular DMA buffer before we
// start draining it, preventing initial underrun.
const PREBUF_MS: u64 = 20;
// Ring buffer capacity as a multiple of the pre-buffer.  Must absorb OS scheduling
// jitter.  Expressed as a multiplier of pre-buffer stereo samples.
const RING_BUF_MULTIPLIER: usize = 16;

// Consecutive dry reads before giving up prebuf and opening stream anyway.
const DRY_LIMIT: u32 = 100;

/// How often a running channel wakes to move samples. A timer per sample
/// (every 21-91 us) cannot keep real time on a host thread: every late or
/// coalesced tick consumed guest audio more slowly than the codec rate, which
/// played as stretched, ever more delayed sound. Instead each wake moves the
/// number of frames the wall clock says are due.
///
/// It must be well under a millisecond. IRIX refills its 202-frame playback
/// ring from a 1 kHz callback that reads the DMA position each time; at 2 ms
/// the position stood still on every other read, and IRIX let the DMA lap
/// the ring: once a lap it wrote only the lap's remainder, and the rest
/// replayed stale audio (a 199-frame jump back every 20 ms at 11025 Hz, 6 ms
/// at 44.1 kHz -- slow, robotic sound).
const PACE_PERIOD: Duration = Duration::from_micros(250);
/// After a host stall, frames more than this far behind are skipped rather
/// than moved in one burst (which would only add latency).
const PACE_MAX_BACKLOG: Duration = Duration::from_millis(100);
/// How much faster than real time a channel may catch up after a late wake
/// (5/4): see `Pacer::due`.
const PACE_CATCHUP_NUM: u64 = 5;
const PACE_CATCHUP_DEN: u64 = 4;

/// Wall-clock frame pacing for one channel: how many frames are due now.
///
/// Codec B and the AES engines have no host consumer, so a straight wall-clock
/// generator is correct for them. Codec A uses `RateCtl` below, which closes
/// the loop on what the host callback actually consumed.
struct Pacer {
    start: Instant,
    rate: u64,
    done: u64,
}

impl Pacer {
    fn new(rate: u32) -> Self {
        Self { start: Instant::now(), rate: rate as u64, done: 0 }
    }

    /// Frames to move now, counting them as done: what the wall clock says is
    /// due, but never much more than one period's worth.
    ///
    /// The DMA position must move smoothly, as on the hardware. IRIX writes
    /// its playback ring only a few milliseconds ahead of the position it
    /// polls; when a late wake (a busy host) moved everything due at once,
    /// the position leapt past what IRIX had written, the channel read the
    /// previous lap's samples, and sound under load (GLQuake) repeated in
    /// 18 ms chunks. A late wake now moves at most PACE_CATCHUP times a
    /// period's frames and the backlog drains over the next wakes.
    fn due(&mut self) -> u64 {
        let elapsed = self.start.elapsed();
        let target = (elapsed.as_nanos() as u64).saturating_mul(self.rate) / 1_000_000_000;
        let backlog = target.saturating_sub(self.done);
        let max_backlog = self.rate * PACE_MAX_BACKLOG.as_millis() as u64 / 1000;
        if backlog > max_backlog {
            // Skip the stall rather than replay it.
            self.done = target - max_backlog;
        }
        let per_period = (self.rate * PACE_PERIOD.as_micros() as u64).div_ceil(1_000_000);
        let cap = (per_period * PACE_CATCHUP_NUM).div_ceil(PACE_CATCHUP_DEN).max(per_period + 1);
        let due = target.saturating_sub(self.done).min(cap);
        self.done += due;
        due
    }

    /// Frames `due` handed out but not moved after all: still due.
    fn give_back(&mut self, frames: u64) {
        self.done -= frames;
    }
}

/// Per-wake catch-up cap reused by `RateCtl`: never move more than
/// `PACE_CATCHUP` periods' frames in one wake, as the open-loop pacer did.
fn pace_wake_cap(rate: u64) -> u64 {
    let per_period = (rate * PACE_PERIOD.as_micros() as u64).div_ceil(1_000_000);
    (per_period * PACE_CATCHUP_NUM).div_ceil(PACE_CATCHUP_DEN).max(per_period + 1)
}

/// Drift-control window for Codec A, in frames: how far the host callback may
/// run ahead of or behind the guest clock before the loop re-anchors the
/// measurement. Reuses `PACE_MAX_BACKLOG` — the same host stall the open-loop
/// pacer skips — so a device change or a long stall re-locks.
fn rate_reset_window(rate: u64) -> i64 {
    (rate * PACE_MAX_BACKLOG.as_millis() as u64 / 1000) as i64
}

/// Closed-loop frame budget for Codec A (QEMU `RateCtl` shape).
///
/// The guest's BRES clock says how many frames are *due* on the wall clock; the
/// host output reports how many the callback has actually *consumed*. The
/// difference is the drift. The per-wake budget is the wall-clock due biased by
/// the measured drift — equivalently, what the host has consumed plus the ring
/// lead we want to keep ahead of it, minus what we have already produced — so
/// production follows the rate the host is really playing at instead of a host
/// crystal slowly accumulating against the guest's fixed `Instant` pace. When
/// the drift since the window opened leaves ±`rate_reset_window`, the window is
/// re-anchored exactly as QEMU's `audio_rate_add_bytes` resets, so a device
/// change or a long stall re-locks. Unlike QEMU the produced/consumed target is
/// cumulative, so re-anchoring neither replays nor drops a backlog and does not
/// re-prime the ring.
struct RateCtl {
    rate: u64,
    start: Instant,
    /// Host frames consumed at the window origin.
    consumed_base: u64,
    produced: u64,
    peeked: i64,
}

impl RateCtl {
    fn new(rate: u32) -> Self {
        Self { rate: rate as u64, start: Instant::now(), consumed_base: 0, produced: 0, peeked: 0 }
    }

    /// Frames the guest clock says should have played since the window opened.
    fn due(&self, now: Instant) -> u64 {
        (now.duration_since(self.start).as_nanos() as u64).saturating_mul(self.rate) / 1_000_000_000
    }

    fn window(&self) -> i64 { rate_reset_window(self.rate) }

    /// Frames to move this wake. `consumed` is the host callback's cumulative
    /// played count, already converted to guest frames; `lead` is the ring
    /// occupancy to keep ahead of it; `cap` bounds one wake (`pace_wake_cap`).
    fn budget(&mut self, now: Instant, consumed: u64, lead: u64, cap: u64) -> u64 {
        let due = self.due(now);
        // Host frames played since the window opened, against the guest clock:
        // positive means the host has run ahead, negative that it is behind.
        let played = consumed.saturating_sub(self.consumed_base);
        let error = played as i64 - due as i64;
        self.peeked = error;
        if error < -self.window() || error > self.window() {
            // The host clock has diverged: re-anchor the drift measurement.
            self.start = now;
            self.consumed_base = consumed;
            self.peeked = 0;
        }
        // due + error == played, so this is the wall-clock budget biased by the
        // drift, expressed as an occupancy target: keep `lead` frames ahead of
        // what the host callback has consumed. Production is cumulative, so a
        // re-anchor above changes nothing physical.
        let want = (consumed as i64 + lead as i64 - self.produced as i64).max(0) as u64;
        want.min(cap)
    }

    /// Account frames actually moved into the host this wake. Frames the ring
    /// refused stay unproduced, so they are due again on the next wake.
    fn accounted(&mut self, moved: u64) {
        self.produced += moved;
    }
}

// Sample rates to try when opening the persistent output stream, in order.
const PREFERRED_RATES: &[u32] = &[48000, 44100, 22050];

// ─── Audio output (owned by Codec A, opened once at start, closed at stop) ───

/// A host audio output the codec feeds. cpal is one implementation; the wav and
/// null sinks make CI capture and headless runs first-class with no sound card.
trait AudioOutput: Send {
    fn stream_rate(&self) -> u32;
    /// Free stereo frames the buffer can accept right now. `usize::MAX` means
    /// unbounded (a file, or a sink that consumes immediately).
    fn free_frames(&self) -> usize;
    /// Push interleaved stereo i16 samples; returns the frames accepted.
    fn push_frames(&mut self, samples: &[i16]) -> usize;
    fn underruns(&self) -> u64;
    /// Gate underrun counting: false while prebuffering or idle.
    fn set_playing(&self, playing: bool);
    /// Frames the host callback has consumed since the stream opened, measured
    /// against a bounded ring. `None` for sinks that accept everything at once
    /// (a file or null sink): they have no playback clock for the rate loop to
    /// follow, so Codec A stays on the open-loop pacer.
    fn consumed_frames(&self) -> Option<u64>;
}

/// Opens a host audio output. Selected from `[audio] backend` and the
/// `IRIS_HAL2_CAPTURE` environment variable.
trait AudioBackend {
    fn open(&self, cfg: &AudioConfig) -> Option<Box<dyn AudioOutput>>;
}

// Opened once at `start()` at the best available host rate; the codec A timer
// pushes i16 stereo pairs through a resampler into the ring buffer producer.
// The stream plays silence when the ring is empty (cpal fills with 0).
struct CpalOutput {
    stream_rate: u32,
    producer: Producer<i16>,
    /// Total i16 samples accepted into the ring since the stream opened.
    pushed_samples: u64,
    /// Ring capacity in i16 samples (both channels).
    capacity_samples: usize,
    underruns: Arc<AtomicU64>,
    // Set once prebuffering finishes and real samples are flowing; cleared when
    // the codec is disarmed/reset. Gates underrun counting so idle silence
    // (stream open, nothing enabled yet) isn't reported as an underrun.
    playing: Arc<AtomicBool>,
    // Keep stream alive; dropped when CpalOutput is dropped at stop().
    _stream: cpal::Stream,
}

// cpal::Stream is !Send/!Sync on some platforms (ALSA uses raw pointers internally),
// but it is safe to hold inside a Mutex.
unsafe impl Send for CpalOutput {}

impl AudioOutput for CpalOutput {
    fn stream_rate(&self) -> u32 { self.stream_rate }
    fn free_frames(&self) -> usize { self.producer.slots() / 2 }
    fn push_frames(&mut self, samples: &[i16]) -> usize {
        let mut n = 0usize;
        for &s in samples {
            if self.producer.push(s).is_err() { break; }
            n += 1;
        }
        self.pushed_samples += n as u64;
        // n is i16 samples; a frame is two (stereo).
        n / 2
    }
    fn underruns(&self) -> u64 { self.underruns.load(Ordering::Relaxed) }
    fn set_playing(&self, playing: bool) { self.playing.store(playing, Ordering::Relaxed); }
    fn consumed_frames(&self) -> Option<u64> {
        let free = self.producer.slots() as u64;
        let occupied = (self.capacity_samples as u64).saturating_sub(free);
        Some(self.pushed_samples.saturating_sub(occupied) / 2)
    }
}

/// A sink that accepts and discards everything, at a nominal rate.
struct NullOutput { rate: u32 }

impl AudioOutput for NullOutput {
    fn stream_rate(&self) -> u32 { self.rate }
    fn free_frames(&self) -> usize { usize::MAX }
    fn push_frames(&mut self, samples: &[i16]) -> usize { samples.len() / 2 }
    fn underruns(&self) -> u64 { 0 }
    fn set_playing(&self, _playing: bool) {}
    fn consumed_frames(&self) -> Option<u64> { None }
}

/// Writes interleaved stereo 16-bit little-endian PCM to a RIFF/WAVE file.
/// Reuses the `IRIS_HAL2_CAPTURE` capture path so CI can record audio without a
/// sound card. Header sizes are patched on drop.
struct WavOutput {
    writer: std::io::BufWriter<std::fs::File>,
    stream_rate: u32,
    frames: u64,
    playing: AtomicBool,
}

fn wav_header(rate: u32, data_bytes: u32) -> [u8; 44] {
    let channels = 2u16;
    let bits = 16u16;
    let block_align = channels * bits / 8;
    let byte_rate = rate * block_align as u32;
    let mut h = [0u8; 44];
    h[0..4].copy_from_slice(b"RIFF");
    h[4..8].copy_from_slice(&(36 + data_bytes).to_le_bytes());
    h[8..12].copy_from_slice(b"WAVE");
    h[12..16].copy_from_slice(b"fmt ");
    h[16..20].copy_from_slice(&16u32.to_le_bytes());
    h[20..22].copy_from_slice(&1u16.to_le_bytes()); // PCM
    h[22..24].copy_from_slice(&channels.to_le_bytes());
    h[24..28].copy_from_slice(&rate.to_le_bytes());
    h[28..32].copy_from_slice(&byte_rate.to_le_bytes());
    h[32..34].copy_from_slice(&block_align.to_le_bytes());
    h[34..36].copy_from_slice(&bits.to_le_bytes());
    h[36..40].copy_from_slice(b"data");
    h[40..44].copy_from_slice(&data_bytes.to_le_bytes());
    h
}

impl WavOutput {
    fn new(path: &std::path::Path, rate: u32) -> std::io::Result<Self> {
        let file = std::fs::File::create(path)?;
        let mut writer = std::io::BufWriter::new(file);
        writer.write_all(&wav_header(rate, 0))?;
        Ok(Self { writer, stream_rate: rate, frames: 0, playing: AtomicBool::new(false) })
    }
}

impl AudioOutput for WavOutput {
    fn stream_rate(&self) -> u32 { self.stream_rate }
    fn free_frames(&self) -> usize { usize::MAX }
    fn push_frames(&mut self, samples: &[i16]) -> usize {
        for &s in samples { let _ = self.writer.write_all(&s.to_le_bytes()); }
        self.frames += (samples.len() / 2) as u64;
        samples.len() / 2
    }
    fn underruns(&self) -> u64 { 0 }
    fn set_playing(&self, playing: bool) { self.playing.store(playing, Ordering::Relaxed); }
    fn consumed_frames(&self) -> Option<u64> { None }
}

impl Drop for WavOutput {
    fn drop(&mut self) {
        let data_bytes = (self.frames * 4).min(u32::MAX as u64) as u32;
        use std::io::Seek;
        let _ = self.writer.flush();
        if self.writer.seek(std::io::SeekFrom::Start(4)).is_ok() {
            let _ = self.writer.write_all(&(36 + data_bytes).to_le_bytes());
        }
        if self.writer.seek(std::io::SeekFrom::Start(40)).is_ok() {
            let _ = self.writer.write_all(&data_bytes.to_le_bytes());
        }
        let _ = self.writer.flush();
    }
}

struct CpalBackend { underruns: Arc<AtomicU64> }

impl AudioBackend for CpalBackend {
    fn open(&self, cfg: &AudioConfig) -> Option<Box<dyn AudioOutput>> {
        open_cpal_output(self.underruns.clone(), Arc::new(AtomicBool::new(false)), cfg)
            .map(|o| Box::new(o) as Box<dyn AudioOutput>)
    }
}

struct NullBackend;

impl AudioBackend for NullBackend {
    fn open(&self, _cfg: &AudioConfig) -> Option<Box<dyn AudioOutput>> {
        Some(Box::new(NullOutput { rate: PREFERRED_RATES[0] }))
    }
}

struct WavBackend { path: std::path::PathBuf }

impl AudioBackend for WavBackend {
    fn open(&self, _cfg: &AudioConfig) -> Option<Box<dyn AudioOutput>> {
        match WavOutput::new(&self.path, PREFERRED_RATES[0]) {
            Ok(w) => Some(Box::new(w)),
            Err(e) => {
                eprintln!("HAL2: cannot open capture file {}: {}", self.path.display(), e);
                None
            }
        }
    }
}

/// Choose the output backend. `IRIS_HAL2_CAPTURE` wins so a CI run records
/// audio through the wav sink with no device; `[audio] backend` selects the
/// rest explicitly.
fn select_backend(cfg: &AudioConfig, underruns: Arc<AtomicU64>) -> Box<dyn AudioBackend> {
    if let Some(path) = std::env::var_os("IRIS_HAL2_CAPTURE") {
        return Box::new(WavBackend { path: std::path::PathBuf::from(path) });
    }
    match cfg.backend {
        AudioBackendKind::Null => Box::new(NullBackend),
        AudioBackendKind::Wav => Box::new(WavBackend { path: std::path::PathBuf::from("hal2-capture.wav") }),
        AudioBackendKind::Cpal | AudioBackendKind::Auto => Box::new(CpalBackend { underruns }),
    }
}

// ─── Resampling ───────────────────────────────────────────────────────────────

/// Converts the codec's guest-rate stereo frames to the host stream rate.
///
/// The codec holds this behind a trait so quality is a config choice:
/// Catmull-Rom is the cheap default; windowed-sinc is band-limited for the low
/// guest rates (8–22 kHz) where Catmull-Rom alone leaves audible images.
trait Resampler: Send {
    fn input_rate(&self) -> u32;
    fn output_rate(&self) -> u32;
    /// Output stereo frames the next `process` will emit, without changing
    /// state. Backpressure uses it to decide whether one input frame fits.
    fn pending_frames(&self) -> usize;
    /// Feed one input stereo frame; append interleaved output samples to `out`.
    fn process(&mut self, l: i16, r: i16, out: &mut Vec<i16>);
}

/// Shared fixed-point accumulator: how many output frames a step of `in_rate`
/// produces while `acc < out_rate`. Both resamplers time identically, so
/// switching implementations does not change the output rate.
fn pending_outputs(acc: u64, in_rate: u32, out_rate: u32) -> usize {
    let out = out_rate as u64;
    let mut acc = acc;
    let mut n = 0usize;
    while acc < out { n += 1; acc += in_rate as u64; }
    n
}

fn clamp_i16(v: f32) -> i16 { v.round().clamp(i16::MIN as f32, i16::MAX as f32) as i16 }

fn make_resampler(kind: ResamplerKind, in_rate: u32, out_rate: u32) -> Box<dyn Resampler> {
    match kind {
        ResamplerKind::CatmullRom => Box::new(CatmullRomResampler::new(in_rate, out_rate)),
        ResamplerKind::Sinc => Box::new(SincResampler::new(in_rate, out_rate)),
    }
}

/// 4-point Catmull-Rom interpolation between input samples.
///
/// It used to repeat the last sample (zero-order hold). From 44.1 or 48 kHz to
/// 48 kHz that is nearly harmless, but Quake plays at 11025 Hz, where each
/// sample became a 4-or-5-sample step: the staircase put loud images of every
/// sound around 11 kHz and its multiples, and the games sounded metallic and
/// robotic. Interpolating through the samples does most of the same as a real
/// DAC's reconstruction filter.
struct CatmullRomResampler {
    in_rate: u32,
    out_rate: u32,
    // Position of the next output between h[1] and h[2], in 1/out_rate
    // input-sample units: an exact rational step, so no drift.
    acc: u64,
    // The last four input frames, oldest first; output is interpolated
    // between h[1] and h[2] (two input samples of latency).
    h: [[f32; 2]; 4],
}

impl CatmullRomResampler {
    fn new(in_rate: u32, out_rate: u32) -> Self {
        Self { in_rate, out_rate, acc: 0, h: [[0.0; 2]; 4] }
    }
}

impl Resampler for CatmullRomResampler {
    fn input_rate(&self) -> u32 { self.in_rate }
    fn output_rate(&self) -> u32 { self.out_rate }
    fn pending_frames(&self) -> usize {
        if self.in_rate == self.out_rate { 1 } else { pending_outputs(self.acc, self.in_rate, self.out_rate) }
    }
    fn process(&mut self, l: i16, r: i16, out: &mut Vec<i16>) {
        if self.in_rate == self.out_rate {
            out.push(l);
            out.push(r);
            return;
        }
        self.h = [self.h[1], self.h[2], self.h[3], [l as f32, r as f32]];
        let or = self.out_rate as u64;
        while self.acc < or {
            let t = self.acc as f32 / or as f32;
            for c in 0..2 {
                let v = catmull_rom(self.h[0][c], self.h[1][c], self.h[2][c], self.h[3][c], t);
                out.push(clamp_i16(v));
            }
            self.acc += self.in_rate as u64;
        }
        self.acc -= or;
    }
}

/// The Catmull-Rom spline through p1 and p2 at t in [0, 1).
fn catmull_rom(p0: f32, p1: f32, p2: f32, p3: f32, t: f32) -> f32 {
    let t2 = t * t;
    let t3 = t2 * t;
    0.5 * (2.0 * p1
        + (p2 - p0) * t
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
        + (3.0 * p1 - p0 - 3.0 * p2 + p3) * t3)
}

/// Half-width of the windowed-sinc kernel, in input frames.
const SINC_TAPS: usize = 16;

/// Windowed-sinc band-limited resampler. Each output is a normalised sinc sum
/// over `2*SINC_TAPS` input frames, Hann-windowed and band-limited to the lower
/// Nyquist when downsampling. This removes the images a real DAC's
/// reconstruction filter would remove, which Catmull-Rom leaves at low rates.
struct SincResampler {
    in_rate: u32,
    out_rate: u32,
    acc: u64,
    cutoff: f32,
    hist: Vec<f32>, // interleaved L/R, 2*SINC_TAPS frames, oldest first
}

impl SincResampler {
    fn new(in_rate: u32, out_rate: u32) -> Self {
        let nyquist = (out_rate as f32 / in_rate as f32).min(1.0);
        Self {
            in_rate,
            out_rate,
            acc: 0,
            // 0.95 keeps the transition band inside Nyquist rather than on it.
            cutoff: 0.95 * nyquist,
            hist: vec![0.0; SINC_TAPS * 4],
        }
    }
}

fn sinc(x: f32) -> f32 {
    if x.abs() < 1e-6 { 1.0 } else {
        let px = std::f32::consts::PI * x;
        px.sin() / px
    }
}

fn hann(x: f32) -> f32 {
    if x.abs() > 1.0 { 0.0 } else { 0.5 * (1.0 + (std::f32::consts::PI * x).cos()) }
}

impl Resampler for SincResampler {
    fn input_rate(&self) -> u32 { self.in_rate }
    fn output_rate(&self) -> u32 { self.out_rate }
    fn pending_frames(&self) -> usize { pending_outputs(self.acc, self.in_rate, self.out_rate) }
    fn process(&mut self, l: i16, r: i16, out: &mut Vec<i16>) {
        let n = self.hist.len();
        self.hist.copy_within(2.., 0);
        self.hist[n - 2] = l as f32;
        self.hist[n - 1] = r as f32;
        let or = self.out_rate as u64;
        let taps = SINC_TAPS as f32;
        while self.acc < or {
            let t = self.acc as f32 / or as f32;
            let pos = (taps - 1.0) + t;
            let (mut sl, mut sr, mut wsum) = (0.0f32, 0.0f32, 0.0f32);
            for k in 0..SINC_TAPS * 2 {
                let d = pos - k as f32;
                let w = sinc(self.cutoff * d) * hann(d / taps);
                sl += self.hist[k * 2] * w;
                sr += self.hist[k * 2 + 1] * w;
                wsum += w;
            }
            if wsum.abs() > 1e-9 { sl /= wsum; sr /= wsum; }
            out.push(clamp_i16(sl));
            out.push(clamp_i16(sr));
            self.acc += self.in_rate as u64;
        }
        self.acc -= or;
    }
}

// ─── Per-channel mutable state, lives inside a Mutex ─────────────────────────

struct CodecAState {
    // Output is opened once at start() and lives until stop().
    // None only before start() or after stop().
    out: Option<Box<dyn AudioOutput>>,
    // Resampler from codec rate → stream rate.  Built (or rebuilt) when
    // codec rate first becomes known or changes.
    resampler: Option<Box<dyn Resampler>>,
    // Interleaved output scratch reused across pushes, so `push_to_ring` does
    // not allocate per frame.
    scratch: Vec<i16>,
    // True while we're still filling the initial prebuffer before feeding the ring.
    prebuffering: bool,
    prebuf: Vec<i16>,
    dry: u32,
    nonzero_seen: bool,
    timer_id: Option<TimerId>,
    /// The DMA channel the armed timer reads, and what it has done since
    /// it was armed: callbacks, frames read, reads that found no data.
    armed_ch: Option<usize>,
    calls: u64,
    frames: u64,
    dry_reads: u64,
}

impl CodecAState {
    fn new() -> Self {
        Self { out: None, resampler: None, scratch: Vec::new(), prebuffering: true, prebuf: Vec::new(),
               dry: 0, nonzero_seen: false, timer_id: None,
               armed_ch: None, calls: 0, frames: 0, dry_reads: 0 }
    }
    fn reset_audio(&mut self) {
        // Keep `out` — stream stays open.  Just reset codec-side state.
        self.resampler = None;
        self.prebuffering = true;
        self.prebuf.clear();
        self.dry = 0;
        self.nonzero_seen = false;
        self.armed_ch = None;
        self.calls = 0;
        self.frames = 0;
        self.dry_reads = 0;
        if let Some(o) = &self.out {
            o.set_playing(false);
        }
    }
    /// Push interleaved i16 stereo pairs through the resampler into the output.
    fn push_to_ring(&mut self, samples: &[i16]) {
        let CodecAState { out, resampler, scratch, .. } = self;
        let (Some(out), Some(rs)) = (out.as_mut(), resampler.as_mut()) else { return };
        scratch.clear();
        for chunk in samples.chunks_exact(2) {
            rs.process(chunk[0], chunk[1], scratch);
        }
        out.push_frames(scratch);
    }

    /// True when the host ring can accept one more input frame's worth of
    /// output. With no host output there is nothing to bound, so the DMA drain
    /// proceeds. This is the bounded-buffer backpressure gate: read only when
    /// the ring has room, so a full ring stalls the read and stops CBP.
    fn can_accept_frame(&self) -> bool {
        match (&self.out, &self.resampler) {
            (Some(o), Some(r)) => r.pending_frames() <= o.free_frames(),
            _ => true,
        }
    }
}

struct CodecBState {
    timer_id: Option<TimerId>,
}

struct AesTxState {
    timer_id: Option<TimerId>,
}

struct AesRxState {
    loopback: std::collections::VecDeque<u32>,
    timer_id: Option<TimerId>,
}

// ─── HAL2 register state ──────────────────────────────────────────────────────

#[derive(Default)]
struct Hal2State {
    isr: u16,
    iar: u16,
    idr: [u16; 4],
    /// The last indirect accesses (IAR, then IDR0..3 as they stood), for
    /// `hal2 status`.
    iar_log: std::collections::VecDeque<(u16, [u16; 4])>,

    // Internal Registers
    // ctrl[0] = CTRL1 (IDR0), ctrl[1] = CTRL2 IDR0, ctrl[2] = CTRL2 IDR1
    codeca_ctrl: [u16; 3],
    codecb_ctrl: [u16; 3],
    aestx_ctrl: [u16; 3],
    aesrx_ctrl: [u16; 3],

    bres_clock_sel: [u16; 3],
    bres_clock_inc: [u16; 3],
    bres_clock_modctrl: [u16; 3],
    bres_clock_rate: [u32; 3],

    dma_enable: u16,
    dma_drive: u16,
    dma_endian: u16,
    dma_relay: u16,
}

impl Hal2State {
    /// Rate of the generator a codec's CLKID selects. CLKID is the generator
    /// NUMBER (1..3) while `bres_clock_rate` is 0-based, so it needs the shift.
    /// CLKID 0 selects nothing; report 0 rather than inventing a rate.
    fn bres_rate(&self, clk: usize) -> u32 {
        match clk {
            1..=3 => self.bres_clock_rate[clk - 1],
            _ => 0,
        }
    }
    fn codeca_cfg(&self) -> (usize, usize, usize) { decode_ctrl1(self.codeca_ctrl[0]) }
    fn codecb_cfg(&self) -> (usize, usize, usize) { decode_ctrl1(self.codecb_ctrl[0]) }
    fn aestx_cfg(&self) -> (usize, usize, usize) { decode_ctrl1(self.aestx_ctrl[0]) }
    fn aesrx_cfg(&self) -> (usize, usize, usize) { decode_ctrl1(self.aesrx_ctrl[0]) }
}

/// A clock generator's output rate. It is a Bresenham counter: every master
/// clock adds `inc`, and each time the sum reaches the modulus it ticks and
/// the modulus is taken off -- at most one tick a master clock. The modulus
/// is programmed as `modctrl = inc - mod - 1`. A modulus of 0 ticks on every
/// master clock: IRIX sets 44.1 kHz from the 44.1 kHz master as inc 0,
/// modctrl 0xffff, which is that, not a stopped clock.
fn bres_rate(master: u32, inc: u16, modctrl: u16) -> u32 {
    let inc = inc as u32;
    let modulus = inc.wrapping_sub(modctrl as u32).wrapping_sub(1) & 0xFFFF;
    if modulus == 0 || inc >= modulus {
        master
    } else {
        master * inc / modulus
    }
}

fn decode_ctrl1(ctrl1: u16) -> (usize, usize, usize) {
    let channel = (ctrl1 & CTRL1_CHAN_MASK) as usize;
    let clock   = ((ctrl1 >> CTRL1_CLOCK_SHIFT) & CTRL1_CLOCK_MASK) as usize;
    let mode    = ((ctrl1 >> CTRL1_MODE_SHIFT)  & CTRL1_MODE_MASK)  as usize;
    (channel, clock, mode)
}

// ─── Hal2 public struct ───────────────────────────────────────────────────────

pub struct Hal2 {
    state: Arc<Mutex<Hal2State>>,
    dma_clients: Vec<Arc<dyn DmaClient>>,
    timer_manager: Arc<std::sync::OnceLock<Arc<TimerManager>>>,
    /// `[audio]` host-output tuning: pre-buffer and cpal buffer size.
    audio_config: AudioConfig,
    // Per-channel mutable state
    ca_state: Arc<Mutex<CodecAState>>,
    cb_state: Arc<Mutex<CodecBState>>,
    at_state: Arc<Mutex<AesTxState>>,
    ar_state: Arc<Mutex<AesRxState>>,
    // cpal output-callback underrun count (ring buffer empty when the host pulled samples).
    underruns: Arc<AtomicU64>,
}

// ─── cpal helpers ─────────────────────────────────────────────────────────────

fn prebuf_samples(rate: u32, prebuf_ms: u64) -> usize {
    (rate as usize * 2 * prebuf_ms as usize) / 1000
}

/// Pure derivation of the two host-output sizes `[audio]` controls, kept out of
/// the cpal-opening path so it can be tested without an audio device.
struct AudioOutputSizing {
    /// cpal's request to the host backend.
    buffer_size: cpal::BufferSize,
    /// Capacity of the rtrb ring, in i16 samples (both channels).
    ring_size: usize,
}

fn audio_output_sizing(cfg: &AudioConfig, rate: u32) -> AudioOutputSizing {
    AudioOutputSizing {
        buffer_size: cfg.cpal_buffer_frames
            .map(cpal::BufferSize::Fixed)
            .unwrap_or(cpal::BufferSize::Default),
        ring_size: prebuf_samples(rate, cfg.prebuf_ms) * RING_BUF_MULTIPLIER,
    }
}

/// Open a persistent stereo i16 cpal output stream, trying PREFERRED_RATES in order.
/// The stream plays silence when the ring buffer is empty.
fn open_cpal_output(underruns: Arc<AtomicU64>, playing: Arc<AtomicBool>, cfg: &AudioConfig) -> Option<CpalOutput> {
    let host = cpal::default_host();
    let device = host.default_output_device()?;

    for &rate in PREFERRED_RATES {
        let sizing = audio_output_sizing(cfg, rate);
        let config = cpal::StreamConfig {
            channels: 2,
            sample_rate: rate,
            buffer_size: sizing.buffer_size,
        };
        let ring_size = sizing.ring_size;
        let err_fn = |err: cpal::Error| { eprintln!("HAL2: cpal stream error: {:?}", err); };

        // Try f32 first (macOS CoreAudio native), then i16 (Linux ALSA).
        let (producer, stream) = {
            let (p, mut c) = RingBuffer::<i16>::new(ring_size);
            let underruns_cb = underruns.clone();
            let playing_cb = playing.clone();
            let data_fn = move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                for sample in data.iter_mut() {
                    let v = match c.pop() {
                        Ok(v) => v,
                        Err(_) => {
                            if playing_cb.load(Ordering::Relaxed) {
                                underruns_cb.fetch_add(1, Ordering::Relaxed);
                            }
                            0
                        }
                    };
                    *sample = v as f32 / 32768.0;
                }
            };
            match device.build_output_stream(config, data_fn, err_fn.clone(), None) {
                Ok(s) => (p, s),
                Err(_) => {
                    // f32 failed, try i16
                    let (p, mut c) = RingBuffer::<i16>::new(ring_size);
                    let underruns_cb = underruns.clone();
                    let playing_cb = playing.clone();
                    let data_fn = move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                        for sample in data.iter_mut() {
                            *sample = match c.pop() {
                                Ok(v) => v,
                                Err(_) => {
                                    if playing_cb.load(Ordering::Relaxed) {
                                        underruns_cb.fetch_add(1, Ordering::Relaxed);
                                    }
                                    0
                                }
                            };
                        }
                    };
                    match device.build_output_stream(config, data_fn, err_fn.clone(), None) {
                        Ok(s) => (p, s),
                        Err(e) => {
                            eprintln!("HAL2: cpal build_output_stream failed at {}Hz: {:?}", rate, e);
                            continue;
                        }
                    }
                }
            }
        };
        if stream.play().is_err() { continue; }
        // cpal 0.18 dropped DeviceTrait::name(); a Device's Display impl is its name.
        println!("HAL2: audio output: {} via {:?} at {}Hz", device, host.id(), rate);
        return Some(CpalOutput {
            stream_rate: rate,
            producer,
            pushed_samples: 0,
            capacity_samples: ring_size,
            underruns,
            playing,
            _stream: stream,
        });
    }

    eprintln!("HAL2: failed to open audio output at any rate (tried {:?})", PREFERRED_RATES);
    None
}


// ─── impl Hal2 ────────────────────────────────────────────────────────────────

impl Hal2 {
    pub fn new(dma_clients: Vec<Arc<dyn DmaClient>>, audio_config: AudioConfig) -> Self {
        Self {
            state: Arc::new(Mutex::new(Hal2State {
                isr: 0,
                iar: 0,
                idr: [0; 4],
                iar_log: std::collections::VecDeque::new(),
                codeca_ctrl: [0; 3],
                codecb_ctrl: [0; 3],
                aestx_ctrl: [0; 3],
                aesrx_ctrl: [0; 3],
                bres_clock_sel: [1; 3],          // 1 = 44100 Hz master (IP22 boot tune is 44100 Hz)
                bres_clock_inc: [1; 3],           // reset: inc=1
                bres_clock_modctrl: [0xFFFF; 3],  // reset: mod=1, so modctrl = 1-1-1 = 0xFFFF
                bres_clock_rate: [44100; 3],
                dma_enable: 0,
                dma_drive: 0,
                dma_endian: 0,
                dma_relay: 0,
            })),
            dma_clients,
            timer_manager: Arc::new(std::sync::OnceLock::new()),
            audio_config,
            ca_state: Arc::new(Mutex::new(CodecAState::new())),
            cb_state: Arc::new(Mutex::new(CodecBState { timer_id: None })),
            at_state: Arc::new(Mutex::new(AesTxState { timer_id: None })),
            ar_state: Arc::new(Mutex::new(AesRxState { loopback: std::collections::VecDeque::new(), timer_id: None })),
            underruns: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn underrun_count(&self) -> u64 {
        self.underruns.load(Ordering::Relaxed)
    }

    pub fn set_timer_manager(&self, tm: Arc<TimerManager>) {
        let _ = self.timer_manager.set(tm);
    }

    /// Power-on reset: restore all registers and per-channel state to defaults.
    /// Must only be called after `stop()` has cancelled all timers.
    pub fn power_on(&self) {
        {
            let mut s = self.state.lock();
            s.isr = 0;
            s.iar = 0;
            s.idr = [0; 4];
            s.codeca_ctrl = [0; 3];
            s.codecb_ctrl = [0; 3];
            s.aestx_ctrl  = [0; 3];
            s.aesrx_ctrl  = [0; 3];
            s.bres_clock_sel     = [1; 3];
            s.bres_clock_inc     = [1; 3];
            s.bres_clock_modctrl = [0xFFFF; 3];
            s.bres_clock_rate    = [44100; 3];
            s.dma_enable = 0;
            s.dma_drive  = 0;
            s.dma_endian = 0;
            s.dma_relay  = 0;
        }
        // Per-channel states: timers already stopped by stop(), just clear audio state.
        self.ca_state.lock().reset_audio();
        self.ar_state.lock().loopback.clear();
    }

    // ── Codec A output timer ──────────────────────────────────────────────────

    fn arm_codeca(&self) {
        let Some(tm) = self.timer_manager.get() else { return; };
        self.disarm_codeca();

        let (dma_ch, mode, rate, pitch_rate) = {
            let s = self.state.lock();
            let (ch, clk, mode) = s.codeca_cfg();
            let rate = s.bres_rate(clk);
            let (_, cb_clk, _) = s.codecb_cfg();
            let cb_rate = s.bres_rate(cb_clk);
            // Codec A is the DAC, so playback is paced by codec A's own
            // clock. Codec B is the ADC and has no say in playback pitch.
            let pitch_rate = if rate > 0 { rate } else if cb_rate > 0 { cb_rate } else { 44100 };
            (ch, mode, rate, pitch_rate)
        };

        if rate == 0 || dma_ch >= self.dma_clients.len() { return; }

        let dma_client = self.dma_clients[dma_ch].clone();
        let ca_state = self.ca_state.clone();
        let prebuf_ms = self.audio_config.prebuf_ms;
        let resampler_kind = self.audio_config.resampler;
        let cap = pace_wake_cap(pitch_rate as u64);
        // Keep the host ring about one prebuffer ahead of the callback. A sink
        // with no playback clock (file/null) reports no consumption, so the
        // loop falls back to the open-loop pacer below.
        let lead = ((prebuf_samples(rate, prebuf_ms) / 2) as u64).max(cap);
        let mut pacer = Pacer::new(pitch_rate);
        let mut rate_ctl = RateCtl::new(pitch_rate);

        self.ca_state.lock().armed_ch = Some(dma_ch);
        let id = tm.add_recurring(Instant::now() + PACE_PERIOD, PACE_PERIOD, (), move |_| {
            let mut st = ca_state.lock();
            // Close the rate loop on what the host callback has actually
            // consumed, converted from stream frames back to guest frames. A
            // sink with no playback clock (file/null) reports none, and the
            // loop stays open (wall-clock pacer).
            let consumed = st.out.as_ref().and_then(|o| {
                let stream_rate = o.stream_rate();
                if stream_rate == 0 { return None; }
                o.consumed_frames()
                    .map(|played| (played as u128 * pitch_rate as u128 / stream_rate as u128) as u64)
            });
            let (due, closed_loop) = match consumed {
                Some(consumed) => (rate_ctl.budget(Instant::now(), consumed, lead, cap), true),
                None => (pacer.due(), false),
            };
            if due == 0 {
                return TimerReturn::Continue;
            }
            let moved = drain_codec_a(&mut st, &dma_client, mode, pitch_rate, rate, prebuf_ms, resampler_kind, due);
            drop(st);
            if closed_loop {
                // Frames the ring would not accept stay unproduced, so they are
                // due again next wake; a stalled read never advances CBP.
                rate_ctl.accounted(moved);
            } else {
                // The pacer counted the whole open budget up front; return what
                // the clamp or a stalled ring left unmoved.
                pacer.give_back(due - moved);
            }
            TimerReturn::Continue
        });

        self.ca_state.lock().timer_id = Some(id);
    }

    fn disarm_codeca(&self) {
        let id = self.ca_state.lock().timer_id.take();
        if let Some(id) = id {
            if let Some(tm) = self.timer_manager.get() { tm.remove(id); }
        }
        self.ca_state.lock().reset_audio(); // keeps `out` (stream stays open)
    }

    // ── Codec B input (silence writer) timer ─────────────────────────────────

    fn arm_codecb(&self) {
        let Some(tm) = self.timer_manager.get() else { return; };
        self.disarm_codecb();

        let (dma_ch, mode, rate) = {
            let s = self.state.lock();
            let (ch, clk, mode) = s.codecb_cfg();
            let rate = s.bres_rate(clk);
            (ch, mode, rate)
        };

        if rate == 0 || dma_ch >= self.dma_clients.len() { return; }

        let dma_client = self.dma_clients[dma_ch].clone();
        let mut pacer = Pacer::new(rate);

        let id = tm.add_recurring(Instant::now() + PACE_PERIOD, PACE_PERIOD, (), move |_| {
            for _ in 0..pacer.due() {
                let _ = dma_client.write(0, false);
                if mode == MODE_STEREO || mode == MODE_QUAD {
                    let _ = dma_client.write(0, false);
                }
                if mode == MODE_QUAD {
                    let _ = dma_client.write(0, false);
                    let _ = dma_client.write(0, false);
                }
            }
            TimerReturn::Continue
        });

        self.cb_state.lock().timer_id = Some(id);
    }

    fn disarm_codecb(&self) {
        let id = self.cb_state.lock().timer_id.take();
        if let Some(id) = id {
            if let Some(tm) = self.timer_manager.get() { tm.remove(id); }
        }
    }

    // ── AES TX drain timer (no cpal output) ──────────────────────────────────

    fn arm_aestx(&self) {
        let Some(tm) = self.timer_manager.get() else { return; };
        self.disarm_aestx();

        let (dma_ch, rate) = {
            let s = self.state.lock();
            let (ch, clk, _mode) = s.aestx_cfg();
            let rate = s.bres_rate(clk);
            (ch, rate)
        };

        if rate == 0 || dma_ch >= self.dma_clients.len() { return; }

        let dma_client = self.dma_clients[dma_ch].clone();
        let ar_state = self.ar_state.clone();
        let mut pacer = Pacer::new(rate);

        let id = tm.add_recurring(Instant::now() + PACE_PERIOD, PACE_PERIOD, (), move |_| {
            for _ in 0..pacer.due() {
                if let Some((val, st, _)) = dma_client.read() {
                    if !st.refused() {
                        ar_state.lock().loopback.push_back(val);
                    }
                }
            }
            TimerReturn::Continue
        });

        self.at_state.lock().timer_id = Some(id);
    }

    fn disarm_aestx(&self) {
        let id = self.at_state.lock().timer_id.take();
        if let Some(id) = id {
            if let Some(tm) = self.timer_manager.get() { tm.remove(id); }
        }
    }

    // ── AES RX loopback write timer ───────────────────────────────────────────

    fn arm_aesrx(&self) {
        let Some(tm) = self.timer_manager.get() else { return; };
        self.disarm_aesrx();

        let (dma_ch, rate) = {
            let s = self.state.lock();
            let (ch, clk, _mode) = s.aesrx_cfg();
            let rate = s.bres_rate(clk);
            (ch, rate)
        };

        if rate == 0 || dma_ch >= self.dma_clients.len() { return; }

        let dma_client = self.dma_clients[dma_ch].clone();
        let ar_state = self.ar_state.clone();
        let mut pacer = Pacer::new(rate);

        let id = tm.add_recurring(Instant::now() + PACE_PERIOD, PACE_PERIOD, (), move |_| {
            for _ in 0..pacer.due() {
                let val = ar_state.lock().loopback.pop_front().unwrap_or(0);
                let _ = dma_client.write(val, false);
            }
            TimerReturn::Continue
        });

        self.ar_state.lock().timer_id = Some(id);
    }

    fn disarm_aesrx(&self) {
        let id = self.ar_state.lock().timer_id.take();
        if let Some(id) = id {
            if let Some(tm) = self.timer_manager.get() { tm.remove(id); }
        }
        self.ar_state.lock().loopback.clear();
    }

    // ── React to dma_enable changes ───────────────────────────────────────────
    // dma_drive uses physical HPC3 channel bits (unrelated to device indices)
    // so we gate arming only on dma_enable.

    fn apply_dma_enable(&self, old: u16, new: u16) {
        let changed = |bit: u16| (old & bit) != (new & bit);
        let enabled = |bit: u16| (new & bit) != 0;

        if changed(DMA_EN_CODECA) {
            if enabled(DMA_EN_CODECA) {
                dlog_dev!(LogModule::Hal2, "HAL2: Codec A DMA enabled");
                self.arm_codeca();
            } else {
                dlog_dev!(LogModule::Hal2, "HAL2: Codec A DMA disabled");
                self.disarm_codeca();
            }
        }

        if changed(DMA_EN_CODECB) {
            if enabled(DMA_EN_CODECB) {
                dlog_dev!(LogModule::Hal2, "HAL2: Codec B DMA enabled");
                self.arm_codecb();
            } else {
                dlog_dev!(LogModule::Hal2, "HAL2: Codec B DMA disabled");
                self.disarm_codecb();
            }
        }

        if changed(DMA_EN_AES_TX) {
            if enabled(DMA_EN_AES_TX) {
                dlog_dev!(LogModule::Hal2, "HAL2: AES TX DMA enabled");
                self.arm_aestx();
            } else {
                dlog_dev!(LogModule::Hal2, "HAL2: AES TX DMA disabled");
                self.disarm_aestx();
            }
        }

        if changed(DMA_EN_AES_RX) {
            if enabled(DMA_EN_AES_RX) {
                dlog_dev!(LogModule::Hal2, "HAL2: AES RX DMA enabled");
                self.arm_aesrx();
            } else {
                dlog_dev!(LogModule::Hal2, "HAL2: AES RX DMA disabled");
                self.disarm_aesrx();
            }
        }
    }

    fn disarm_all(&self) {
        self.disarm_codeca();
        self.disarm_codecb();
        self.disarm_aestx();
        self.disarm_aesrx();
    }

    // ── Register access ───────────────────────────────────────────────────────

    fn update_rates(state: &mut Hal2State) {
        for i in 0..3 {
            let master = match state.bres_clock_sel[i] {
                0 => 48000u32,
                1 => 44100,
                _ => 48000,
            };
            state.bres_clock_rate[i] = bres_rate(master,
                state.bres_clock_inc[i], state.bres_clock_modctrl[i]);
        }
    }

    fn handle_iar_write(&self, val: u16) {
        let mut state = self.state.lock();
        state.iar = val;
        let idr = state.idr;
        if state.iar_log.len() == 32 {
            state.iar_log.pop_front();
        }
        state.iar_log.push_back((val, idr));

        let is_read  = (val & IAR_ACCESS_READ) != 0;
        let typ      = val & IAR_TYPE_MASK;
        let num      = val & IAR_NUM_MASK;
        let param    = val & IAR_PARAM_MASK;
        let bres_idx = ((val >> 8) & 0xF) as usize;

        if is_read {
            match typ {
                IAR_TYPE_GLOBAL_DMA => match param {
                    IAR_PARAM_0 => state.idr[0] = state.dma_relay,
                    IAR_PARAM_1 => state.idr[0] = state.dma_enable,
                    IAR_PARAM_2 => state.idr[0] = state.dma_endian,
                    IAR_PARAM_3 => state.idr[0] = state.dma_drive,
                    _ => {}
                },
                IAR_TYPE_DMA => match num {
                    IAR_NUM_CODECA => match param {
                        IAR_PARAM_1 => state.idr[0] = state.codeca_ctrl[0],
                        IAR_PARAM_2 => { state.idr[0] = state.codeca_ctrl[1]; state.idr[1] = state.codeca_ctrl[2]; }
                        _ => {}
                    },
                    IAR_NUM_CODECB => match param {
                        IAR_PARAM_1 => state.idr[0] = state.codecb_ctrl[0],
                        IAR_PARAM_2 => { state.idr[0] = state.codecb_ctrl[1]; state.idr[1] = state.codecb_ctrl[2]; }
                        _ => {}
                    },
                    IAR_NUM_AES_TX => match param {
                        IAR_PARAM_1 => state.idr[0] = state.aestx_ctrl[0],
                        IAR_PARAM_2 => { state.idr[0] = state.aestx_ctrl[1]; state.idr[1] = state.aestx_ctrl[2]; }
                        _ => {}
                    },
                    IAR_NUM_AES_RX => match param {
                        IAR_PARAM_1 => state.idr[0] = state.aesrx_ctrl[0],
                        IAR_PARAM_2 => { state.idr[0] = state.aesrx_ctrl[1]; state.idr[1] = state.aesrx_ctrl[2]; }
                        _ => {}
                    },
                    _ => {}
                },
                IAR_TYPE_BRES if bres_idx >= 1 && bres_idx <= 3 => {
                    let idx = bres_idx - 1;
                    match param {
                        IAR_PARAM_1 => state.idr[0] = state.bres_clock_sel[idx],
                        IAR_PARAM_2 => {
                            state.idr[0] = state.bres_clock_inc[idx];
                            state.idr[1] = state.bres_clock_modctrl[idx];
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        } else {
            // Write path — handle DMA enable specially (need old value)
            match typ {
                IAR_TYPE_GLOBAL_DMA => match param {
                    IAR_PARAM_0 => state.dma_relay  = state.idr[0],
                    IAR_PARAM_1 => {
                        let old = state.dma_enable;
                        let new = state.idr[0];
                        state.dma_enable = new;
                        if old != new { dlog_dev!(LogModule::Hal2, "HAL2: DMA enable 0x{:02x} -> 0x{:02x}", old, new); }
                        drop(state);
                        self.apply_dma_enable(old, new);
                        return;
                    }
                    IAR_PARAM_2 => state.dma_endian = state.idr[0],
                    IAR_PARAM_3 => {
                        if state.dma_drive != state.idr[0] {
                            dlog_dev!(LogModule::Hal2, "HAL2: DMA drive 0x{:02x} -> 0x{:02x}", state.dma_drive, state.idr[0]);
                        }
                        state.dma_drive = state.idr[0];
                    }
                    _ => {}
                },
                IAR_TYPE_DMA => {
                    // CTRL1 writes (param=1) may change the channel number or clock index
                    // for an already-active channel — re-arm it if DMA is currently enabled.
                    let rearm = match (num, param) {
                        (IAR_NUM_CODECA, IAR_PARAM_1) => {
                            state.codeca_ctrl[0] = state.idr[0];
                            (state.dma_enable & DMA_EN_CODECA) != 0
                        }
                        (IAR_NUM_CODECA, IAR_PARAM_2) => {
                            state.codeca_ctrl[1] = state.idr[0]; state.codeca_ctrl[2] = state.idr[1]; false
                        }
                        (IAR_NUM_CODECB, IAR_PARAM_1) => {
                            state.codecb_ctrl[0] = state.idr[0];
                            (state.dma_enable & DMA_EN_CODECB) != 0
                        }
                        (IAR_NUM_CODECB, IAR_PARAM_2) => {
                            state.codecb_ctrl[1] = state.idr[0]; state.codecb_ctrl[2] = state.idr[1]; false
                        }
                        (IAR_NUM_AES_TX, IAR_PARAM_1) => {
                            state.aestx_ctrl[0] = state.idr[0];
                            (state.dma_enable & DMA_EN_AES_TX) != 0
                        }
                        (IAR_NUM_AES_TX, IAR_PARAM_2) => {
                            state.aestx_ctrl[1] = state.idr[0]; state.aestx_ctrl[2] = state.idr[1]; false
                        }
                        (IAR_NUM_AES_RX, IAR_PARAM_1) => {
                            state.aesrx_ctrl[0] = state.idr[0];
                            (state.dma_enable & DMA_EN_AES_RX) != 0
                        }
                        (IAR_NUM_AES_RX, IAR_PARAM_2) => {
                            state.aesrx_ctrl[1] = state.idr[0]; state.aesrx_ctrl[2] = state.idr[1]; false
                        }
                        _ => false,
                    };
                    if rearm {
                        drop(state);
                        match num {
                            IAR_NUM_CODECA => self.arm_codeca(),
                            IAR_NUM_CODECB => self.arm_codecb(),
                            IAR_NUM_AES_TX => self.arm_aestx(),
                            IAR_NUM_AES_RX => self.arm_aesrx(),
                            _ => {}
                        }
                        return;
                    }
                }
                IAR_TYPE_BRES if bres_idx >= 1 && bres_idx <= 3 => {
                    let idx = bres_idx - 1;
                    let changed = match param {
                        IAR_PARAM_1 => {
                            state.bres_clock_sel[idx] = state.idr[0];
                            Self::update_rates(&mut state);
                            let master = if state.bres_clock_sel[idx] == 0 { 48000 } else { 44100 };
                            dlog_dev!(LogModule::Hal2, "HAL2: BRES{} sel={} ({}Hz master) → {}Hz",
                                bres_idx, state.bres_clock_sel[idx], master, state.bres_clock_rate[idx]);
                            true
                        }
                        IAR_PARAM_2 => {
                            state.bres_clock_inc[idx]     = state.idr[0];
                            state.bres_clock_modctrl[idx] = state.idr[1];
                            Self::update_rates(&mut state);
                            dlog_dev!(LogModule::Hal2, "HAL2: BRES{} inc={} modctrl={} → {}Hz",
                                bres_idx, state.bres_clock_inc[idx], state.bres_clock_modctrl[idx],
                                state.bres_clock_rate[idx]);
                            true
                        }
                        _ => false,
                    };
                    if changed {
                        drop(state);
                        // reclock_active compares against codec CLKIDs, which
                        // are generator numbers, so pass the 1-based value.
                        self.reclock_active(bres_idx);
                        return;
                    }
                }
                _ => {}
            }
        }
    }

    /// Re-arm any active channels using generator `bres_idx` (1..3, the same
    /// basis as a codec CTRL1 CLKID).
    fn reclock_active(&self, bres_idx: usize) {
        // Read current clock indices and active mask under lock, then re-arm outside lock.
        let (ca_clk, cb_clk, at_clk, ar_clk, dma_enable) = {
            let s = self.state.lock();
            let (_, ca_clk, _) = s.codeca_cfg();
            let (_, cb_clk, _) = s.codecb_cfg();
            let (_, at_clk, _) = s.aestx_cfg();
            let (_, ar_clk, _) = s.aesrx_cfg();
            (ca_clk, cb_clk, at_clk, ar_clk, s.dma_enable)
        };

        // Codec A re-arms when the generator it selected is reprogrammed. It
        // no longer re-arms on a codec B change: its period is its own now.
        if (dma_enable & DMA_EN_CODECA) != 0 && ca_clk == bres_idx {
            self.arm_codeca();
        }
        if (dma_enable & DMA_EN_CODECB) != 0 && cb_clk == bres_idx {
            self.arm_codecb();
        }
        if (dma_enable & DMA_EN_AES_TX) != 0 && at_clk == bres_idx {
            self.arm_aestx();
        }
        if (dma_enable & DMA_EN_AES_RX) != 0 && ar_clk == bres_idx {
            self.arm_aesrx();
        }
    }

    pub fn read(&self, addr: u32) -> BusRead16 {
        let offset = addr & 0xFF;
        let state = self.state.lock();

        let val: u16 = match offset & 0xF0 {
            HAL2_ISR  => state.isr,
            HAL2_REV  => 0x4010,
            HAL2_IAR  => state.iar,
            HAL2_IDR0 => state.idr[0],
            HAL2_IDR1 => state.idr[1],
            HAL2_IDR2 => state.idr[2],
            HAL2_IDR3 => state.idr[3],
            _ => 0,
        };

        dlog_dev!(LogModule::Hal2, "HAL2: Read offset {:02x} -> {:04x}", offset, val);
        BusRead16::ok(val)
    }

    pub fn write(&self, addr: u32, val: u16) -> u32 {
        let offset = addr & 0xFF;

        dlog_dev!(LogModule::Hal2, "HAL2: Write offset {:02x} <- {:04x}", offset, val);

        match offset & 0xF0 {
            HAL2_ISR => {
                        let old_enable;
                {
                    let mut state = self.state.lock();
                    old_enable = state.dma_enable;
                    if (val & (isr::GLOBAL_RESET_N as u16)) == 0 {
                        dlog_dev!(LogModule::Hal2, "HAL2: global reset (ISR=0x{:04x})", val);
                        state.dma_enable = 0;
                        state.dma_drive  = 0;
                        state.codeca_ctrl = [0; 3];
                        state.codecb_ctrl = [0; 3];
                        state.aestx_ctrl  = [0; 3];
                        state.aesrx_ctrl  = [0; 3];
                        state.bres_clock_sel     = [1; 3];
                        state.bres_clock_inc     = [1; 3];
                        state.bres_clock_modctrl = [0xFFFF; 3];
                        state.bres_clock_rate    = [44100; 3];
                    } else if (val & (isr::CODEC_RESET_N as u16)) == 0 {
                        dlog_dev!(LogModule::Hal2, "HAL2: codec reset (ISR=0x{:04x})", val);
                        state.dma_enable &= !(DMA_EN_CODECA | DMA_EN_CODECB | DMA_EN_AES_TX | DMA_EN_AES_RX);
                        state.codeca_ctrl = [0; 3];
                        state.codecb_ctrl = [0; 3];
                        state.aestx_ctrl  = [0; 3];
                        state.aesrx_ctrl  = [0; 3];
                    }
                    state.isr = val;
                }
                // Apply any DMA enable changes that happened during reset
                let new_enable = self.state.lock().dma_enable;
                if old_enable != new_enable {
                    self.apply_dma_enable(old_enable, new_enable);
                }
            }
            HAL2_IAR  => self.handle_iar_write(val),
            HAL2_IDR0 => self.state.lock().idr[0] = val,
            HAL2_IDR1 => self.state.lock().idr[1] = val,
            HAL2_IDR2 => self.state.lock().idr[2] = val,
            HAL2_IDR3 => self.state.lock().idr[3] = val,
            _ => {}
        }
        BUS_OK
    }

    pub fn register_locks(self: &Arc<Self>) {
        use crate::locks::register_lock_fn;
        let me = self.clone(); register_lock_fn("hal2::state",    move || me.state.is_locked());
        let me = self.clone(); register_lock_fn("hal2::ca_state", move || me.ca_state.is_locked());
        let me = self.clone(); register_lock_fn("hal2::cb_state", move || me.cb_state.is_locked());
        let me = self.clone(); register_lock_fn("hal2::at_state", move || me.at_state.is_locked());
        let me = self.clone(); register_lock_fn("hal2::ar_state", move || me.ar_state.is_locked());
    }
}

// ─── Codec A bounded read ─────────────────────────────────────────────────────

/// Move up to `due` guest frames from the DMA channel into Codec A's host
/// output, stopping early when the ring can accept no more. This is the
/// bounded-buffer backpressure core: a full ring stalls the read (so `CBP`
/// stops advancing) rather than coupling the drain to the guest's polls.
///
/// Returns the number of frames actually moved. The caller hands the remainder
/// back to the pacer, so the stalled frames stay due and are delivered once the
/// host drains. With no host output the DMA is drained unconditionally so the
/// kernel waiting on `PDMA_CTRL_ACT` does not hang.
fn drain_codec_a(
    st: &mut CodecAState,
    dma_client: &Arc<dyn DmaClient>,
    mode: usize,
    pitch_rate: u32,
    rate: u32,
    prebuf_ms: u64,
    resampler_kind: ResamplerKind,
    due: u64,
) -> u64 {
    st.calls += 1;

    // No audio output — still drain DMA so the kernel doesn't hang waiting for
    // PDMA_CTRL_ACT to clear.
    let stream_rate = match st.out.as_ref() {
        Some(o) => o.stream_rate(),
        None => {
            for _ in 0..due {
                let _ = read_frame_from(dma_client, mode);
            }
            return due;
        }
    };

    // (Re)build resampler if codec rate (or kind) changed.
    if st.resampler.as_ref().map_or(true, |r| r.input_rate() != pitch_rate) {
        st.resampler = Some(make_resampler(resampler_kind, pitch_rate, stream_rate));
        dlog_dev!(LogModule::Hal2, "HAL2: Codec A resampler {}Hz (pitch={}) → {}Hz", rate, pitch_rate, stream_rate);
    }

    let mut moved = 0u64;
    for _ in 0..due {
        // Backpressure gate. While prebuffering the frames accumulate in a Vec,
        // not the ring, so nothing to bound yet.
        if !st.prebuffering && !st.can_accept_frame() {
            break;
        }
        let frame = read_frame_from(dma_client, mode);
        if frame.is_some() { st.frames += 1; } else { st.dry_reads += 1; }
        let was_prebuffering = st.prebuffering;

        match frame {
            Some((l, r)) => {
                st.dry = 0;
                if !st.nonzero_seen && (l != 0 || r != 0) {
                    dlog_dev!(LogModule::Hal2, "HAL2: Codec A first non-zero: l={} r={}", l, r);
                    st.nonzero_seen = true;
                }
                if st.prebuffering {
                    // Accumulate before feeding the ring to prevent underrun.
                    st.prebuf.push(l);
                    st.prebuf.push(r);
                    if st.prebuf.len() >= prebuf_samples(rate, prebuf_ms) {
                        let samples = std::mem::take(&mut st.prebuf);
                        st.push_to_ring(&samples);
                        dlog_dev!(LogModule::Hal2, "HAL2: Codec A prebuf flushed ({} frames)", samples.len() / 2);
                        st.prebuffering = false;
                    }
                } else {
                    // Active: push directly.
                    st.push_to_ring(&[l, r]);
                }
            }
            None => {
                st.dry += 1;
                if st.prebuffering && !st.prebuf.is_empty() && st.dry >= DRY_LIMIT {
                    // Flush whatever we buffered so far rather than waiting forever.
                    let samples = std::mem::take(&mut st.prebuf);
                    st.push_to_ring(&samples);
                    dlog_dev!(LogModule::Hal2, "HAL2: Codec A prebuf flushed (dry) after {} dry reads", st.dry);
                    st.prebuffering = false;
                    st.dry = 0;
                }
            }
        }

        // Prebuffering just finished — real audio is now flowing into the ring,
        // so the cpal callback can start treating an empty ring as a genuine underrun.
        if was_prebuffering && !st.prebuffering {
            if let Some(o) = &st.out {
                o.set_playing(true);
            }
        }
        moved += 1;
    }
    moved
}

// ─── DMA read helper (free function to avoid borrow issues in closures) ───────

fn read_frame_from(client: &Arc<dyn DmaClient>, mode: usize) -> Option<(i16, i16)> {
    if mode == MODE_MONO {
        let (v, st, _) = client.read()?;
        if st.refused() { return None; }
        let s = v as i16;
        Some((s, s))
    } else {
        let (lv, lst, _) = client.read()?;
        if lst.refused() { return None; }
        let (rv, rst, _) = match client.read() {
            Some(r) => r,
            None => return Some((lv as i16, lv as i16)),
        };
        let r = if rst.refused() { lv as i16 } else { rv as i16 };
        if mode == MODE_QUAD {
            let _ = client.read();
            let _ = client.read();
        }
        Some((lv as i16, r))
    }
}

// ─── Device impl ──────────────────────────────────────────────────────────────

impl Default for Hal2 {
    fn default() -> Self {
        Self::new(Vec::new(), AudioConfig::default())
    }
}

impl Device for Hal2 {
    fn step(&self, _cycles: u64) {}

    fn start(&self) {
        // Open persistent audio output once.  Codec A timer will push into it.
        let audio = select_backend(&self.audio_config, self.underruns.clone())
            .open(&self.audio_config);
        if audio.is_none() {
            eprintln!("HAL2: no audio output available");
        }
        self.ca_state.lock().out = audio;

        // Re-arm any channels that were already enabled (e.g. after a snapshot restore)
        let dma_enable = self.state.lock().dma_enable;
        self.apply_dma_enable(0, dma_enable);
    }

    fn stop(&self) {
        self.disarm_all();
        // Drop the audio output stream.
        self.ca_state.lock().out = None;
    }

    fn is_running(&self) -> bool {
        self.timer_manager.get().is_some()
    }

    fn get_clock(&self) -> u64 { 0 }

    fn register_commands(&self) -> Vec<(String, String)> {
        vec![("hal2".to_string(), "HAL2 commands: hal2 status".to_string())]
    }

    fn execute_command(&self, cmd: &str, args: &[&str], mut writer: Box<dyn Write + Send>) -> Result<(), String> {
        if cmd != "hal2" { return Err("Command not found".to_string()); }

        match args.first().map(|s| *s) {
            Some("status") => {
                let s = self.state.lock();

                writeln!(writer, "ISR: 0x{:04x}  global_reset_n={} codec_reset_n={} codec_mode={}",
                    s.isr,
                    (s.isr & isr::GLOBAL_RESET_N as u16 != 0) as u8,
                    (s.isr & isr::CODEC_RESET_N  as u16 != 0) as u8,
                    if s.isr & isr::CODEC_MODE as u16 != 0 { "quad" } else { "indigo" },
                ).unwrap();

                writeln!(writer, "DMA enable: 0x{:02x}  codeca={} codecb={} aes_tx={} aes_rx={}",
                    s.dma_enable,
                    (s.dma_enable & DMA_EN_CODECA != 0) as u8,
                    (s.dma_enable & DMA_EN_CODECB != 0) as u8,
                    (s.dma_enable & DMA_EN_AES_TX != 0) as u8,
                    (s.dma_enable & DMA_EN_AES_RX != 0) as u8,
                ).unwrap();
                // dma_drive uses physical HPC3 channel bits (bit N = PBUS channel N)
                writeln!(writer, "DMA drive:  0x{:02x}  (physical HPC3 channel bitmask)", s.dma_drive).unwrap();

                for i in 0..3 {
                    let master = if s.bres_clock_sel[i] == 0 { 48000u32 } else { 44100 };
                    writeln!(writer, "BRES{}: sel={} ({} Hz master)  inc={}  modctrl={}  → {}Hz",
                        i + 1, s.bres_clock_sel[i], master,
                        s.bres_clock_inc[i], s.bres_clock_modctrl[i],
                        s.bres_clock_rate[i],
                    ).unwrap();
                }

                let mode_str = |m| match m { 1 => "mono", 2 => "stereo", 3 => "quad", _ => "off" };
                // bres=0 means CLKID 0: no generator selected.

                let (ca_ch, ca_clk, ca_mode) = s.codeca_cfg();
                writeln!(writer, "Codec A: ch={} bres={} rate={}Hz mode={}",
                    ca_ch, ca_clk, s.bres_rate(ca_clk), mode_str(ca_mode)).unwrap();
                writeln!(writer, "  ctrl1=0x{:04x} ctrl2=[0x{:04x} 0x{:04x}]",
                    s.codeca_ctrl[0], s.codeca_ctrl[1], s.codeca_ctrl[2]).unwrap();

                let (cb_ch, cb_clk, cb_mode) = s.codecb_cfg();
                writeln!(writer, "Codec B: ch={} bres={} rate={}Hz mode={}",
                    cb_ch, cb_clk, s.bres_rate(cb_clk), mode_str(cb_mode)).unwrap();

                let (at_ch, at_clk, _) = s.aestx_cfg();
                let (ar_ch, ar_clk, _) = s.aesrx_cfg();
                writeln!(writer, "AES TX: ch={} bres={} rate={}Hz", at_ch, at_clk, s.bres_rate(at_clk)).unwrap();
                writeln!(writer, "AES RX: ch={} bres={} rate={}Hz", ar_ch, ar_clk, s.bres_rate(ar_clk)).unwrap();
                drop(s);

                let ca = self.ca_state.lock();
                writeln!(writer, "Codec A out: {}  pitch: {}  prebuf: {}  prebuffering: {}  timer: {}",
                    ca.out.as_ref().map_or("none".to_string(), |o| format!("{}Hz", o.stream_rate())),
                    ca.resampler.as_ref().map_or("none".to_string(), |r| format!("{}Hz", r.input_rate())),
                    ca.prebuf.len() / 2,
                    ca.prebuffering,
                    ca.timer_id.map_or("none".to_string(), |id| format!("{:#x}", id)),
                ).unwrap();
                writeln!(writer, "Codec A timer: reads ch={}  calls={}  frames={}  dry reads={}",
                    ca.armed_ch.map_or("-".to_string(), |c| c.to_string()), ca.calls, ca.frames, ca.dry_reads).unwrap();
                drop(ca);
                writeln!(writer, "cpal underruns (samples): {}", self.underruns.load(Ordering::Relaxed)).unwrap();

                writeln!(writer, "Codec B timer: {}",
                    self.cb_state.lock().timer_id.map_or("none".to_string(), |id| format!("{:#x}", id))).unwrap();
                writeln!(writer, "AES TX timer: {}",
                    self.at_state.lock().timer_id.map_or("none".to_string(), |id| format!("{:#x}", id))).unwrap();
                let ar = self.ar_state.lock();
                writeln!(writer, "AES RX timer: {}  loopback_len={}",
                    ar.timer_id.map_or("none".to_string(), |id| format!("{:#x}", id)),
                    ar.loopback.len(),
                ).unwrap();
                drop(ar);
                let log: Vec<(u16, [u16; 4])> = self.state.lock().iar_log.iter().copied().collect();
                writeln!(writer, "Recent indirect accesses (IAR: IDR0 IDR1 IDR2 IDR3), oldest first:").unwrap();
                for (iar, idr) in log {
                    writeln!(writer, "  {:04x}{}: {:04x} {:04x} {:04x} {:04x}", iar,
                        if iar & IAR_ACCESS_READ != 0 { " (read)" } else { "" },
                        idr[0], idr[1], idr[2], idr[3]).unwrap();
                }
            }
            _ => return Err("Usage: hal2 status".to_string()),
        }
        Ok(())
    }
}

impl Saveable for Hal2 {
    fn state_desc(&self) -> Option<crate::state_desc::StateDesc<'_>> {
        use crate::state_desc::{FieldKind, StateDesc};
        use toml::Value;
        Some(
            StateDesc::new("hal2", 1)
                .field(
                    "isr",
                    FieldKind::U16,
                    1,
                    |t| { t.insert("isr".into(), hex_u16(self.state.lock().isr)); },
                    |v| { if let Some(n) = toml_u16(v) { self.state.lock().isr = n; } Ok(()) },
                )
                .field(
                    "iar",
                    FieldKind::U16,
                    1,
                    |t| { t.insert("iar".into(), hex_u16(self.state.lock().iar)); },
                    |v| { if let Some(n) = toml_u16(v) { self.state.lock().iar = n; } Ok(()) },
                )
                .field(
                    "idr",
                    FieldKind::U16Array,
                    1,
                    |t| { t.insert("idr".into(), u16_slice_to_toml(&self.state.lock().idr)); },
                    |v| { load_u16_slice(v, &mut self.state.lock().idr); Ok(()) },
                )
                .field(
                    "codeca_ctrl",
                    FieldKind::U16Array,
                    1,
                    |t| { t.insert("codeca_ctrl".into(), u16_slice_to_toml(&self.state.lock().codeca_ctrl)); },
                    |v| { load_u16_slice(v, &mut self.state.lock().codeca_ctrl); Ok(()) },
                )
                .field(
                    "codecb_ctrl",
                    FieldKind::U16Array,
                    1,
                    |t| { t.insert("codecb_ctrl".into(), u16_slice_to_toml(&self.state.lock().codecb_ctrl)); },
                    |v| { load_u16_slice(v, &mut self.state.lock().codecb_ctrl); Ok(()) },
                )
                .field(
                    "aestx_ctrl",
                    FieldKind::U16Array,
                    1,
                    |t| { t.insert("aestx_ctrl".into(), u16_slice_to_toml(&self.state.lock().aestx_ctrl)); },
                    |v| { load_u16_slice(v, &mut self.state.lock().aestx_ctrl); Ok(()) },
                )
                .field(
                    "aesrx_ctrl",
                    FieldKind::U16Array,
                    1,
                    |t| { t.insert("aesrx_ctrl".into(), u16_slice_to_toml(&self.state.lock().aesrx_ctrl)); },
                    |v| { load_u16_slice(v, &mut self.state.lock().aesrx_ctrl); Ok(()) },
                )
                .field(
                    "bres_clock_sel",
                    FieldKind::U16Array,
                    1,
                    |t| { t.insert("bres_clock_sel".into(), u16_slice_to_toml(&self.state.lock().bres_clock_sel)); },
                    |v| { load_u16_slice(v, &mut self.state.lock().bres_clock_sel); Ok(()) },
                )
                .field(
                    "bres_clock_inc",
                    FieldKind::U16Array,
                    1,
                    |t| { t.insert("bres_clock_inc".into(), u16_slice_to_toml(&self.state.lock().bres_clock_inc)); },
                    |v| { load_u16_slice(v, &mut self.state.lock().bres_clock_inc); Ok(()) },
                )
                .field(
                    "bres_clock_modctrl",
                    FieldKind::U16Array,
                    1,
                    |t| { t.insert("bres_clock_modctrl".into(), u16_slice_to_toml(&self.state.lock().bres_clock_modctrl)); },
                    |v| { load_u16_slice(v, &mut self.state.lock().bres_clock_modctrl); Ok(()) },
                )
                .field(
                    "bres_clock_rate",
                    FieldKind::U32Array,
                    1,
                    |t| { t.insert("bres_clock_rate".into(), u32_slice_to_toml(&self.state.lock().bres_clock_rate)); },
                    |v| { load_u32_slice(v, &mut self.state.lock().bres_clock_rate); Ok(()) },
                )
                .field(
                    "dma_drive",
                    FieldKind::U16,
                    1,
                    |t| { t.insert("dma_drive".into(), hex_u16(self.state.lock().dma_drive)); },
                    |v| { if let Some(n) = toml_u16(v) { self.state.lock().dma_drive = n; } Ok(()) },
                )
                .field(
                    "dma_endian",
                    FieldKind::U16,
                    1,
                    |t| { t.insert("dma_endian".into(), hex_u16(self.state.lock().dma_endian)); },
                    |v| { if let Some(n) = toml_u16(v) { self.state.lock().dma_endian = n; } Ok(()) },
                )
                .field(
                    "dma_relay",
                    FieldKind::U16,
                    1,
                    |t| { t.insert("dma_relay".into(), hex_u16(self.state.lock().dma_relay)); },
                    |v| { if let Some(n) = toml_u16(v) { self.state.lock().dma_relay = n; } Ok(()) },
                )
                // Loading the enable mask re-arms every live channel. This must
                // happen before `armed_ch` is restored below, so the captured
                // value wins over the derived one.
                .field(
                    "dma_enable",
                    FieldKind::U16,
                    1,
                    |t| { t.insert("dma_enable".into(), hex_u16(self.state.lock().dma_enable)); },
                    |v| {
                        let n = toml_u16(v).unwrap_or(0);
                        self.state.lock().dma_enable = n;
                        // Re-arm every channel the enable mask says is live.
                        self.apply_dma_enable(0, n);
                        Ok(())
                    },
                )
                // AES TX→RX loopback, in order — the audio path's only
                // cross-channel queue not derivable from a register.
                .field(
                    "aes_rx_loopback",
                    FieldKind::U32Array,
                    1,
                    |t| {
                        let loopback: Vec<u32> = self.ar_state.lock().loopback.iter().copied().collect();
                        t.insert("aes_rx_loopback".into(), u32_slice_to_toml(&loopback));
                    },
                    |v| {
                        let mut ar = self.ar_state.lock();
                        ar.loopback.clear();
                        if let Some(items) = v.as_array() {
                            for item in items {
                                // toml_u32 handles both the hex-string encoding and ints.
                                if let Some(n) = crate::snapshot::toml_u32(item) { ar.loopback.push_back(n); }
                            }
                        }
                        Ok(())
                    },
                )
                // The armed HPC3 channel Codec A drains, or -1 when disarmed.
                // Only the channel is persisted — the TimerId that drives it is
                // rebuilt on load. Restored after `dma_enable`'s re-arm so a
                // host without a timer manager (tests, headless) round-trips
                // identically.
                .field(
                    "armed_ch",
                    FieldKind::I64,
                    1,
                    |t| {
                        let armed = self.ca_state.lock().armed_ch.map(|c| c as i64).unwrap_or(-1);
                        t.insert("armed_ch".into(), Value::Integer(armed));
                    },
                    |v| {
                        let armed = v.as_integer().filter(|n| *n >= 0).map(|n| n as usize);
                        self.ca_state.lock().armed_ch = armed;
                        Ok(())
                    },
                ),
        )
    }

    fn save_state(&self) -> toml::Value {
        self.state_desc().expect("hal2 has a state description").save()
    }

    fn load_state(&self, v: &toml::Value) -> Result<(), String> {
        self.state_desc().expect("hal2 has a state description").load(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::DmaStatus;
    use rtrb::RingBuffer;

    #[test]
    fn default_audio_config_reproduces_the_builtin_prebuffer() {
        // Default config must be byte-for-byte the old hard-coded behaviour:
        // host-default cpal buffer, ring sized from PREBUF_MS.
        let sizing = audio_output_sizing(&AudioConfig::default(), 44100);
        assert_eq!(sizing.buffer_size, cpal::BufferSize::Default);
        assert_eq!(
            sizing.ring_size,
            prebuf_samples(44100, PREBUF_MS) * RING_BUF_MULTIPLIER
        );
    }

    #[test]
    fn audio_config_drives_buffer_and_ring_sizes() {
        let cfg = AudioConfig { prebuf_ms: 40, cpal_buffer_frames: Some(512), ..AudioConfig::default() };
        let sizing = audio_output_sizing(&cfg, 48000);
        assert_eq!(sizing.buffer_size, cpal::BufferSize::Fixed(512));
        assert_eq!(sizing.ring_size, prebuf_samples(48000, 40) * RING_BUF_MULTIPLIER);
        // 40 ms of stereo at 48 kHz is twice the 20 ms default.
        assert_eq!(sizing.ring_size, 2 * prebuf_samples(48000, PREBUF_MS) * RING_BUF_MULTIPLIER);
    }

    /// A DMA client with an endless supply of samples and a read counter,
    /// enough to observe how many frames the bounded drain actually moved.
    struct CountingDma {
        reads: AtomicU64,
    }

    impl DmaClient for CountingDma {
        fn read(&self) -> Option<(u32, DmaStatus, Option<(u32, u16)>)> {
            self.reads.fetch_add(1, Ordering::Relaxed);
            Some((0x1234 << 8, DmaStatus::ok(), None))
        }
        fn write(&self, _val: u32, _eop: bool) -> (DmaStatus, Option<(u32, u16)>) {
            (DmaStatus::ok(), None)
        }
    }

    /// A host output backed by a real ring with no cpal stream, so the
    /// backpressure and sink paths are exercisable without audio hardware.
    struct TestOutput {
        rate: u32,
        producer: Producer<i16>,
        capacity_samples: usize,
        pushed_samples: u64,
        playing: AtomicBool,
    }

    impl AudioOutput for TestOutput {
        fn stream_rate(&self) -> u32 { self.rate }
        fn free_frames(&self) -> usize { self.producer.slots() / 2 }
        fn push_frames(&mut self, samples: &[i16]) -> usize {
            let mut n = 0usize;
            for &s in samples {
                if self.producer.push(s).is_err() { break; }
                n += 1;
            }
            self.pushed_samples += n as u64;
            n / 2
        }
        fn underruns(&self) -> u64 { 0 }
        fn set_playing(&self, playing: bool) { self.playing.store(playing, Ordering::Relaxed); }
        fn consumed_frames(&self) -> Option<u64> {
            let free = self.producer.slots() as u64;
            let occupied = (self.capacity_samples as u64).saturating_sub(free);
            Some(self.pushed_samples.saturating_sub(occupied) / 2)
        }
    }

    fn test_output(frames: usize) -> (Box<dyn AudioOutput>, rtrb::Consumer<i16>) {
        let (producer, consumer) = RingBuffer::<i16>::new(frames * 2);
        let out = TestOutput {
            rate: 48000,
            producer,
            capacity_samples: frames * 2,
            pushed_samples: 0,
            playing: AtomicBool::new(false),
        };
        (Box::new(out), consumer)
    }

    /// Bounded-buffer backpressure: a full ring stalls the codec read instead
    /// of letting the open-loop pacer run into slots the guest has not filled.
    #[test]
    fn full_ring_stalls_the_codec_read() {
        let cap = 8usize;
        let (out, mut cons) = test_output(cap);
        let mut st = CodecAState::new();
        st.out = Some(out);
        st.resampler = Some(make_resampler(ResamplerKind::CatmullRom, 48000, 48000));

        let dma = Arc::new(CountingDma { reads: AtomicU64::new(0) });
        let client: Arc<dyn DmaClient> = dma.clone();

        // prebuf_ms = 0 forces the initial prebuffer to flush on the first
        // frame, so the ring fills and then stalls within one wake.
        let moved = drain_codec_a(&mut st, &client, MODE_STEREO, 48000, 48000, 0, ResamplerKind::CatmullRom, 100);
        assert_eq!(moved, cap as u64, "ring capacity must bound one wake's drain");
        assert_eq!(
            dma.reads.load(Ordering::Relaxed),
            cap as u64 * 2,
            "a stereo frame is two 32-bit words; no read may run past the ring"
        );

        // Drain the host ring; the frames left due become deliverable again.
        for _ in 0..cap {
            assert!(cons.pop().is_ok());
            assert!(cons.pop().is_ok());
        }
        let moved2 = drain_codec_a(&mut st, &client, MODE_STEREO, 48000, 48000, 0, ResamplerKind::CatmullRom, 100);
        assert_eq!(moved2, cap as u64, "the read resumes once the ring drains");
    }

    /// A DMA client that replays an interleaved stereo PCM capture, one i16
    /// sample per 32-bit read, for the recorded-audio validation below.
    struct CorpusDma {
        samples: Vec<i16>,
        served: AtomicU64,
    }

    impl DmaClient for CorpusDma {
        fn read(&self) -> Option<(u32, DmaStatus, Option<(u32, u16)>)> {
            let i = self.served.fetch_add(1, Ordering::Relaxed) as usize;
            match self.samples.get(i) {
                Some(&s) => Some((s as u16 as u32, DmaStatus::ok(), None)),
                None => {
                    self.served.fetch_sub(1, Ordering::Relaxed);
                    None
                }
            }
        }
        fn write(&self, _val: u32, _eop: bool) -> (DmaStatus, Option<(u32, u16)>) {
            (DmaStatus::ok(), None)
        }
    }

    /// Outcome of replaying a PCM capture through the Codec A drain under a
    /// simulated host consumer.
    struct RecordedAudio {
        /// Interleaved stereo samples the host actually pulled, in order.
        played: Vec<i16>,
        /// The capture, for a prefix comparison that catches loss or reorder.
        source: Vec<i16>,
        /// Host callbacks that found the ring empty while audio was playing.
        underruns: u64,
        /// Frames the codec moved from the DMA channel.
        frames_moved: u64,
    }

    /// Replays `source` through the real Codec A drain and closed-loop rate
    /// control, with a host consumer draining the ring at `host_rate` frames/s.
    /// `stall`, if set, pauses the consumer for `(from, to)` wakes to model a
    /// busy host.
    ///
    /// This is the recorded-audio validation that retired the legacy
    /// `read_ahead_of_poll` clamp: it follows the same production path as
    /// `Hal2::arm_codeca`, driving a synthetic PCM capture at a bounded ring.
    fn replay_recorded(
        source: Vec<i16>,
        host_rate: u64,
        ring_frames: usize,
        prebuf_ms: u64,
        wakes: u64,
        stall: Option<(u64, u64)>,
    ) -> RecordedAudio {
        let guest = 48_000u32;
        let dma = Arc::new(CorpusDma { samples: source.clone(), served: AtomicU64::new(0) });
        let client: Arc<dyn DmaClient> = dma.clone();
        let (out, mut cons) = test_output(ring_frames);
        let mut st = CodecAState::new();
        st.out = Some(out);
        st.resampler = Some(make_resampler(ResamplerKind::CatmullRom, guest, guest));

        let cap = pace_wake_cap(guest as u64);
        // The same ring lead `arm_codeca` keeps ahead of the host callback.
        let lead = ((prebuf_samples(guest, prebuf_ms) / 2) as u64).max(cap);
        let mut ctl = RateCtl::new(guest);
        let mut now = ctl.start;

        let mut played = Vec::new();
        let mut underruns = 0u64;
        let mut frames_moved = 0u64;
        let mut host_acc: u128 = 0;

        for w in 0..wakes {
            now += PACE_PERIOD;
            let stalled = stall.map_or(false, |(from, to)| w >= from && w < to);
            if !stalled {
                host_acc += host_rate as u128 * PACE_PERIOD.as_nanos() as u128;
            }
            let host_due = (host_acc / 1_000_000_000) as usize;
            host_acc %= 1_000_000_000;

            let consumed = st.out.as_ref().unwrap().consumed_frames().unwrap();
            let budget = ctl.budget(now, consumed, lead, cap);
            let moved = drain_codec_a(
                &mut st, &client, MODE_STEREO, guest, guest,
                prebuf_ms, ResamplerKind::CatmullRom, budget,
            );
            ctl.accounted(moved);
            frames_moved += moved;

            if st.prebuffering {
                // Still filling the initial pre-buffer; the ring is empty and
                // silence (not a corpus sample) is what the host would hear.
                continue;
            }
            for _ in 0..host_due {
                match cons.pop() {
                    Ok(v) => played.push(v),
                    Err(_) => {
                        // Policy: an underrun is rendered as silence.
                        underruns += 1;
                        played.push(0);
                    }
                }
            }
        }

        RecordedAudio { played, source, underruns, frames_moved }
    }

    /// A deterministic, nonzero stereo capture: a dropped, duplicated or
    /// reordered sample cannot hide in a run of silence.
    fn sawtooth_capture(frames: usize) -> Vec<i16> {
        let mut v = Vec::with_capacity(frames * 2);
        for k in 0..frames as i64 {
            let l = ((k * 7) % 30_000) as i16 + 1;
            let r = -(((k * 11) % 30_000) as i16 + 1);
            v.push(l);
            v.push(r);
        }
        v
    }

    /// Recorded-audio validation for the clamp removal. A captured PCM stream
    /// is replayed through the Codec A drain under host consumers running slow,
    /// on-rate and fast (clock drift either way). With the legacy poll clamp
    /// retired, the bounded ring and closed-loop rate control must deliver the
    /// capture intact: no dropped, duplicated or reordered samples, no underruns
    /// once playback has started, and production never running away from the
    /// host.
    #[test]
    fn recorded_audio_delivers_intact_under_slow_and_fast_hosts() {
        let capture_frames = 120_000;
        let cap = pace_wake_cap(48_000);
        let lead = ((prebuf_samples(48_000, PREBUF_MS) / 2) as u64).max(cap);
        for &host_rate in &[46_000u64, 48_000, 50_000] {
            let run = replay_recorded(
                sawtooth_capture(capture_frames),
                host_rate,
                4096,
                PREBUF_MS,
                8_000,
                None,
            );
            assert_eq!(
                run.underruns, 0,
                "host {} Hz: {} underruns without the poll clamp",
                host_rate, run.underruns
            );
            assert!(!run.played.is_empty(), "host {} Hz: nothing played", host_rate);
            assert_eq!(
                &run.played[..], &run.source[..run.played.len()],
                "host {} Hz: capture not delivered intact (loss/dup/reorder)",
                host_rate
            );
            // Production follows the host callback, bounded by the ring lead:
            // it neither starves the host nor runs away from it.
            let played_frames = (run.played.len() / 2) as i64;
            let gap = run.frames_moved as i64 - played_frames;
            assert!(
                (0..=(lead + 2 * cap) as i64).contains(&gap),
                "host {} Hz: produced-consumed gap {} outside 0..{}",
                host_rate, gap, lead + 2 * cap
            );
        }
    }

    /// A captured stream must ride out a busy host without clicks: the bounded
    /// ring absorbs the pause, production stalls rather than reading ahead, and
    /// on resume the capture continues in order with no underrun or lost sample.
    #[test]
    fn recorded_audio_absorbs_a_host_stall_without_underrun_or_loss() {
        // A 60 ms host pause after steady state is established.
        let run = replay_recorded(
            sawtooth_capture(120_000),
            48_000,
            4096,
            PREBUF_MS,
            8_000,
            Some((2_000, 2_240)),
        );
        assert_eq!(run.underruns, 0, "host stall caused {} underruns", run.underruns);
        assert_eq!(
            &run.played[..], &run.source[..run.played.len()],
            "capture not delivered intact across the host stall"
        );
    }

    /// The wav sink writes a RIFF/WAVE file with a valid header and the pushed
    /// samples, so CI can capture audio with no sound card.
    #[test]
    fn wav_sink_writes_a_valid_capture() {
        let path = std::env::temp_dir().join(format!("iris-hal2-wav-{}.wav", std::process::id()));
        {
            let mut w = WavOutput::new(&path, 44100).expect("create wav");
            assert_eq!(w.stream_rate(), 44100);
            assert_eq!(w.free_frames(), usize::MAX);
            assert_eq!(w.push_frames(&[100, -100, 200, -200]), 2);
        } // drop patches the header sizes
        let b = std::fs::read(&path).expect("read wav");
        let _ = std::fs::remove_file(&path);

        assert_eq!(b.len(), 44 + 8, "header plus four samples");
        assert_eq!(&b[0..4], b"RIFF");
        assert_eq!(&b[8..12], b"WAVE");
        assert_eq!(&b[12..16], b"fmt ");
        assert_eq!(u16::from_le_bytes([b[20], b[21]]), 1); // PCM
        assert_eq!(u16::from_le_bytes([b[22], b[23]]), 2); // stereo
        assert_eq!(u32::from_le_bytes([b[24], b[25], b[26], b[27]]), 44100);
        assert_eq!(u16::from_le_bytes([b[34], b[35]]), 16); // bits
        assert_eq!(u32::from_le_bytes([b[4], b[5], b[6], b[7]]), 36 + 8);
        assert_eq!(&b[36..40], b"data");
        assert_eq!(u32::from_le_bytes([b[40], b[41], b[42], b[43]]), 8);
        assert_eq!(i16::from_le_bytes([b[44], b[45]]), 100);
        assert_eq!(i16::from_le_bytes([b[46], b[47]]), -100);
    }

    /// The null and wav backends open through the trait with no device.
    #[test]
    fn null_and_wav_backends_open_through_the_trait() {
        let null = NullBackend.open(&AudioConfig::default()).expect("null opens");
        assert_eq!(null.stream_rate(), 48000);

        let path = std::env::temp_dir().join(format!("iris-hal2-be-{}.wav", std::process::id()));
        let backend = WavBackend { path: path.clone() };
        let mut out = backend.open(&AudioConfig::default()).expect("wav opens");
        out.push_frames(&[7, -7]);
        drop(out);
        assert!(path.exists(), "the wav backend created its capture file");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn null_sink_accepts_and_discards() {
        let mut n = NullOutput { rate: 48000 };
        assert_eq!(n.stream_rate(), 48000);
        assert_eq!(n.free_frames(), usize::MAX);
        assert_eq!(n.push_frames(&[1, 2, 3, 4, 5, 6]), 3);
        assert_eq!(n.underruns(), 0);
    }

    #[test]
    fn save_load_round_trip() {
        let src = Hal2::new(Vec::new(), AudioConfig::default());
        {
            let mut s = src.state.lock();
            s.isr = 0x18;
            s.iar = 0x9104;
            s.idr = [0x1111, 0x2222, 0x3333, 0x4444];
            s.codeca_ctrl = [0x0208, 0x0a0b, 0x0c0d];
            s.codecb_ctrl = [0x0102, 0x0304, 0x0506];
            s.aestx_ctrl  = [0x0213, 0x0000, 0x0000];
            s.aesrx_ctrl  = [0x0102, 0x0000, 0x0000];
            s.bres_clock_sel     = [0, 1, 2];
            s.bres_clock_inc     = [4, 1, 4];
            s.bres_clock_modctrl = [0xFFF9, 0xFFFF, 0xFFF3];
            s.bres_clock_rate    = [48000, 44100, 16000];
            s.dma_enable = DMA_EN_CODECA | DMA_EN_AES_TX;
            s.dma_drive  = 0x0007;
            s.dma_endian = 1;
            s.dma_relay  = 1;
        }
        src.ca_state.lock().armed_ch = Some(3);
        {
            let mut ar = src.ar_state.lock();
            ar.loopback.push_back(0xdead_beef);
            ar.loopback.push_back(0x0000_1234);
        }

        let v1 = src.save_state();

        let dst = Hal2::new(Vec::new(), AudioConfig::default());
        dst.load_state(&v1).expect("load_state");
        let v2 = dst.save_state();

        assert_eq!(v1, v2, "Hal2 save_state mismatch after load_state round-trip");
    }

    /// The registered description catches a renamed/removed field: the
    /// generated payload verifies, but moving a key fails Verify (and the
    /// schema signature is what `measure` records).
    #[test]
    fn state_desc_verify_rejects_a_renamed_field() {
        let src = Hal2::new(Vec::new(), AudioConfig::default());
        let desc = src.state_desc().expect("hal2 has a state description");
        let saved = desc.save();
        desc.verify(&saved).expect("freshly saved hal2 value verifies");
        assert_eq!(desc.measure(), desc.signature());

        let mut t = saved.as_table().cloned().unwrap();
        let isr = t.remove("isr").unwrap();
        t.insert("isr_renamed".into(), isr);
        let err = desc.verify(&toml::Value::Table(t)).unwrap_err();
        assert!(err.contains("isr"), "names the field: {err}");
        assert!(err.contains("missing") || err.contains("unknown"), "{err}");
    }

    #[test]
    fn bres_modulus_zero_runs_at_the_master_rate() {
        // What IRIX writes for 44.1 kHz from the 44.1 kHz master, and for
        // 48 kHz from 48 kHz: both are the master rate.
        assert_eq!(bres_rate(44100, 0, 0xFFFF), 44100);
        assert_eq!(bres_rate(48000, 1, 0xFFFF), 48000);
        // inc 4, modulus 8 (modctrl = 4 - 8 - 1): half the master rate.
        assert_eq!(bres_rate(44100, 4, 4u16.wrapping_sub(8).wrapping_sub(1)), 22050);
        // No increment into a nonzero modulus never ticks.
        assert_eq!(bres_rate(48000, 0, 0u16.wrapping_sub(4).wrapping_sub(1)), 0);
    }

    fn resample(kind: ResamplerKind, in_rate: u32, out_rate: u32, n_frames: usize) -> usize {
        let mut r = make_resampler(kind, in_rate, out_rate);
        let mut out = Vec::new();
        for i in 0..n_frames {
            r.process(i as i16, i as i16, &mut out);
        }
        out.len() / 2 // stereo pairs → frames
    }

    /// The resampler option is a config choice; both implementations honour the
    /// same output count, so a swap does not change the stream rate.
    #[test]
    fn resampler_config_selects_an_implementation() {
        assert_eq!(AudioConfig::default().resampler, ResamplerKind::CatmullRom);
        let cat = make_resampler(ResamplerKind::CatmullRom, 44100, 48000);
        let sinc = make_resampler(ResamplerKind::Sinc, 44100, 48000);
        assert_eq!(cat.input_rate(), 44100);
        assert_eq!(cat.output_rate(), 48000);
        assert_eq!(sinc.input_rate(), 44100);
        assert_eq!(sinc.output_rate(), 48000);
        for kind in [ResamplerKind::CatmullRom, ResamplerKind::Sinc] {
            assert_eq!(resample(kind, 44100, 48000, 44100), 48000);
        }
    }

    #[test]
    fn resampler_passthrough() {
        // 1:1 — every input frame produces exactly one output frame
        assert_eq!(resample(ResamplerKind::CatmullRom, 44100, 44100, 1000), 1000);
    }

    #[test]
    fn resampler_downsample_2x() {
        // 44100 → 22050: every 2 inputs → 1 output, so 1000 in → 500 out
        let out = resample(ResamplerKind::CatmullRom, 44100, 22050, 1000);
        assert_eq!(out, 500, "44100→22050: expected 500 frames, got {}", out);
    }

    #[test]
    fn resampler_upsample_2x() {
        // 22050 → 44100: every input → 2 outputs, so 1000 in → 2000 out
        let out = resample(ResamplerKind::CatmullRom, 22050, 44100, 1000);
        assert_eq!(out, 2000, "22050→44100: expected 2000 frames, got {}", out);
    }

    #[test]
    fn resampler_upsample_44100_to_48000() {
        // 44100 → 48000: ratio ~1.0884, so 44100 in → 48000 out (over one second of audio)
        let out = resample(ResamplerKind::CatmullRom, 44100, 48000, 44100);
        assert_eq!(out, 48000, "44100→48000: expected 48000 frames, got {}", out);
    }

    /// A late wake must not move a burst: after a 10 ms stall at 11025 Hz
    /// the next call moves at most 1.25 periods' worth, and the backlog is
    /// still delivered over the following calls.
    #[test]
    fn pacer_spreads_catch_up_after_a_late_wake() {
        let mut p = Pacer::new(11025);
        p.start -= Duration::from_millis(10);
        let per_period = (11025 * PACE_PERIOD.as_micros() as u64).div_ceil(1_000_000);
        let first = p.due();
        assert!(first <= (per_period * 5).div_ceil(4).max(per_period + 1), "moved {} at once", first);
        let mut total = first;
        for _ in 0..200 { total += p.due(); }
        assert!(total >= 110, "backlog not delivered: {}", total);
    }

    /// The rate loop must follow the host callback, not the guest's fixed
    /// `Instant`: a consumer running faster or slower than the guest codec must
    /// not accumulate a produced-vs-consumed gap. Driven directly, with no
    /// audio hardware; wakes match `PACE_PERIOD`.
    #[test]
    fn rate_loop_corrects_a_fast_or_slow_consumer() {
        let guest = 44_100u32;
        let lead = 512u64;
        let cap = pace_wake_cap(guest as u64);
        let step = PACE_PERIOD;
        let steps = 20_000u64; // 5 s of 250 µs wakes
        // Consumer rates are multiples of 4000/s, so a 250 µs wake is a whole
        // number of frames and truncation cannot masquerade as drift.
        for &consumer in &[40_000u64, 44_000, 48_000, 52_000] {
            let mut ctl = RateCtl::new(guest);
            let mut now = ctl.start;
            let (mut consumed, mut produced) = (0u64, 0u64);
            for _ in 0..steps {
                now += step;
                consumed += consumer * step.as_nanos() as u64 / 1_000_000_000;
                let budget = ctl.budget(now, consumed, lead, cap);
                produced += budget;
                ctl.accounted(budget);
            }
            let gap = produced as i64 - consumed as i64;
            assert!(gap >= 0, "consumer {} Hz: producer fell behind the callback ({})", consumer, gap);
            assert!(
                gap <= (lead + 2 * cap) as i64,
                "consumer {} Hz: produced-consumed gap {} ran away from the {} frame lead",
                consumer, gap, lead
            );
        }
    }

    /// A host that stalls for longer than the reset window must not be sent a
    /// huge catch-up burst: the loop drops its origin and re-locks, exactly as
    /// QEMU's `RateCtl` reset does.
    #[test]
    fn rate_loop_resets_the_window_after_a_long_stall() {
        let guest = 44_100u32;
        let lead = 512u64;
        let cap = pace_wake_cap(guest as u64);
        let mut ctl = RateCtl::new(guest);
        let mut now = ctl.start;
        now += PACE_MAX_BACKLOG + Duration::from_millis(1000);
        // The host consumed nothing across the stall; the budget must stay
        // capped instead of replaying a second of audio at once.
        let budget = ctl.budget(now, 0, lead, cap);
        assert!(budget <= cap, "stall replayed as a {} frame burst", budget);
        assert_eq!(ctl.peeked, 0, "the drift window was not reset");
    }

    /// A 1 kHz tone at 11025 Hz, resampled to 48 kHz, against the ideal
    /// sine at the best-fitting delay. Repeating samples (the old resampler)
    /// misses by about a fifth of the amplitude; interpolation must be close.
    #[test]
    fn resampler_11025_to_48000_follows_the_waveform() {
        let (inr, outr, f, amp) = (11025u32, 48000u32, 1000.0f64, 16000.0f64);
        let n = 11025;
        let mut r = make_resampler(ResamplerKind::CatmullRom, inr, outr);
        let mut out = Vec::new();
        for k in 0..n {
            let v = (amp * (2.0 * std::f64::consts::PI * f * k as f64 / inr as f64).sin()) as i16;
            r.process(v, v, &mut out);
        }
        let left: Vec<f64> = out.iter().step_by(2).map(|&v| v as f64).collect();
        let body = &left[1000..left.len() - 1000];
        let mut best = f64::MAX;
        for step in 0..400 {
            let delay = step as f64 * 0.01; // input samples
            let mut err = 0.0;
            for (j, v) in body.iter().enumerate() {
                let t = (j + 1000) as f64 / outr as f64 - delay / inr as f64;
                let want = amp * (2.0 * std::f64::consts::PI * f * t).sin();
                err += (v - want) * (v - want);
            }
            best = best.min((err / body.len() as f64).sqrt() / amp);
        }
        assert!(best < 0.03, "RMS error {:.3} of the amplitude", best);
    }

    /// Magnitude of a single frequency in `sig` (sampled at `rate`) via a DFT.
    fn tone_magnitude(sig: &[f64], rate: f64, f: f64) -> f64 {
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (i, &v) in sig.iter().enumerate() {
            let ph = 2.0 * std::f64::consts::PI * f * i as f64 / rate;
            re += v * ph.cos();
            im += v * ph.sin();
        }
        (re * re + im * im).sqrt() * 2.0 / sig.len() as f64
    }

    /// Magnitude of the 3 kHz alias produced by downsampling a 5 kHz tone from
    /// 11025 Hz to 8000 Hz. 5 kHz is above the output's 4 kHz Nyquist, so a
    /// band-limited resampler must remove it before decimating.
    fn alias_magnitude(kind: ResamplerKind) -> f64 {
        let (inr, outr, f, amp, n) = (11025u32, 8000u32, 5000.0f64, 12000.0f64, 11025usize);
        let mut r = make_resampler(kind, inr, outr);
        let mut out = Vec::new();
        for k in 0..n {
            let v = (amp * (2.0 * std::f64::consts::PI * f * k as f64 / inr as f64).sin()) as i16;
            r.process(v, v, &mut out);
        }
        let left: Vec<f64> = out.iter().step_by(2).map(|&v| v as f64).collect();
        let body = &left[2000..]; // drop the filter transient
        tone_magnitude(body, outr as f64, 3000.0)
    }

    /// Catmull-Rom interpolates through the samples but does not band-limit, so
    /// a tone above the output Nyquist folds down; the windowed-sinc option
    /// suppresses it.
    #[test]
    fn sinc_resampler_suppresses_imaging_that_catmull_rom_leaves() {
        let cat = alias_magnitude(ResamplerKind::CatmullRom);
        let sinc = alias_magnitude(ResamplerKind::Sinc);
        assert!(cat > 300.0, "catmull should alias a 5 kHz tone; got {:.0}", cat);
        assert!(
            sinc < 0.25 * cat,
            "sinc should suppress the alias: cat={:.0} sinc={:.0}",
            cat, sinc
        );
    }

    #[test]
    fn resampler_downsample_48000_to_44100() {
        // 48000 → 44100: ratio ~0.919, so 48000 in → 44100 out
        let out = resample(ResamplerKind::CatmullRom, 48000, 44100, 48000);
        assert_eq!(out, 44100, "48000→44100: expected 44100 frames, got {}", out);
    }
}

#[cfg(test)]
mod clkid_tests {
    use super::*;

    /// Three generators at distinguishable rates, so an off-by-one shows up as
    /// a wrong number rather than a coincidence.
    fn state_with_rates() -> Hal2State {
        let mut s = Hal2State::default();
        s.bres_clock_rate = [48000, 44100, 32000];
        s
    }

    #[test]
    fn clkid_is_a_generator_number_not_an_index() {
        let s = state_with_rates();
        // CLKID n selects BRESn, so it indexes the array at n-1.
        assert_eq!(s.bres_rate(1), 48000, "CLKID 1 is BRES1");
        assert_eq!(s.bres_rate(2), 44100, "CLKID 2 is BRES2");
        assert_eq!(s.bres_rate(3), 32000, "CLKID 3 is BRES3");
        // 0 selects no generator — reporting a rate here is inventing one.
        assert_eq!(s.bres_rate(0), 0, "CLKID 0 selects nothing");
    }

    #[test]
    fn a_codec_reads_the_generator_its_driver_programmed() {
        // What NetBSD's haltwo and Linux's hal2 both do for playback: program
        // BRES1 to the wanted rate, then point the DAC at it with CLKID 1.
        // ctrl1 = 0x0208 is what NetBSD 10.2 and 11.0 actually write.
        let mut s = state_with_rates();
        s.codeca_ctrl[0] = 0x0208;
        let (_, clk, _) = s.codeca_cfg();
        assert_eq!(clk, 1, "ctrl1 0x0208 carries CLKID 1");
        assert_eq!(
            s.bres_rate(clk),
            48000,
            "codec A must read BRES1, the generator the driver configured"
        );
    }

    #[test]
    fn the_adc_reads_its_own_generator_too() {
        // Linux records on BRES2 with CLKID 2 ("2nd Bresenham clock generator
        // for record"), which must not resolve to BRES3.
        let mut s = state_with_rates();
        s.codecb_ctrl[0] = (2 << CTRL1_CLOCK_SHIFT) as u16;
        let (_, clk, _) = s.codecb_cfg();
        assert_eq!(clk, 2);
        assert_eq!(s.bres_rate(clk), 44100, "CLKID 2 is BRES2, not BRES3");
    }
}
