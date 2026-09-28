//! `setFades{clipId, fadeInMs?, fadeOutMs?, fadeCurve?}` (Task 29; F-17,
//! F-18, DATA-MODEL.md § Fade and transition rules: "Edge fade envelopes
//! multiply the clip's alpha/audio amplitude. The reference limits each
//! fade to half the clip duration and supports documented curve choices").
//!
//! **The limit is half the clip's own OUTPUT duration** (`clip_end -
//! start_ms`, reusing `clips::clip_end` rather than a second copy of the
//! speed-aware duration formula) — the same rule `validate::check_clip`
//! already re-checks as a backstop (`fade_in_ms.saturating_mul(2) >
//! output_duration`). `fade_limit` is the ONE place that arithmetic lives;
//! `set_fades` below and `clips::trim_clip`'s own fade-clamp arm (F11) both
//! call it rather than restating `duration / 2`.
//!
//! **`gain_at` is the shared fade-curve algebra, held apart from the
//! frontend's `src/editor/fadeCurves.ts` by ONE fixture table**
//! (`tests/fixtures/editor-fade-cases.json`, `include_str!` — the
//! `time.rs`/`timeMap.ts` precedent, GAP-136's discipline): moving or
//! deleting that file breaks this crate's build, not just a TypeScript
//! import. It has no caller in THIS crate's production code today — the
//! render (Task 44) maps curve names straight to ffmpeg's own `afade`
//! curve arguments (`linear` -> `tri`, `smooth` -> `hsin`, `equal-power` ->
//! `qsin`) rather than evaluating this function, because ffmpeg computes
//! its own curve shape internally. It stays `pub` (and this module `pub
//! mod`, unlike its private siblings `clips`/`mix`/`tracks`) so it is
//! reachable at `vault_buddy_core::editor::commands::fades::gain_at` —
//! genuinely part of this crate's public surface, not merely alive for its
//! own `#[cfg(test)]` callers, the same posture `time::cue_output_span`
//! already has. `hsin` is a close but not bit-identical approximation of
//! this function's `smooth` (smoothstep) shape; that gap is recorded as
//! docs/Gaps.md GAP-173, not claimed as exact.

use crate::editor::error::EditorError;
use crate::editor::model::{FadeCurve, Project};

use super::clips::{clip_end, ensure_unlocked, find_clip, invalid_request};
use super::payloads::SetFadesPayload;

/// The gain (0..=1) a fade of progress `u` (0 = fade start, 1 = fully faded
/// in/out) has reached under `curve`. `linear` is `u` itself; `smooth` is
/// the smoothstep polynomial `3u^2 - 2u^3` — an S-curve that eases into and
/// out of the fade rather than crossing it at a constant rate; `equal-power`
/// is `sin(u * pi/2)`, a quarter sine whose MIDPOINT is `sin(pi/4) ≈
/// 0.7071` (`1/sqrt(2)`), not 0.5 — two clips crossfading with it sum to
/// constant PERCEIVED loudness rather than constant amplitude
/// (`sin²+cos²=1`, never `u+(1-u)=1` the way a linear cut would). `u` is
/// clamped to `[0,1]` defensively; every caller in this crate already
/// passes an in-range value.
pub fn gain_at(curve: FadeCurve, u: f64) -> f64 {
    let u = u.clamp(0.0, 1.0);
    match curve {
        FadeCurve::Linear => u,
        FadeCurve::Smooth => u * u * (3.0 - 2.0 * u),
        FadeCurve::EqualPower => (u * std::f64::consts::FRAC_PI_2).sin(),
    }
}

/// The maximum `fadeInMs`/`fadeOutMs` a clip whose own OUTPUT duration is
/// `duration_ms` may carry — half of it, floored. Equivalent to
/// `validate::check_clip`'s own `ms.saturating_mul(2) > duration` refusal
/// (proof: for `duration = 2k`, `ms*2>2k` iff `ms>k`; for `duration =
/// 2k+1`, `ms*2>2k+1` iff `ms>=k+1` iff `ms>k` since `ms` is an integer —
/// `k = duration/2` either way), stated as a single number so a refusal
/// message can NAME the limit rather than only the inequality it failed.
pub(super) fn fade_limit(duration_ms: u64) -> u64 {
    duration_ms / 2
}

/// `setFades{clipId, fadeInMs?, fadeOutMs?, fadeCurve?}`: refuses a payload
/// that changes nothing (the `setClipMix` precedent, `mix.rs`), an unknown
/// or locked-track clip, and either `*Ms` field that exceeds `fade_limit`
/// of the clip's CURRENT output duration — naming the limit in the message
/// rather than only the value that failed it. A payload may set any subset
/// of the three fields; the label distinguishes a pure curve change from a
/// pure fade-in/out edit and falls back to a generic label when more than
/// one field moves at once (a drag never sends more than one `*Ms` field —
/// `useTimelineDrag.ts`'s `endFade` — so that arm is reachable only from a
/// hand-built multi-field request).
pub(super) fn set_fades(
    project: &Project,
    payload: &SetFadesPayload,
) -> Result<(Project, String), EditorError> {
    if payload.fade_in_ms.is_none() && payload.fade_out_ms.is_none() && payload.fade_curve.is_none()
    {
        return Err(invalid_request(
            "setFades must set fadeInMs, fadeOutMs or fadeCurve",
        ));
    }
    let clip = find_clip(project, &payload.clip_id)?;
    ensure_unlocked(project, &clip.track_id)?;

    let duration = clip_end(clip) - clip.start_ms;
    let limit = fade_limit(duration);
    if let Some(v) = payload.fade_in_ms {
        if v > limit {
            return Err(invalid_request(format!(
                "fadeInMs {v} exceeds the {limit} ms maximum (half the clip's {duration} ms output duration)"
            )));
        }
    }
    if let Some(v) = payload.fade_out_ms {
        if v > limit {
            return Err(invalid_request(format!(
                "fadeOutMs {v} exceeds the {limit} ms maximum (half the clip's {duration} ms output duration)"
            )));
        }
    }

    let label = match (
        payload.fade_in_ms.is_some(),
        payload.fade_out_ms.is_some(),
        payload.fade_curve.is_some(),
    ) {
        (true, false, false) => "Fade in",
        (false, true, false) => "Fade out",
        (false, false, true) => "Fade curve",
        _ => "Set fades",
    };

    let mut candidate = project.clone();
    for c in candidate
        .clips
        .iter_mut()
        .filter(|c| c.id == payload.clip_id)
    {
        if let Some(v) = payload.fade_in_ms {
            c.fade_in_ms = v;
        }
        if let Some(v) = payload.fade_out_ms {
            c.fade_out_ms = v;
        }
        if let Some(v) = payload.fade_curve {
            c.fade_curve = v;
        }
    }
    Ok((candidate, label.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::commands::payloads::SetFadesPayload;
    use crate::editor::model::TrackKind;
    use crate::editor::test_support::{asset, clip, minimal_project, track};

    fn project_with_clip(fade_in_ms: u64, fade_out_ms: u64) -> Project {
        let mut p = minimal_project();
        p.assets
            .push(asset("a1", crate::editor::model::AssetKind::Video, 30_000));
        p.tracks.push(track("v1", TrackKind::Video, false));
        let mut c = clip("c1", "v1", "a1", 0, 0, 1_000);
        c.fade_in_ms = fade_in_ms;
        c.fade_out_ms = fade_out_ms;
        p.clips.push(c);
        p
    }

    // Named test 1/2 (brief): a 1000ms-duration clip's own limit is 500ms.
    #[test]
    fn fade_longer_than_half_is_refused_naming_the_limit() {
        let project = project_with_clip(0, 0);
        let err = set_fades(
            &project,
            &SetFadesPayload {
                clip_id: "c1".into(),
                fade_in_ms: Some(501),
                fade_out_ms: None,
                fade_curve: None,
            },
        )
        .unwrap_err();
        assert!(
            err.message.contains("500"),
            "message {:?} must name the 500 ms limit",
            err.message
        );

        // Exactly the limit is accepted.
        let (candidate, label) = set_fades(
            &project,
            &SetFadesPayload {
                clip_id: "c1".into(),
                fade_in_ms: Some(500),
                fade_out_ms: None,
                fade_curve: None,
            },
        )
        .unwrap();
        assert_eq!(candidate.clips[0].fade_in_ms, 500);
        assert_eq!(label, "Fade in");

        // fadeOutMs is bounded the same way.
        let err = set_fades(
            &project,
            &SetFadesPayload {
                clip_id: "c1".into(),
                fade_in_ms: None,
                fade_out_ms: Some(501),
                fade_curve: None,
            },
        )
        .unwrap_err();
        assert!(err.message.contains("500"));
    }

    #[test]
    fn setting_only_the_curve_changes_no_ms_field_and_labels_itself() {
        let project = project_with_clip(200, 300);
        let (candidate, label) = set_fades(
            &project,
            &SetFadesPayload {
                clip_id: "c1".into(),
                fade_in_ms: None,
                fade_out_ms: None,
                fade_curve: Some(FadeCurve::EqualPower),
            },
        )
        .unwrap();
        let c = &candidate.clips[0];
        assert_eq!(c.fade_curve, FadeCurve::EqualPower);
        assert_eq!((c.fade_in_ms, c.fade_out_ms), (200, 300));
        assert_eq!(label, "Fade curve");
    }

    #[test]
    fn an_empty_payload_is_refused() {
        let project = project_with_clip(0, 0);
        let err = set_fades(
            &project,
            &SetFadesPayload {
                clip_id: "c1".into(),
                fade_in_ms: None,
                fade_out_ms: None,
                fade_curve: None,
            },
        )
        .unwrap_err();
        assert!(err.message.contains("fadeInMs"));
    }

    #[test]
    fn an_unknown_clip_and_a_locked_track_are_both_refused() {
        let project = project_with_clip(0, 0);
        let missing = set_fades(
            &project,
            &SetFadesPayload {
                clip_id: "nope".into(),
                fade_in_ms: Some(1),
                fade_out_ms: None,
                fade_curve: None,
            },
        )
        .unwrap_err();
        assert!(missing.message.contains("nope"));

        let mut locked = project.clone();
        locked.tracks[0].locked = true;
        let refused = set_fades(
            &locked,
            &SetFadesPayload {
                clip_id: "c1".into(),
                fade_in_ms: Some(1),
                fade_out_ms: None,
                fade_curve: None,
            },
        )
        .unwrap_err();
        assert!(refused.message.contains("locked"));
    }

    // Named test 2/2 (brief): the SHARED fixture table both languages read.
    // `include_str!` means moving or deleting the file breaks this crate's
    // own build (the `timeline-cases.json` / GAP-136 precedent), rather
    // than quietly testing nothing.
    const SHARED_FIXTURES: &str =
        include_str!("../../../../../tests/fixtures/editor-fade-cases.json");

    fn fixtures() -> serde_json::Value {
        serde_json::from_str(SHARED_FIXTURES).expect("the shared editor-fade fixture table is JSON")
    }

    fn curve_of(name: &str) -> FadeCurve {
        match name {
            "linear" => FadeCurve::Linear,
            "smooth" => FadeCurve::Smooth,
            "equal-power" => FadeCurve::EqualPower,
            other => panic!("unknown curve {other} in the shared fixture"),
        }
    }

    #[test]
    fn shared_fade_curve_table_has_six_cases_and_agrees() {
        let table = fixtures();
        let cases = table["cases"].as_array().expect("cases");
        // A table nothing iterates proves nothing, and one that silently
        // shrinks proves almost nothing. The TypeScript half
        // (`tests/editorFades.test.ts`) asserts the same count against the
        // same file.
        assert_eq!(
            cases.len(),
            6,
            "the shared fade table lost or gained a case"
        );
        for case in cases {
            let name = case["name"].as_str().expect("name");
            let curve = curve_of(case["curve"].as_str().expect("curve"));
            let u = case["u"].as_f64().expect("u");
            let expected = case["gain"].as_f64().expect("gain");
            let got = gain_at(curve, u);
            assert!(
                (got - expected).abs() < 1e-9,
                "{name}: gain_at({curve:?}, {u}) = {got}, expected {expected}"
            );
        }
    }
}
