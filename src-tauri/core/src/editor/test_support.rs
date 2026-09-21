//! Shared test fixtures for `core::editor`'s own test modules.
//!
//! Every task from here on hand-built a valid `Project` for its tests
//! (`time.rs`'s `empty_project`, `validate_tests.rs`'s own copy) — a third
//! independent copy of the same defaults (schema id, a supported canvas,
//! `master_gain`, empty collections, an empty `Destination`) was flagged in
//! review as a duplication trend rather than a one-off. This module is the
//! single shared builder new tests reach for instead: start from
//! `minimal_project()` and override only the fields the test cares about.
//!
//! `#[cfg(test)] pub(crate)`: this exists only for `core`'s own test builds
//! and is never part of the crate's public API.

use super::model::{Canvas, Destination, Project};
use super::{Map, PROJECT_SCHEMA};

/// A minimal `Project` that already passes `validate_project` (a supported
/// canvas, an in-range `master_gain`, every collection empty). Callers
/// override whichever fields their test is actually about — id, title,
/// canvas, assets/tracks/clips, destination — rather than restating every
/// default.
pub(crate) fn minimal_project() -> Project {
    Project {
        schema: PROJECT_SCHEMA.to_string(),
        id: "project".to_string(),
        title: "Project".to_string(),
        canvas: Canvas {
            width: 1280,
            height: 720,
            fps: 30,
            extra: Map::new(),
        },
        master_gain: 1.0,
        assets: Vec::new(),
        tracks: Vec::new(),
        clips: Vec::new(),
        effects: Vec::new(),
        markers: Vec::new(),
        transitions: Vec::new(),
        captions: None,
        destination: Destination {
            vault: String::new(),
            folder: String::new(),
            dated: false,
            extra: Map::new(),
        },
        extra: Map::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::validate_project;

    #[test]
    fn minimal_project_is_valid() {
        assert!(
            validate_project(&minimal_project()).is_ok(),
            "the shared minimal project fixture must itself be schema-valid"
        );
    }
}
