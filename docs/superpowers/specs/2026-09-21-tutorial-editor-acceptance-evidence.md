# Tutorial Editor — Acceptance Evidence

**Written by Task 60** (the plan's last task), 2026-09-25, against branch
`claude/vault-buddy-improvement-polish-95f5bc`. One row per F-ID of the
concept bundle's feature catalog (`docs/concepts/vault-buddy-editor/contracts/feature-catalog.json`,
50 F-IDs — the bundle is a frozen input and is not edited). The ADR
(`2026-09-21-tutorial-editor-integration-design.md`) §5 names each F-ID's
authoritative module; this file names the evidence.

**This file is checked by `tests/editorEvidence.test.ts`**: every F-01..F-50
must appear exactly once, every row must name at least one test file that
exists, and every `path#name` token must name a test that appears verbatim in
that file — so a renamed or deleted test turns CI red instead of leaving a
row that still reads "covered". The GAP mapping below is checked the same
way against `docs/Gaps.md`.

Reading a row:

- **Automated evidence** — `path#test name` tokens (a Rust `#[test]` fn, or a
  Vitest/Playwright `it(...)` title). Rust tests under `src-tauri/core/` and
  `src-tauri/screen/` run in CI's `rust-core` job on Linux; the shell's
  (`src-tauri/src/`) in `linux-app`; `tests/*.test.ts` in `frontend`
  (Vitest, happy-dom); `tests/e2e/*.spec.ts` in `frontend` (Playwright,
  Chromium against the built `dist/`). The ffmpeg round trips
  (`src-tauri/screen/tests/render_roundtrip.rs`, `render_graph_roundtrip.rs`,
  `media_derive_tests.rs`' `*_round_trip_through_real_ffmpeg`) SKIP visibly
  without ffmpeg; CI installs it.
- **Native result** — the manual Windows checklist
  (`2026-09-21-tutorial-editor-windows-verification.md`) rows that cover what
  no automated gate can observe. **No row has a result yet**: every native
  result below is *not yet evaluated*, which is not the same as failed.

## F-ID evidence

| F-ID | Capability | Automated evidence | Native result |
| --- | --- | --- | --- |
| F-01 | Native capture handoff | `src-tauri/src/editor/session_commands_tests.rs#open_staged_resolves_the_vault_from_the_sidecar`, `src-tauri/src/editor/session_commands_tests.rs#open_staged_twice_returns_the_same_project`, `src-tauri/core/src/editor/migrate_tests.rs#untouched_capture_becomes_one_whole_clip`, `tests/screenCaptureEditHandoff.test.ts#stillSaving does not expose Edit`, `tests/editorRoot.test.ts#opens the stashed base through editor_open_staged` | not yet evaluated — T1–T3, T62 |
| F-02 | Local media import | `src-tauri/src/editor/media_import_tests.rs#a_failing_file_does_not_discard_the_successful_ones`, `src-tauri/src/editor/media_import_tests.rs#missing_ffmpeg_fails_only_video_and_audio_and_names_ffmpeg`, `tests/editorMediaLibrary.test.ts#Import starts a job, shows its progress, cancels it, and lists per-file errors at the end` | not yet evaluated — T11 |
| F-03 | Reconnect originals | `src-tauri/core/src/editor/relink_tests.rs#same_size_same_duration_twice_is_ambiguous_and_unselected`, `src-tauri/src/editor/relink_media_tests.rs#a_unique_match_is_copied_and_recorded_from_its_own_probe`, `tests/editorReconnect.test.ts#ambiguous assets require an explicit choice` | not yet evaluated — T22 |
| F-04 | Multi-track video | `src-tauri/core/src/editor/render_plan_tests.rs#upper_tracks_are_composited_last`, `src-tauri/screen/tests/render_roundtrip.rs#upper_layer_covers_lower_layer`, `tests/editorTimelineView.test.ts#renders track lanes in project.tracks order` | not yet evaluated — T6, T23 |
| F-05 | Multi-track audio (and opt-in stems) | `src-tauri/core/src/editor/commands/mix_tests.rs#set_clip_mix_sets_volume_and_mute_on_every_named_clip`, `src-tauri/screen/tests/render_roundtrip.rs#two_audible_sources_both_present`, `src-tauri/screen/src/render/audio_graph_tests.rs#amix_does_not_normalise`, `src-tauri/core/src/editor/migrate_stem_tests.rs#migration_creates_one_audio_track_per_stem_and_mutes_the_embedded_mix`, `src-tauri/screen/src/session/stems_tests.rs#stems_default_off` | not yet evaluated — T12, T13, T42–T44 (R-H4) |
| F-06 | Track management | `src-tauri/core/src/editor/commands/tracks_tests.rs#locked_track_refuses_rename_move_delete_and_flag_changes_but_allows_unlock`, `src-tauri/core/src/editor/commands/tracks_tests.rs#move_track_reorders_compositing_order`, `tests/editorTrackHeader.test.ts#lock toggles through setTrackFlags` | not yet evaluated — no dedicated row |
| F-07 | Split | `src-tauri/core/src/editor/commands/clips_tests.rs#split_conserves_total_duration`, `src-tauri/core/src/editor/commands/clips_tests.rs#split_moves_whole_cues_and_splits_straddling_cues`, `tests/editorActions.test.ts#split uses the right-clicked time, not the playhead` | not yet evaluated — T6 |
| F-08 | Trim | `src-tauri/core/src/editor/commands/clips_tests.rs#trim_keeps_source_cue_times`, `src-tauri/core/src/editor/commands/clips_tests.rs#trim_below_the_minimum_is_refused_naming_100ms`, `tests/editorClipSection.test.ts#numeric start entry sends trimClip with unchanged in/out` | not yet evaluated — T6 |
| F-09 | Delete and ripple | `src-tauri/core/src/editor/commands/clips_tests.rs#delete_close_gap_ripples_only_its_track`, `src-tauri/core/src/editor/commands/clips_tests.rs#delete_leave_gap_moves_nothing`, `src-tauri/core/src/editor/commands/clips_tests.rs#ripple_through_a_group_is_refused_atomically`, `tests/editorActions.test.ts#cut and delete both build deleteClips-shaped commands, cut always closing the gap` | not yet evaluated — T6 |
| F-10 | Move and reorder | `src-tauri/core/src/editor/commands/clips_move_reorder_tests.rs#reorder_swaps_adjacent_clips_keeping_span`, `src-tauri/core/src/editor/commands/clips_move_reorder_tests.rs#move_into_overlap_rejects_the_whole_group`, `src-tauri/screen/tests/render_roundtrip.rs#split_and_reorder_timing` | not yet evaluated — T6 |
| F-11 | Multi-selection and grouping | `src-tauri/core/src/editor/commands/groups_tests.rs#group_clips_requires_at_least_two_clips_and_mints_a_fresh_group_id`, `src-tauri/core/src/editor/commands/clips_move_reorder_tests.rs#group_move_with_a_locked_member_changes_nothing`, `tests/editorTimelineView.test.ts#Ctrl+click toggles a clip in and out of the selection` | not yet evaluated — T6 |
| F-12 | Copy, cut, paste, duplicate | `src-tauri/core/src/editor/commands/groups_tests.rs#paste_repoints_cues_to_new_clips`, `src-tauri/core/src/editor/commands/groups_tests.rs#duplicate_mints_fresh_ids_everywhere`, `tests/editorClipboard.test.ts#cut copies then deletes as one undo step`, `tests/editorFragment.test.ts#copies attached cues and markers only` | not yet evaluated — T64 |
| F-13 | Undo and redo | `src-tauri/core/src/editor/history.rs#push_evicts_the_oldest_beyond_max_history`, `src-tauri/core/src/editor/session.rs#undo_advances_the_revision`, `src-tauri/core/src/editor/session.rs#undo_of_paste_restores_the_whole_operation`, `tests/editorShell.test.ts#Ctrl+Z sends undo through the SAME registry the toolbar/menu read` | not yet evaluated — T64 |
| F-14 | Timeline navigation | `src-tauri/core/src/editor/time.rs#shared_fixture_table_has_ten_cases`, `tests/editorTimeFixtures.test.ts#the fixture table has 10 cases`, `tests/editorTimelineView.test.ts#a ruler click moves the playhead, not the selection`, `tests/editorWorkspaceStore.test.ts#playhead clamps to project duration` | not yet evaluated — T8 |
| F-15 | Contextual menus | `tests/editorContextMenu.test.ts#Enter activates the focused item only when it is enabled`, `tests/editorActions.test.ts#every disabled action in an empty context has a non-empty reason`, `tests/editorActions.test.ts#resolves every one of the 39 declared action ids exactly once` | not yet evaluated — T58 |
| F-16 | Clip speed | `src-tauri/core/src/editor/commands/layout_tests.rs#speed_two_halves_output_duration_and_keeps_cue_source_times`, `src-tauri/screen/tests/render_roundtrip.rs#speed_two_halves_duration`, `src-tauri/screen/src/render/audio_graph_tests.rs#atempo_chain_splits_four_into_two_twos`, `tests/editorLayoutSpeedSections.test.ts#numeric speed is bounded to 0.25x..4x and says how long the clip plays` | not yet evaluated — T15 |
| F-17 | Video fades | `src-tauri/core/src/editor/commands/fades.rs#fade_longer_than_half_is_refused_naming_the_limit`, `src-tauri/screen/src/render/video_layers_tests.rs#fade_is_alpha_not_black`, `tests/editorFades.test.ts#dragging the fade-in handle by 200ms sends exactly what typing 200 into the Fade in field sends` | not yet evaluated — T64 |
| F-18 | Audio fades | `src-tauri/screen/tests/render_roundtrip.rs#audio_fade_ramps`, `src-tauri/screen/src/render/audio_graph_tests.rs#fade_curves_map_to_ffmpeg_names`, `src-tauri/core/src/editor/commands/fades.rs#shared_fade_curve_table_has_six_cases_and_agrees`, `tests/editorFades.test.ts#equal-power midpoint is 0.7071, not 0.5` | not yet evaluated — T64 |
| F-19 | Clip transitions | `src-tauri/core/src/editor/commands/transitions_tests.rs#transition_shortens_only_its_track`, `src-tauri/screen/tests/render_roundtrip.rs#dissolve_blends_midpoint`, `src-tauri/screen/tests/render_graph_roundtrip.rs#a_dissolve_blends_the_pair_across_their_overlap`, `tests/editorTransitions.test.ts#an audio pair crossfades at equal power` | not yet evaluated — T64 |
| F-20 | Webcam recording | `src-tauri/src/editor/webcam_commands_tests.rs#a_real_webm_streamed_in_chunks_lands_indexed`, `src-tauri/src/editor/webcam_commands_tests.rs#begin_is_refused_during_a_screen_capture`, `src-tauri/core/src/editor/take_tests.rs#sequence_gaps_are_refused`, `tests/editorWebcamDialog.test.ts#opening the dialog requests no device`, `tests/webcamRecorder.test.ts#chunks are sent strictly in order even if one append is slow` | not yet evaluated — T32–T36 (R-H1) |
| F-21 | Presenter picture-in-picture | `tests/editorPipLayout.test.ts#corner preset keeps a circle circular on a portrait canvas`, `tests/editorPipLayout.test.ts#resize handle drag sends one setLayout`, `src-tauri/core/src/editor/migrate_webcam_tests.rs#the_presenter_placement_matches_the_shared_table`, `tests/editorWebcamDialog.test.ts#add to timeline inserts a separate clip on a new top track` | not yet evaluated — T15, T35 |
| F-22 | Synchronized screen + webcam | `src-tauri/core/src/editor/migrate_webcam_tests.rs#migration_places_the_webcam_on_its_own_track_at_its_offset`, `src-tauri/src/editor/session_commands_tests.rs#open_staged_places_the_webcam_for_its_measured_length`, `src-tauri/screen/src/session/webcam_tests.rs#webcam_timestamps_map_into_the_capture_clock`, `src-tauri/screen/src/session/webcam_tests.rs#start_without_a_webcam_is_byte_identical_to_today`, `tests/captureWebcams.test.ts#drops malformed rows and non-lists` | not yet evaluated — T37–T41 (R-H3) |
| F-23 | Source layout and transforms | `src-tauri/core/src/editor/commands/layout_tests.rs#layout_round_trips_through_serialization`, `src-tauri/core/src/editor/commands/layout_tests.rs#multi_clip_layout_is_atomic`, `src-tauri/screen/src/render/video_layers_tests.rs#rotation_ninety_uses_transpose_one`, `tests/editorLayoutGeometry.test.ts#cover crops a window of the frame` | not yet evaluated — T15 |
| F-24 | Audio detachment | `src-tauri/core/src/editor/commands/mix_tests.rs#detach_creates_a_linked_asset_and_mutes_the_original`, `src-tauri/core/src/editor/commands/mix_tests.rs#detach_of_a_silent_source_is_refused`, `src-tauri/core/src/editor/render_plan_tests.rs#detached_audio_reads_its_linked_source`, `tests/editorAudioSection.test.ts#Detach audio lands on the free audio track through the shared action registry` | not yet evaluated — T12 |
| F-25 | Mixer and monitoring | `src-tauri/core/src/editor/render_plan_tests.rs#monitor_mute_cannot_affect_the_plan`, `src-tauri/core/src/editor/render_plan_tests.rs#solo_silences_non_soloed_tracks`, `tests/editorMixerPopover.test.ts#monitoring mute never sends a command`, `tests/editorWorkspaceStore.test.ts#monitor mute is workspace state only` | not yet evaluated — T9, T13 |
| F-26 | Waveforms | `src-tauri/core/src/editor/peaks.rs#peaks_fold_is_chunk_boundary_independent`, `src-tauri/src/editor/media_derive_tests.rs#peaks_round_trip_through_real_ffmpeg`, `src-tauri/src/editor/media_derive_tests.rs#a_cancelled_decode_reports_cancelled`, `tests/editorWaveform.test.ts#missing ffmpeg shows the install hint` | not yet evaluated — T14 |
| F-27 | Text callouts | `src-tauri/core/src/editor/commands/cues_tests.rs#each_kind_requires_its_props`, `src-tauri/screen/tests/render_roundtrip.rs#text_cue_is_burned_in`, `src-tauri/screen/src/render/ass_tests.rs#text_is_escaped`, `tests/editorCueOverlay.test.ts#draws every kind: text, highlight, spotlight, step and an opaque mask` | not yet evaluated — T17, T23, T24 |
| F-28 | Arrows | `src-tauri/screen/src/render/ass_tests.rs#arrow_matches_the_shared_fixture`, `src-tauri/core/src/editor/validate_tests.rs#arrow_effect_missing_x2_y2_is_rejected`, `tests/editorCueOverlay.test.ts#draws the arrow as the shared polygons`, `tests/editorCueOverlay.test.ts#endpoint drag sends one updateEffect` | not yet evaluated — T17 |
| F-29 | Highlights | `src-tauri/screen/src/render/ass_tests.rs#highlight_ring_is_two_nested_rects_at_the_stroke_inset`, `tests/editorCueOverlay.test.ts#draws every kind: text, highlight, spotlight, step and an opaque mask` | not yet evaluated — T17 |
| F-30 | Spotlight | `src-tauri/screen/src/render/ass_tests.rs#spotlight_dim_converts_to_ass_alpha_at_two_values`, `tests/editorCueOverlay.test.ts#draws every kind: text, highlight, spotlight, step and an opaque mask` | not yet evaluated — T17 |
| F-31 | Zoom and focus | `src-tauri/core/src/editor/render_plan_tests.rs#a_zoom_keeps_its_ramp_through_a_range`, `src-tauri/screen/tests/render_graph_roundtrip.rs#a_composed_render_draws_every_layer_in_order_and_zooms_the_stage`, `src-tauri/core/src/editor/validate_tests.rs#zoom_effect_missing_factor_is_rejected`, `src-tauri/screen/src/render/ass_tests.rs#zoom_cues_produce_no_dialogue` | not yet evaluated — T17 |
| F-32 | Numbered steps | `src-tauri/screen/src/render/ass_tests.rs#step_draws_a_circle_and_a_separate_number_dialogue`, `src-tauri/core/src/editor/validate_tests.rs#step_effect_missing_number_is_rejected` | not yet evaluated — T17 |
| F-33 | Privacy covers | `src-tauri/screen/tests/render_graph_roundtrip.rs#a_burned_mask_cue_is_a_real_opaque_rectangle`, `src-tauri/screen/src/render/ass_tests.rs#mask_has_no_fade`, `src-tauri/core/src/editor/checks_tests.rs#every_privacy_cover_is_a_warning_to_check_the_render` | not yet evaluated — T17 |
| F-34 | Caption authoring and import | `src-tauri/core/src/editor/captions_io_tests.rs#srt_parses_crlf_and_multiline_cues`, `src-tauri/src/editor/caption_commands_tests.rs#import_converts_to_source_time_and_reports_the_skipped_cue`, `src-tauri/core/src/editor/commands/captions_tests.rs#import_converts_output_to_source_time_at_speed_two`, `tests/editorCaptions.test.ts#library never claims automatic transcription` | not yet evaluated — T18 |
| F-35 | Caption outputs | `src-tauri/src/editor/subtitle_commands_tests.rs#subtitle_export_uses_output_time`, `src-tauri/core/src/editor/render_plan_tests.rs#captions_burn_in_only_when_enabled_and_burned_in`, `src-tauri/screen/src/render/ass_tests.rs#build_caption_ass_wires_the_position_into_the_style`, `src-tauri/core/src/editor/captions_io_tests.rs#srt_round_trips_through_export` | not yet evaluated — T18, T29 |
| F-36 | Chapter markers | `src-tauri/core/src/editor/commands/cues_tests.rs#chapters_resolve_to_output_time_after_trim`, `src-tauri/core/src/editor/note.rs#note_lists_chapters_at_range_relative_time`, `tests/editorChapters.test.ts#resolves source markers to OUTPUT time, sorted, trimmed ones omitted` | not yet evaluated — T18 |
| F-37 | Title and background cards | `src-tauri/core/src/editor/commands/cards_tests.rs#intro_shifts_every_track_together`, `src-tauri/core/src/editor/commands/cards_tests.rs#add_card_creates_the_builtin_asset_once_and_inserts_a_clip`, `src-tauri/screen/src/render/video_layers_tests.rs#a_card_is_a_color_source_sized_to_its_box`, `src-tauri/screen/src/render/ass_tests.rs#card_layout_matches_the_preview_centered_stack`, `tests/editorTitles.test.ts#titles library inserts a chapter card at the playhead` | not yet evaluated — no dedicated row |
| F-38 | Canvas formats | `src-tauri/core/src/editor/commands/layout_tests.rs#only_the_four_canvases_are_accepted`, `src-tauri/core/src/editor/migrate_tests.rs#nearest_canvas_picks_by_aspect`, `src-tauri/core/src/editor/checks_tests.rs#a_full_frame_source_of_another_aspect_needs_a_canvas_review` | not yet evaluated — T16, T47 |
| F-39 | Color treatment | `src-tauri/core/src/editor/commands/layout_tests.rs#adjustments_ranges_are_enforced`, `tests/editorColor.test.ts#none maps to the CSS identity filter`, `tests/editorColor.test.ts#a preset sends every field at once and None clears` | not yet evaluated — T16 |
| F-40 | Save editable project | `src-tauri/src/editor/save_commands_tests.rs#save_commits_the_acknowledged_revision`, `src-tauri/src/editor/save_commands_tests.rs#injected_write_failure_keeps_the_last_good_file`, `src-tauri/src/editor/package_commands_tests.rs#portable_export_then_import_reopens_identically`, `src-tauri/core/src/editor/package_tests.rs#ratio_bomb_is_rejected`, `tests/editorCutSaveReopen.test.ts#cuts through the UI, saves, and reopens to exactly what Rust acknowledged`, `tests/editorSaveDialog.test.ts#dialog shows success only after a receipt` | not yet evaluated — T21, T65 (R-H5) |
| F-41 | Rendered products | `src-tauri/src/editor/render_jobs_tests.rs#completed_render_records_an_immutable_product`, `src-tauri/src/editor/render_ledger_tests.rs#ledger_survives_without_saving_the_project`, `src-tauri/screen/tests/render_roundtrip.rs#identity_render_of_an_untouched_capture_is_lossless`, `tests/editorProductLibrary.test.ts#restore asks for confirmation and sends restore_product` | not yet evaluated — T23–T28, T62 (R-H2) |
| F-42 | Output review | `src-tauri/src/editor/render_review_tests.rs#review_renders_never_add_a_product_library_entry`, `src-tauri/screen/tests/render_roundtrip.rs#range_render_starts_at_zero`, `tests/editorProductLibrary.test.ts#watch uses the product url, not the preview`, `tests/editorRenderDialog.test.ts#fraction 1 without a terminal does not show complete` | not yet evaluated — T28 |
| F-43 | Companion note | `src-tauri/src/editor/publish_tests.rs#note_embeds_the_final_reserved_name`, `src-tauri/src/editor/publish_tests.rs#publish_never_overwrites`, `src-tauri/src/editor/publish_tests.rs#note_failure_keeps_the_video_and_warns`, `src-tauri/core/src/editor/note.rs#the_vault_template_is_additive`, `tests/editorPublishDialog.test.ts#publish defaults to the capture` | not yet evaluated — T29–T31, T62 |
| F-44 | Project and session recovery | `src-tauri/src/editor/recovery_tests.rs#journal_is_written_after_an_acknowledged_command`, `src-tauri/src/editor/recovery_tests.rs#open_with_recovery_starts_dirty_with_the_journals_project`, `src-tauri/src/editor/recovery_tests.rs#startup_sweep_repins_an_unpinned_project_whose_staged_base_still_exists`, `tests/editorRecoveryDialog.test.ts#Resume closes the clean session and reopens with the journal as the working copy`, `tests/editorCloseGuard.test.ts#dirty close offers Save, Keep and Discard` | not yet evaluated — T19, T20 |
| F-45 | Before-you-share checks | `src-tauri/core/src/editor/checks_tests.rs#no_quality_score_is_ever_emitted`, `src-tauri/core/src/editor/checks_tests.rs#missing_media_blocks_when_a_visible_clip_uses_the_file`, `src-tauri/src/editor/checks_commands_tests.rs#checks_read_missing_media_sound_and_open_takes_from_the_session`, `tests/editorChecks.test.ts#reconnect opens the media library and asks it for the Reconnect dialog` | not yet evaluated — T45–T47 |
| F-46 | Guided onboarding | `src-tauri/core/src/editor/guide_tests.rs#the_compiled_steps_are_the_22_lessons_in_order`, `src-tauri/src/editor/guide_commands_tests.rs#guide_progress_round_trips_through_the_app_wide_prefs_folder`, `tests/editorGuideCoach.test.ts#walking all 22 steps sends zero editor_execute and zero device or file requests`, `tests/editorOnboardingStore.test.ts#progress resumes at the exact step id`, `tests/e2e/editorGuide.spec.ts#coach resolves every target at 960x640` | not yet evaluated — T48–T55 |
| F-47 | Learning center | `tests/editorLearningCenter.test.ts#search ignores case and diacritics`, `tests/editorLearningCenter.test.ts#shortcut table matches shortcuts.ts`, `src-tauri/src/editor/guide_commands_tests.rs#guide_progress_file_round_trips_and_rejects_foreign_json` | not yet evaluated — T56, T57 |
| F-48 | Focused responsive shell | `tests/editorShell.test.ts#shows Save project and Render video as separate buttons`, `tests/editorShell.test.ts#below 1180px the library is a drawer, closed by default, and the header stays visible`, `tests/e2e/editorShell.spec.ts#exactly one preview toolbar row at 960x640`, `src-tauri/core/src/editor/workspace.rs#sanitize_drops_unknown_and_mistyped_fields` | not yet evaluated — T59, T60 (R-A2) |
| F-49 | Keyboard and accessible controls | `tests/e2e/editorKeyboard.spec.ts#keyboard-only journey completes`, `tests/e2e/editorKeyboard.spec.ts#forced colors keep selection visible`, `tests/editorA11y.test.ts#every icon-only button has an accessible name`, `tests/editorShortcutsWired.test.ts#every key the shortcut table lists does what the table says` | not yet evaluated — T53, T58 (R-A1) |
| F-50 | Local privacy and diagnostics | `src-tauri/src/editor/diagnostics_tests.rs#diagnostics_contain_no_paths_or_names`, `src-tauri/src/editor/redact_guard.rs#editor_logs_redact_paths`, `src-tauri/src/editor/redact.rs#a_path_becomes_a_stable_hash_and_nothing_else`, `tests/editorLearningCenter.test.ts#Export diagnostics asks Rust to write the file and says where it landed` | not yet evaluated — T61 |

## Cross-cutting invariants (ADR §8)

Not F-IDs, so not checked row by row, but each is pinned by a test that runs
in CI:

| # | Invariant | Pinned by |
| --- | --- | --- |
| 1 | Rust is the only authority for committed edits | `src-tauri/core/src/editor/session.rs#invalid_candidate_is_rejected_atomically`, `tests/editorProjectStore.test.ts#a non-increasing revision is ignored` |
| 2 | Every `editor_*` command checks the caller window | `src-tauri/src/editor/authz_guard.rs#every_editor_command_checks_the_caller_window`, `src-tauri/src/editor/capability_guard.rs` |
| 3 | The asset scope is R7's enumerated list | `src-tauri/src/tray.rs#the_asset_protocol_scope_is_pinned_to_staging_and_the_editor_project_media_dirs` |
| 5 | Products are immutable | `src-tauri/src/editor/publish_tests.rs#publish_leaves_the_product_byte_identical` |
| 6 | A pinned staged capture is never discarded or cleared | `src-tauri/src/staged_commands.rs#pinned_capture_cannot_be_discarded`, `src-tauri/src/staging_commands.rs#clear_skips_pinned_captures_and_counts_them` |
| 7 | Job progress travels only on per-job Channels | `src-tauri/src/editor/media_import_tests.rs#progress_sequence_strictly_increases_and_ends_with_exactly_one_terminal` |
| 8 | The render fast path keys on `is_identity` | `src-tauri/core/src/editor/render_plan_tests.rs#identity_detection_matches_is_untouched`, `src-tauri/screen/tests/render_roundtrip.rs#identity_render_is_a_remux` |
| 9 | `editor-time-cases.json` holds TS to Rust | `src-tauri/core/src/editor/time.rs#shared_fixture_table_has_ten_cases`, `tests/editorTimeFixtures.test.ts#the fixture table has 10 cases` |
| 10 | Guide actions never execute editor commands | `tests/editorGuideCoach.test.ts#walking all 22 steps sends zero editor_execute and zero device or file requests` |

Invariant 4 (project files written only through `write_atomic_replacing` or
an owned temp + `rename_noreplace`) is held by review and by the individual
writers' tests (`save_commands_tests.rs`, `package_commands_tests.rs`,
`subtitle_commands_tests.rs`), not by one structural scan.

## GAP placeholder mapping

The ADR (§7) and the plan named five gaps before their numbers existed. Each
maps to a real `docs/Gaps.md` entry:

| Placeholder | Entry | What it records |
| --- | --- | --- |
| GAP-N1 | GAP-193 | A webcam take does not claim `CaptureGuard`; the begin-side refusal is native (Task 49, ADR §9 (a)), the reverse direction is unrefused |
| GAP-N2 | GAP-194 | No native WebView2 permission handler restricts the camera to the editor window |
| GAP-N3 | GAP-173 | The preview approximates the render (GAP-145's successor) |
| GAP-N4 | GAP-212 | The ADR's wording: a published product occupies the disk twice until its project is discarded. The plan's own GAP-N4 wording (F36, publish has no resume-from-journal) is GAP-192 |
| GAP-N5 | GAP-170 | The app-wide ACL is verified by build and a generated-artifact replica, not inside a live app |

Other gaps found or opened while the increment landed: GAP-172 through
GAP-211 (each names its task), plus GAP-213 (the tutorial note's
`created-by` value, recorded by Task 60).

## Residual gates (ADR §7) — all OPEN

None of these can be closed on the development host or in CI. Each hardware
gate has checklist rows; none carries a result.

| Gate | What | Checklist rows | Status |
| --- | --- | --- | --- |
| R-H1 | Physical camera and microphone | T32–T36 | open, unrun |
| R-H2 | Windows ffmpeg against a real staged fMP4 (layers, libass fonts, xfade/acrossfade, encoder choice) | T23–T26, T62 | open, unrun |
| R-H3 | Synchronized webcam producer (drift, unplug, busy device) | T37–T41 | open, unrun |
| R-H4 | Per-input stems writer | T42–T44 | open, unrun |
| R-H5 | Disk full during save, render and publish on a real volume | T30 (publish), T65 (save and render) | open, unrun |
| R-A1 | Narrator and NVDA on WebView2 | T53, T58 | open, unrun |
| R-A2 | Windows contrast themes at 150 % and 200 % | T59, T60 | open, unrun |
| R-M1 | Memory: ten-minute 1080p30 tutorial, 3 video + 2 audio layers | T66 | open, unrun |
| R-P1 | Product decision: a native package limit beyond 200 MiB of media | — (a decision, not a check) | open |
| R-P2 | Product decision: the `zip` crate dependency (major 4, the one `tauri-plugin-updater` already locks) | — (a decision, not a check) | open |

The concept bundle's **final representative journey**
(`docs/concepts/vault-buddy-editor/docs/IMPLEMENTATION-PLAN.md` § Final
acceptance tasks for representative users) needs a human driving the real
app. It has NOT been walked; it is checklist row **T64**, unrun.

## Release checks run by Task 60

- **Branch completeness** (plan F33): `git log 9a8069c..HEAD --oneline`
  lists 120 commits before Task 60's own; 119 carry an `(editor)`, `(core)`
  or `(screen)` scope (the one other is `fix(editor,tests)`, the pre-Task 11
  chore commit). The plan requires at least 59.
- **Baselines**: `scripts/loc-baseline.json` gained no file over the branch
  (one existing entry shrank, 1282 → 1093, when Task 53 split
  `vault_config_screen.rs` out); `scripts/quality-baseline.json`'s five
  counters are unchanged (deadCode 0, complexFunctions 13,
  criticalComplexity 3, cloneGroups 0, circularDependencies 0), and
  `averageMaintainability` moved 91.6 → 90.6 under the controller's recorded
  rulings (each drop measured and justified in the baseline's one Tutorial
  Editor sentence; floor 90.0).
- **Gates**: the task report
  (`.superpowers/sdd/2026-09-21-tutorial-editor/task-60-report.md`) carries
  every gate's real output tail. `cargo llvm-cov`, `cargo machete` and
  `cargo deny` are not installed on the development host and were NOT run
  there; CI's `rust-core` job is their gate.
