//! The sink's FORMAT CONTRACT: the plain-data video/audio parameters a
//! capture is opened with, their boundary validation, the AAC Encoder MFT's
//! fixed byte-rate table, the Media Foundation time-base conversion, and the
//! refusal text shown when the H.264 encoder rejects a format outright.
//!
//! Its own module rather than more of `sink.rs` for a measured reason: that
//! file sat at 774 of this repo's 800-line Rust cap (shrink-only, inline
//! tests counted). The seam is the one `sink.rs` already drew in prose on
//! every item moved here ("pure logic (no cfg(windows) gate), so it must be
//! exercised here where ... CI can actually reach it"): each is either data
//! both platform arms share or arithmetic the `cfg(windows)` arm feeds to
//! COM, and that arm executes in no automated test anywhere (docs/Gaps.md
//! GAP-117).
//!
//! **Which side of the seam this is: the PURE side.** Nothing here names a
//! `windows` type; everything takes plain integers or a `Duration` and
//! compiles and tests on every platform. What stays in `sink.rs` is the
//! `IMFSinkWriter` itself (`cfg(windows)`, with an `Unsupported` stub
//! elsewhere) and the structural scans that guard it -- including the rule
//! that it never calls `Flush` and never probes the file size. `sink.rs`
//! re-exports `VideoFormat`, `AudioFormat` and `unsupported_format_message`,
//! so every caller's path is unchanged. `snap_aac_bytes_per_sec` and
//! `to_hns` are `pub(crate)` and consumed only by `sink.rs`'s Windows arm,
//! which is why they keep their `allow(dead_code)`: off Windows only the
//! tests below call them.

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

impl VideoFormat {
    /// Validate at the boundary rather than let a bad value surface as a
    /// bare Media Foundation HRESULT from deep inside COM. `fps: 0` would
    /// otherwise silently produce `MF_MT_FRAME_RATE = 0/1` and
    /// `MF_MT_MAX_KEYFRAME_SPACING = 0` (both accepted by `SetUINT64`/
    /// `SetUINT32` - the failure, if any, only surfaces later and
    /// unhelpfully out of `SetInputMediaType`).
    pub fn validate(&self) -> Result<(), crate::ScreenError> {
        if self.width == 0 || self.height == 0 {
            return Err(crate::ScreenError::Sink(format!(
                "video frame size must be non-zero (got {}x{})",
                self.width, self.height
            )));
        }
        if self.fps == 0 {
            return Err(crate::ScreenError::Sink(
                "video fps must be non-zero".into(),
            ));
        }
        Ok(())
    }
}

/// AAC output parameters. Input is always 16-bit interleaved PCM.
#[derive(Debug, Clone, Copy)]
pub struct AudioFormat {
    pub sample_rate: u32,
    pub channels: u16,
    pub bitrate_bps: u32,
}

impl AudioFormat {
    /// Reject a value the AAC Encoder MFT's output media type cannot
    /// express, at the boundary, rather than let it surface as a bare
    /// `E_INVALIDARG` from deep inside COM. These are hard requirements
    /// MSDN documents for the AAC Encoder MFT's output type (bits/sample
    /// fixed at 16 - not a caller-supplied field here, so nothing to check;
    /// sample rate 44100 or 48000, matching the input type; 1, 2 or 6
    /// channels) - they are fixed by the encoder, not chosen by us.
    pub fn validate(&self) -> Result<(), crate::ScreenError> {
        if !matches!(self.sample_rate, 44_100 | 48_000) {
            return Err(crate::ScreenError::Sink(format!(
                "AAC sample rate must be 44100 or 48000 (got {})",
                self.sample_rate
            )));
        }
        if !matches!(self.channels, 1 | 2 | 6) {
            return Err(crate::ScreenError::Sink(format!(
                "AAC channel count must be 1, 2 or 6 (got {})",
                self.channels
            )));
        }
        Ok(())
    }
}

/// The AAC Encoder MFT accepts EXACTLY these average byte rates for
/// `MF_MT_AUDIO_AVG_BYTES_PER_SECOND` at stereo/mono channel counts - this
/// set is fixed by the encoder (verified against MSDN's AAC Encoder page),
/// not a choice made here. An unvalidated `bitrate_bps / 8` produces an
/// illegal value for almost any requested bitrate and fails with
/// `E_INVALIDARG`.
#[allow(dead_code)] // used only by sink.rs's Windows arm (and by the tests below)
const AAC_BASE_BYTES_PER_SEC: [u32; 4] = [12_000, 16_000, 20_000, 24_000];

/// The legal `MF_MT_AUDIO_AVG_BYTES_PER_SECOND` set for a given channel
/// count. For 6-channel (5.1) audio the encoder scales the whole set by 6;
/// mono and stereo share the base set.
#[allow(dead_code)] // used only by sink.rs's Windows arm (and by the tests below)
fn legal_aac_bytes_per_sec(channels: u16) -> [u32; 4] {
    if channels == 6 {
        AAC_BASE_BYTES_PER_SEC.map(|b| b * 6)
    } else {
        AAC_BASE_BYTES_PER_SEC
    }
}

/// Snap a requested bitrate (bits/sec) to the nearest
/// `MF_MT_AUDIO_AVG_BYTES_PER_SECOND` value the AAC Encoder MFT will
/// actually accept for this channel count, rather than passing an
/// unvalidated `bitrate_bps / 8` through and letting COM reject it.
#[allow(dead_code)] // used only by sink.rs's Windows arm (and by the tests below)
pub(crate) fn snap_aac_bytes_per_sec(bitrate_bps: u32, channels: u16) -> u32 {
    let requested = bitrate_bps / 8;
    let legal = legal_aac_bytes_per_sec(channels);
    // legal is a fixed non-empty array, so min_by_key always yields Some.
    legal
        .into_iter()
        .min_by_key(|&v| requested.abs_diff(v))
        .unwrap_or(legal[0])
}

/// Convert to Media Foundation's time base (100-nanosecond units).
#[allow(dead_code)] // used only by sink.rs's Windows arm
pub(crate) fn to_hns(d: Duration) -> i64 {
    // saturating: a Duration beyond ~29 000 years cannot be represented, and
    // a panic in the capture write path is never the right answer.
    i64::try_from(d.as_nanos() / 100).unwrap_or(i64::MAX)
}

/// The refusal shown when the H.264 encoder rejects the capture's video
/// format outright (`MF_E_INVALIDMEDIATYPE` from `SetInputMediaType`).
///
/// Media Foundation's own text for this is a bare, LOCALISED HRESULT string —
/// a user on a German Windows reads "Die für den Medientyp angegebenen Daten
/// sind ungültig" and has nothing to act on. `ScreenError::Refused` exists so
/// a condition the app can describe is never dressed up as somebody else's
/// error, and this is one: we know the size and the frame rate we asked for.
///
/// The remedy DIFFERS by frame rate, which is the whole reason this branches
/// rather than printing one sentence. Microsoft's encoder tops out around
/// 4096x2304 and its level caps trade resolution against rate, so a large
/// source that encodes happily at 30 is refused at 60 — the reported case was
/// a 3840x2400 monitor that had recorded fine until the frame rate became
/// settable. At 30 there is no rate left to give up, so the advice has to be
/// about the SOURCE instead.
pub fn unsupported_format_message(width: u32, height: u32, fps: u32) -> String {
    let head =
        format!("This machine's H.264 encoder will not record {width}x{height} at {fps} fps.");
    if fps > 30 {
        format!("{head} Set Frame rate to 30 fps in Vault settings → Screen, or capture a window or a region instead of the whole screen.")
    } else {
        format!("{head} Capture a window or a region instead of the whole screen — a smaller frame is within more encoders' limits.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // REGRESSION: reported from the running app. A 3840x2400 monitor had
    // recorded fine at 30 fps; the moment the settings tab made 60 settable,
    // `SetInputMediaType` returned MF_E_INVALIDMEDIATYPE and the user was
    // shown Media Foundation's own LOCALISED HRESULT text:
    //
    //   "Die für den Medientyp angegebenen Daten sind ungültig ... (0xC00D36B4)"
    //
    // which names neither what was refused nor what to do about it.
    #[test]
    fn a_refused_format_names_the_size_the_rate_and_a_remedy() {
        let at_60 = unsupported_format_message(3840, 2400, 60);
        assert!(at_60.contains("3840x2400"), "{at_60}");
        assert!(at_60.contains("60 fps"), "{at_60}");
        // The remedy the reporter could actually take, naming where it lives.
        assert!(at_60.contains("30 fps"), "{at_60}");
        assert!(at_60.contains("Vault settings"), "{at_60}");
        // Never Media Foundation's own words.
        assert!(!at_60.contains("0xC00D36B4"), "{at_60}");
    }

    // At 30 there is no frame rate left to trade, so advice to lower it would
    // send the user in a circle. The branch is the point of the function.
    #[test]
    fn at_thirty_the_remedy_is_the_source_not_the_frame_rate() {
        let at_30 = unsupported_format_message(3840, 2400, 30);
        assert!(
            at_30.contains("window") && at_30.contains("region"),
            "{at_30}"
        );
        assert!(
            !at_30.contains("30 fps in Vault settings"),
            "advising 30 fps at 30 fps is a loop: {at_30}"
        );
    }

    // --- VideoFormat::validate --------------------------------------------
    // Pure logic (no cfg(windows) gate), so it must be exercised here where
    // the Linux compile gate and CI can actually reach it - a check that
    // only lived inside `mod imp`'s Windows arm would never run in CI.

    #[test]
    fn a_zero_fps_video_format_is_rejected() {
        let v = VideoFormat {
            width: 1920,
            height: 1080,
            fps: 0,
            bitrate_bps: 8_000_000,
        };
        assert!(v.validate().is_err());
    }

    #[test]
    fn a_zero_width_or_height_video_format_is_rejected() {
        let zero_width = VideoFormat {
            width: 0,
            height: 1080,
            fps: 30,
            bitrate_bps: 8_000_000,
        };
        assert!(zero_width.validate().is_err());
        let zero_height = VideoFormat {
            width: 1920,
            height: 0,
            fps: 30,
            bitrate_bps: 8_000_000,
        };
        assert!(zero_height.validate().is_err());
    }

    #[test]
    fn a_well_formed_video_format_validates() {
        let v = VideoFormat {
            width: 1920,
            height: 1080,
            fps: 30,
            bitrate_bps: 8_000_000,
        };
        assert!(v.validate().is_ok());
    }

    // --- AudioFormat::validate ----------------------------------------

    #[test]
    fn an_unsupported_aac_sample_rate_is_rejected() {
        let a = AudioFormat {
            sample_rate: 44_099, // neither of the two MSDN-mandated rates
            channels: 2,
            bitrate_bps: 128_000,
        };
        assert!(a.validate().is_err());
    }

    #[test]
    fn an_unsupported_aac_channel_count_is_rejected() {
        let a = AudioFormat {
            sample_rate: 48_000,
            channels: 3, // AAC output type only allows 1, 2 or 6
            bitrate_bps: 128_000,
        };
        assert!(a.validate().is_err());
    }

    #[test]
    fn every_legal_aac_channel_count_validates() {
        for channels in [1u16, 2, 6] {
            let a = AudioFormat {
                sample_rate: 48_000,
                channels,
                bitrate_bps: 128_000,
            };
            assert!(a.validate().is_ok(), "channels={channels} should be legal");
        }
    }

    // --- snap_aac_bytes_per_sec -----------------------------------------

    #[test]
    fn a_bitrate_already_on_a_legal_aac_value_is_unchanged() {
        // 128 kbit/s = 16 000 bytes/s, one of the four legal stereo values.
        assert_eq!(snap_aac_bytes_per_sec(128_000, 2), 16_000);
    }

    #[test]
    fn an_illegal_bitrate_snaps_to_the_nearest_legal_aac_value() {
        // 100 kbit/s = 12 500 bytes/s: nearer to 12 000 than to 16 000.
        assert_eq!(snap_aac_bytes_per_sec(100_000, 2), 12_000);
        // 190 kbit/s = 23 750 bytes/s: nearer to 24 000 than to 20 000.
        assert_eq!(snap_aac_bytes_per_sec(190_000, 1), 24_000);
    }

    #[test]
    fn six_channel_aac_bitrates_snap_against_the_scaled_set() {
        // The legal set for 6 channels is the base set times 6:
        // [72 000, 96 000, 120 000, 144 000] bytes/s. 700 kbit/s =
        // 87 500 bytes/s: nearer to 96 000 than to 72 000.
        assert_eq!(snap_aac_bytes_per_sec(700_000, 6), 96_000);
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
}
