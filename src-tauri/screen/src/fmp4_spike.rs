//! Phase 2's gating spike (spec §6.4): can `IMFSinkWriter` produce fragmented
//! MP4, and — the question that actually decides the design — is a capture
//! that never reached `Finalize` still playable?
//!
//! Why this is not answerable from documentation: `MFCreateFMPEG4MediaSink`
//! is documented as "creates a media sink for authoring fragmented MP4
//! files", which settles that fragments *can* be emitted. It says nothing
//! about the crash case, and the crash case is the entire reason §6.4 chose
//! fMP4 over a standard MP4. A standard MP4 writes its `moov` index at
//! finalize, so a crashed capture is unopenable; the claim under test is
//! that a fragmented one degrades to a playable prefix instead.
//!
//! The spike is built so the *analysis* is pure and runs on Linux
//! (`crate::mp4_boxes`), leaving only "produce the bytes" here. It is gated
//! behind the non-default `fmp4-spike` feature so the ordinary build — and
//! every phase 1 gate — stays free of the `windows` dependency until phase 2
//! genuinely adopts it.
//!
//! Run it on Windows with:
//!   cargo test -p vault_buddy_screen --features fmp4-spike -- --nocapture
//!
//! The `--nocapture` matters: the tests print the observed box layout, which
//! is the evidence the spike exists to produce.

#![cfg(all(windows, feature = "fmp4-spike"))]

use std::path::Path;

use windows::core::{Result as WinResult, HSTRING};
use windows::Win32::Media::MediaFoundation::*;

const WIDTH: u32 = 640;
const HEIGHT: u32 = 360;
const FPS: u32 = 30;
/// 3 seconds. Long enough that a ~1 s fragment cadence must close several
/// fragments; short enough to keep a CI runner honest.
const FRAMES: u32 = 90;
const BITRATE: u32 = 2_000_000;
/// 100-nanosecond units per second — Media Foundation's time base.
const HNS_PER_SEC: i64 = 10_000_000;

/// Which sink to build. The standard variant is the CONTROL: without it, a
/// successful read of the fragmented file proves nothing, because we would
/// not have shown that an un-finalized standard MP4 actually fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SinkKind {
    Fragmented,
    Standard,
}

/// Whether the writer was closed cleanly or abandoned mid-capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ending {
    /// `Finalize()` — the clean stop path.
    Finalized,
    /// Frames written, the encoder drained into the sink, the byte stream
    /// flushed to the OS, then `std::process::abort()`.
    ///
    /// This must run in a CHILD process, because it kills the process it
    /// runs in. That is the point: `Finalize` never runs, no destructor
    /// runs, and nothing still held in user space is written — the same
    /// boundary a real crash draws.
    Crashed,
}

struct MfRuntime;

impl MfRuntime {
    fn start() -> WinResult<Self> {
        unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL)? };
        Ok(MfRuntime)
    }
}

impl Drop for MfRuntime {
    fn drop(&mut self) {
        unsafe {
            let _ = MFShutdown();
        }
    }
}

fn video_output_type() -> WinResult<IMFMediaType> {
    unsafe {
        let t = MFCreateMediaType()?;
        t.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        t.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)?;
        t.SetUINT32(&MF_MT_AVG_BITRATE, BITRATE)?;
        t.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
        // MF packs these as a single UINT64: high 32 bits then low 32.
        t.SetUINT64(&MF_MT_FRAME_SIZE, ((WIDTH as u64) << 32) | HEIGHT as u64)?;
        t.SetUINT64(&MF_MT_FRAME_RATE, ((FPS as u64) << 32) | 1u64)?;
        t.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, (1u64 << 32) | 1u64)?;
        Ok(t)
    }
}

fn video_input_type() -> WinResult<IMFMediaType> {
    unsafe {
        let t = MFCreateMediaType()?;
        t.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        t.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)?;
        t.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
        t.SetUINT64(&MF_MT_FRAME_SIZE, ((WIDTH as u64) << 32) | HEIGHT as u64)?;
        t.SetUINT64(&MF_MT_FRAME_RATE, ((FPS as u64) << 32) | 1u64)?;
        t.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, (1u64 << 32) | 1u64)?;
        Ok(t)
    }
}

/// One NV12 frame carrying a moving horizontal band, so successive frames
/// genuinely differ and the encoder cannot collapse the whole capture into a
/// single trivially-predicted GOP.
fn nv12_frame(index: u32) -> Vec<u8> {
    let y_len = (WIDTH * HEIGHT) as usize;
    let uv_len = y_len / 2;
    let mut buf = vec![0u8; y_len + uv_len];
    let band = ((index * 7) % HEIGHT) as usize;
    for row in 0..HEIGHT as usize {
        let luma = if row.abs_diff(band) < 20 { 235u8 } else { 16u8 };
        let start = row * WIDTH as usize;
        buf[start..start + WIDTH as usize].fill(luma);
    }
    // Neutral chroma: 128 is the achromatic midpoint for NV12.
    buf[y_len..].fill(128);
    buf
}

fn sample_for(index: u32) -> WinResult<IMFSample> {
    unsafe {
        let pixels = nv12_frame(index);
        let buffer = MFCreateMemoryBuffer(pixels.len() as u32)?;
        let mut dst: *mut u8 = std::ptr::null_mut();
        let mut max_len = 0u32;
        buffer.Lock(&mut dst, Some(&mut max_len), None)?;
        std::ptr::copy_nonoverlapping(pixels.as_ptr(), dst, pixels.len());
        buffer.Unlock()?;
        buffer.SetCurrentLength(pixels.len() as u32)?;

        let sample = MFCreateSample()?;
        sample.AddBuffer(&buffer)?;
        let dur = HNS_PER_SEC / FPS as i64;
        sample.SetSampleDuration(dur)?;
        sample.SetSampleTime(dur * index as i64)?;
        Ok(sample)
    }
}

/// Write `FRAMES` synthetic frames into `path` through the chosen sink, then
/// end the capture the chosen way. Returns nothing: the artefact under test
/// is the file on disk.
///
/// `Ending::Crashed` never returns — it aborts the process. Only the child
/// worker calls it that way.
pub fn write_capture(path: &Path, kind: SinkKind, ending: Ending) -> WinResult<()> {
    unsafe {
        let _mf = MfRuntime::start()?;

        let byte_stream = MFCreateFile(
            MF_ACCESSMODE_WRITE,
            MF_OPENMODE_DELETE_IF_EXIST,
            MF_FILEFLAGS_NONE,
            &HSTRING::from(path.to_string_lossy().as_ref()),
        )?;

        let out_type = video_output_type()?;
        let sink: IMFMediaSink = match kind {
            // The only line that differs between the two arms, which is what
            // makes the control meaningful.
            SinkKind::Fragmented => MFCreateFMPEG4MediaSink(&byte_stream, &out_type, None)?,
            SinkKind::Standard => MFCreateMPEG4MediaSink(&byte_stream, &out_type, None)?,
        };

        let writer = MFCreateSinkWriterFromMediaSink(&sink, None)?;
        writer.SetInputMediaType(0, &video_input_type()?, None)?;
        writer.BeginWriting()?;

        for i in 0..FRAMES {
            writer.WriteSample(0, &sample_for(i)?)?;
        }

        match ending {
            Ending::Finalized => {
                writer.Finalize()?;
            }
            Ending::Crashed => {
                // Drain the encoder into the sink, then push the byte
                // stream's own buffer out to the OS. That second flush is
                // the part a production session must also do (spec §6.4
                // says fragments are "flushed as they close") — without it
                // the bytes sit in user space and die with the process, and
                // no container format can survive that.
                writer.Flush(0)?;
                byte_stream.Flush()?;

                // Abort rather than return: no destructors, no CRT flushing,
                // no COM teardown, and above all no Finalize. Whatever
                // reached the OS stays on disk; whatever did not, does not.
                // That is exactly the boundary a real crash draws, which a
                // clean `drop` does NOT — a dropped byte stream discards its
                // buffer, which is how the first version of this spike
                // produced a 0-byte file for BOTH sinks and measured nothing.
                std::process::abort();
            }
        }
        Ok(())
    }
}

/// Try to decode frames back out of `path` using Media Foundation's own
/// source reader. Returns the number of samples successfully read.
///
/// This is the playability question answered by the platform that will play
/// these files, rather than by inspecting bytes and reasoning about what a
/// player *would* do.
pub fn decodable_sample_count(path: &Path) -> WinResult<u32> {
    unsafe {
        let _mf = MfRuntime::start()?;
        let reader =
            MFCreateSourceReaderFromURL(&HSTRING::from(path.to_string_lossy().as_ref()), None)?;

        let mut count = 0u32;
        loop {
            let mut flags = 0u32;
            let mut sample: Option<IMFSample> = None;
            reader.ReadSample(
                MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32,
                0,
                None,
                Some(&mut flags),
                None,
                Some(&mut sample),
            )?;
            if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                break;
            }
            if sample.is_some() {
                count += 1;
            } else if flags != 0 {
                // A format change or gap with no sample: keep going, but do
                // not count it as a decoded frame.
                continue;
            } else {
                break;
            }
            if count > FRAMES * 2 {
                break; // defensive: never spin on a pathological file
            }
        }
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mp4_boxes;
    use std::path::PathBuf;
    use std::process::Command;

    /// Env var naming the file the crash worker should write.
    const CRASH_PATH_ENV: &str = "VB_FMP4_CRASH_PATH";
    /// Env var selecting which sink the crash worker should use.
    const CRASH_KIND_ENV: &str = "VB_FMP4_CRASH_KIND";

    fn scratch(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("vb-fmp4-spike-{name}.mp4"));
        let _ = std::fs::remove_file(&p);
        p
    }

    fn report(label: &str, path: &Path) -> mp4_boxes::Scan {
        let bytes = std::fs::read(path).unwrap_or_default();
        let scan = mp4_boxes::scan(&bytes);
        println!(
            "SPIKE[{label}] bytes={} boxes={:?} end={:?} fragmented={} fragments={}",
            bytes.len(),
            scan.kinds(),
            scan.end,
            scan.is_fragmented(),
            scan.fragment_count()
        );
        scan
    }

    /// Re-entry point for the crash tests. Spawned as a CHILD of the test
    /// binary; kills itself on purpose.
    ///
    /// Without the env var this is a deliberate no-op so an ordinary test run
    /// just passes it.
    #[test]
    fn crash_child_worker() {
        let Ok(path) = std::env::var(CRASH_PATH_ENV) else {
            return;
        };
        let kind = match std::env::var(CRASH_KIND_ENV).as_deref() {
            Ok("standard") => SinkKind::Standard,
            _ => SinkKind::Fragmented,
        };
        // Never returns: write_capture aborts under Ending::Crashed.
        let _ = write_capture(Path::new(&path), kind, Ending::Crashed);
        unreachable!("the crash worker must abort, not return");
    }

    /// Run a capture in a child process that dies mid-recording, and leave
    /// whatever reached the disk behind for the caller to inspect.
    fn capture_then_crash(path: &Path, kind: SinkKind) {
        let exe = std::env::current_exe().expect("test binary path");
        let out = Command::new(exe)
            .args([
                "--exact",
                "fmp4_spike::tests::crash_child_worker",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(CRASH_PATH_ENV, path)
            .env(
                CRASH_KIND_ENV,
                match kind {
                    SinkKind::Standard => "standard",
                    SinkKind::Fragmented => "fragmented",
                },
            )
            .output()
            .expect("spawn crash worker");
        // An aborted child never exits 0. A clean exit means the worker
        // returned instead of crashing, which would silently turn these into
        // tests of nothing.
        assert!(
            !out.status.success(),
            "crash worker exited cleanly ({:?}) — it did not actually crash; \
             stderr: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// Q1: after a real crash, does the fragmented file still carry `moof`
    /// fragments on disk?
    #[test]
    fn q1_crashed_fragmented_capture_contains_fragments() {
        let p = scratch("q1-frag-crashed");
        capture_then_crash(&p, SinkKind::Fragmented);
        let scan = report("q1 fragmented/crashed", &p);
        assert!(
            scan.is_fragmented(),
            "a crashed fMP4 capture must still carry moof fragments; saw {:?}",
            scan.kinds()
        );
    }

    /// Q2: the headline question. Is that crashed file playable?
    #[test]
    fn q2_crashed_fragmented_capture_is_still_decodable() {
        let p = scratch("q2-frag-crashed");
        capture_then_crash(&p, SinkKind::Fragmented);
        report("q2 fragmented/crashed", &p);
        let n = decodable_sample_count(&p).unwrap_or(0);
        println!("SPIKE[q2 fragmented/crashed] decoded_samples={n}");
        assert!(
            n > 0,
            "spec §6.4's crash-safety claim requires a crashed fragmented \
             capture to decode as a prefix; decoded {n} samples"
        );
    }

    /// The CONTROL, and the reason Q2 means anything: the same crash through
    /// the STANDARD mp4 sink must NOT decode. If it did, fragmentation would
    /// be buying nothing and §6.4's premise would be wrong.
    ///
    /// Note this asserts on a NON-EMPTY file. The first version of this spike
    /// passed a near-identical assertion against a 0-byte file — true, but
    /// for the wrong reason, and it measured nothing.
    #[test]
    fn control_crashed_standard_capture_is_not_decodable() {
        let p = scratch("control-std-crashed");
        capture_then_crash(&p, SinkKind::Standard);
        let scan = report("control standard/crashed", &p);
        let n = decodable_sample_count(&p).unwrap_or(0);
        println!("SPIKE[control standard/crashed] decoded_samples={n}");
        assert!(
            !scan.is_fragmented(),
            "the standard sink must not be emitting fragments"
        );
        assert_eq!(
            n, 0,
            "an un-finalized standard MP4 is expected to be unplayable — if \
             this decodes, fMP4 is not buying the crash-safety §6.4 assumes"
        );
    }

    /// Sanity: the clean path produces a file both checks agree on. This is
    /// also what proves the harness itself works, independent of the crash
    /// simulation.
    #[test]
    fn finalized_fragmented_capture_is_decodable() {
        let p = scratch("finalized-frag");
        write_capture(&p, SinkKind::Fragmented, Ending::Finalized).expect("write");
        let scan = report("finalized fragmented", &p);
        let n = decodable_sample_count(&p).unwrap_or(0);
        println!("SPIKE[finalized fragmented] decoded_samples={n}");
        assert!(scan.is_fragmented());
        assert!(n > 0, "a finalized capture must decode");
    }
}
