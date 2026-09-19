# Executable reference verification

## Run

From the handover root:

```sh
python reference/build.py
python tests/run-all.py
```

Requirements: Python 3.10+; `playwright`, `Pillow` and `jsonschema`; Chromium at `/usr/bin/chromium`; ffmpeg and ffprobe on PATH. Install these in your test environment as appropriate; they are not bundled application dependencies. The test code uses the supplied local media fixtures. There is no CDN or runtime app dependency to install just to open the editor HTML.

`run-all.py` runs eight suites serially, deletes stale result files before each suite, preserves stdout/stderr logs, validates every reported status and checks that the HTML hash stays unchanged throughout the run. An assertion failure or process error fails the aggregate run. A reported BLOCKED status stays separate from PASS. The runner uses a 300-second per-suite timeout and writes `final-suite-results.json` incrementally.

## What the suites cover

| Suite | Coverage |
|---|---|
| `focus-ui.py` | One preview row, unique command homes, overflow, responsive panels, menus, shortcuts and feature reachability. |
| `quality.py` | Save/import/validation/recovery behavior, focus, compact dialogs, resource and revision guards. |
| `onboarding.py` | All 22 lessons, dismiss/resume, progress validation, modal interruption, context targets, keyboard and passive safety. |
| `editing.py` | Advanced/contextual edits, captions, speed/transform, selection/grouping and the teaching workflow. |
| `regression.py` | Core multi-track editing, media imports, serialization, actual outputs and source/product preservation. |
| `webcam-smoke.py` | Simulated camera/microphone input with the real MediaRecorder; real recorded bytes and portable reopening. |
| `render-review.py` | Real nonzero range render, actual decoded frames/timestamps/pixel changes, audio energy and product-file playback. |
| `handover.py` | Final current-product labels, startup error injection, no per-frame toolbar mutation, schema/content contracts, screenshots and byte-identical rebuild. |

Tests use the reference's public seams and, where needed, private test setup to produce known graph states. This does not exercise Vue components, Pinia stores or Tauri commands. Add those tests to the actual application during implementation; do not describe these browser suites as native integration coverage.

## Evidence and limitations

Fresh browser contexts can verify portable project and progress-file reopen. In the provided environment, navigation to a real localhost/test origin is administratively blocked, so two origin-backed reload checks are reported BLOCKED. Memory-storage injection is explicitly a simulation, not proof of native or browser durable persistence.

Camera fixtures simulate devices but use the real browser recorder. Physical webcam/capture permissions, driver differences and synchronized native acquisition remain unverified. Browser render tests decode actual output rather than testing only file existence; passing temporal coverage does not establish 30 fps or production encoder quality.

`artifacts/` holds representative current outputs and diagnostics. `screens/` at the handover root holds accepted current product captures; tests may also emit auxiliary screenshots into `screenshots/`. Failure screenshots from development are not part of the accepted product evidence. The exact recorded run and unchanged artifact hash are in [Verification](../docs/VERIFICATION.md).

## Native-stack examples

```sh
cd implementation-starter
sh tests/run-pure.sh
```

The dependency-free TypeScript helpers can run with an installed `tsc` and Node. Full `npm run typecheck` / `npm test` requires the actual Vue/Pinia/Tauri dependencies. Those framework tests are supplied, but were not run in the handover environment because dependency installation was blocked. Rust examples were not compiled because Cargo is unavailable. Read the starter README and environment/result JSON before making stronger claims.
