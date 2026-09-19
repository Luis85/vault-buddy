//! Screen Capture engine.
//!
//! What compiles where: `clock`, `convert`, `select` and `session::pacing`
//! are pure and build and test on ANY platform — they carry this feature's
//! correctness precisely because no CI runner can record a screen.
//! `frames` (the WGC callback) and `sink` (the fragmented-MP4 Media
//! Foundation writer) are the Windows-only surfaces, and `session` is the
//! thin arm that joins them.

pub mod clock;
pub mod convert;
// The WGC frame callback. Windows-only: it exists solely to feed
// `session`'s mux, and everything it decides is decided by a pure function
// in `session::pacing`.
#[cfg(windows)]
pub(crate) mod frames;
pub mod mp4_boxes;
pub mod select;
pub mod session;
pub mod sink;
pub mod source;
pub mod staging;

// Phase 2's gating spike (spec 6.4). Windows-only and feature-gated, so it
// is absent from every default build; see the module docs for how to run it.
#[cfg(all(windows, feature = "fmp4-spike"))]
pub mod fmp4_spike;

/// Every way a screen capture can fail, as a typed value the shell renders.
/// Stringly-typed errors would let a user-supplied window title reach a UI
/// that expects a fixed set of outcomes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScreenError {
    /// This platform has no screen-capture engine (non-Windows), or the
    /// engine is not implemented yet.
    Unsupported,
    /// The chosen window or monitor disappeared before capture began.
    SourceGone,
    /// No usable H.264 encoder, or Media Foundation is unavailable.
    EncoderUnavailable,
    Io(String),
    /// The capture could not be written — sink creation, a sample write, or
    /// finalize failed. Carries the OS message.
    Sink(String),
    /// A capture was requested while one is already running.
    ///
    /// RESERVED, not dead: spec §14 names this among the typed start
    /// refusals and the frontend store's own comment lists it, but the
    /// SHIPPED refusal comes from `CaptureKind::busy_message()` one layer up
    /// — the cross-domain guard has to name WHICH kind is running (an audio
    /// recording blocks a screen capture and vice versa), which this crate,
    /// being unaware of the audio domain, cannot say. Kept so the crate's
    /// error vocabulary still matches the spec and a future in-crate refusal
    /// has a variant to use; constructed today only by its own Display test.
    AlreadyCapturing,
    /// `ScreenSession::stop` failed (finalize, or the publish rename) AFTER
    /// real footage had already been written to the `.part` file at `path`
    /// — and that file was deliberately left where it is rather than
    /// deleted, precisely so a failed stop still leaves something playable
    /// (see `sink.rs`'s fragmented-MP4 module docs). `path` is carried
    /// TYPED, not stringified into the message, so a caller (Task 8) can
    /// offer the retained file to the user instead of only logging its
    /// location.
    Retained {
        path: std::path::PathBuf,
        cause: Box<ScreenError>,
    },
}

impl std::fmt::Display for ScreenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScreenError::Unsupported => write!(f, "screen capture is not supported here"),
            ScreenError::SourceGone => write!(f, "the capture source is no longer available"),
            ScreenError::EncoderUnavailable => write!(f, "no usable video encoder was found"),
            ScreenError::Io(e) => write!(f, "screen capture I/O error: {e}"),
            ScreenError::Sink(e) => write!(f, "screen capture could not be written: {e}"),
            ScreenError::AlreadyCapturing => write!(f, "a capture is already running"),
            ScreenError::Retained { path, cause } => write!(
                f,
                "screen capture could not finish, but {} still holds the recording: {cause}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for ScreenError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ScreenError::Retained { cause, .. } => Some(cause.as_ref()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The three FIXED variants carry no caller-supplied data at all — their
    // Display text is a constant, so it can never leak a value a caller
    // passed in (there is nothing to interpolate). This must hold for every
    // fixed variant, not just the one previously asserted, or a future
    // variant added without a matching test could silently start
    // interpolating.
    #[test]
    fn fixed_variants_render_a_constant_message_with_no_caller_data() {
        assert_eq!(
            ScreenError::Unsupported.to_string(),
            "screen capture is not supported here"
        );
        assert_eq!(
            ScreenError::SourceGone.to_string(),
            "the capture source is no longer available"
        );
        assert_eq!(
            ScreenError::EncoderUnavailable.to_string(),
            "no usable video encoder was found"
        );
        assert_eq!(
            ScreenError::AlreadyCapturing.to_string(),
            "a capture is already running"
        );
    }

    // `Io`, unlike the fixed variants above, deliberately DOES carry and
    // render caller-supplied data (the underlying I/O error text) — this
    // pins that the two variant kinds behave differently on purpose,
    // rather than asserting `Io` also renders nothing caller-supplied
    // (which would be false and was the previous version of this test's
    // actual, misleadingly-named, assertion).
    #[test]
    fn io_variant_interpolates_the_caller_supplied_message() {
        assert!(ScreenError::Io("disk full".into())
            .to_string()
            .contains("disk full"));
    }

    // `Sink`, like `Io`, deliberately carries and renders OS-supplied text:
    // a sink failure the user can act on ("no H.264 encoder") is useless
    // without the message. Pinned separately so the fixed/interpolating
    // split above stays a deliberate distinction rather than an accident.
    #[test]
    fn sink_variant_interpolates_the_underlying_message() {
        assert!(ScreenError::Sink("MF_E_TOPO_CODEC_NOT_FOUND".into())
            .to_string()
            .contains("MF_E_TOPO_CODEC_NOT_FOUND"));
    }

    // `Retained` exists so a caller can offer the user the `.part` file a
    // failed stop deliberately left behind — that only works if the path
    // is real DATA on the error, not just text baked into the message.
    #[test]
    fn retained_carries_the_part_path_as_typed_data_and_renders_the_cause() {
        let err = ScreenError::Retained {
            path: std::path::PathBuf::from("/vault/Screen Recordings/.foo.mp4.part"),
            cause: Box::new(ScreenError::Io("disk full".into())),
        };
        // The path must be readable back as a PATH, not re-parsed out of
        // the message.
        let ScreenError::Retained { path, .. } = &err else {
            unreachable!("constructed as Retained");
        };
        assert_eq!(
            path,
            &std::path::PathBuf::from("/vault/Screen Recordings/.foo.mp4.part")
        );
        let rendered = err.to_string();
        assert!(rendered.contains("/vault/Screen Recordings/.foo.mp4.part"));
        assert!(
            rendered.contains("disk full"),
            "the cause must still surface: {rendered}"
        );
    }

    #[test]
    fn retained_exposes_its_cause_through_the_error_trait() {
        use std::error::Error as _;
        let err = ScreenError::Retained {
            path: std::path::PathBuf::from("/tmp/x.mp4.part"),
            cause: Box::new(ScreenError::Sink("MF_E_INVALIDMEDIATYPE".into())),
        };
        let source = err.source().expect("Retained always carries a cause");
        assert!(source.to_string().contains("MF_E_INVALIDMEDIATYPE"));
    }
}
