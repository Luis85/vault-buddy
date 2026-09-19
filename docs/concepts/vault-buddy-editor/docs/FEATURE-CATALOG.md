# Feature catalog

This register defines the full handover scope. “Browser functional” describes the executable reference, not proof of native implementation. Native-specific rows have acceptance targets even where the browser cannot demonstrate the underlying OS behavior.

| ID | Capability | Behavior | Reference status | Implementation owner | Acceptance anchor |
|---|---|---|---|---|---|
| F-01 | Native capture handoff | Open the correct staged capture and retain its intended vault. | Native integration | screenCapture store; staging sidecar; EditorRoot | Capture A stops while vault B is selected; the editor still proposes vault A. |
| F-02 | Local media import | Import playable video/audio and PNG/JPEG/WebP with per-file feedback. | Browser functional | MediaLibrary; media service | Import a mixed batch; valid items remain when another item fails or the batch stops. |
| F-03 | Reconnect originals | Reconnect by explicit/unique matches, preserving all edits. | Browser functional | MediaLibrary; recovery service | Ambiguous candidates are never selected automatically. |
| F-04 | Multi-track video | Independent timed video layers with deterministic compositing order. | Browser functional | Timeline; composition engine | Upper layers cover lower layers; invisible tracks are excluded from output. |
| F-05 | Multi-track audio | Mix concurrent tracks with clip/track/master controls. | Browser functional | AudioMixer; native mixer | Multiple audible sources occur in decoded output, not only preview. |
| F-06 | Track management | Create, rename, order, lock, hide, mute, solo and delete tracks. | Browser functional | TrackHeader; command registry | A locked track refuses mutation from pointer, menus, shortcuts and batches. |
| F-07 | Split | Split at playhead or clicked context time with half-open ranges. | Browser functional | TimelineCommands | Duration is conserved; annotations/captions/markers map correctly. |
| F-08 | Trim | Drag edges or edit numeric source bounds. | Browser functional | ClipInspector; TimelineCommands | No empty clip; source bounds and transition dependencies remain valid. |
| F-09 | Delete and ripple | Choose leave-gap or close-gap behavior; ripple is track-local. | Browser functional | TimelineToolbar | Other tracks do not silently move; blocked grouped/transition edits explain why. |
| F-10 | Move and reorder | Drag or numerically position clips; Earlier/Later swaps adjacent clips. | Browser functional | Timeline; ClipInspector | Relative teaching timing follows source-linked content. |
| F-11 | Multi-selection and grouping | Select multiple clips and move/duplicate as a synchronized group. | Browser functional | SelectionStore; TimelineCommands | One shared delta and all-or-nothing validation preserve relative timing. |
| F-12 | Copy, cut, paste and duplicate | Create new clip identities and copy relevant attached content. | Browser functional | Clipboard service | Cloned content has no colliding IDs; Undo restores the entire operation. |
| F-13 | Undo and redo | Reversible document edits with bounded history. | Browser functional | ProjectCommands | Undo does not evict originals or modify immutable rendered products. |
| F-14 | Timeline navigation | Seek, play, zoom, fit, snap, markers and adjustable timeline height. | Browser functional | TimelineViewport; Transport | Fit shows the entire edit; Escape cancels an uncommitted drag. |
| F-15 | Contextual menus | Object-specific actions with mouse, More and Shift+F10 access. | Browser functional | CommandMenu | Clicked-time commands use the clicked position; keyboard focus returns logically. |
| F-16 | Clip speed | 0.25×–4× timing with pitch option when supported. | Browser functional | SpeedInspector; time mapping | New duration, audio and source-linked cues use the same speed mapping. |
| F-17 | Video fades | Independent in/out fade durations and supported curves. | Browser functional | FadeInspector | Handle and numeric edits produce equivalent opacity envelopes. |
| F-18 | Audio fades | Gain envelopes with supported fade presets/curves. | Browser functional | FadeInspector; mixer | Decoded audio ramps instead of merely changing a drawn waveform. |
| F-19 | Clip transitions | Explicit same-track video dissolve or equal-power audio crossfade. | Browser functional | TransitionCommands | Only the target track is shortened by the overlap; removal restores spacing. |
| F-20 | Webcam recording | Explicit setup, permission, countdown, recording, review/retake and commit. | Browser functional; simulated-device verification | WebcamDialog; capture service | Opening setup alone never acquires devices; closing releases active tracks. |
| F-21 | Presenter picture-in-picture | Move/resize webcam as an independent clip; shapes, crop, mirror and corner presets. | Browser functional | PreviewSurface; LayoutInspector | Geometry is reflected in the encoded file and portable project. |
| F-22 | Synchronized screen + webcam | Native composite capture with a shared clock and independent editable layers. | Native integration | Capture coordinator | Long-run drift is measured; failures retain usable recordings and offsets. |
| F-23 | Source layout and transforms | Position, size, opacity, fit/fill, crop, rotation and flips. | Browser functional | LayoutInspector; compositor | Preview and output agree; callout alignment is reviewed after source orientation changes. |
| F-24 | Audio detachment | Create a linked audio asset from imported video and mute its original audio. | Browser functional | AssetCommands | Dependency survives portable packaging and source reconnection. |
| F-25 | Mixer and monitoring | Clip/track/master gain, mute/solo and monitoring-only mute. | Browser functional | AudioMixer; Transport | Monitoring mute never changes export audio; sample peak is not described as loudness. |
| F-26 | Waveforms | Display locally derived sample peaks for editing guidance. | Browser functional | Media service; Timeline | Waveform generation is cancelable/bounded in native implementation. |
| F-27 | Text callouts | Timed text with position, size, appearance and background. | Browser functional | TeachingToolbar; EffectInspector | Only the intersecting source span is rendered. |
| F-28 | Arrows | Timed arrows with draggable endpoints and numeric appearance. | Browser functional | PreviewSurface; EffectInspector | Endpoints update predictably and survive clip movement. |
| F-29 | Highlights | Timed outlined/fill highlights for a region of interest. | Browser functional | TeachingTools | Size and source timing match in preview and encoded output. |
| F-30 | Spotlight | Dim the surrounding region while emphasizing the target. | Browser functional | TeachingTools; compositor | Effect respects clipping and output aspect ratio. |
| F-31 | Zoom and focus | Timed animated zoom into a point of interest. | Browser functional | TeachingTools; compositor | Zoom enters/exits smoothly and does not expose empty frame margins. |
| F-32 | Numbered steps | Timed numbered instructional labels. | Browser functional | TeachingTools | Step text, number and timing remain editable and clip-linked. |
| F-33 | Privacy covers | Opaque stationary cover with explicit limitations. | Browser functional | TeachingTools; Checks | Warn about motion/layers and uncensored originals; encoded review is required. |
| F-34 | Caption authoring/import | Manual cues and SRT/plain WebVTT import with text/timing editing and splitting. | Browser functional | CaptionsLibrary; caption commands | No automatic transcription claim; overlap/density warnings are actionable. |
| F-35 | Caption outputs | Burn-in choice and subtitle-file export. | Browser functional | Caption settings; render pipeline | Disabled/excluded captions are surfaced before export; exported time is timeline time. |
| F-36 | Chapter markers | Name and position chapters attached to footage. | Browser functional | ChaptersLibrary | Chapters follow source edits and appear in the companion note. |
| F-37 | Title and background cards | Editable intro, chapter, closing and plain-background clips. | Browser functional | TitlesLibrary | Cards remain editable; intro-before-all shifts every existing track together. |
| F-38 | Canvas formats | Landscape, portrait, square and classic output geometry. | Browser functional | PreviewHeader; compositor | Circle frames stay circular; crop/caption warnings prompt a review. |
| F-39 | Color treatment | Presets and basic brightness/contrast/saturation. | Browser functional | ColorInspector | Treat source footage, not teaching labels; not professional color-managed mastering. |
| F-40 | Save editable project | Save without rendering, preserving workspace and available sources. | Browser functional; native persistence required | Project service | Portable ZIP reopens directly; lightweight JSON asks for missing originals. |
| F-41 | Rendered products | Independent output files with revision/range/snapshot provenance. | Browser functional | ProductLibrary; render service | Further edits never rewrite a prior product; restore snapshot is a new edit revision. |
| F-42 | Output review | Review mode, short-range rendering and actual encoded-file playback. | Browser functional; native encoder required | ReviewDialog; render service | Encoded frames/audio are inspected; no simulated render completion. |
| F-43 | Obsidian companion note | Markdown embed, chapters and contextual metadata. | Browser package functional; native publish required | Note service | Final reserved filename is used in the note; unrelated files are never overwritten. |
| F-44 | Project/session recovery | Unsaved-state explanations, close guards, retained media and render history. | Browser best-effort; native durability required | ProjectSession; recovery worker | Interrupted operations cannot destroy the last committed project. |
| F-45 | Before-you-share checks | Actionable missing-media, gap, audio, caption and visibility findings. | Browser functional | ChecksDialog | Each finding opens a relevant action; no synthetic quality score. |
| F-46 | Guided onboarding | 22 optional interactive lessons across seven chapters. | Browser functional | GuideController | Dismiss/resume at the same step; starting/reading lessons makes no document edit. |
| F-47 | Learning center | Searchable answers, shortcuts, chapter jumps and progress/preferences. | Browser functional | LearningCenter | Missing storage is explicit; a progress file can be restored separately from projects. |
| F-48 | Focused responsive shell | One preview header, contextual properties, drawers, focus view and themes. | Browser functional | EditorRoot; WorkspaceStore | Overflow relocates tools without removing them or adding a second preview toolbar. |
| F-49 | Keyboard and accessible controls | Non-dragging alternatives, roving toolbar, menu/dialog focus and visible states. | Browser functional; Windows AT acceptance required | Shared UI components | Core creation journey completes by keyboard; zoom and forced-colors remain usable. |
| F-50 | Local privacy and diagnostics | No content uploads; content-free diagnostic export and clear source/output distinction. | Browser functional | Diagnostics service | Report omits names, paths, captions, frames and audio. |

## Compatibility obligations

All browser-functional rows must remain reachable in the native implementation. No feature is silently removed to simplify the port. Native behavior must preserve timing, layering and file semantics even when the implementation mechanism changes.

The complete composition is not the same as the current pure single-source `Timeline` struct. Preserve that tested algebra and add/adapt a multi-track composition aggregate; do not force overlays or source reuse into a sequential-only model.

The production encoder must implement the same media treatments; copying inspector fields without rendering their effects is not completion. Onboarding uses actual controls from these features, never a disconnected mock screen.
