# Editable project, workspace and product data

## Document roles

The **project** is the non-destructive composition. The **workspace** is presentation state around that composition. The **project record** tracks revision identity and immutable rendered products. Originals are assets; a rendered video is a product, never the only saved form of the work.

The supplied executable file format uses `vault-buddy-video-project/3` inside `vault-buddy-workspace/1`, and portable packages use `vault-buddy-project-package/1`. These are serialization identifiers, not UI release labels. Preserve compatibility deliberately; a browser-to-native adapter is preferable to renaming the schema string and assuming equivalence.

See [the real example](../contracts/reference-workspace.example.json), [structural schema](../contracts/workspace.schema.json), and runtime validation in `reference/editor.js`, `editing-features.js`, `captions.js`, `project-lifecycle.js` and `session-safety.js`. The JSON schema does not replace semantic graph validation.

## Entities

| Entity | Stable identity and fields | Semantics |
|---|---|---|
| Project | `id,title,canvas,master_gain,assets,tracks,clips,effects,markers,transitions,captions,destination` | Render-affecting and instructional state. |
| Asset | `id,kind,name,duration_ms`, optional `width,height,size,type,media_type,builtin,linked_asset` | Original immutable source or procedural source. `linked_asset` supports detached audio; validate DAG and identity. |
| Track | `id,kind,name,visible,locked,muted,solo,volume` | Ordered composition layer or audio lane. Visibility and audio mute are separate. |
| Clip | `id,asset_id,track_id,name,start_ms,in_ms,out_ms` plus layout/mix/effect properties | Instance of a source range at an output time. |
| Effect | `id,clip_id,kind,start_ms,end_ms` plus geometry/style/text | Clip-linked teaching cue; source-time bounds. |
| Caption cue | `id,clip_id,start_ms,end_ms,text` | Clip-linked source time; display derives from actual output clip range/speed. |
| Chapter marker | `id,clip_id,source_ms,title` | Clip-linked chapter; output timestamp is derived. |
| Transition | `id,from,to,duration_ms,kind` | Explicit relationship between two compatible clips, not a hidden track overlap. |
| Workspace | tabs, selection IDs, playhead, panels, timeline scroll/zoom, snap, delete mode, monitoring preferences | Does not change rendered output or create undo edits. |
| Record | `id,revision,created_at,updated_at,products[]` | Revision history summary. Undo still advances current revision. |
| Product | `id,project_id,name,filename,mime,revision,duration_ms,created_at,edit_fingerprint,snapshot,render_range?` | Exact source revision and independent edit snapshot; encoded binary stored separately. |

Images use `kind:video` with `media_type:image` in the executable reference. A native enum may distinguish media kinds, but migration must preserve still duration, transforms and IDs. Never assume all visual assets have video frame clocks.

## Timing rules

Persist integer milliseconds for the reference interchange. A clip represents a **half-open** source interval `[in_ms,out_ms)` and starts at `start_ms` on the output timeline. With speed `s`, output duration is `round((out_ms-in_ms)/s)`; at an in-range output time `t`, source position is `in_ms + (t-start_ms)*s`, clamped to the playable source interval. Native sample/frame evaluation uses rational/integer media clocks rather than repeated float accumulation. [Media pipeline](NATIVE-MEDIA.md).

Attached annotation/caption/chapter time is source-linked. Trim changes the visible intersection, not the original cue timestamps. Move shifts output time without changing source cues. Speed changes their output durations. Split creates two valid source ranges and intersects/reassigns linked cues; markers on the cut belong to exactly one half-open child. Delete removes the selected clip and its clip-owned references without mutating shared originals. Duplicate/paste creates fresh entity IDs while retaining source identity. Tests must cover cuts at cue boundaries and non-unit speed.

Track-local ripple deletion shifts later clips on the same track; it is never advertised as globally synchronization-safe. The explicit intro insertion operation shifts all populated tracks together and refuses an atomic move that would affect locked tracks. A group move uses a shared clamped offset, not separate per-clip clamping that changes relative sync.

## Layout and compositing properties

Clip placement uses normalized output-canvas `x,y,w,h`. The reference preview may letterbox the canvas; pointer coordinates must first undo CSS sizing/letterboxing, then map into output coordinates. Native DPI affects window/capture geometry, not serialized normalized overlay geometry.

Visual clip controls include opacity; rectangle/rounded/circle frame; contain/cover; crop zoom and anchor; horizontal mirror/vertical flip; rotation; and brightness/contrast/saturation/sepia/grayscale adjustments. Speed is bounded to 0.25×–4×. Audio controls include clip volume/mute, track gain/mute/solo, master gain, fades and preserve-pitch intent. Audio monitoring state belongs to workspace, not export mix.

An effect's geometry/style depends on its kind: `text`, `arrow`, `highlight`, `spotlight`, `zoom`, `step`, `mask`. Do not flatten them into drawn pixels inside project storage. Editable arrow endpoints and zoom target/ramp must survive reopening. Static privacy masks are not tracked redaction; retain their source-time ownership and privacy warning.

Titles are editable generated card clips with preset/title/subtitle/background/foreground/accent properties. Caption presentation is project-level (`enabled,burn_in,font_size,position,background`) plus source-linked cue records. Caption visibility in preview and inclusion in output are explicit settings.

## Fade and transition rules

Clip edge fades and pairwise transitions are different. Edge fade envelopes multiply the clip's alpha/audio amplitude. The reference limits each fade to half the clip duration and supports documented curve choices. A dissolve/equal-power crossfade has compatible endpoints, positive bounded duration and available source/time geometry. No dangling, self-referential or cross-kind transition is valid.

The reference transition model permits one paired transition association per clip. Do not silently grow a different overlapping-transition algebra while porting; change the contract and tests explicitly when extending it. At an audio crossfade midpoint, evaluate the selected power curve, not a linear visual alpha assumption. Headroom handling belongs to the mixer.

## Persistence and source identity

The workspace envelope is `{schema,project,workspace,record,saved_at}`. Source bytes are not JSON. Portable packages contain a manifest, workspace and available media/outputs. Lightweight project JSON requires external media reconnection. The source collector includes assets referenced by **both the current graph and retained product snapshots**. Removing an asset from the current edit does not authorize deleting an original needed by a saved rendered revision.

Native identity should use an opaque asset ID plus content hash, byte size, media descriptor and approved storage locator. Hashes are computed off the UI thread and are not reconstructed from names. The reference's CRC32 edit fingerprint is a comparison aid, not an integrity/security primitive. Use canonical serialization plus an appropriate cryptographic digest for native revision/source integrity; preserve exact rendered snapshot bytes or canonical form.

A product's missing encoded binary does not erase its lineage. Show file availability separately. Restoring a product opens its edit snapshot as a new working revision; replaying an already encoded product uses its actual binary, not the editable preview.

## Validation order

1. Bound total input bytes, archive count/expansion and field collection sizes before allocating unbounded buffers.
2. Parse only accepted format identifiers; apply explicit tested migration functions.
3. Validate scalar types, finite safe integers, string lengths, dates, supported enum members and canvas constraints.
4. Require unique IDs per entity collection; validate all cross-references and compatible media/track kinds.
5. Reject cyclic/dangling linked assets, invalid source ranges, negative/overflow durations and invalid transforms.
6. Validate cue intersections, transition semantics, group movement/locking and product snapshot ownership.
7. Validate every retained snapshot and its referenced sources, not only the active graph.
8. Install the entire valid candidate atomically; failures leave the current workspace unchanged.

Paths and URLs embedded in JSON are not native file grants. Unknown fields must follow a documented round-trip or reject policy. Do not silently discard edit-affecting fields from a newer document and then overwrite it.

## Reference safety limits

The browser reference bounds assets at 200, tracks 32, clips 600, teaching effects 1,200, chapters/transitions 300 each, captions 2,000 and rendered records 40. Source metadata is bounded to two hours, project JSON to 8 MiB, and packaged original media to 200 MiB (220 MiB total package parser allowance). Its four canvases are 1280×720, 720×1280, 720×720 and 960×720 at 30 fps. Browser review output is limited to three minutes per whole output or range.

These are protective **reference limits**, not a negotiated native product ceiling or performance guarantee. The starter uses the same two-hour summary bound. Align interchange and native decoder limits together when the approved native product limits change; do not introduce inconsistent boundaries.
