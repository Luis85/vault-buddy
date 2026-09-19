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
    /// Frames written and flushed, then the writer released WITHOUT
    /// `Finalize()`. This is the closest faithful proxy for a crash that a
    /// test can produce: the index-writing step never runs. It is not
    /// byte-identical to power loss (the OS still flushes its buffers on
    /// handle close), but it exercises the property under test — whether the
    /// file is usable when the finalize step never happened.
    Abandoned,
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
            Ending::Abandoned => {
                // Push everything the writer is holding down to the byte
                // stream, then drop without Finalize. Flushing first is what
                // makes this a fair proxy: a real crash would also have had
                // whatever the sink already handed to the OS.
                writer.Flush(0)?;
                drop(writer);
                let _ = sink.Shutdown();
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

    fn scratch(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("vb-fmp4-spike-{name}.mp4"));
        let _ = std::fs::remove_file(&p);
        p
    }

    fn report(label: &str, path: &Path) -> mp4_boxes::Scan {
        let bytes = std::fs::read(path).expect("spike output should exist");
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

    /// Q1: does the fragmented sink actually emit `moof` fragments, without
    /// ever being finalized?
    #[test]
    fn q1_abandoned_fragmented_capture_contains_fragments() {
        let p = scratch("q1-frag-abandoned");
        write_capture(&p, SinkKind::Fragmented, Ending::Abandoned)
            .expect("fragmented sink should accept frames");
        let scan = report("q1 fragmented/abandoned", &p);
        assert!(
            scan.is_fragmented(),
            "an abandoned fMP4 capture must still carry moof fragments; \
             saw {:?}",
            scan.kinds()
        );
        assert!(
            scan.fragment_count() >= 2,
            "3 s of video should close more than one fragment, saw {}",
            scan.fragment_count()
        );
    }

    /// Q2: the headline question. Is that un-finalized file playable?
    #[test]
    fn q2_abandoned_fragmented_capture_is_still_decodable() {
        let p = scratch("q2-frag-abandoned");
        write_capture(&p, SinkKind::Fragmented, Ending::Abandoned).expect("write");
        let n = decodable_sample_count(&p).unwrap_or(0);
        println!("SPIKE[q2 fragmented/abandoned] decoded_samples={n}");
        assert!(
            n > 0,
            "the crash-safety claim in spec §6.4 requires an un-finalized \
             fragmented capture to decode as a prefix; decoded {n} samples"
        );
    }

    /// The CONTROL. If an abandoned STANDARD mp4 also decoded fine, then
    /// fragmentation would be buying us nothing and §6.4's premise would be
    /// wrong. This test is what gives Q2 its meaning.
    #[test]
    fn control_abandoned_standard_capture_is_not_decodable() {
        let p = scratch("control-std-abandoned");
        write_capture(&p, SinkKind::Standard, Ending::Abandoned).expect("write");
        let scan = report("control standard/abandoned", &p);
        let n = decodable_sample_count(&p).unwrap_or(0);
        println!("SPIKE[control standard/abandoned] decoded_samples={n}");
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

    /// Sanity: the clean path still produces a file both checks agree on.
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
