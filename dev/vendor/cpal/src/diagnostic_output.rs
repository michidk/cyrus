//! Opt-in Linux output-callback evidence. Filesystem delivery never timestamps generation.
use std::{
    fs::{File, OpenOptions},
    io::Write,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, SyncSender},
        Arc, OnceLock,
    },
    thread,
};

/// One completed output callback, before submission to the audio device.
#[derive(Clone, Debug)]
pub struct Cursor {
    /// Completed stereo frames since this stream began.
    pub frames: u64,
    /// Completed callback count, starting at one.
    pub sequence: u64,
    /// CLOCK_MONOTONIC nanoseconds immediately after the callback returns.
    pub monotonic_ns: u64,
    /// Device sample frames per second.
    pub rate: u32,
    /// Exact interleaved stereo sample encoding.
    pub format: &'static str,
}
struct State {
    revision: AtomicU64,
    frames: AtomicU64,
    sequence: AtomicU64,
    monotonic_ns: AtomicU64,
    rate: u32,
    format: &'static str,
    failed: AtomicBool,
}
static CURRENT: OnceLock<Arc<State>> = OnceLock::new();
/// Returns no cursor after overflow, writer failure, teardown or before the first callback.
pub fn cursor() -> Option<Cursor> {
    let state = CURRENT.get()?;
    if state.failed.load(Ordering::SeqCst) {
        return None;
    }
    let revision = state.revision.load(Ordering::SeqCst);
    if revision & 1 != 0 {
        return None;
    }
    let cursor = Cursor {
        frames: state.frames.load(Ordering::SeqCst),
        sequence: state.sequence.load(Ordering::SeqCst),
        monotonic_ns: state.monotonic_ns.load(Ordering::SeqCst),
        rate: state.rate,
        format: state.format,
    };
    (cursor.sequence > 0
        && revision == state.revision.load(Ordering::SeqCst)
        && !state.failed.load(Ordering::SeqCst))
    .then_some(cursor)
}
struct Block {
    cursor: Cursor,
    first: u64,
    bytes: Vec<u8>,
}
pub(crate) struct Capture {
    sender: Option<SyncSender<Block>>,
    state: Arc<State>,
    worker: Option<thread::JoinHandle<()>>,
    rate: u32,
    format: &'static str,
    frame_bytes: usize,
    frames: u64,
    sequence: u64,
}
impl Capture {
    pub(crate) fn open(rate: u32, channels: u16, format: crate::SampleFormat) -> Option<Self> {
        let directory = std::env::var_os("CYRUS_MIXER_CAPTURE_DIR")?;
        let (format, frame_bytes) = match (channels, format) {
            (2, crate::SampleFormat::I16) if cfg!(target_endian = "little") => ("s16le", 4),
            (2, crate::SampleFormat::F32) if cfg!(target_endian = "little") => ("f32le", 8),
            _ => ("unsupported", 0),
        };
        let state = Arc::new(State {
            revision: AtomicU64::new(0),
            frames: AtomicU64::new(0),
            sequence: AtomicU64::new(0),
            monotonic_ns: AtomicU64::new(0),
            rate,
            format,
            failed: AtomicBool::new(frame_bytes == 0),
        });
        if CURRENT.set(state.clone()).is_err() {
            CURRENT.get()?.failed.store(true, Ordering::SeqCst);
            return None;
        }
        if frame_bytes == 0 {
            return None;
        }
        let open = |name| {
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(std::path::Path::new(&directory).join(name))
        };
        let files = open("generated.raw")
            .and_then(|pcm| open("generated.jsonl").map(|events| (pcm, events)));
        let Ok((mut pcm, mut events)): Result<(File, File), _> = files else {
            state.failed.store(true, Ordering::SeqCst);
            return None;
        };
        let (sender, receiver) = mpsc::sync_channel::<Block>(64);
        let delivery = state.clone();
        let worker = thread::Builder::new().name("pcm-capture".into()).spawn(move || {
            for block in receiver {
                let c = block.cursor;
                if pcm.write_all(&block.bytes).and_then(|_| pcm.flush()).is_err() ||
                    writeln!(events,"{{\"sequence\":{},\"first\":{},\"last\":{},\"monotonic_ns\":{},\"rate\":{},\"format\":\"{}\"}}",c.sequence,block.first,c.frames,c.monotonic_ns,c.rate,c.format).and_then(|_|events.flush()).is_err() {
                    delivery.failed.store(true,Ordering::SeqCst);
                }
            }
            if delivery.failed.load(Ordering::SeqCst) {
                let _ = writeln!(
                    events,
                    "{{\"error\":\"generated PCM discontinuity or delivery failure\"}}"
                );
            }
        });
        let Ok(worker) = worker else {
            state.failed.store(true, Ordering::SeqCst);
            return None;
        };
        Some(Self {
            sender: Some(sender),
            state,
            worker: Some(worker),
            rate,
            format,
            frame_bytes,
            frames: 0,
            sequence: 0,
        })
    }
    pub(crate) fn generated(&mut self, bytes: &[u8]) {
        if self.state.failed.load(Ordering::SeqCst) {
            return;
        }
        let mut now = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // CLOCK_MONOTONIC shares the original capture's clock domain on Linux.
        if unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut now) } != 0
            || bytes.is_empty()
            || bytes.len() % self.frame_bytes != 0
        {
            self.state.failed.store(true, Ordering::SeqCst);
            return;
        }
        let first = self.frames;
        self.frames += (bytes.len() / self.frame_bytes) as u64;
        self.sequence += 1;
        let cursor = Cursor {
            frames: self.frames,
            sequence: self.sequence,
            monotonic_ns: now.tv_sec as u64 * 1_000_000_000 + now.tv_nsec as u64,
            rate: self.rate,
            format: self.format,
        };
        if self
            .sender
            .as_ref()
            .unwrap()
            .try_send(Block {
                cursor: cursor.clone(),
                first,
                bytes: bytes.to_vec(),
            })
            .is_err()
        {
            self.state.failed.store(true, Ordering::SeqCst);
            return;
        }
        self.state.revision.fetch_add(1, Ordering::SeqCst);
        self.state.frames.store(cursor.frames, Ordering::SeqCst);
        self.state.sequence.store(cursor.sequence, Ordering::SeqCst);
        self.state
            .monotonic_ns
            .store(cursor.monotonic_ns, Ordering::SeqCst);
        self.state.revision.fetch_add(1, Ordering::SeqCst);
    }
}
impl Drop for Capture {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        self.state.failed.store(true, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn completed_callback_precedes_delivery_and_overflow_invalidates_receipt() {
        let state = Arc::new(State {
            revision: AtomicU64::new(0),
            frames: AtomicU64::new(0),
            sequence: AtomicU64::new(0),
            monotonic_ns: AtomicU64::new(0),
            rate: 48000,
            format: "f32le",
            failed: AtomicBool::new(false),
        });
        assert!(CURRENT.set(state.clone()).is_ok());
        let (sender, receiver) = mpsc::sync_channel(1);
        let mut capture = Capture {
            sender: Some(sender),
            state,
            worker: None,
            rate: 48000,
            format: "f32le",
            frame_bytes: 8,
            frames: 0,
            sequence: 0,
        };
        assert!(cursor().is_none());
        let bytes = [0u8; 3840];
        capture.generated(&bytes);
        // No worker has delivered anything, but the generation prefix is public.
        let first = cursor().unwrap();
        assert_eq!(
            (first.frames, first.sequence, first.rate, first.format),
            (480, 1, 48000, "f32le")
        );
        assert!(first.monotonic_ns > 0);
        // A stalled delivery queue must never block production or hide loss.
        capture.generated(&bytes);
        assert!(cursor().is_none());
        let block = receiver.recv().unwrap();
        assert_eq!(block.bytes, bytes);
        assert_eq!(block.cursor.monotonic_ns, first.monotonic_ns);
    }
}
