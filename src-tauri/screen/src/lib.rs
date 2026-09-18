//! Screen Capture engine.
//!
//! What compiles where: `clock` and `select` are pure and build and test on
//! ANY platform — they carry this feature's correctness precisely because no
//! CI runner can record a screen. `engine` is the Windows-only surface, a
//! stub until phase 2.

pub mod clock;
pub mod engine;
pub mod select;

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
}

impl std::fmt::Display for ScreenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScreenError::Unsupported => write!(f, "screen capture is not supported here"),
            ScreenError::SourceGone => write!(f, "the capture source is no longer available"),
            ScreenError::EncoderUnavailable => write!(f, "no usable video encoder was found"),
            ScreenError::Io(e) => write!(f, "screen capture I/O error: {e}"),
        }
    }
}

impl std::error::Error for ScreenError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_engine_stub_reports_unsupported_rather_than_panicking() {
        // The Linux compile gate builds this crate; the stub must degrade,
        // never abort, or CI dies instead of reporting.
        assert_eq!(engine::start_capture(), Err(ScreenError::Unsupported));
    }

    #[test]
    fn errors_render_without_interpolating_caller_data_into_the_variant() {
        assert_eq!(
            ScreenError::SourceGone.to_string(),
            "the capture source is no longer available"
        );
        assert!(ScreenError::Io("disk full".into())
            .to_string()
            .contains("disk full"));
    }
}
