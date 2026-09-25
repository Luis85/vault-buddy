//! The shared wire table (final whole-branch review I7): one instance of
//! every `EditorCommand` kind, read from `tests/fixtures/editorCommandWire.ts`
//! — the SAME file `vue-tsc` checks against the TypeScript union
//! (`satisfies EditorCommand[]`). A kind or field renamed on one side only,
//! a payload field one side does not know, or a variant nobody listed turns
//! one of the two suites red.

use std::collections::BTreeSet;

use super::EditorCommand;

const FIXTURE: &str = include_str!("../../../../../tests/fixtures/editorCommandWire.ts");

/// Every variant, by position — an EXHAUSTIVE match, so a variant added to
/// the enum does not compile until it is counted here (and then the table
/// must list it too).
fn variant_index(command: &EditorCommand) -> usize {
    use EditorCommand::*;
    match command {
        Rename(_) => 0,
        Undo => 1,
        Redo => 2,
        InsertClip(_) => 3,
        UpdateClip(_) => 4,
        SplitClip(_) => 5,
        TrimClip(_) => 6,
        DeleteClips(_) => 7,
        MoveClips(_) => 8,
        ReorderClip(_) => 9,
        GroupClips(_) => 10,
        UngroupClips(_) => 11,
        DuplicateClips(_) => 12,
        PasteFragment(_) => 13,
        CutClips(_) => 14,
        AddTrack(_) => 15,
        RenameTrack(_) => 16,
        MoveTrack(_) => 17,
        SetTrackFlags(_) => 18,
        DeleteTrack(_) => 19,
        SetClipMix(_) => 20,
        SetMasterGain(_) => 21,
        DetachAudio(_) => 22,
        SetFades(_) => 23,
        AddTransition(_) => 24,
        SetTransitionDuration(_) => 25,
        RemoveTransition(_) => 26,
        SetSpeed(_) => 27,
        SetLayout(_) => 28,
        SetAdjustments(_) => 29,
        SetCanvas(_) => 30,
        AddCard(_) => 31,
        UpdateCard(_) => 32,
        InsertIntro(_) => 33,
        AddEffect(_) => 34,
        UpdateEffect(_) => 35,
        RemoveEffect(_) => 36,
        SetCaptionSettings(_) => 37,
        AddCaption(_) => 38,
        UpdateCaption(_) => 39,
        SplitCaption(_) => 40,
        RemoveCaptions(_) => 41,
        AddMarker(_) => 42,
        UpdateMarker(_) => 43,
        RemoveMarker(_) => 44,
        SetDestination(_) => 45,
    }
}

const VARIANTS: usize = 46;

fn wire_entries() -> Vec<serde_json::Value> {
    let (_, rest) = FIXTURE
        .split_once("// BEGIN WIRE")
        .expect("the fixture's BEGIN WIRE marker");
    let (json, _) = rest
        .split_once(" satisfies EditorCommand[]; // END WIRE")
        .expect("the fixture's END WIRE marker");
    serde_json::from_str(json).expect("the part between the markers is JSON")
}

#[test]
fn the_shared_wire_table_round_trips_every_entry_exactly() {
    for entry in wire_entries() {
        let command: EditorCommand = serde_json::from_value(entry.clone())
            .unwrap_or_else(|e| panic!("Rust refuses {entry}: {e}"));
        let back = serde_json::to_value(&command).expect("serializes");
        assert_eq!(back, entry, "the wire spelling drifted");
    }
}

#[test]
fn the_shared_wire_table_lists_every_variant_exactly_once() {
    let entries = wire_entries();
    assert_eq!(
        entries.len(),
        VARIANTS,
        "one entry per EditorCommand variant"
    );
    let indices: BTreeSet<usize> = entries
        .iter()
        .map(|e| variant_index(&serde_json::from_value(e.clone()).expect("an EditorCommand")))
        .collect();
    assert_eq!(
        indices.len(),
        VARIANTS,
        "a variant is listed twice, another never"
    );
}
