//! `read_sidecar`'s bound (final whole-branch review M4), split from
//! `staging.rs`' inline tests for its 800-line cap.

use super::*;

fn sidecar(base: &str) -> StagedSidecar {
    StagedSidecar {
        base: base.to_string(),
        vault_id: "v1".into(),
        source_title: "Demo".into(),
        source_kind: "screen".into(),
        duration_ms: 6_100,
        width: 1366,
        height: 768,
        recorded_at: "2026-09-22T09:00:00Z".into(),
        ..Default::default()
    }
}

// A sidecar is hand-editable and was read whole into memory, however
// large. One over the bound is not a staged capture this app wrote, and is
// read as none — the posture a malformed one already gets.
#[test]
fn an_oversized_sidecar_is_not_read() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sidecar(dir.path(), "A", &sidecar("A")).unwrap();
    assert!(read_sidecar(&path).is_some(), "precondition: it reads");
    let mut bytes = std::fs::read(&path).unwrap();
    bytes.resize(MAX_SIDECAR_BYTES as usize + 1, b' ');
    std::fs::write(&path, bytes).unwrap();
    assert!(read_sidecar(&path).is_none());
}
