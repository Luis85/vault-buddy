# User guide

## Make a tutorial

Open a capture or choose **Import media**. The included sample is safe to explore. Press **Space** to play or pause; click the ruler to move to a moment. Select a clip before changing its properties.

Use **Split** to cut at the playhead. Select an unwanted piece and Delete. The timeline's **Delete behavior** explicitly chooses whether to leave a gap or close it on that track. Drag a clip to arrange it, or use its numeric timeline position. **Undo** reverses a mistake. Ripple edits and transitions are track-local: check synchronization when other tracks must remain aligned.

Choose **Text**, **Arrow**, **Highlight** or **Zoom** from the row above the preview. **More tools** contains Spotlight, Numbered step and Privacy cover. Place the cue in the preview and adjust its timing in Properties. Cues are attached to footage, so moving the clip moves their intended moment as well.

Add a presenter with **Webcam**. Choose devices and explicitly enable them, record a take, review it, then add it. Resize the resulting layer using its corners, move it, or use Layout's size and corner controls. Camera setup is optional; importing an existing presenter recording works too. A webcam take recorded here is not a synchronized live screen/webcam recording session.

Add narration or background audio as another audio track. Use **Audio mixer** to set levels. The speaker beside Play mutes your monitoring only; clip and track mute affect the video output. **Fades** controls edge fades. A transition blends adjacent clips by overlapping them, shortening only their track.

Use **Captions** to type cues or import SRT/plain WebVTT. Review every cue and its reading time. **Chapters** creates named moments for the video/companion note. **Titles** provides editable cards, including an intro action that moves all existing tracks together.

Choose **Checks** before sharing. Fix missing sources and inspect gaps, audio, caption and visibility warnings. Checks are an aid, not certification. **Review** hides manipulation handles; it does not alter the edit.

## Save first, render when ready

**Save project** keeps the work editable without making a video. A portable `.vbproject.zip` includes available originals and optionally available rendered files. The lighter `.vbproject.json` keeps edit decisions but relies on originals being available later. Keep the project file somewhere safe; browser recovery is not a backup.

**Render video** creates a separate flattened output. Use a short review range to check a difficult section before rendering the full tutorial. **Watch rendered file** plays the encoded result—not the editing preview. Listen for missing audio and inspect captions, timing, crops and covered information.

The browser reference downloads files. The Rust/Tauri implementation will explicitly publish to the selected vault after its file transaction succeeds. A download prompt alone does not confirm that a file was saved to a vault.

## Continue after rendering

Open **Project → Workspace & rendered products** to see the working file and outputs. Save the project again after edits or a render whose history you need to retain. Open a portable project directly to continue. A rendered video alone does not contain editable tracks. Restoring the snapshot behind a product changes the current editable project; it does not rewrite the product file.

Missing originals appear in **Reconnect original media**. Choose the correct original file(s). Ambiguous files require your choice; the editor does not guess. Reconnecting changes source availability, not the edit's timing.

## Learn as you work

Choose **Show me around** on first use or **Help** later. The walkthrough highlights real controls. Next/Back and chapter selection control the pace. The optional task in a lesson is not mandatory. Trying an editing control does edit your current project and remains undoable normally.

**Pause guide**, close or Escape dismisses the guide. Help resumes the same lesson. Minimize makes room to work. An editor dialog temporarily suspends the guide; closing it returns to the lesson. **F1** opens the learning center. **F6** switches between the explanation and its highlighted target. Progress is separate from your project. A progress-file backup is available when browser storage is unavailable or you need portability.

## Find commands quickly

Right-click a clip, track, cue, source or gap for relevant actions. **Edit actions** beside the timeline and **Shift+F10** provide keyboard/non-right-click access. Numeric controls in Properties are alternatives to dragging. **View options** restores hidden panels; **Focus preview** hides side panels while retaining the timeline and editing tools.

## Troubleshooting

| Symptom | What to check |
|---|---|
| No sound | Monitoring mute, clip mute, track mute/solo, clip/track/master level and the actual encoded product. |
| Split unavailable | Select an unlocked clip and place the playhead inside it—not exactly on an edge. |
| Clip cannot move | Track lock, incompatible destination, overlapping clips/transition dependency, or a group that cannot move as a whole. |
| Camera unavailable | Explicit permission, secure browser context, OS privacy settings and whether another app holds the device. Import a recording instead. |
| Caption missing in output | Caption enabled/burn-in choice, cue source bounds and Checks. |
| Crop changed after portrait format | Inspect fit/fill, position and caption placement in the new output ratio. |
| Unsaved status after download | A browser requests the download but cannot guarantee where it was stored. Check Downloads and retain the project file. |
| Portable package too large | Save lightweight JSON and retain originals, or omit available rendered files. The browser has a 200 MB media-package cap. |
| Render slow or short | Browser rendering runs in real time and is limited to three minutes per output/range. Keep the tab active. Native production rendering is separate implementation work. |

## Private information

A static privacy cover can hide only the region and time you specified. It does not track movement and may be covered by another layer. Review the encoded output. A portable project retains original source content, including content trimmed or covered in the output. Share a rendered product rather than the full project when recipients must not receive originals.
