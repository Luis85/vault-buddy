//! The staged capture's container: a FRAGMENTED MP4 written through Media
//! Foundation's `IMFSinkWriter`.
//!
//! Why fragmented rather than a standard MP4 (spec 6.4, RESOLVED): a
//! standard MP4 writes its `moov` index at finalize, so a capture that
//! crashes mid-recording is unopenable — every byte of video present, no
//! index. A fragmented MP4 is self-describing per fragment and degrades to
//! a playable prefix. Measured on a windows-latest runner, killing the
//! process with abort() after 300 frames: the fragmented file decoded back
//! 280 of 300 frames; the standard-MP4 control, holding a comparable
//! amount of data, decoded 0. That control is what makes the result mean
//! anything.
//!
//! The measurement covers a PROCESS CRASH, not power loss. Power loss only
//! shortens the recoverable prefix; it cannot make a fragmented file behave
//! like an unindexed one.
//!
//! TWO THINGS THIS MODULE MUST NEVER DO, both learned the expensive way:
//!
//! 1. **Never call `IMFSinkWriter::Flush`.** It is documented to "drop all
//!    pending samples" — a seek-style discard, not a drain. Two spike runs
//!    called it before teardown and destroyed the footage they were
//!    measuring. Only `Finalize` drains.
//! 2. **Never infer anything from the file's size during capture.** Windows
//!    updates a directory entry's size lazily while a handle is open; a
//!    per-frame probe of the file's length through the filesystem reported
//!    0 bytes across a capture that had written 59 KB. A retracted reading
//!    of that probe once invented a "flush every fragment" requirement
//!    that does not exist.

use std::path::Path;
use std::time::Duration;

/// H.264 output parameters. `bitrate_bps` comes from
/// `vault_buddy_core::screen_capture_config::bitrate_bps` — do not
/// re-derive it here.
#[derive(Debug, Clone, Copy)]
pub struct VideoFormat {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate_bps: u32,
}

/// AAC output parameters. Input is always 16-bit interleaved PCM.
#[derive(Debug, Clone, Copy)]
pub struct AudioFormat {
    pub sample_rate: u32,
    pub channels: u16,
    pub bitrate_bps: u32,
}

/// Convert to Media Foundation's time base (100-nanosecond units).
#[allow(dead_code)] // used only by the Windows arm
fn to_hns(d: Duration) -> i64 {
    // saturating: a Duration beyond ~29 000 years cannot be represented, and
    // a panic in the capture write path is never the right answer.
    i64::try_from(d.as_nanos() / 100).unwrap_or(i64::MAX)
}

#[cfg(not(windows))]
mod imp {
    use super::*;
    use crate::ScreenError;

    /// Non-Windows stub. It must DEGRADE rather than panic: the Linux
    /// compile gate builds this crate, and an abort here would kill CI
    /// instead of reporting.
    pub struct FragmentedSink;

    impl FragmentedSink {
        pub fn create(
            _path: &Path,
            _video: VideoFormat,
            _audio: Option<AudioFormat>,
        ) -> Result<FragmentedSink, ScreenError> {
            Err(ScreenError::Unsupported)
        }

        pub fn write_video(
            &mut self,
            _nv12: &[u8],
            _ts: Duration,
            _duration: Duration,
        ) -> Result<(), ScreenError> {
            Err(ScreenError::Unsupported)
        }

        pub fn write_audio(
            &mut self,
            _pcm: &[i16],
            _ts: Duration,
            _duration: Duration,
        ) -> Result<(), ScreenError> {
            Err(ScreenError::Unsupported)
        }

        pub fn finalize(self) -> Result<(), ScreenError> {
            Err(ScreenError::Unsupported)
        }
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use crate::ScreenError;
    use windows::core::{Result as WinResult, HSTRING};
    use windows::Win32::Media::MediaFoundation::*;

    /// Stream indices are fixed by the ORDER the sink declares its streams:
    /// `MFCreateFMPEG4MediaSink(stream, video_type, audio_type)` gives
    /// video 0 and audio 1. They are not ours to choose.
    const VIDEO_STREAM: u32 = 0;
    const AUDIO_STREAM: u32 = 1;

    fn sink_err(context: &str, e: windows::core::Error) -> ScreenError {
        log::error!("screen sink: {context}: {e}");
        ScreenError::Sink(format!("{context}: {e}"))
    }

    /// Media Foundation must be started per process and shut down once. The
    /// sink owns one of these for its lifetime.
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

    fn video_output_type(v: VideoFormat) -> WinResult<IMFMediaType> {
        unsafe {
            let t = MFCreateMediaType()?;
            t.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            t.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)?;
            t.SetUINT32(&MF_MT_AVG_BITRATE, v.bitrate_bps)?;
            t.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
            // Spec 12: a fixed 1-second keyframe interval regardless of
            // preset. It costs little, bounds the editor preview's seek
            // latency, and leaves a future no-re-encode fast-cut path
            // possible without changing the capture format.
            t.SetUINT32(&MF_MT_MAX_KEYFRAME_SPACING, v.fps)?;
            // MF packs these as one UINT64: high 32 bits then low 32.
            t.SetUINT64(
                &MF_MT_FRAME_SIZE,
                ((v.width as u64) << 32) | v.height as u64,
            )?;
            t.SetUINT64(&MF_MT_FRAME_RATE, ((v.fps as u64) << 32) | 1u64)?;
            t.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, (1u64 << 32) | 1u64)?;
            Ok(t)
        }
    }

    fn video_input_type(v: VideoFormat) -> WinResult<IMFMediaType> {
        unsafe {
            let t = MFCreateMediaType()?;
            t.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            // NV12 is the format every hardware H.264 encoder MFT accepts;
            // handing the encoder BGRA would work only where a software
            // colour converter happens to be inserted.
            t.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)?;
            t.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
            t.SetUINT64(
                &MF_MT_FRAME_SIZE,
                ((v.width as u64) << 32) | v.height as u64,
            )?;
            t.SetUINT64(&MF_MT_FRAME_RATE, ((v.fps as u64) << 32) | 1u64)?;
            t.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, (1u64 << 32) | 1u64)?;
            Ok(t)
        }
    }

    fn audio_output_type(a: AudioFormat) -> WinResult<IMFMediaType> {
        unsafe {
            let t = MFCreateMediaType()?;
            t.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
            t.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_AAC)?;
            t.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
            t.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, a.sample_rate)?;
            t.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, a.channels as u32)?;
            // The AAC encoder MFT takes BYTES per second, not bits.
            t.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, a.bitrate_bps / 8)?;
            Ok(t)
        }
    }

    fn audio_input_type(a: AudioFormat) -> WinResult<IMFMediaType> {
        unsafe {
            let t = MFCreateMediaType()?;
            t.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
            t.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM)?;
            t.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
            t.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, a.sample_rate)?;
            t.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, a.channels as u32)?;
            let block_align = a.channels as u32 * 2; // 16-bit samples
            t.SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, block_align)?;
            t.SetUINT32(
                &MF_MT_AUDIO_AVG_BYTES_PER_SECOND,
                a.sample_rate * block_align,
            )?;
            t.SetUINT32(&MF_MT_ALL_SAMPLES_INDEPENDENT, 1)?;
            Ok(t)
        }
    }

    pub struct FragmentedSink {
        writer: IMFSinkWriter,
        has_audio: bool,
        /// Dropped last, after the writer: MFShutdown must not run while a
        /// Media Foundation object is still alive.
        _mf: MfRuntime,
    }

    impl FragmentedSink {
        pub fn create(
            path: &Path,
            video: VideoFormat,
            audio: Option<AudioFormat>,
        ) -> Result<FragmentedSink, ScreenError> {
            unsafe {
                let mf = MfRuntime::start().map_err(|e| sink_err("MFStartup", e))?;

                let byte_stream = MFCreateFile(
                    MF_ACCESSMODE_WRITE,
                    MF_OPENMODE_DELETE_IF_EXIST,
                    MF_FILEFLAGS_NONE,
                    &HSTRING::from(path.to_string_lossy().as_ref()),
                )
                .map_err(|e| sink_err("create the capture file", e))?;

                let video_out =
                    video_output_type(video).map_err(|e| sink_err("build the H.264 type", e))?;
                let audio_out = match audio {
                    Some(a) => {
                        Some(audio_output_type(a).map_err(|e| sink_err("build the AAC type", e))?)
                    }
                    None => None,
                };

                // FRAGMENTED, not MFCreateMPEG4MediaSink. This one line is
                // the difference between a crashed capture that plays as a
                // prefix and one that cannot be opened at all (spec 6.4).
                let sink: IMFMediaSink =
                    MFCreateFMPEG4MediaSink(&byte_stream, &video_out, audio_out.as_ref())
                        .map_err(|e| sink_err("create the fragmented MP4 sink", e))?;

                let writer = MFCreateSinkWriterFromMediaSink(&sink, None)
                    .map_err(|e| sink_err("create the sink writer", e))?;

                writer
                    .SetInputMediaType(
                        VIDEO_STREAM,
                        &video_input_type(video).map_err(|e| sink_err("build the NV12 type", e))?,
                        None,
                    )
                    // The one failure a user can act on: no usable H.264
                    // encoder on this machine.
                    .map_err(|_| ScreenError::EncoderUnavailable)?;

                if let Some(a) = audio {
                    writer
                        .SetInputMediaType(
                            AUDIO_STREAM,
                            &audio_input_type(a).map_err(|e| sink_err("build the PCM type", e))?,
                            None,
                        )
                        .map_err(|e| sink_err("configure the audio stream", e))?;
                }

                writer
                    .BeginWriting()
                    .map_err(|e| sink_err("begin writing", e))?;

                Ok(FragmentedSink {
                    writer,
                    has_audio: audio.is_some(),
                    _mf: mf,
                })
            }
        }

        pub fn write_video(
            &mut self,
            nv12: &[u8],
            ts: Duration,
            duration: Duration,
        ) -> Result<(), ScreenError> {
            self.write(VIDEO_STREAM, nv12, ts, duration)
        }

        pub fn write_audio(
            &mut self,
            pcm: &[i16],
            ts: Duration,
            duration: Duration,
        ) -> Result<(), ScreenError> {
            if !self.has_audio {
                // Not an error: a capture with no audio devices selected is
                // legal (spec 6.5), and the mux thread should not have to
                // branch on it.
                return Ok(());
            }
            let bytes: &[u8] = unsafe {
                std::slice::from_raw_parts(pcm.as_ptr() as *const u8, std::mem::size_of_val(pcm))
            };
            self.write(AUDIO_STREAM, bytes, ts, duration)
        }

        fn write(
            &mut self,
            stream: u32,
            bytes: &[u8],
            ts: Duration,
            duration: Duration,
        ) -> Result<(), ScreenError> {
            unsafe {
                let len = u32::try_from(bytes.len())
                    .map_err(|_| ScreenError::Sink("sample larger than 4 GiB".into()))?;
                let buffer =
                    MFCreateMemoryBuffer(len).map_err(|e| sink_err("allocate a sample", e))?;
                let mut dst: *mut u8 = std::ptr::null_mut();
                buffer
                    .Lock(&mut dst, None, None)
                    .map_err(|e| sink_err("lock a sample", e))?;
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), dst, bytes.len());
                buffer
                    .Unlock()
                    .map_err(|e| sink_err("unlock a sample", e))?;
                buffer
                    .SetCurrentLength(len)
                    .map_err(|e| sink_err("size a sample", e))?;

                let sample = MFCreateSample().map_err(|e| sink_err("create a sample", e))?;
                sample
                    .AddBuffer(&buffer)
                    .map_err(|e| sink_err("attach a sample buffer", e))?;
                sample
                    .SetSampleTime(to_hns(ts))
                    .map_err(|e| sink_err("stamp a sample", e))?;
                sample
                    .SetSampleDuration(to_hns(duration))
                    .map_err(|e| sink_err("set a sample duration", e))?;

                self.writer
                    .WriteSample(stream, &sample)
                    .map_err(|e| sink_err("write a sample", e))
            }
        }

        /// Drain and close. Consumes `self`, so writing after finalizing is
        /// a compile error rather than a runtime surprise.
        ///
        /// There is NO `Flush` here and there must never be one: MF's
        /// `Flush` drops pending samples instead of draining them. Finalize
        /// is the only drain.
        pub fn finalize(self) -> Result<(), ScreenError> {
            unsafe {
                self.writer
                    .Finalize()
                    .map_err(|e| sink_err("finalize the capture", e))
            }
        }
    }
}

pub use imp::FragmentedSink;

#[cfg(test)]
mod tests {
    use super::*;

    // The Linux compile gate builds this crate. The stub must DEGRADE, not
    // panic and not silently succeed: a stub that returned Ok would make
    // every caller's happy path untestable proof of nothing.
    #[cfg(not(windows))]
    #[test]
    fn the_non_windows_sink_reports_unsupported_rather_than_panicking() {
        let r = FragmentedSink::create(
            std::path::Path::new("/tmp/vb-unused.mp4"),
            VideoFormat {
                width: 1920,
                height: 1080,
                fps: 30,
                bitrate_bps: 8_000_000,
            },
            None,
        );
        assert!(matches!(r, Err(crate::ScreenError::Unsupported)));
    }

    #[test]
    fn a_video_format_is_plain_data_both_platforms_agree_on() {
        // The format structs are shared by both arms, so a field added to
        // the Windows arm only would not compile here — which is the point.
        let v = VideoFormat {
            width: 1280,
            height: 720,
            fps: 60,
            bitrate_bps: 5_000_000,
        };
        assert_eq!(v.width * v.height, 921_600);
        let a = AudioFormat {
            sample_rate: 48_000,
            channels: 2,
            bitrate_bps: 128_000,
        };
        assert_eq!(a.channels, 2);
    }

    // The file's own content up to (not including) this `#[cfg(test)]`
    // module. The two regression scans below necessarily quote their own
    // banned substring inside the assertion that checks for it, so scanning
    // the WHOLE file via a bare `include_str!("sink.rs")` (the brief's
    // literal sample) would make both tests self-match their own assertion
    // text and fail unconditionally, on any implementation, including a
    // correct one. `src-tauri/src/config_lock_guard.rs` hit the identical
    // self-reference problem for its own structural scan and fixed it by
    // excluding the scanning file; trimming to the non-test prefix is the
    // same fix applied to one file instead of two.
    fn production_src() -> &'static str {
        let src = include_str!("sink.rs");
        src.split("#[cfg(test)]").next().unwrap_or(src)
    }

    // Regression, spec 6.4's second caveat: IMFSinkWriter::Flush DROPS
    // pending samples rather than draining them. Two spike runs called it
    // before teardown and destroyed the footage they were measuring, then
    // reported the absence as a result. A future "tidy up the teardown"
    // edit that reintroduces it would silently empty every capture, with no
    // test failing and no error logged. This scan is the alarm.
    #[test]
    fn the_sink_never_calls_flush() {
        assert!(
            !production_src().contains(".Flush("),
            "sink.rs must never call IMFSinkWriter::Flush - it discards \
             pending samples. Only Finalize drains."
        );
    }

    // Regression, spec 6.4's first caveat: a per-frame std::fs::metadata
    // probe reported 0 bytes across a capture that had written 59 KB,
    // because Windows updates a directory entry's size lazily while a
    // handle is open. Reading it literally once produced an invented
    // "flush every fragment" requirement.
    #[test]
    fn the_sink_never_probes_the_file_size_while_writing() {
        assert!(
            !production_src().contains("fs::metadata"),
            "sink.rs must not probe file size during capture - it measures \
             a stale directory entry, not the sink."
        );
    }
}
