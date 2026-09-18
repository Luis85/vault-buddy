//! The Windows-only capture engine surface.
//!
//! Phase 1 ships the shape and a stub only: the real implementation
//! (windows-capture frame acquisition + a Media Foundation SinkWriter)
//! arrives in phase 2, after the fragmented-MP4 spike settles the staged
//! container format. The non-Windows arm exists so the Linux compile gate
//! and every pure test in this crate keep running (spec §4.1).

use crate::ScreenError;

/// Begin capturing. Phase 2 replaces this signature with the real parameter
/// set; until then it exists so callers and the stub agree on the shape.
///
/// One unconditional body, NOT a #[cfg(windows)] / #[cfg(not(windows))] pair:
/// phase 1 has nothing platform-specific to say, and two arms with identical
/// bodies is dead weight that reads as though they differ. Phase 2 introduces
/// the cfg split when the Windows arm actually diverges.
pub fn start_capture() -> Result<(), ScreenError> {
    Err(ScreenError::Unsupported)
}
