# Verification and evidence

Reviewed September 19, 2026. This report describes the **actual final run**, not native-product certification. The complete aggregate and per-suite records are included in the package.

## Final executable-reference run

**222 scenario groups passed, 0 failed, 2 blocked.** All eight suites ran against the same unchanged self-contained HTML. Artifact/content checks are included in the handover suite; the total is not a claim that every group is a separate end-user journey.

| Suite | Pass | Fail | Blocked | Evidence |
|---|---:|---:|---:|---|
| `focus-ui.py` | 31 | 0 | 0 | [Results](../tests/focus-results.json) |
| `quality.py` | 38 | 0 | 0 | [Results](../tests/quality-results.json) |
| `onboarding.py` | 39 | 0 | 1 | [Results](../tests/onboarding-results.json) |
| `editing.py` | 38 | 0 | 0 | [Results](../tests/editing-results.json) |
| `regression.py` | 38 | 0 | 0 | [Results](../tests/results.json) |
| `webcam-smoke.py` | 5 | 0 | 0 | [Results](../tests/webcam-results.json) |
| `render-review.py` | 9 | 0 | 1 | [Results](../tests/render-review-results.json) |
| `handover.py` | 24 | 0 | 0 | [Results](../tests/handover-results.json) |

Aggregate: [final-suite-results.json](../tests/final-suite-results.json). The runner removes each old result file before execution, captures process logs, checks every status, treats a crashed/timed-out process or assertion failure as failure, and verifies the HTML hash after each suite. No failed assertions were weakened to obtain these results.

HTML SHA-256: `576ecde17da0d1d62d852151097eb7335e21b138bfb6206f4f7d34e658f9e62f`.

The current product captures are in [screens/](../screens/); the annotated review and flow qualifications are in [Product review](PRODUCT-REVIEW.md). Startup-recovery testing uses an explicitly injected initialization rejection in a separate page. Physical failures are not silently simulated and described as native tests.

## What was actually exercised

Real browser UI operations and known-state fixtures covered clip/track editing, group/lock invariants, context menus, waveform/audio controls, fades/transitions, timed teaching cues, captions, source mapping, transformations, compact layouts, product lineage, local media imports, portable archives and missing-source recovery. The full 22-lesson onboarding is exercised, including optional entry, dismissal, exact-step resume, no passive project mutation, modal suspension, keyboard handling and content/target alignment.

Media checks use real encoded output, not just a successful download or a preview screenshot. The webcam fixture uses simulated camera/microphone streams with the real browser MediaRecorder, then reopens the actual recorded bytes in a portable project. Project/progress file round trips use fresh browser contexts. A file-download request is not counted as a durable native save receipt.

The final polish includes a regression that repeats canvas sizing 20 times and observes **zero aspect-ratio toolbar mutations** when geometry is unchanged. A real ratio change is separately verified. Current-product title synchronization, startup recovery, forced-colors focus, source/content contracts and byte-identical rebuilding are also checked.

## Encoded media evidence and important limitation

The filtered, captioned portrait fixture rendered an actual nonzero source range into a **720 × 1280** video with audio. The encoded duration was **1.600000 seconds**. Decoding returned **4 images**, with timestamps:

`[0.0, 0.469, 0.844, 1.221]`

First-to-last comparison found **195,635 changed pixels**. Decoded mixed-audio RMS was **0.090878** across 41,760 analyzed samples. The product-player check used the actual encoded Blob and released its media URL afterwards.

**This is low video cadence, not 30 fps.** It establishes temporal/pixel coverage for this fixture; it does not certify smooth playback, exact frame/sample timing, color management or production render performance. The browser path is a real-time review tool. The native deterministic renderer is still a required implementation and release gate. Test ffmpeg/ffprobe are external verification tools, not bundled application dependencies.

Earlier inspection of this output path found insufficient end coverage. The aspect-ratio control's redundant per-frame DOM work was removed; the same decoded-coverage assertions remained in place. The final evidence above is the observed result, not a guarantee for different hardware or projects.

## Blocked browser checks

Two checks could not navigate to an origin-backed page because the environment returned `net::ERR_BLOCKED_BY_ADMINISTRATOR`: guide progress after a real origin reload and project/cache recovery after a real origin reload. **They are not passes.** In-memory storage injection is a simulation and does not replace those tests. Actual downloaded guide/project files were independently reopened in fresh contexts.

## Native-stack starter verification

The dependency-free DTO, timing and listener-cleanup helpers compiled with **TypeScript 5.8.3** and ran under **Node 22.16.0**: **12 passed, 0 failed**. [Pure results](../implementation-starter/tests/pure-results.json).

The checked repository declares TypeScript 6 and Vue/Pinia/Tauri dependencies. Full `vue-tsc`, Vue/Pinia/Vitest execution and native Tauri integration were **not run**: dependency installation was blocked by registry DNS resolution and no local dependency set was available. The supplied **22 Vitest cases** are implementation acceptance scaffolding, not passing evidence. No fake dependency declaration stubs were used. The Rust examples and their sample tests were **not compiled** because Cargo was unavailable. [Starter environment](../implementation-starter/tests/environment.json).

The small starter illustrates the typed integration boundary; it is not a complete Vue editor or native engine. Validate the examples in the application's lockfile-controlled toolchain before adoption.

## Environment and reproducibility

Reference execution used Python 3.13.5, Playwright 1.57.0, Chromium 144.0.7559.96, Pillow 12.3.0, jsonschema 4.26.0 and ffmpeg/ffprobe 7.1.5. [Recorded environment](../tests/environment.json).

```sh
python reference/build.py
python tests/run-all.py
cd implementation-starter
sh tests/run-pure.sh
```

See [test setup](../tests/README.md) for prerequisites. Browser source building needs only Python's standard library. Merely opening the HTML needs no npm installation, server or external assets.

## Explicitly unverified release obligations

Physical screen/webcam/microphone devices and permissions; synchronized native capture; actual Windows/WebView2 performance; native frame/sample accuracy; app-command/capability enforcement; atomic project/vault writes; disk-full, loss-of-power and permission faults; native session/job recovery; assistive technologies and beginner usability sessions remain **unverified here**. Those obligations are listed with evidence slots in [Release checklist](RELEASE-CHECKLIST.md) and [feature acceptance matrix](FEATURE-ACCEPTANCE-MATRIX.md).

The **30 Given/When/Then acceptance scenarios** describe required native outcomes. They are not added to the passing browser-test count.
