# Feature acceptance matrix

Use each row as a release evidence slot. Native status is not implied by browser availability. The feature catalog is authoritative for IDs and scope.

| ID | Capability | Owner | Acceptance evidence required | Native result |
|---|---|---|---|---|
| F-01 | Native capture handoff | screenCapture store; staging sidecar; EditorRoot | Capture A stops while vault B is selected; the editor still proposes vault A. | Not yet evaluated |
| F-02 | Local media import | MediaLibrary; media service | Import a mixed batch; valid items remain when another item fails or the batch stops. | Not yet evaluated |
| F-03 | Reconnect originals | MediaLibrary; recovery service | Ambiguous candidates are never selected automatically. | Not yet evaluated |
| F-04 | Multi-track video | Timeline; composition engine | Upper layers cover lower layers; invisible tracks are excluded from output. | Not yet evaluated |
| F-05 | Multi-track audio | AudioMixer; native mixer | Multiple audible sources occur in decoded output, not only preview. | Not yet evaluated |
| F-06 | Track management | TrackHeader; command registry | A locked track refuses mutation from pointer, menus, shortcuts and batches. | Not yet evaluated |
| F-07 | Split | TimelineCommands | Duration is conserved; annotations/captions/markers map correctly. | Not yet evaluated |
| F-08 | Trim | ClipInspector; TimelineCommands | No empty clip; source bounds and transition dependencies remain valid. | Not yet evaluated |
| F-09 | Delete and ripple | TimelineToolbar | Other tracks do not silently move; blocked grouped/transition edits explain why. | Not yet evaluated |
| F-10 | Move and reorder | Timeline; ClipInspector | Relative teaching timing follows source-linked content. | Not yet evaluated |
| F-11 | Multi-selection and grouping | SelectionStore; TimelineCommands | One shared delta and all-or-nothing validation preserve relative timing. | Not yet evaluated |
| F-12 | Copy, cut, paste and duplicate | Clipboard service | Cloned content has no colliding IDs; Undo restores the entire operation. | Not yet evaluated |
| F-13 | Undo and redo | ProjectCommands | Undo does not evict originals or modify immutable rendered products. | Not yet evaluated |
| F-14 | Timeline navigation | TimelineViewport; Transport | Fit shows the entire edit; Escape cancels an uncommitted drag. | Not yet evaluated |
| F-15 | Contextual menus | CommandMenu | Clicked-time commands use the clicked position; keyboard focus returns logically. | Not yet evaluated |
| F-16 | Clip speed | SpeedInspector; time mapping | New duration, audio and source-linked cues use the same speed mapping. | Not yet evaluated |
| F-17 | Video fades | FadeInspector | Handle and numeric edits produce equivalent opacity envelopes. | Not yet evaluated |
| F-18 | Audio fades | FadeInspector; mixer | Decoded audio ramps instead of merely changing a drawn waveform. | Not yet evaluated |
| F-19 | Clip transitions | TransitionCommands | Only the target track is shortened by the overlap; removal restores spacing. | Not yet evaluated |
| F-20 | Webcam recording | WebcamDialog; capture service | Opening setup alone never acquires devices; closing releases active tracks. | Not yet evaluated |
| F-21 | Presenter picture-in-picture | PreviewSurface; LayoutInspector | Geometry is reflected in the encoded file and portable project. | Not yet evaluated |
| F-22 | Synchronized screen + webcam | Capture coordinator | Long-run drift is measured; failures retain usable recordings and offsets. | Not yet evaluated |
| F-23 | Source layout and transforms | LayoutInspector; compositor | Preview and output agree; callout alignment is reviewed after source orientation changes. | Not yet evaluated |
| F-24 | Audio detachment | AssetCommands | Dependency survives portable packaging and source reconnection. | Not yet evaluated |
| F-25 | Mixer and monitoring | AudioMixer; Transport | Monitoring mute never changes export audio; sample peak is not described as loudness. | Not yet evaluated |
| F-26 | Waveforms | Media service; Timeline | Waveform generation is cancelable/bounded in native implementation. | Not yet evaluated |
| F-27 | Text callouts | TeachingToolbar; EffectInspector | Only the intersecting source span is rendered. | Not yet evaluated |
| F-28 | Arrows | PreviewSurface; EffectInspector | Endpoints update predictably and survive clip movement. | Not yet evaluated |
| F-29 | Highlights | TeachingTools | Size and source timing match in preview and encoded output. | Not yet evaluated |
| F-30 | Spotlight | TeachingTools; compositor | Effect respects clipping and output aspect ratio. | Not yet evaluated |
| F-31 | Zoom and focus | TeachingTools; compositor | Zoom enters/exits smoothly and does not expose empty frame margins. | Not yet evaluated |
| F-32 | Numbered steps | TeachingTools | Step text, number and timing remain editable and clip-linked. | Not yet evaluated |
| F-33 | Privacy covers | TeachingTools; Checks | Warn about motion/layers and uncensored originals; encoded review is required. | Not yet evaluated |
| F-34 | Caption authoring/import | CaptionsLibrary; caption commands | No automatic transcription claim; overlap/density warnings are actionable. | Not yet evaluated |
| F-35 | Caption outputs | Caption settings; render pipeline | Disabled/excluded captions are surfaced before export; exported time is timeline time. | Not yet evaluated |
| F-36 | Chapter markers | ChaptersLibrary | Chapters follow source edits and appear in the companion note. | Not yet evaluated |
| F-37 | Title and background cards | TitlesLibrary | Cards remain editable; intro-before-all shifts every existing track together. | Not yet evaluated |
| F-38 | Canvas formats | PreviewHeader; compositor | Circle frames stay circular; crop/caption warnings prompt a review. | Not yet evaluated |
| F-39 | Color treatment | ColorInspector | Treat source footage, not teaching labels; not professional color-managed mastering. | Not yet evaluated |
| F-40 | Save editable project | Project service | Portable ZIP reopens directly; lightweight JSON asks for missing originals. | Not yet evaluated |
| F-41 | Rendered products | ProductLibrary; render service | Further edits never rewrite a prior product; restore snapshot is a new edit revision. | Not yet evaluated |
| F-42 | Output review | ReviewDialog; render service | Encoded frames/audio are inspected; no simulated render completion. | Not yet evaluated |
| F-43 | Obsidian companion note | Note service | Final reserved filename is used in the note; unrelated files are never overwritten. | Not yet evaluated |
| F-44 | Project/session recovery | ProjectSession; recovery worker | Interrupted operations cannot destroy the last committed project. | Not yet evaluated |
| F-45 | Before-you-share checks | ChecksDialog | Each finding opens a relevant action; no synthetic quality score. | Not yet evaluated |
| F-46 | Guided onboarding | GuideController | Dismiss/resume at the same step; starting/reading lessons makes no document edit. | Not yet evaluated |
| F-47 | Learning center | LearningCenter | Missing storage is explicit; a progress file can be restored separately from projects. | Not yet evaluated |
| F-48 | Focused responsive shell | EditorRoot; WorkspaceStore | Overflow relocates tools without removing them or adding a second preview toolbar. | Not yet evaluated |
| F-49 | Keyboard and accessible controls | Shared UI components | Core creation journey completes by keyboard; zoom and forced-colors remain usable. | Not yet evaluated |
| F-50 | Local privacy and diagnostics | Diagnostics service | Report omits names, paths, captions, frames and audio. | Not yet evaluated |
