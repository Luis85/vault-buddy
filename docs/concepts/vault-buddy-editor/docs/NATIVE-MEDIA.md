# Native capture, preview, composition and encoding

## Scope and technology choice

Use the existing Windows capture and Media Foundation direction. The checked `vault_buddy_screen` crate uses `windows-capture` and the `windows` crate and reuses `vault_buddy_capture`. Do not add a bundled FFmpeg runtime or a browser WASM encoder as an incidental consequence of implementing this design. The test bundle uses ffmpeg/ffprobe externally to inspect files; those are **test-environment tools**, not application dependencies. [Repository evidence](REFERENCES.md).

The browser's Canvas/Web Audio/MediaRecorder output proves interaction and basic composition behavior. It is a real-time review path with limited duration/cadence and does not establish frame-accurate native encoding, driver compatibility or acceptable production quality.

## Acquisition and synchronization

Capture starts only through explicit user action and device consent. Native workers own screen frames, webcam frames and microphone/system audio resources. They share one monotonic session clock with pause/resume accounting; source timestamps are mapped into that clock, not aligned by the time the UI happened to receive an event.

Screen, webcam and independent audio stems require a deliberate capture-session extension. The current native screen path mixes chosen audio devices down to stereo; separate editable microphone/system tracks cannot be reconstructed from that mixed output. Preserve stems when that functionality is offered. A webcam recorded afterward remains a separate take placed on the timeline; label this distinctly from synchronized simultaneous capture. Do not bypass the application's capture guard with a second uncoordinated session.

Track source discontinuities, dropped-frame warnings and device changes. If a capture source closes, finalize usable material and report the warning according to existing capture policy. If finalization fails, retain owned recoverable artifacts and metadata. Never delete a retained prefix because the UI did not receive success. Test actual crash/finalization behavior rather than assuming the configured container guarantees recovery.

## Render plan

Freeze an acknowledged project revision and requested output range. Build a pure, validated render plan: canvas/timebase, source registry, visible video layers, audio contributions, effect/caption schedules, transforms, transition envelopes and chapter mapping. Keep all relevant assets referenced until the job is terminal and any retained product snapshot has durable ownership.

At each output video timestamp:

1. Resolve active clips from half-open output intervals and map to source time, including speed.
2. Acquire the appropriate decoded frame or procedural still/title frame; implement source seeks against actual presentation timestamps.
3. Apply source orientation/rotation/mirroring, crop and placement under a shared geometry contract; handle contain/cover and frame masks deterministically.
4. Evaluate opacity, edge fades, pairwise dissolve and selected color treatments; compose ordered layers with defined premultiplied-alpha/color behavior.
5. Draw clip-linked teaching cues at the agreed coordinate/time stage, then captions in output coordinates. Match the executable reference using golden-frame comparisons; do not guess an order from CSS alone.
6. Exclude selection outlines, resize handles, guide overlays, focus rings and UI controls.
7. Encode and mux with monotonic timestamps. Native progress refers to completed work, not UI paint callbacks.

Audio evaluates active source samples on a chosen output sample clock, applies speed/resampling and the preserve-pitch decision, per-clip/track/master levels, mute/solo, fades and transitions. Equal-power crossfade curves are not linear pixel fades. Provide headroom/clipping behavior with measurable fixtures; do not silently promise loudness normalization or noise removal. Browser monitoring volume/mute never changes the exported mix unless the user changes a render-affecting control.

## Timebase contract

Keep interoperable edit decisions in integer milliseconds, then convert to the native media clock at plan construction/evaluation. For output frame `n` and rational frame rate `p/q`, a 100-nanosecond timestamp is `floor(n * 10,000,000 * q / p)`. Frame duration is the difference to the next timestamp, not one repeatedly rounded constant. Audio sample boundaries are derived similarly. Use checked wide integer arithmetic; reject overflowing duration/range requests. Keep native BigInt/u128 internals out of JSON or serialize explicitly as validated decimal strings.

A non-unit-speed cue is evaluated in the same mapped source time as its clip. Interpolate zoom/fade by source/output semantics recorded in the graph. Endpoints are half-open: a cut marker/caption must not display one extra frame in the next clip. Variable-frame-rate sources need correct presentation-time selection and cannot be treated as a constant frame index inferred from nominal FPS.

## Preview architecture

A `PreviewController` owns decoding/playback resources outside deep Vue reactivity. Vue provides selected handles and low-rate display state. Use bounded decoded-frame/audio queues, small thumbnails/waveform caches, offscreen work where useful, and viewport virtualization. A stalled renderer must not accumulate unbounded samples. Cancel obsolete seeks and distinguish a still-preview seek from continuous playback.

For the first native milestone, media elements can display scoped source URLs while Rust owns committed graph state; independently prove preview/export parity for effects. A production composed native preview may use a supported efficient surface/stream. Do not send full RGBA frames as base64 JSON events or rebuild Pinia for every sample. Tauri Channels are suitable for bounded progress messages, not an excuse to transport unlimited raw media through reactive state.

## Output and publication

Offer a tested native MP4 profile only after encoder availability is established. Freeze width/height/frame rate/audio sample rate/color conventions with approved presets. Probe supported encoders and hardware acceleration; fallback must be explicit and verified. An unavailable encoder leaves the project editable and reports a recovery route; it does not silently emit a file with a different extension/codec.

Encode to an owned temporary output. Cancellation releases decoder/encoder resources, removes only job-owned incomplete outputs and keeps originals/project snapshots. Publication validates nonempty/decodable streams and expected duration, reserves a collision-free destination, publishes the output and companion note, then commits its product record. Treat file/note/project metadata as a recoverable multi-file transaction; a sequence of renames is not globally atomic. [Persistence protocol](PERSISTENCE-AND-SECURITY.md).

Review-range rendering has explicit start/end and output-relative timestamp zero; associated chapter/caption note timestamps must be shifted/clipped. A range product retains its full source snapshot and range bounds. Watch rendered file plays the actual file; it must not reopen a mutable preview and describe it as the exported result.

## Required media fixtures and acceptance

Use identifiable moving screen content, visible frame numbers and an audible synchronized pulse. Include layered webcam, portrait/landscape/image media, multiple audio rates, source rotation, VFR footage, non-unit speeds, overlapping video/audio, silence, fades and transitions, title cards, captions, arrows and privacy cover. Check decoded frames at beginning/middle/end and every critical boundary, audio energy and channel presence, monotonic timestamps, expected duration and A/V offset.

Test hardware/software encoders on the supported Windows matrix, DPI changes, capture source closure, webcam denial/unplug, microphone loss, long pause, slow disks, disk full, file locks, cancellation during rendering and publication, sleep/resume and app close. Record CPU/GPU/memory and frame cadence against named machines. Smooth output, bounded memory and acceptable sync are native release gates, not inferred from file existence or browser test counts.
