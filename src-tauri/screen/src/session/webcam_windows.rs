//! The Windows webcam producer (F-22): an `IMFSourceReader` on the chosen
//! device feeding its own video-only fragmented-MP4 sink.
//!
//! TWO threads, both named, for the three-threads rule's own reason
//! (`session/mod.rs`): an `IMFSample` is a COM pointer and not `Send`, so
//! `screen-webcam` reads, maps the sample time onto the shared clock and
//! converts to NV12 bytes, and `screen-webcam-mux` is the only thread that
//! touches the webcam's sink. A bounded channel sits between them; a full
//! channel DROPS a frame (counted, rate-limited in the log) rather than
//! blocking the reader behind a fragment write.
//!
//! EVERY decision is made by a pure function in `session/webcam.rs`. This
//! file moves bytes between Media Foundation and those functions and
//! EXECUTES IN NO AUTOMATED TEST on any platform: the tutorial editor's
//! Windows checklist rows T37-T41 are its gate.
//!
//! Spec 14's posture: a webcam that vanishes mid-capture raises a
//! `screen:warning`, its file finalizes with what it holds, and the screen
//! capture carries on. The sink is NEVER flushed — `IMFSinkWriter::Flush`
//! discards pending samples; `FragmentedSink::finalize` is the only drain.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TryRecvError, TrySendError};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use vault_buddy_core::capture_paths;
use windows::core::GUID;
use windows::Win32::Media::MediaFoundation::{
    IMFAttributes, IMFMediaSource, IMFMediaType, IMFSample, IMFSourceReader, MFCreateAttributes,
    MFCreateMediaType, MFCreateSourceReaderFromMediaSource, MFMediaType_Video, MFShutdown,
    MFStartup, MFVideoFormat_NV12, MFVideoFormat_YUY2, MFSTARTUP_FULL, MF_MT_FRAME_RATE,
    MF_MT_FRAME_SIZE, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MF_SOURCE_READERF_ENDOFSTREAM,
    MF_SOURCE_READERF_ERROR, MF_SOURCE_READER_ALL_STREAMS,
    MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, MF_SOURCE_READER_FIRST_VIDEO_STREAM, MF_VERSION,
};

use crate::sink::{FragmentedSink, VideoFormat};
use crate::source::WebcamDeviceId;
use crate::source_webcam::{enumerate, ComScope};
use crate::ScreenError;

use super::pacing;
use super::webcam::{
    choose_webcam_mode, webcam_frame_to_nv12, webcam_video_format, WebcamClockMap, WebcamMode,
    WebcamOutcome, WebcamPacer, WebcamParams, WebcamPixels,
};
use super::{output_ts, SharedClock, Warnings};

/// Frames in flight between the reader and the webcam's mux.
const CHANNEL_DEPTH: usize = 8;
/// Opening a device and reading its FIRST frame (a camera warms up for up
/// to a second or two). Inside the shell's 15 s ready handshake with room
/// for the screen's own start.
const OPEN_TIMEOUT: Duration = Duration::from_secs(8);
/// How long a stop waits for the reader to leave `ReadSample`. A device that
/// stalls without erroring is left to finish on its own; the mux does not
/// wait on it (`STOP_GRACE`).
const READER_GRACE: Duration = Duration::from_secs(2);
const STOP_GRACE: Duration = Duration::from_secs(2);
const MUX_WAIT: Duration = Duration::from_millis(100);
/// A bound on native-type enumeration, so a misbehaving driver that never
/// answers `MF_E_NO_MORE_TYPES` cannot loop forever.
const MAX_NATIVE_TYPES: u32 = 512;

const FIRST_VIDEO: u32 = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
const ALL_STREAMS: u32 = MF_SOURCE_READER_ALL_STREAMS.0 as u32;

const BUSY: &str = "The webcam could not be opened. It may be in use by another app: close \
                    that app, or choose No webcam.";
const GONE: &str = "The chosen webcam is no longer connected. Pick it again, or choose No webcam.";
const VANISHED: &str = "The webcam stopped delivering video. The screen capture continues, and \
                        the webcam footage up to this point is kept.";

/// What the reader reports once the device delivered its first frame.
struct Opened {
    mode: WebcamMode,
    pixels: WebcamPixels,
    label: String,
}

struct Frame {
    nv12: Vec<u8>,
    clock_ts: Duration,
}

/// What the mux wrote: the first frame's clock time and the file's end.
struct Written {
    offset: Duration,
    until: Duration,
}

/// Media Foundation on the reader thread, paired with its shutdown.
struct MfScope;

impl MfScope {
    fn start() -> Result<MfScope, ScreenError> {
        unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL) }.map_err(|e| {
            log::error!("screen webcam: MFStartup failed: {e}");
            ScreenError::Sink(format!("Media Foundation could not start: {e}"))
        })?;
        Ok(MfScope)
    }
}

impl Drop for MfScope {
    fn drop(&mut self) {
        if let Err(e) = unsafe { MFShutdown() } {
            log::warn!("screen webcam: MFShutdown failed: {e}");
        }
    }
}

/// A webcam recording beside a screen capture.
pub(crate) struct WebcamProducer {
    stop: Arc<AtomicBool>,
    go_tx: Option<mpsc::Sender<SharedClock>>,
    /// Disconnects when the reader thread ends (it owns the sender).
    reader_done: Receiver<()>,
    reader: Option<JoinHandle<()>>,
    mux: Option<JoinHandle<Result<Written, ScreenError>>>,
    frames: Arc<AtomicU64>,
    part: PathBuf,
    staged: PathBuf,
    width: u32,
    height: u32,
    label: String,
}

impl WebcamProducer {
    /// Open the device, read its first frame and open the webcam's sink —
    /// BEFORE the screen's clock starts, so a camera another app holds
    /// refuses the whole start while the user can still act on it. Frames
    /// are read and discarded until `go`, so nothing stale sits in the
    /// reader's queue to be anchored late.
    pub(crate) fn open(
        params: WebcamParams,
        warnings: Arc<Warnings>,
    ) -> Result<WebcamProducer, ScreenError> {
        let WebcamParams {
            device,
            part,
            staged,
            quality,
        } = params;
        let stop = Arc::new(AtomicBool::new(false));
        let frames = Arc::new(AtomicU64::new(0));
        let (tx, rx) = mpsc::sync_channel::<Frame>(CHANNEL_DEPTH);
        let (open_tx, open_rx) = mpsc::channel::<Result<Opened, ScreenError>>();
        let (go_tx, go_rx) = mpsc::channel::<SharedClock>();
        let (done_tx, done_rx) = mpsc::channel::<()>();
        let reader = std::thread::Builder::new()
            .name("screen-webcam".into())
            .spawn({
                let (stop, warnings) = (Arc::clone(&stop), Arc::clone(&warnings));
                move || {
                    let _done = done_tx;
                    run_reader(&device, open_tx, go_rx, tx, &stop, &warnings);
                }
            })
            .map_err(|e| {
                log::error!("screen webcam: could not start the reader thread: {e}");
                ScreenError::Io(format!("could not start the webcam thread: {e}"))
            })?;
        let mut producer = WebcamProducer {
            stop,
            go_tx: Some(go_tx),
            reader_done: done_rx,
            reader: Some(reader),
            mux: None,
            frames,
            part,
            staged,
            width: 0,
            height: 0,
            label: String::new(),
        };
        // Every early return below drops `producer`, whose Drop stops the
        // reader and finalizes whatever sink exists.
        let opened = match open_rx.recv_timeout(OPEN_TIMEOUT) {
            Ok(Ok(opened)) => opened,
            Ok(Err(e)) => return Err(e),
            Err(_) => {
                log::warn!(
                    "screen webcam: the device did not deliver a frame within {OPEN_TIMEOUT:?}"
                );
                return Err(ScreenError::Refused(BUSY.into()));
            }
        };
        let format = webcam_video_format(opened.mode, quality).ok_or_else(|| {
            log::warn!("screen webcam: unusable mode {:?}", opened.mode);
            ScreenError::Refused(BUSY.into())
        })?;
        producer.width = format.width;
        producer.height = format.height;
        producer.label = opened.label;

        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), ScreenError>>();
        let mux = std::thread::Builder::new()
            .name("screen-webcam-mux".into())
            .spawn({
                let (part, stop, frames) = (
                    producer.part.clone(),
                    Arc::clone(&producer.stop),
                    Arc::clone(&producer.frames),
                );
                move || run_mux(part, format, ready_tx, rx, &stop, &frames, &warnings)
            })
            .map_err(|e| {
                log::error!("screen webcam: could not start the writer thread: {e}");
                ScreenError::Io(format!("could not start the webcam writer: {e}"))
            })?;
        producer.mux = Some(mux);
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(producer),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(ScreenError::Sink(
                "the webcam writer could not be started".into(),
            )),
        }
    }

    /// Hand over the shared clock: from here on frames are stamped and kept.
    pub(crate) fn go(&mut self, clock: SharedClock) {
        if let Some(tx) = self.go_tx.take() {
            if tx.send(clock).is_err() {
                log::warn!("screen webcam: the reader ended before the capture began");
            }
        }
    }

    fn release_reader(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.go_tx = None;
        let Some(reader) = self.reader.take() else {
            return;
        };
        match self.reader_done.recv_timeout(READER_GRACE) {
            Err(RecvTimeoutError::Timeout) => log::warn!(
                "screen webcam: the reader did not leave ReadSample within {READER_GRACE:?}; \
                 it is left to finish on its own"
            ),
            _ => {
                if reader.join().is_err() {
                    log::error!("screen webcam: the reader thread panicked");
                }
            }
        }
    }

    fn retained(&self, cause: ScreenError) -> ScreenError {
        ScreenError::Retained {
            path: self.part.clone(),
            holds_footage: self.frames.load(Ordering::Relaxed) > 0,
            cause: Box::new(cause),
        }
    }

    /// Stop, FINALIZE, then publish — the screen session's own order. A
    /// failure leaves the `.part` where it is (the recovery sweep promotes a
    /// webcam part that holds footage) and says so as `Retained`.
    pub(crate) fn stop(mut self) -> Result<WebcamOutcome, ScreenError> {
        self.release_reader();
        let written = match self.mux.take().map(JoinHandle::join) {
            Some(Ok(Ok(written))) => written,
            Some(Ok(Err(e))) => return Err(self.retained(e)),
            Some(Err(_)) => {
                log::error!("screen webcam: the writer thread panicked");
                return Err(self.retained(ScreenError::Sink(
                    "the webcam writer stopped unexpectedly".into(),
                )));
            }
            None => return Err(ScreenError::Sink("the webcam was already stopped".into())),
        };
        if let Err(e) = capture_paths::rename_noreplace(&self.part, &self.staged) {
            log::error!(
                "screen webcam: could not publish {:?} as {:?}: {e}; the part is kept",
                self.part,
                self.staged
            );
            return Err(self.retained(ScreenError::Io(format!(
                "could not finish the webcam file: {e}"
            ))));
        }
        Ok(WebcamOutcome {
            file: self.staged.clone(),
            width: self.width,
            height: self.height,
            device_label: self.label.clone(),
            offset_ms: i64::try_from(written.offset.as_millis()).unwrap_or(i64::MAX),
            duration_ms: u64::try_from(written.until.as_millis()).unwrap_or(u64::MAX),
        })
    }
}

impl Drop for WebcamProducer {
    /// A producer dropped without `stop` (a failed start, an abandoned
    /// session) still finalizes, and leaves its `.part` for recovery.
    fn drop(&mut self) {
        self.release_reader();
        if let Some(mux) = self.mux.take() {
            match mux.join() {
                Ok(Ok(_)) => {}
                Ok(Err(e)) => log::warn!("screen webcam: an abandoned webcam track: {e}"),
                Err(_) => log::error!("screen webcam: the writer thread panicked"),
            }
        }
    }
}

fn run_reader(
    device: &WebcamDeviceId,
    open_tx: mpsc::Sender<Result<Opened, ScreenError>>,
    go_rx: Receiver<SharedClock>,
    tx: SyncSender<Frame>,
    stop: &AtomicBool,
    warnings: &Warnings,
) {
    let _com = ComScope::enter();
    let _mf = match MfScope::start() {
        Ok(mf) => mf,
        Err(e) => {
            let _ = open_tx.send(Err(e));
            return;
        }
    };
    let (reader, source, opened) = match open_device(device) {
        Ok(opened) => opened,
        Err(e) => {
            let _ = open_tx.send(Err(e));
            return;
        }
    };
    let (mode, pixels) = (opened.mode, opened.pixels);
    // The first real frame before reporting success: a camera another app
    // holds often OPENS fine and fails its first read (checklist T39).
    let first_read = read_one(&reader);
    let ready = match first_read {
        Ok(_) => open_tx.send(Ok(opened)).is_ok(),
        Err(cause) => {
            log::warn!("screen webcam: the device opened but delivered nothing: {cause}");
            let _ = open_tx.send(Err(ScreenError::Refused(BUSY.into())));
            false
        }
    };
    if ready {
        if let Some(clock) = warm_up(&reader, &go_rx, stop, warnings) {
            stream(&reader, &clock, (mode, pixels), &tx, stop, warnings);
        }
    }
    if let Err(e) = unsafe { source.Shutdown() } {
        log::warn!("screen webcam: shutting the device down failed: {e}");
    }
}

/// Read and discard until the session hands over its clock.
fn warm_up(
    reader: &IMFSourceReader,
    go_rx: &Receiver<SharedClock>,
    stop: &AtomicBool,
    warnings: &Warnings,
) -> Option<SharedClock> {
    loop {
        if stop.load(Ordering::Relaxed) {
            return None;
        }
        match go_rx.try_recv() {
            Ok(clock) => return Some(clock),
            Err(TryRecvError::Disconnected) => return None,
            Err(TryRecvError::Empty) => {}
        }
        if let Err(cause) = read_one(reader) {
            log::warn!("screen webcam: {cause}");
            warnings.raise(VANISHED.into());
            return None;
        }
    }
}

fn stream(
    reader: &IMFSourceReader,
    clock: &SharedClock,
    (mode, pixels): (WebcamMode, WebcamPixels),
    tx: &SyncSender<Frame>,
    stop: &AtomicBool,
    warnings: &Warnings,
) {
    let mut map = WebcamClockMap::new();
    let mut dropped: u64 = 0;
    while !stop.load(Ordering::Relaxed) {
        let (hns, sample) = match read_one(reader) {
            Ok(Some(read)) => read,
            // A stream tick: a gap with no sample, nothing to write.
            Ok(None) => continue,
            Err(cause) => {
                log::warn!("screen webcam: {cause}");
                warnings.raise(VANISHED.into());
                return;
            }
        };
        // Paused: drained and discarded (spec 6.3).
        let Some(clock_ts) = map.map(hns, output_ts(clock)) else {
            continue;
        };
        let nv12 = match sample_nv12(&sample, pixels, mode.width, mode.height) {
            Ok(nv12) => nv12,
            Err(cause) => {
                dropped += 1;
                if pacing::should_log_drop(dropped) {
                    log::warn!("screen webcam: dropped frame {dropped}: {cause}");
                }
                continue;
            }
        };
        match tx.try_send(Frame { nv12, clock_ts }) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                dropped += 1;
                if pacing::should_log_drop(dropped) {
                    log::warn!("screen webcam: dropped frame {dropped}: the writer is behind");
                }
            }
            Err(TrySendError::Disconnected(_)) => return,
        }
    }
}

/// One `ReadSample`: `Ok(None)` is a stream tick, `Err` is a device that is
/// gone (an error, or the end of its stream).
fn read_one(reader: &IMFSourceReader) -> Result<Option<(i64, IMFSample)>, String> {
    let mut flags = 0u32;
    let mut ts = 0i64;
    let mut sample: Option<IMFSample> = None;
    unsafe {
        reader.ReadSample(
            FIRST_VIDEO,
            0,
            None,
            Some(&mut flags as *mut u32),
            Some(&mut ts as *mut i64),
            Some(&mut sample as *mut Option<IMFSample>),
        )
    }
    .map_err(|e| format!("reading a webcam frame failed: {e}"))?;
    let gone = MF_SOURCE_READERF_ERROR.0 | MF_SOURCE_READERF_ENDOFSTREAM.0;
    if flags as i32 & gone != 0 {
        return Err(format!("the webcam ended its stream (flags {flags:#x})"));
    }
    Ok(sample.map(|s| (ts, s)))
}

fn sample_nv12(
    sample: &IMFSample,
    pixels: WebcamPixels,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, String> {
    unsafe {
        let buffer = sample
            .ConvertToContiguousBuffer()
            .map_err(|e| format!("reading a frame buffer: {e}"))?;
        let mut data: *mut u8 = std::ptr::null_mut();
        let mut len = 0u32;
        buffer
            .Lock(&mut data, None, Some(&mut len as *mut u32))
            .map_err(|e| format!("locking a frame buffer: {e}"))?;
        let out = if data.is_null() {
            Err("an empty frame buffer".to_string())
        } else {
            let bytes = std::slice::from_raw_parts(data, len as usize);
            webcam_frame_to_nv12(pixels, bytes, width, height).map_err(|e| e.to_string())
        };
        if let Err(e) = buffer.Unlock() {
            log::warn!("screen webcam: unlocking a frame buffer failed: {e}");
        }
        out
    }
}

fn open_device(
    device: &WebcamDeviceId,
) -> Result<(IMFSourceReader, IMFMediaSource, Opened), ScreenError> {
    let found = enumerate()
        .into_iter()
        .find(|d| &d.id == device)
        .ok_or_else(|| {
            log::warn!("screen webcam: {device} is not connected");
            ScreenError::Refused(GONE.into())
        })?;
    let source: IMFMediaSource = unsafe { found.activate.ActivateObject() }.map_err(|e| {
        log::warn!("screen webcam: activating {device} failed: {e}");
        ScreenError::Refused(BUSY.into())
    })?;
    match unsafe { configure(&source) } {
        Ok((reader, mode, pixels)) => Ok((
            reader,
            source,
            Opened {
                mode,
                pixels,
                label: found.label,
            },
        )),
        Err(e) => {
            if let Err(shut) = unsafe { source.Shutdown() } {
                log::warn!("screen webcam: shutting the device down failed: {shut}");
            }
            Err(e)
        }
    }
}

/// Build the reader and ask for NV12, then YUY2, at the chosen native mode.
/// Video processing lets the reader decode an MJPG camera into either.
unsafe fn configure(
    source: &IMFMediaSource,
) -> Result<(IMFSourceReader, WebcamMode, WebcamPixels), ScreenError> {
    let mf = |what: &str, e: windows::core::Error| {
        log::warn!("screen webcam: {what}: {e}");
        ScreenError::Refused(BUSY.into())
    };
    let mut attrs: Option<IMFAttributes> = None;
    MFCreateAttributes(&mut attrs, 1).map_err(|e| mf("reader attributes", e))?;
    let attrs = attrs.ok_or_else(|| ScreenError::Refused(BUSY.into()))?;
    attrs
        .SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1)
        .map_err(|e| mf("reader attributes", e))?;
    let reader = MFCreateSourceReaderFromMediaSource(source, &attrs)
        .map_err(|e| mf("create the source reader", e))?;
    if let Err(e) = reader.SetStreamSelection(ALL_STREAMS, false) {
        log::debug!("screen webcam: deselecting streams: {e}");
    }
    reader
        .SetStreamSelection(FIRST_VIDEO, true)
        .map_err(|e| mf("select the video stream", e))?;
    let natives: Vec<IMFMediaType> = (0..MAX_NATIVE_TYPES)
        .map_while(|i| reader.GetNativeMediaType(FIRST_VIDEO, i).ok())
        .collect();
    let modes: Vec<WebcamMode> = natives.iter().map(|t| mode_of(t)).collect();
    let chosen = choose_webcam_mode(&modes).map(|i| &natives[i]);
    let accepted = [MFVideoFormat_NV12, MFVideoFormat_YUY2]
        .iter()
        .any(|subtype| match request(&reader, chosen, subtype) {
            Ok(()) => true,
            Err(e) => {
                log::debug!("screen webcam: output {subtype:?} refused: {e}");
                false
            }
        });
    if !accepted {
        log::warn!("screen webcam: the device offers neither NV12 nor YUY2");
        return Err(ScreenError::Refused(
            "This webcam offers no video format Vault Buddy can record. Choose No webcam.".into(),
        ));
    }
    let current = reader
        .GetCurrentMediaType(FIRST_VIDEO)
        .map_err(|e| mf("read the chosen format", e))?;
    let subtype = current
        .GetGUID(&MF_MT_SUBTYPE)
        .map_err(|e| mf("read the chosen format", e))?;
    let pixels = if subtype == MFVideoFormat_NV12 {
        WebcamPixels::Nv12
    } else if subtype == MFVideoFormat_YUY2 {
        WebcamPixels::Yuy2
    } else {
        log::warn!("screen webcam: the reader settled on {subtype:?}");
        return Err(ScreenError::Refused(BUSY.into()));
    };
    Ok((reader, mode_of(&current), pixels))
}

/// `MF_MT_FRAME_SIZE`/`MF_MT_FRAME_RATE` pack two u32s high-then-low. A
/// missing attribute reads as zero, which `choose_webcam_mode` and
/// `webcam_video_format` treat as unknown.
unsafe fn mode_of(t: &IMFMediaType) -> WebcamMode {
    let size = t.GetUINT64(&MF_MT_FRAME_SIZE).unwrap_or(0);
    let rate = t.GetUINT64(&MF_MT_FRAME_RATE).unwrap_or(0);
    WebcamMode {
        width: (size >> 32) as u32,
        height: size as u32,
        fps_num: (rate >> 32) as u32,
        fps_den: rate as u32,
    }
}

unsafe fn request(
    reader: &IMFSourceReader,
    native: Option<&IMFMediaType>,
    subtype: &GUID,
) -> windows::core::Result<()> {
    let wanted = MFCreateMediaType()?;
    match native {
        Some(native) => native.CopyAllItems(&wanted)?,
        None => wanted.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?,
    }
    wanted.SetGUID(&MF_MT_SUBTYPE, subtype)?;
    reader.SetCurrentMediaType(FIRST_VIDEO, None, &wanted)
}

fn put(
    sink: &mut FragmentedSink,
    (nv12, ts, dur): (Vec<u8>, Duration, Duration),
    until: &mut Duration,
    frames: &AtomicU64,
) -> Result<(), ScreenError> {
    sink.write_video(&nv12, ts, dur)?;
    *until = (*until).max(ts + dur);
    frames.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// The webcam's mux: the ONLY owner of its sink, created here because the
/// sink writer is a COM pointer and not `Send`.
fn run_mux(
    part: PathBuf,
    format: VideoFormat,
    ready_tx: mpsc::Sender<Result<(), ScreenError>>,
    rx: Receiver<Frame>,
    stop: &AtomicBool,
    frames: &AtomicU64,
    warnings: &Warnings,
) -> Result<Written, ScreenError> {
    // Video-only: the capture's sound lives on the screen track (ADR 4).
    let mut sink = match FragmentedSink::create(&part, format, None) {
        Ok(sink) => sink,
        Err(e) => {
            let _ = ready_tx.send(Err(e.clone()));
            return Err(e);
        }
    };
    if ready_tx.send(Ok(())).is_err() {
        log::warn!("screen webcam: nobody waited for the writer; closing it");
    }
    let mut pacer = WebcamPacer::new(format.fps);
    let mut until = Duration::ZERO;
    let mut stop_seen: Option<Instant> = None;
    let result = loop {
        match rx.recv_timeout(MUX_WAIT) {
            Ok(Frame { nv12, clock_ts }) => {
                if let Some(out) = pacer.push(nv12, clock_ts) {
                    if let Err(e) = put(&mut sink, out, &mut until, frames) {
                        break Err(e);
                    }
                }
            }
            Err(RecvTimeoutError::Disconnected) => break Ok(()),
            Err(RecvTimeoutError::Timeout) => {
                if stop.load(Ordering::Relaxed)
                    && stop_seen.get_or_insert_with(Instant::now).elapsed() >= STOP_GRACE
                {
                    log::warn!("screen webcam: the reader never released the channel; finalizing");
                    break Ok(());
                }
            }
        }
    };
    let result = result.and_then(|()| match pacer.finish() {
        Some(out) => put(&mut sink, out, &mut until, frames),
        None => Ok(()),
    });
    if let Err(e) = &result {
        warnings.raise(format!("the webcam recording stopped writing early: {e}"));
    }
    let finalized = sink.finalize();
    if frames.load(Ordering::Relaxed) == 0 {
        if let Err(e) = &finalized {
            log::warn!("screen webcam: finalizing an empty webcam file: {e}");
        }
        return Err(ScreenError::Sink("the webcam delivered no video".into()));
    }
    finalized.map(|()| Written {
        offset: pacer.offset().unwrap_or_default(),
        until,
    })
}
