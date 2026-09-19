# Optional onboarding and contextual learning

## Outcome and constraints

A new user understands the editor well enough to make a cut, add a teaching cue, save an editable project and render a separate product. Learning is **optional, dismissible and resumable**. It must never become a prerequisite for editing or silently make edits on a user's behalf.

The complete editorial content is shipped in [onboarding.steps.json](../contracts/onboarding.steps.json) and [onboarding.chapters.json](../contracts/onboarding.chapters.json). These files are extracted from the executable reference, not abbreviated examples. The sequence contains **22 lessons in seven chapters**. Stable IDs, not numeric array indices, identify persisted progress.

## Entry and return

The first-use nonmodal invitation offers **Show me around** and **Not now**. Dismissing it leaves the workspace usable. Help remains visible at compact widths; **Help → Resume walkthrough** returns to the same stable lesson. **F1** and **?** open learning. **F6** switches focus between the guide and its highlighted target. Close, Pause and Escape preserve progress. Collapse keeps a small resume control while the user works. Start over requires confirmation and resets guide state only.

The learning center contains chapter navigation, individual lessons, searchable quick answers, shortcuts, progress/preferences and restart. A completed guide can be revisited. Reading a lesson and trying a control are separate indicators; exploring a control never auto-advances. Back/Next remain available when a source/control is unavailable.

## State and persistence

Track current step ID, reviewed IDs, explored IDs, dismissed invitation, active/collapsed status, completion and presentation preferences. Distinguish transient `suspended` from persisted user dismissal. Persist separately from the project, render snapshot and Undo. Native progress belongs in the application's preference storage with validated content revision. Unknown/retired IDs resolve through an explicit mapping or safe nearest chapter; never discard unrelated preferences.

The browser uses best-effort local storage and reports **Session only** when unavailable. Its progress-file download/restore is an explicit fallback and contains no media. Do not make native onboarding reliability depend on browser storage under a file URL. Restoring a guide file validates IDs/status and does not activate camera, edit content, open files or replace the workspace.

## Overlay, positioning and focus

Resolve targets from registered Vue component refs/action IDs. The reference CSS selectors in the content are a behavior map; they should become stable typed target keys in production. Observe target position after panel changes, overflow, scrolling, viewport resizing and zoom. Highlight the actual visible command; when a tool moved into overflow, reveal/highlight the matching More entry rather than an invisible duplicate.

Prefer positioning beside the target without covering it or essential controls. On narrow windows dock the card with a visible target region, scroll if needed and retain pause/collapse controls. Dimming is optional. Respect reduced motion and forced colors. Do not encode any guide layer into preview output.

Guide focus stays within the explanation controls while interacting with it; F6 permits deliberate transfer to the real target. Editing keyboard shortcuts are suppressed when the guide, menu, dialog or text field owns those keys. Menu Escape closes the menu before dismissing the guide. A modal dialog suspends the coach; closing it resumes the same lesson and restores appropriate focus. No hidden focus trap may make the editor unreachable.

## Safe preparation and optional actions

Preparation may reveal a tab/panel, select a suitable existing item, or position the playhead to illustrate a lesson. It must not add/remove/trim/split media, change gains/effects, clear Undo, render, save/download, request device permission or start recording. Every optional action that changes the project says so. User actions remain normal undoable editor actions.

Empty projects, missing/locked tracks, unsupported camera access and unavailable selection produce explanatory fallback copy, not a blocked tutorial. Avoid silently loading a sample over user work. Opening a native dialog/camera panel suspends the guide; actual permission/recording still needs its own user action.

## Full lesson catalog

### Find your way

Media, preview & timeline.

**1. A recording is just the beginning.** (`welcome`)

Turn a screen recording into a short tutorial. Media lives on the left, your preview is in the middle, and the timeline below controls what plays when.

Tip: The guide only changes the view and selection. It never applies edits for you. Actions you choose in the editor do affect this project.

Reference target: `.projectbar`.

**2. Start with your source material.** (`media`)

Bring in a screen recording, narration, music or an image. Originals stay unchanged. Drag media to a compatible track, or use its + button.

Tip: The built-in project is ready to explore. Importing your own file is optional.

Optional action: Open the import picker, or use the sample and continue.

Reference target: `#library [data-action="import"]`.

**3. See your edit in motion.** (`preview`)

Press Play to watch the assembled timeline. The time readout is the position in your edit, not in the original recording. Click the timeline ruler to jump to another moment.

Tip: The speed next to the speaker changes preview playback only. It does not change the exported video.

Optional action: Play a few seconds, then pause.

Reference target: `.transport`.

**4. Time runs from left to right.** (`timeline`)

Each rectangle is a clip. The vertical playhead shows the current moment. Zoom into short details, fit the whole edit, or drag the divider to make more room for tracks.

Tip: Video layers stack from top to bottom; audio layers mix together. Higher video tracks appear in front.

Optional action: Try timeline zoom or Fit.

Reference target: `.timeline-toolbar`.


### Make the first edit

Select, split, undo & arrange.

**5. Choose the clip you want to change.** (`select`)

Click a clip to select it. Its settings appear in Properties on the right. Shift-click adds another clip to the selection. A locked track protects its clips from editing.

Tip: Selection is not an edit. Use Edit actions → Clear selection or V to deselect without changing the video.

Optional action: Select a different clip, or keep this one.

Reference target: `@clip`.

**6. Cut a clip, not the original.** (`split`)

Put the playhead inside a selected clip, then choose Split. You get two independently editable pieces; no source frames are deleted. Remove an unwanted piece only after checking the delete mode.

Tip: Leave gap keeps other clips in place. Ripple this track closes the gap on this track only, which can change its alignment with other tracks.

Optional action: Optional edit: split here, then try Undo in the next step.

Reference target: `#splitButton`.

**7. You can take an edit back.** (`undo`)

Undo reverses your most recent edit. Redo brings it back. This applies to cuts, moved clips, added teaching layers and property changes.

Tip: Undo reverses the most recent actual edit, not a tutorial step. If Undo is unavailable, there is nothing to undo yet.

Optional action: Use Undo only if you want to reverse the last edit.

Reference target: `#undoButton`.

**8. Put each moment in the right place.** (`arrange`)

Drag a clip horizontally to change when it starts, or use its numeric timing fields. Drag an edge to trim. Compatible tracks accept a vertical move; original media stays untouched.

Tip: Group related clips before moving them to preserve their relative timing. Keep snapping on when you want edges to line up.

Optional action: Explore the timing fields. Changing a value edits this project.

Reference target: `.inspector`.

**9. The action you need is right here.** (`context`)

Right-click a clip, annotation, track or media item for actions specific to that object. Edit actions in the timeline opens the same kind of menu without a right click.

Tip: When you right-click a clip, time-specific commands use the position you clicked. Disabled items explain what needs to change first.

Optional action: Open Edit actions, explore the menu, then close it with Escape.

Reference target: `.timeline-toolbar [data-editor="more"]`.


### Build your scene

Tracks, webcam & layout.

**10. Give each layer its own track.** (`tracks`)

Use a video track for a screen recording, an image or a presenter. Use audio tracks for independent narration and music. Track controls handle visibility, mute, solo and locking.

Tip: Adding a track is optional. The sample already has multiple video and audio tracks.

Optional action: Open Add track to see the choices. You do not need to add one.

Reference target: `[data-action="trackMenu"]`.

**11. Add a face to the explanation.** (`webcam`)

Record a presenter take and place it above your screen recording. Review or retake it before adding it. It remains an independent clip that you can trim and position.

Tip: Opening the dialog does not activate your camera. This guide never requests camera or microphone permission. Close the dialog to return here.

Optional action: Optional: inspect the recording options. A camera is not needed for this guide.

Reference target: `.webcam-entry`.

**12. Keep the presenter out of the way.** (`layout`)

Select the presenter or another video layer. Drag it in the preview; drag a corner to resize it. Layout offers numeric sizing, placement presets, framing and crop controls.

Tip: Choose a corner that does not cover the button, text or detail you are teaching. Review the crop before exporting.

Optional action: Inspect Layout, or reposition a layer if you are ready to edit.

Reference target: `.inspector`.


### Smooth the edges

Fades & audio levels.

**13. Give clips a softer start and end.** (`fades`)

Set Fade in and Fade out, or drag the gold handles on a timeline clip. Video fades reveal the layer beneath; audio fades change volume. A preset is a quick starting point.

Tip: An edge fade is not a crossfade. A crossfade overlaps adjacent clips and shortens only their track. Check synchronization afterward.

Optional action: Optional edit: try a 0.5-second fade. Undo remains available.

Reference target: `.inspector`.

**14. Make the voice easy to hear.** (`audio`)

Balance narration, source audio and music in the mixer. Track mute and solo affect your rendered video. The speaker beside Play only changes what you hear while monitoring.

Tip: Listen to the actual rendered file before sharing. The sample sounds are synthesized cues and ambience, not a recorded voice.

Optional action: Open the mixer. Close it when you are ready to continue.

Reference target: `[data-action="mixer"]`.


### Make it easy to follow

Callouts, captions & chapters.

**15. Point to the next action.** (`callouts`)

Use the single preview header for text, arrows, highlights and zoom. More tools contains spotlights, numbered steps and privacy covers. Add them at the playhead. Annotations follow their source clip when you rearrange it.

Tip: Select an annotation to change its text, style, position and timing. One clear cue is often enough for a single action.

Optional action: Optional edit: add an arrow or another teaching cue.

Reference target: `.toolstrip`.

**16. Let viewers follow without sound.** (`captions`)

Write captions or import an SRT or plain WebVTT file, then check text and timing against the selected clip. Caption cues remain linked to their source footage.

Tip: Captions are manual or imported; automatic transcription is not connected. Readability checks flag dense or overlapping captions, but still review them yourself.

Optional action: Explore caption options. Adding captions is optional.

Reference target: `.sidebar`.

**17. Make the tutorial easy to revisit.** (`chapters`)

Add chapter markers for important steps. The companion Markdown note can embed the video and list timestamps, making the tutorial easier to find and use in Obsidian.

Tip: Chapters describes your video. Help opens this application walkthrough. They are separate.

Optional action: Preview the companion note without changing your project.

Reference target: `.sidebar`.


### Save your work. Share a video.

Checks, editable projects & outputs.

**18. Catch problems before you share.** (`checks`)

Checks points you to missing media, uncovered gaps, audio concerns and caption issues. Each item explains the problem and offers a relevant next action.

Tip: Checks is a review aid, not a guarantee. Watch and listen to the encoded video, especially when it contains private information.

Optional action: Open Checks, inspect its findings, then close it.

Reference target: `#issuesButton`.

**19. Save the project. Keep the options.** (`save`)

Save project keeps the editable workspace without rendering. A portable project includes available originals, timeline, teaching layers and render history so you can continue later.

Tip: Saving here downloads a project file. Keep that file somewhere safe. Browser recovery is not a backup, and a portable project may include uncensored originals.

Optional action: Open Save project to inspect the formats. No download is required to finish the guide.

Reference target: `.header-actions [data-action="downloadProject"]`.

**20. A video is a product of the project.** (`render`)

Render video creates a watchable output, not a replacement for the project. You can review a short range, play the actual output, then refine your edit and render another version.

Tip: Browser rendering here is a real-time review path with a three-minute limit. It does not write directly into a vault. Keep the editable project for later changes.

Optional action: Open Render video to inspect settings. You do not need to render anything.

Reference target: `.header-actions [data-action="save"]`.


### Come back anytime

Your projects & this guide.

**21. Keep working after the first export.** (`products`)

Open Project in the app header to find workspace files and rendered products. This keeps the editable workspace separate from rendered products. Open a saved project to continue, or inspect the edit snapshot behind an earlier output.

Tip: Missing originals can be reconnected without rebuilding your edit. An exported video by itself does not preserve separate tracks or annotations.

Optional action: Look through Project, then continue.

Reference target: `.sidebar`.

**22. Pick up here whenever you need.** (`help`)

Help is always here. Resume a paused walkthrough at the same step, jump to a chapter, search a quick answer, or revisit shortcuts. You can repeat any lesson.

Tip: Escape or Pause guide dismisses the overlays immediately. Your guide progress is separate from project saves.

Optional action: Finish the walkthrough, or pause and return later.

Reference target: `#editorHelp`.

## Acceptance

Read all 22 lessons without modifying the composition, history or saved products. Dismiss at each chapter and resume exact step. Test every target at desktop/compact widths, light/dark/forced colors and reduced motion. Open/close menus and all related dialogs mid-lesson. Verify keyboard ownership, no camera access before permission, no automatic downloads, no guide in actual decoded output, and accurate Session only messaging. Test real native persistence and screen-reader interaction on Windows separately from browser simulations.
