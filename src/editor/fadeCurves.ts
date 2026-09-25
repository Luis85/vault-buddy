/**
 * TypeScript half of the shared fade-curve algebra (Task 29; F-17, F-18,
 * DATA-MODEL.md § Fade and transition rules: "Edge fade envelopes multiply
 * the clip's alpha/audio amplitude … supports documented curve choices").
 *
 * Held apart from Rust's `core::editor::commands::fades::gain_at` by ONE
 * shared fixture table, `tests/fixtures/editor-fade-cases.json` — read by
 * both `fades.rs` (`include_str!`) and `tests/editorFades.test.ts` — the
 * `time.rs`/`timeMap.ts` precedent (GAP-136's discipline): a disagreement
 * between the two languages has to redden one of the two suites rather than
 * staying invisible with every test in the repo green.
 *
 * `previewLayers.ts`'s `computeLayers` calls `gainAt` for every ACTIVE clip
 * at the requested output time, so the preview and this fixture table agree
 * by construction. The render (Task 44) does NOT call this function at all —
 * it maps curve names straight to ffmpeg's own `afade` curve arguments
 * (`linear` -> `tri`, `smooth` -> `hsin`, `equal-power` -> `qsin`), because
 * ffmpeg computes its own curve shape internally. `hsin` is a close but not
 * bit-identical approximation of this module's `smooth` (smoothstep) shape —
 * recorded as docs/Gaps.md GAP-173, never claimed as exact.
 */
import type { FadeCurve } from "../editorTypes";

/**
 * The gain (0..1) a fade of progress `u` (0 = fade start, 1 = fully faded
 * in/out) has reached under `curve`. `linear` is `u` itself; `smooth` is the
 * smoothstep polynomial `3u² − 2u³` — an S-curve that eases into and out of
 * the fade rather than crossing it at a constant rate; `equal-power` is
 * `sin(u · π/2)`, a quarter sine whose MIDPOINT is `sin(π/4) ≈ 0.7071`
 * (`1/√2`), not 0.5 — two clips crossfading with it sum to constant
 * PERCEIVED loudness rather than constant amplitude (`sin²+cos²=1`, never
 * `u+(1-u)=1` the way a linear cut would). `u` is clamped to `[0,1]`
 * defensively; every caller in this module already passes an in-range
 * value.
 */
export function gainAt(curve: FadeCurve, u: number): number {
  const clamped = Math.min(1, Math.max(0, u));
  if (curve === "linear") return clamped;
  if (curve === "smooth") return clamped * clamped * (3 - 2 * clamped);
  return Math.sin(clamped * (Math.PI / 2));
}
