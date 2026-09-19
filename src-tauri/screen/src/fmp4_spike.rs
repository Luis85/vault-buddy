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
/// 10 seconds. The first runs used 3 s, which left open whether the sink
/// simply had not reached a fragment boundary yet. 10 s at a ~333 ms
/// observed cadence is ~30 fragments — far past any plausible threshold.
const FRAMES: u32 = 300;
/// Probe the on-disk size every N frames (~1 s) during the capture.
const PROBE_EVERY: u32 = 30;
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
    /// Frames written, then `std::process::abort()` — and deliberately
    /// nothing in between. No `Finalize`, and no `Flush` either: MF's
    /// `IMFSinkWriter::Flush` DROPS pending samples rather than draining
    /// them, so calling it before the crash destroys the very footage the
    /// spike is trying to recover.
    ///
    /// This must run in a CHILD process, because it kills the process it
    /// runs in. Whatever the sink pushed to the OS during the capture is
    /// what survives — the same boundary a real crash draws.
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

            // Does the sink write to disk DURING the capture, or does it
            // hold everything until Finalize? That is the real question
            // behind crash-safety, and one size probe per second answers it
            // without needing a crash at all. std::fs::metadata is a
            // metadata query, so it does not disturb MF's open handle.
            if PROBE_EVERY > 0 && i % PROBE_EVERY == PROBE_EVERY - 1 {
                let on_disk = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
                println!("SPIKE-PROBE frame={} on_disk_bytes={}", i + 1, on_disk);
            }
        }

        match ending {
            Ending::Finalized => {
                writer.Finalize()?;
            }
            Ending::Crashed => {
                // NOTE: deliberately NO writer.Flush() here.
                //
                // IMFSinkWriter::Flush is documented to "drop all pending
                // samples" — it is a seek-style discard, not a drain. Both
                // earlier versions of this spike called it immediately
                // before crashing, which threw the footage away and then
                // measured its absence: every run produced a 0-byte file,
                // for the fragmented AND the standard sink alike.
                //
                // A real crash does not get to call anything. So we call
                // nothing: whatever the sink already pushed to the OS during
                // the capture is what a crash would have left behind, and
                // that is exactly what we want to inspect.
                let on_disk = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
                println!("SPIKE-PROBE pre_abort on_disk_bytes={on_disk}");

                // Abort: no destructors, no CRT flush, no COM teardown, no
                // Finalize — the boundary a real crash draws.
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

    /// The spike's actual measurement, reported rather than asserted.
    ///
    /// Two earlier iterations asserted the outcome spec §6.4 HOPES for and
    /// failed for reasons that had nothing to do with the question (a
    /// discarding `Flush`). A spike exists to find out, so this prints the
    /// evidence and asserts only what is already established. The gate
    /// decision is made from these lines.
    #[test]
    fn spike_measurement_crashed_fragmented_vs_standard() {
        for (label, kind) in [
            ("fragmented", SinkKind::Fragmented),
            ("standard", SinkKind::Standard),
        ] {
            let p = scratch(&format!("crash-{label}"));
            capture_then_crash(&p, kind);
            let scan = report(&format!("crashed/{label}"), &p);
            let n = decodable_sample_count(&p).unwrap_or(0);
            println!(
                "SPIKE-RESULT crashed/{label} on_disk_bytes={} fragments={} decoded_samples={}",
                std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0),
                scan.fragment_count(),
                n
            );
        }
        println!(
            "SPIKE-VERDICT read the SPIKE-PROBE and SPIKE-RESULT lines above: \
             if crashed/fragmented has bytes>0 and decoded_samples>0 while \
             crashed/standard decodes 0, spec 6.4 holds as written; if the \
             PROBE lines stay at 0 for the whole capture, the sink does not \
             write incrementally and the chunked-rolling fallback applies."
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
