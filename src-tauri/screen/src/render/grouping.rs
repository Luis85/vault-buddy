//! The crossfade-grouping predicate shared by `video_graph::group` and
//! `audio_graph::groups` (Task 44 fix round 1). A review found the two
//! copies had already diverged in what their CONSUMERS could do with a
//! group once formed -- video's `unit_source` chains any number of
//! members, audio's `emit_group` silently dropped every member past the
//! second. Factoring the shared PREDICATE into one place cannot by itself
//! fix a consumer that mishandles the shape it defines, but it removes the
//! one way the two copies could disagree about what counts as a run in the
//! first place, which is what let them diverge unnoticed.
//!
//! A "run" is a maximal sequence of same-track items where each one
//! (after the first) is the planned crossfade partner of the one before
//! it: the previous item carries a transition OUT, this one carries a
//! transition IN, and their OUTPUT spans actually overlap. Whether the two
//! transitions are really the SAME one is never checked directly here --
//! both sides independently name a transition of some kind/duration, and
//! the reference model permits at most one incoming and one outgoing
//! transition per clip (`model_cues::Transition`'s own doc), so an
//! agreeing pair of "outgoing here, incoming there, spans overlap" is
//! never a coincidence in a plan this codebase produces. An item that
//! merely abuts or gaps its neighbour -- even one nominally carrying a
//! transition on both sides -- does not join a run at that boundary.

/// Does `item` continue the run `prev` ended? Both callers pass their own
/// item type's track index, whether it is (respectively) the outgoing or
/// incoming half of a planned transition, and the OUTPUT-time bound the
/// overlap is measured against -- `video_graph::Item`'s methods and
/// `AudioContribution`'s own fields have different shapes, so this stays a
/// plain predicate over primitives rather than a shared trait.
pub(super) fn joins_run(
    prev_track: usize,
    prev_has_outgoing_transition: bool,
    prev_output_end: u64,
    item_track: usize,
    item_has_incoming_transition: bool,
    item_output_start: u64,
) -> bool {
    prev_track == item_track
        && prev_has_outgoing_transition
        && item_has_incoming_transition
        && item_output_start < prev_output_end
}

#[cfg(test)]
mod tests {
    use super::joins_run;

    #[test]
    fn a_same_track_overlapping_outgoing_into_incoming_pair_joins() {
        assert!(joins_run(0, true, 2_000, 0, true, 1_800));
    }

    #[test]
    fn a_different_track_never_joins() {
        assert!(!joins_run(0, true, 2_000, 1, true, 1_800));
    }

    #[test]
    fn no_outgoing_transition_on_prev_never_joins() {
        assert!(!joins_run(0, false, 2_000, 0, true, 1_800));
    }

    #[test]
    fn no_incoming_transition_on_item_never_joins() {
        assert!(!joins_run(0, true, 2_000, 0, false, 1_800));
    }

    // Even a nominally transition-carrying pair does not join once their
    // spans stop overlapping -- the `check_track_overlaps` precedent this
    // predicate leans on only guarantees overlap WHILE a transition is
    // genuinely in force, not for every pair that happens to carry one.
    #[test]
    fn items_that_merely_abut_or_gap_do_not_join() {
        assert!(!joins_run(0, true, 2_000, 0, true, 2_000));
        assert!(!joins_run(0, true, 2_000, 0, true, 2_100));
    }
}
