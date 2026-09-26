# Vault Buddy tutorial editor — concept design spec (visual parity reference)

Source bundle: `docs/concepts/vault-buddy-editor/`. Citations below are `file: "grep-able snippet"`; CSS/JS files are under `reference/` unless noted. The CSS is minified; grep the snippet verbatim.

**Cascade order matters.** `reference/build.py` concatenates CSS as `CSS_FILES=['editor.css','onboarding.css','session-safety.css','workspace-ui.css','handover-polish.css']` and JS as `drawing-primitives, editor, project-lifecycle, webcam, editing-features, captions, direct-edit, contextual-ui, preflight-recovery, onboarding, session-safety, workspace-ui, handover-polish`. Later files wrap/override earlier ones (`workspace-ui.css` is the final layout; `workspace-ui.js` wraps `renderLibrary`/`renderInspector`/`contextItems`). Where editor.css and workspace-ui.css disagree, the value given here is the one that wins at runtime (checked against the screenshots).

Screenshots (visual truth): `screens/01-welcome … 11-learning-center` at 1600×1000, `12-compact` at 960×640, `13-light`, `14-high-contrast` (forced colors), `15-startup-recovery` (startup failure page).

**BROWSER-ONLY** marks affordances that exist only because the concept runs in a browser (downloads, local-storage recovery, demo media, "Browser reference"). A native port should replace their *copy* (DESIGN-SYSTEM.md: "Say **Download started** in the browser path and **Saved** only for native confirmed persistence") but can keep their *slot/shape*.

---

## 1. Global frame

### 1.1 Tokens (dark = default)

`shell.html: <html data-theme="dark"` — dark is the default; light is a View-menu toggle (`workspace-ui.js: item('Light theme','sun',routeCommand('theme',anchor),{checked:document.documentElement.dataset.theme==='light'})`).

| token | dark (`editor.css: ":root{color-scheme:dark;--bg:#18191e"`) | light (`editor.css: "[data-theme=light]{color-scheme:light;--bg:#f3f3f7"`) | role |
|---|---|---|---|
| --bg | #18191e | #f3f3f7 | app body, inputs, preview header, transport, status bar, inner wells |
| --panel | #202127 | #fff | header, sidebar, inspector, timeline, dialogs, menus |
| --raised | #292a33 | #f1f0f5 | pills, toast, title-template tiles |
| --stage | #131419 | #e8e7ef | preview stage letterbox area |
| --line | #353640 | #dcdce5 | every 1px border/separator, grid lines, slider track |
| --hover | #30303c | #eeebf5 | button hover bg |
| --ink | #f0eef6 | #292632 | primary text |
| --sub | #c5c2d0 | #504b5d | secondary text (labels, tool labels) |
| --muted | #a39fac | #696274 | helper/meta text, icon buttons |
| --accent | #b6a2f5 | #7250ad | accent strokes, active underline, playhead, selection outline |
| --accent-ink | #dacdff | #603696 | text on accent-bg |
| --accent-bg | #393049 | #ece5f8 | active tab/tile/glyph backgrounds |
| --primary | #8b6ad4 (hover #9475d8, `".primary:hover{background:#9475d8}"`) | #7853b8 | primary buttons (white text, 600) |
| --video / --video-bg | #aa92ed / #40364f | #724f9e / #e5ddee | video clips, V badges, video thumbs |
| --audio / --audio-bg | #8cccc0 / #263e3d | #246e62 / #d9eeea | audio clips, A badges, saved dot, meter fill, success |
| --gold / --gold-bg | #ebc582 / #493d2c | #8b5c16 / #f7ecd8 | fades, markers, zoom cue, active M/S/lock, captions, warnings |
| --danger | #ffa3b4 | #b63854 | destructive text |
| --ring | #d4c1ff | #7853b8 | focus outline |
| --shadow | `0 18px 60px #0005` | `0 18px 60px #2b194025` | dialogs, drawers, toast, menus |
| --guide-edge | #ac8feb | #7952ba (`onboarding.css: "[data-theme=light]{--guide-edge:#7952ba"`) | coach/invite border, highlight ring |
| --guide-shadow | `0 20px 70px #0006` | `0 20px 70px #25163a33` | guide surfaces |
| --guide-dim | `rgba(10,11,18,.52)` | `rgba(20,16,35,.28)` | walkthrough shade |

Non-token literals used: success `#7bd4bc` (`editor.css: ".pill.success{background:rgba(84,175,144,.14);color:var(--success,#7bd4bc)}"`), amber `#ffd279`, unsaved dot `#ecc783` (`".dot.unsaved{background:#ecc783}"`), recording `#f48796`, canvas bg `#17151f`, clip duration badge `#16141a80`, backdrop `#080711aa`. `contracts/design-tokens.json` repeats the dark set (its `--sidebar:244px;--inspector:276px` are the runtime values).

### 1.2 Typography

- Body: `editor.css: "body{margin:0;background:var(--bg);color:var(--ink);font:12px/1.5 -apple-system,BlinkMacSystemFont,\"Segoe UI\",sans-serif;overflow:hidden}"` → on Windows Segoe UI 12px / 1.5.
- Mono: `".mono{font-family:ui-monospace,SFMono-Regular,Consolas,monospace;font-variant-numeric:tabular-nums}"` (timecode, ratio button, ruler ticks, track badges, kbd, issue chips, step counter).
- Size ladder (px): 8 (clip duration/badges), 9 (meta, pills, status bar, ruler ticks, fx cues, context kbd/hint), 10 (field labels, helpers, eyebrow, inspector heading, tab labels, toolbar selects), 11 (buttons, menu items, section h3, names, tool labels), 12 (body, clip-card titles, dialog body), 13 (checks summary, guide body), 14 (project title, brand), 15 (save intro h3), 16 (dialog h2), 22/23 (invite/coach h2), 29 (learning hero h2), 31 (progress %), 35 (speed readout).
- Weights: 500/550 (names, titles), 600 (h2/h3/primary/eyebrow), 650 (brand, guide headings), 700 (guide overline).
- Eyebrow: `".eyebrow{font-size:10px;font-weight:600;letter-spacing:1px;text-transform:uppercase;color:var(--muted)}"`; library override `workspace-ui.css: ".library-heading .eyebrow{font-size:10px;letter-spacing:.65px}"`.
- Utility: `.tiny{font-size:10px}`, `.small{font-size:11px}`, `.muted{color:var(--muted)}`, `.sub{color:var(--sub)}`.

### 1.3 App grid (1600×1000)

- `workspace-ui.css: ".app{grid-template-rows:56px minmax(170px,1fr) 8px var(--timeline) 25px}"` → header 56 / workspace 511 / resize bar 8 / timeline 400 / status 25. `editor.css: ".app{height:100dvh;min-height:520px;display:grid;grid-template-columns:minmax(0,1fr)"`.
- Workspace columns: `workspace-ui.css: ":root{--sidebar:244px;--inspector:276px}"` + `".workspace{position:relative;grid-template-columns:var(--sidebar) minmax(0,1fr) var(--inspector)}"` → 244 / 1080 / 276. Grid children pinned: `".workspace>.sidebar{grid-column:1;grid-row:1}"` etc. (a hidden sibling does not collapse the others).
- Timeline height: `--timeline` default 400, set inline by `editor.js: "function setTimelineHeight(h){h=clamp(h,170,Math.max(170,Math.min(540,innerHeight-370)))"`. At 1000px tall → 400; at 640 tall → 270.
- Timeline label column: `--label:196px` (`editor.css ":root{…--label:196px}"`).
- Panel visibility classes: `.workspace.library-hidden` (col 1 = 0), `.properties-hidden` (col 3 = 0), both; `.app.focus-view` (both 0: "Focus preview"); `.app.review-mode` (Review).

### 1.4 Breakpoints (what changes)

| condition | changes (winning rules) |
|---|---|
| ≤1350w | brand wordmark hidden (mark only), save status hidden, header gap 5–8 (`workspace-ui.css: "@media(max-width:1350px){ .topbar{gap:8px} .topbar .brand>span{display:none}"`) |
| ≤1300w | context-menu width 260 (`editor.css: ".context-panel{width:260px}"`) |
| ≤1200w | `--label:184px` (editor.css; its --sidebar/--inspector here are overridden by workspace-ui's unconditional :root) |
| ≤1080w | `--sidebar:232px`; inspector hidden and becomes an overlay drawer: `".workspace.inspector-open .inspector{display:flex;position:absolute;top:0;bottom:0;right:0;width:276px;z-index:18;box-shadow:var(--shadow)}"`, `top:48px` (44 when ≤760h); title 12px; header buttons padding 6 9 |
| ≤1000w | `--label:174px` (editor.css `@media(max-width:1000px){:root{…--label:174px}`) |
| ≤860w | sidebar hidden → drawer `".app.library-drawer .sidebar{display:flex;position:absolute;top:0;bottom:0;left:0;width:250px;z-index:20"` (top 48/44); brand hidden; Checks label hidden; Help icon-only |
| ≤620w | rows `52px minmax(160px,1fr) 8px var(--timeline) 23px`; Render label "Render"; Save project icon-only; zoom slider hidden; transport right badge hidden |
| ≤760h | rows `52px … 23px`, topbar 52, preview header 44, transport 40, stage padding `10px 14px` (`workspace-ui.css: "@media(max-height:760px){ .app{grid-template-rows:52px minmax(160px,1fr) 8px var(--timeline) 23px}"`); dialog paddings shrink (`session-safety.css: `.dialog .dialog-body{padding:16px;gap:13px}`) |

**960×640 (12-compact)**: header 52; sidebar 232 visible; inspector hidden (drawer on demand); label column 174; preview header 44 with density "medium" ("Preview" word hidden, "More tools" → "More"); transport 40; timeline 270; status 23; Help shows the gold resume dot. Preview-header density is container-based: `workspace-ui.js: "bar.dataset.density = width < 520 ? 'compact' : width < 800 ? 'medium' : 'wide'"`.

### 1.5 Surfaces, borders, radii, spacing

- Every region separator is `1px solid var(--line)` (topbar `border-bottom`, sidebar `border-right`, inspector `border-left`, preview header `border-bottom`, transport `border-top`, toolbar/ruler/track rows `border-bottom`, footer/status `border-top`).
- Backgrounds: header/sidebar/inspector/timeline = panel; preview header/transport/status bar = bg; stage = stage.
- Radii: button 7 (`"button{border:1px solid transparent;border-radius:7px"`), input/select 6, icon tiles 7, clip 5, fx cue 4, pill 4, tabs 6, menu 8/10, dialog 13, guide coach 13, invite 14, learning center 15, canvas 6.
- Button base: `"button{…gap:7px;padding:7px 10px;min-height:32px;cursor:pointer;white-space:nowrap}"`; `.icon-btn{width:32px;height:32px;padding:6px}`; `.btn{border-color:var(--line);background:var(--panel)}`; `.primary{background:var(--primary);color:white;font-weight:600;border-color:transparent}`.
- Inputs: `"input:not([type=range])…,select,textarea{border:1px solid var(--line);border-radius:6px;min-height:34px;padding:7px 9px;width:100%;background:var(--bg)}"`; range 24px tall, `accent-color:var(--accent)` (native thumb in accent #b6a2f5); checkbox 16×16.
- Spacing is ad-hoc px (3,5,6,7,8,9,10,12,13,14,15,16,20). Common: panel padding 14/12, section padding `15px 0`, field gap 5, fieldpair gap 9.
- Pills: `".pill{font-size:9px;letter-spacing:.3px;border:1px solid var(--line);padding:2px 6px;border-radius:4px;background:var(--raised);color:var(--sub)}"`, `.pill.accent` accent-bg/accent-ink.
- kbd: `"kbd{font:10px ui-monospace,monospace;padding:2px 4px;border:1px solid var(--line);border-radius:3px;color:var(--muted)}"`.
- Divider: `".divider{width:1px;height:20px;background:var(--line);margin:0 3px}"`.

### 1.6 Focus, selection, states, scrollbars

- Focus: `"button:focus-visible,input:focus-visible,…,[tabindex]:focus-visible{outline:2px solid var(--ring);outline-offset:2px}"`. Clips/cues: `session-safety.css: `".clip:focus-visible,.fx-clip:focus-visible{outline:2px solid var(--ring);outline-offset:2px;z-index:9}"`. Menu items use inset ring: `".context-panel [role^=\"menuitem\"]:focus-visible{box-shadow:inset 0 0 0 1px var(--ring)}"`. (Screens 02/12/13 show a focus ring on Help because focus returned there after the invitation.)
- Hover: `button:hover{background:var(--hover)}`. Disabled: `button:disabled{opacity:.4;cursor:not-allowed}`.
- Active/pressed toggle: `"button.active{color:var(--accent-ink);background:var(--accent-bg);border-color:color-mix(in srgb,var(--accent) 28%,transparent)}"`.
- Scrollbars: only timeline and guide surfaces are styled: `".timeline-scroll{overflow:auto;…scrollbar-color:var(--line) transparent;scrollbar-width:thin"`; coach/hub same. Others are native (dark via `color-scheme`).
- Forced colors (screen 14): every button/input gets a `ButtonText` border, selected = `outline:2px solid Highlight;outline-offset:-2px`, focus 3px Highlight, fade handle `ButtonFace`, playhead `Highlight` (`handover-polish.css: "@media (forced-colors: active)"`).
- Reduced motion: `editor.css: "@media(prefers-reduced-motion:reduce){*,*:before,*:after{transition:none!important;…animation:none!important}"`.

---

## 2. Header (topbar, 56px)

`workspace-ui.css: ".topbar{gap:14px;padding:0 16px;height:56px}"`, bg panel, border-bottom line. Left→right: brand, Project menu, document title (flex:1), header actions.

- **Brand**: `.brand` 14px/650, letter-spacing -.5px, gap 8 (`".topbar .brand{width:auto;font-size:14px;gap:8px;margin-right:2px}"`). Mark = CSS drawing, not an image: `".brand-mark{font-size:16px;letter-spacing:3px;width:29px;height:31px;background:var(--accent-bg);color:var(--accent);border:1px solid var(--accent);border-radius:9px 9px 12px 12px;display:grid;place-items:center"` containing the text `··` (two eyes), plus two ears `".brand-mark:before,.brand-mark:after{content:'';position:absolute;top:-5px;width:6px;height:7px;border:1px solid var(--accent);background:var(--accent-bg);border-bottom:0;border-radius:3px 3px 0 0}"` at left/right 4px. Wordmark "vault-buddy" (hidden ≤1350w). Tooltip "Vault Buddy · Tutorial Editor".
- **Project menu trigger**: `shell.html: class="btn project-menu-trigger"` = folder icon + "Project" + chevronDown (12px); `".project-menu-trigger{font-size:11px;border-color:transparent;padding:6px 8px;gap:7px;background:transparent}"`. Opens a command menu (context-panel styling, §8) anchored at trigger left, bottom+6, heading **"Project"** / subtitle "Working files, originals and rendered products". Items (`workspace-ui.js: "if (kind === 'project') return ["`):
  1. plus — New project…
  2. folder — Open project…
  3. edit — Rename tutorial…
  4. — separator —
  5. layers — Workspace & rendered products (opens library "project" section)
  6. shield — Project files & session… (session dialog, §9.9)
  7. — separator —
  8. undo — Restore sample project… (BROWSER-ONLY sample)
- **Document title**: button `.document-name` containing `h1.project-title` 14px/600 ink (`".document-name .project-title{font-size:14px;letter-spacing:-.1px"`) + pencil (edit) icon 13px that is `opacity:0` until hover/focus (`".document-name>.icon-slot{width:13px;opacity:0"`). Click → Rename dialog (§9.10). Ellipsis on overflow.
- **Header actions** (`".header-actions{flex-shrink:0;margin-left:0;gap:7px}"`, buttons `min-height:34px;font-size:11px`):
  - **Save state**: `#saveStatus` text 10px muted, 5px dot (`".dot{…width:5px;height:5px;border-radius:50%;background:var(--audio);margin-right:5px}"`), dot gold `#ecc783` when unsaved. Text is a button (role=button) opening the session dialog; hover underlines. Labels (preflight-recovery.js): "Save project needed" / "Project download requested" — BROWSER-ONLY wording (native: e.g. "Unsaved changes" / "Saved"). Hidden ≤1350w.
  - **Help**: `onboarding.js: help.className='btn guide-help-button';help.innerHTML=icon('book')+'<span>Help</span><i class="guide-resume-dot"`; transparent (`"#editorHelp:hover{background:var(--hover)}"`), book icon in `--accent`, 5px gold dot after label when a walkthrough is paused (`".has-guide-progress .guide-resume-dot{display:inline-block}"`). Opens the learning center (§9.3). Tooltip "Help & learning center (F1)" / "Resume guide: … (F1)".
  - **Checks**: transparent button, check icon + "Checks" + count chip `#issueCount` (`"#issueCount{font:9px ui-monospace,monospace;…background:var(--gold-bg);color:var(--gold);padding:2px 5px;border-radius:8px}"`, hidden when empty).
  - **Save project**: `.btn` (bordered, panel bg) save icon + "Save project" (tooltip "…(Ctrl+S)").
  - **Render video**: `.primary` video icon + "Render video" (label "Render" ≤620w).
  - (≤860w a `…`/compact controls appear; not needed at desktop.)

---

## 3. Left library (sidebar, 244px)

`.sidebar{background:var(--panel);border-right:1px solid var(--line);display:flex;flex-direction:column}`.

### 3.1 Tabs row (48px)
`workspace-ui.css: ".sidebar .tabs{display:flex;flex-wrap:nowrap;min-height:48px;height:48px;flex:0 0 48px;padding:5px 6px;gap:0"`; buttons `flex:1;padding:6px 4px;border:0;border-radius:6px;font-size:10px;min-height:32px`; active `background:var(--accent-bg);color:var(--accent-ink)` (a filled pill, not an underline). Border-bottom line. Labels: **Media**, **Titles** (`data-tab="create"`), **Captions**, **Chapters** (`data-tab="guide"`, renamed in `onboarding.js: guideTab.textContent='Chapters'`).

Library body: `workspace-ui.css: ".library{padding:14px 12px}"`, scrolls.

### 3.2 Media tab (screens 02–05)
Order (after `FocusUI.compactLibrary` + contextual-ui search injection):
1. **Media actions row** `".media-actions{display:flex;gap:7px;margin-bottom:12px}"`:
   - **Import media**: primary, flex:1, upload icon + bold "Import media" 11px/600 white (`".media-actions .import{…border:0;color:#fff;background:var(--primary)}"`).
   - **Webcam**: bordered, bg var(--bg), color sub, webcam icon 16px + "Webcam" 11px, radius 7 (`".media-actions .webcam-entry{…background:var(--bg);border:1px solid var(--line);border-radius:7px;color:var(--sub);font-size:11px}"`). Note `editor.css: ".webcam-entry{…padding:11px!important"` wins → both buttons render **40px tall** (screen 02: y 118–158).
2. **Search**: `".media-search{position:relative;margin-bottom:14px}"`, search icon 14px at left 10 top 10 muted, input `min-height:32px;padding-left:32px;font-size:11px`, placeholder "Find media…". Filters rows by name.
3. **Heading** `.library-heading` (space-between, mb 11): eyebrow **"SOURCE MEDIA"** + pill "5 assets".
4. **Media rows** `".media-card{display:flex;align-items:center;gap:9px;padding:9px 0;border-bottom:1px solid var(--line)}"` (focusable, Enter adds):
   - Thumb `".media-thumb{width:58px;height:40px}"`, radius 5, bg video-bg, `object-fit:cover`; audio thumb bg audio-bg with teal music icon centered.
   - Name `b` 11px/550 single-line ellipsis; meta `".media-card .meta{font-size:9px}"` muted, mt 4: `"00:33 · Built-in sample"` (format `fmt(duration) · {Built-in sample | W × H | Local audio | Missing source}`).
   - Trailing button 28×30 (`".media-card button{padding:4px;width:28px;min-height:30px}"`): plus icon ("Add … to timeline"), or link icon when source missing ("Relink").
   - Rows are draggable to tracks.
5. (Removed in final UI: the dashed "Import video…" tile and tips.)

### 3.3 Titles tab (`contextual-ui.js: if(ui.tab==='create')`)
Heading "GIVE IT STRUCTURE" + pill "Local"; p small muted "A clear beginning, useful chapters, and a next step."; **intro-insert** button (accent-bg, 1px accent border, radius 9, layers icon 19px accent, b 11px "Insert intro at the beginning" / small 9px "Move every existing track together"); grid gap 12 of 4 **title-template** cards (radius 9, raised bg, hover border accent) each = 16:9 `.template-canvas` (padding 18, b 15px/600 title, small 6px letter-spacing 1.3 "VAULT BUDDY · YOUR TUTORIAL") + label row 11px with plus icon. Canvas colors: intro `#393049`, chapter `#272333`, outro `#243b38` (small `#b8dacf`), blank `#3a334d` (b 13px `#b9abc9`). Texts: "A clear beginning"/Intro card, "One step at a time"/Chapter card, "What happens next?"/Closing card, "Room for your idea"/Plain background. Then help text, `.btn` image icon "Import a still image", help.

### 3.4 Captions tab (screen 06; `captions.js: function renderCaptionLibrary`)
Heading **"EVERY WORD, ACCESSIBLE"** + pill count; p "Write or import captions. No speech service is connected."; `.caption-source` box (bg, line border, radius 7, padding 10, 10px) with link icon + "Attached to **Presenter · demo**" (or "Select footage or audio to add captions."); two full-width left-aligned `.btn` (11px): plus "Add caption", upload "Import SRT / VTT" (disabled without a selected clip); `<details class="caption-settings">` "Caption appearance" (borders top/bottom, padding 12 0) with checks Show captions / Burn into rendered video / Readable background, Position select (Bottom/Top), Font size number (18–56); caption list of `.caption-card` (bg, radius 8, padding 10): mono gold time button "00:01.0 — 00:03.2" 8px, edit icon-btn 25px, text 11px/1.6, optional gold advice "Review reading time / line length", row with "Split cue" small and trash icon-btn; empty state (panel-empty centered muted, lh 1.7): "Make your tutorial understandable without sound. Select a clip, then add or import captions."; `.btn` download icon "Download timeline SRT" (BROWSER-ONLY verb; native = export); help "Imported timing begins at 00:00 of the visible clip…".

### 3.5 Chapters tab (`editor.js: const ms=visibleMarkers();root.innerHTML=`)
Heading "TUTORIAL CHAPTERS" + pill count; "Chapters follow their source clip."; rows `.guide-item` (padding 10 6, border-bottom) = 26px accent-bg circle number + b 11px title + mono 10px muted time, and an edit icon-btn; empty "Add chapters to turn your recording into a reusable guide."; full-width `.btn` bookmark "Add chapter at playhead"; `.btn` file "Preview companion note"; `.library-tip` (raised, radius 8, padding 12, 11px muted, b sub) "Keep the context. …".

### 3.6 Project section (not a tab; opened from Project menu)
`workspace-ui.js: back.innerHTML=svg('arrowLeft')+'Back to media'` link (10px muted) on top; heading "YOUR WORKSPACE" + accent pill "r1"; `.project-summary` (folder icon 25px accent, title, hint); product list `.product-card` (radius 9, padding 12; pill success "Matches this edit"/amber "Earlier edit", mono r#; name; meta; Video button + restore icon); empty `.empty-products` (dashed, radius 9, video icon 26px) "No renders yet"; `.btn wide` video "Render a new video".

---

## 4. Preview column

### 4.1 Preview header (48px)
`workspace-ui.css: ".preview-header.projectbar{height:48px;min-height:48px;flex:0 0 48px;padding:0 12px;gap:8px;border-bottom:1px solid var(--line);background:var(--bg)"`.
Left→right:
1. **Library toggle** icon-btn 30px wide, panelLeft icon, muted (sub when pressed) — `".preview-header .panel-toggle{color:var(--muted);width:30px"`.
2. **"Preview"** 11px/600 sub (`".preview-heading{font-size:11px;font-weight:600;color:var(--sub)"`); hidden at medium/compact density.
3. **Ratio button** "16:9 ⌄": mono 10px muted, transparent border, min-width 55, 30 tall, chevron 12px (`".preview-header .ratio-button{font-size:10px;color:var(--muted);padding:4px 7px;border-color:transparent;min-width:55px;min-height:30px}"`). Opens "Frame your tutorial" dialog (§9.11).
4. **Toolstrip** (role=toolbar, roving tabindex, arrow/Home/End), `margin-left:auto` so it centers between left group and right actions; buttons height 32, padding 5 9, gap 6, 11px sub, icons 15px: text **Text**, arrowUpRight **Arrow**, box **Highlight**, zoomIn **Zoom**, then **More tools ⌄** separated by `border-left:1px solid var(--line);border-radius:0 6px 6px 0;margin-left:5px` (plus icon + label + 10px chevron). Density: medium → "More"; compact → Highlight/Zoom move into More.
   - **More tools menu** (heading "More teaching tools" / "Choose a cue for the current moment"): [overflowed Text/Arrow/Highlight/Zoom + separator when hidden] → spotlight **Spotlight**, step **Numbered step**, shield **Privacy cover** (disabled w/ reason "Select an unlocked video clip, or move the playhead over visible footage."), separator, layers **Browse all teaching tools**.
5. **Preview actions** (`margin-left:auto`, gap 3): **Review** (transparent .btn, play icon + "Review"; in review mode edit icon + "Back to edit", `.active`), **View options** icon-btn (more ⋯), **Properties toggle** icon-btn (sliders) with aria-pressed.
   - **View menu** (heading "View & workspace" / "Change your workspace, not your edit"), checkbox items show a check glyph when checked: panelLeft Show media library ☑; sliders Show properties ☑; video Focus preview; undo Reset panel layout; — ; sun Light theme ☐; — ; camera Download annotated frame… (BROWSER-ONLY); sliders Audio mixer…; info Keyboard shortcuts & help….

### 4.2 Stage
- `workspace-ui.css: ".stage{padding:14px 20px;background:var(--stage)}"`, flex centered, overflow hidden.
- Canvas box sized by `fitCanvas()` to the largest box of the project ratio (16:9 default; also 9:16, 1:1, 4:3 — `editing-features.js: FORMATS={wide:{label:'16:9',name:'Landscape',w:1280,h:720},portrait…}`) inside the padded stage: `".canvas-wrap{…border-radius:8px;overflow:hidden;outline:1px solid #0002;…background:#17151f}"` + `".canvas-wrap{border-radius:6px;box-shadow:0 8px 28px #0003}"`. At 1600×1000 the canvas is 692×389 at (438,118).
- **Video-layer selection box** (drawn on the preview only, never rendered; `editor.js: "target.strokeStyle='#bba1f5';target.lineWidth=2;target.setLineDash([6,4])"`): dashed 2px `#bba1f5` rect around the selected video layer (only when not full-frame), 4 corner handles 10×10 radius 2 white fill with `#916bc6` stroke. Drag body = move, drag corner = proportional resize.
- **Cue selection** (`drawing-primitives.js: function drawSelection`): 1.4px `#8565c5` dashed rect offset 4px around the cue bounds; round white handles r=6 stroke `#8062b7` (bottom-right resize; arrow has both endpoints + dashed white guide line `#ffffffb0`); zoom shows dashed bounds + 15px crosshair circle.
- **Cue rendering** (`function drawEffect`): default cue color `#ffd279` (text `#ffffff`); arrow stroke 5 with 23px head, soft shadow; text box `#292332eF` radius 9, 29px/550; highlight fill color+`20` alpha, stroke 4 radius 8; spotlight dims outside `rgba(15,12,24,.62)`; numbered step pill `#292332ed` + 22px colored circle with `#47364e` number.
- **Captions on canvas** (`captions.js: function drawCaptions`): centered box `#11111cee` radius 7·k, 600 weight "Segoe UI" 30px·k white, max 4 lines, 6% from bottom (or top).
- Overlays that exist but are hidden in final UI: `#selectionBar`, `.stagehint`, `.inspector-bottom` (`workspace-ui.css: "#selectionBar,.stagehint,.inspector-bottom{display:none!important}"`).

### 4.3 Transport (46px)
`workspace-ui.css: ".transport{height:46px;min-height:46px;padding:4px 15px;border-top:1px solid var(--line);background:var(--bg)}"`, three rows, space-between.
- Left: monitor icon-btn (volume / muted icon; "Mute monitoring" – preview only), master peak meter `".master-meter{width:48px;height:8px;border-radius:3px;background:var(--line)"` fill `var(--audio)` (danger when hot: `"#peakMeter.peak-hot{background:var(--danger)}"`), speed `<select>` transparent borderless 11px: 0.5× / 1× / 1.5× / 2×.
- Center: skipBack icon-btn ("Go to start (Home)"), **Play** 34px circle accent-bg with line border, accent-ink icon (`".transport .play{width:34px;height:34px;min-height:34px;border-radius:50%;background:var(--accent-bg);color:var(--accent-ink);border:1px solid var(--line)}"`), skipForward icon-btn, time `mono` 11px min-width 137: `00:13.4 / 00:33.5` (current ink, "/ total" muted). Format `fmt(ms,true)` = `MM:SS.d` (`drawing-primitives.js: "const fmt=(ms,decimal=false)=>"`).
- Right (max-width 160, right-aligned): badge text 9px muted uppercase ellipsis "SAMPLE PROJECT · LOCAL PREVIEW" / "LOCAL MEDIA · PREVIEW" (`".transport .preview-badge{position:static;…color:var(--muted);padding:0;font-size:9px"`). BROWSER-ONLY sample wording; tooltip "Preview label only…".

---

## 5. Inspector (276px)

`.inspector{background:var(--panel);border-left:1px solid var(--line);display:flex;flex-direction:column}`.

- **Heading** 48px: `".inspector-heading{height:48px;flex:0 0 48px;color:var(--muted);font-size:10px;font-weight:500}"`, padding 0 15, border-bottom; title "Clip properties" / "Properties" / "Track properties" / "Teaching properties" / "Selection properties"; right x icon-btn = **Hide properties** (`workspace-ui.js: hideProperties.dataset.action='inspector'`).
- **Body** `".inspector-body{padding:14px;scroll-padding:12px}"`, scrolls.
- **Asset card / selection title**: `".selection-title{display:flex;align-items:center;gap:10px;margin-bottom:12px}"`; glyph tile 34×34 radius 7 accent-bg accent-ink (video / music / layers / cue icon); b 12px/550 "Presenter · demo"; small 10px muted "{track name} · {d}s" e.g. "Webcam · presenter · 32.0s". In screen 02 the glyph+name sit in a raised-looking card because the tab grid follows directly; no extra wrapper.
- **Category tabs (2×3 grid)**: `editor.css: ".expanded-tabs{display:grid!important;grid-template-columns:repeat(3,minmax(0,1fr));padding:4px!important;background:var(--bg);border:1px solid var(--line);border-radius:8px;gap:3px!important;margin-bottom:15px}"`, buttons `min-height:30px;font-size:11px;border-radius:5px`, active = `button.active` (accent-bg/accent-ink + 28% accent border). Video: **Clip, Layout, Fades / Audio, Speed, Color**; audio: Clip, Fades, Audio, Speed (`contextual-ui.js: (video?['properties','layout','fades','audio','speed','color']:['properties','fades','audio','speed'])`). Locked track adds `.field-help.warning` (gold-bg, gold text, radius 6, padding 9) "Track locked. Unlock it using the padlock below."
- **Sections**: `".section{padding:15px 0;border-top:1px solid var(--line);display:flex;flex-direction:column;gap:10px}"`, h3 11px/600. Fields `".field{…gap:5px;font-size:10px;color:var(--sub)}"`, inputs 11px ink. `".fieldpair{display:grid;grid-template-columns:1fr 1fr;gap:9px}"`. Help `".field-help{font-size:10px;color:var(--muted);line-height:1.6}"`. Preset rows `".preset-row button{border:1px solid var(--line);font-size:10px;min-height:29px;padding:4px 8px}"`. Range fields show label left + mono value right above the slider (`editor.js: const range=`).
- **Precision disclosures** (collapsed by default, remembered per selection): sections titled "Frame & crop" and "Transform source" become `<details class="section precision-section">` with summary 11px/550, min-height 40, `+` / `−` marker in 16px muted (`workspace-ui.css: ".precision-section summary::after{content:'+';font-size:16px"`).

### Tab contents (clip selected)
- **Clip** (`editor.js: inspectorSection('Placement'`): *Placement* — Clip name input; Track select (same-kind tracks, "· locked" suffix); Timeline start (s) number; preset-row arrowLeft **Earlier**, arrowRight **Later**, copy **Duplicate**; help "Earlier / Later swaps adjacent clips. Other tracks are not moved." *Source range* — In (s) / Out (s) fieldpair; help "Original length 00:36.0. Numeric source trims keep this clip's timeline start fixed." Title-card clips prepend *Title card* (title/subtitle textareas, Background/Text color inputs, Duration).
- **Layout** (screens 02/03, `webcam.js: function videoLayoutHTML`): section **"Webcam overlay"** (or "Video layout"): preset-row **Full frame** / **Picture-in-picture**; fieldpair Horizontal (%) / Vertical (%) numbers (e.g. 77.5 / 6); **Size** range 10–100 with "19%" value; `.corner-presets` 2×2 grid (`"grid-template-columns:1fr 1fr;gap:6px;margin-top:12px"`, buttons min-height 34 bordered 10px) each with a CSS corner glyph `".corner-icon{width:21px;height:15px;border:1px solid var(--muted);border-radius:2px"` + 6×5 accent dot in its corner: Top left, Top right, Bottom left, Bottom right; help "Drag to move. Drag any corner to resize. Size keeps the frame's proportions; these controls work without dragging." Then disclosure **Frame & crop** (shape presets Rounded/Circle/Rectangle, Image fitting select "Fill · crop to frame"/"Fit · show entire image", Crop zoom + Focus X/Y when fill, Mirror checkbox, Opacity range, help) and disclosure **Transform source** (`.transform-buttons` 2-col: rotate Rotate 90°, arrowRight Flip horizontal, down Flip vertical, locate Center, zoomOut Fit source, zoomIn Fill frame; help).
- **Fades** (screen 04): h3 "A softer entrance. A cleaner exit." (audio: "Let the sound arrive naturally."); **fade graph** `".fade-graph{…height:70px;background:var(--bg);border:1px solid var(--line);border-radius:7px}"` overridden to `".fade-graph{height:58px}"`, SVG `viewBox="0 0 240 70"` in accent: faint guide lines at y 15/55, 2.5px trapezoid envelope with quadratic ease, 10% fill, 3px dots at the fade knees (`editor.js: function fadeGraph`); help "Fade to reveal the layer underneath. On the bottom track, fade to black."; Fade in (s) / Fade out (s); Curve select Linear / Smooth / Equal power (audio); preset-row None / Quick · 0.5s / Gentle · 1s; `.btn` play "Preview entrance"; help "Gold handles on the clip do the same thing. Each fade is limited to half the clip." Section **Between two clips**: help + presets Blend · 0.5s / Blend · 1s, or `.callout` "Cross dissolve / Equal-power crossfade · 0.50s overlap · selected track only" + x "Remove crossfade".
- **Audio**: *Clip audio* — Volume range 0–200 %; checkbox Mute this clip; help; warning "This built-in video sample has no source audio."; music `.btn` "Detach source audio". *Mix* — sliders `.btn` "Open audio mixer"; help "Monitoring mute does not mute exports. Track mute and solo do affect exports."
- **Speed**: h3 "Keep the useful pace"; `.speed-readout` (`"display:flex;align-items:center;gap:18px;background:var(--accent-bg);color:var(--accent-ink);padding:18px 13px;border-radius:10px"`, big `b` 35px/550 "1×" with 17px "×", right 11px "00:32.0 on the timeline / 00:32.0 source duration"); Clip speed select (0.25…4×, "1× · original"); "When duration changes" select (Move following clips on this track / Keep following clips in place); checkbox Preserve voice pitch; warnings; help.
- **Color**: h3 "A consistent look" → `.filter-grid` 3 cols gap 8 of `.filter-tile` (radius 7, border line, padding 4, 10px label; swatch aspect 1.25 radius 4 = gradient `#244f4a→#efae77` with lilac blob and white "Aa", CSS-filtered per preset; active accent-bg + accent border): Original, Clear, Warm, Soft, Mono. h3 "Fine adjustments": Brightness 25–200, Contrast 25–200, Saturation 0–200 sliders with mono `%` output (10px sub, min-width 38). undo `.btn` "Reset color"; help.

### Other selection states
- **None**: title "Properties"; `.panel-empty` centered muted lh 1.7: cursor icon 27px, b 12px ink "Select something to shape it.", p "Choose a clip, annotation or track. Its controls will appear here."; section *Project*: "N clips · 00:33.5 / 1280 × 720 · 30 fps review output" + check `.btn` "Review readiness".
- **Track**: "Track properties"; glyph layers/music; subtitle "Upper tracks appear in front" / "Audio is mixed with other tracks"; *Track*: Name, Track volume %, preset-row up **Up** / down **Down**, help "Lock prevents edits, not playback. Solo affects sound only."; *Manage*: trash `.btn` "Remove track…".
- **Teaching cue**: "Teaching properties"; glyph = cue icon; "Attached to {clip}"; per kind: *Instruction* (Text textarea, Step number, Text size range, Readable text background), *Focus* (Magnification ×, Ease in/out), *Appearance* (Line weight / Background dim); *Color* `.color-row` 27px round swatches `#ffd279 #ffffff #ac93f1 #7bd4bc #f297a7` + custom color; *Timing within this clip* (Starts/Ends at (s)); *Position · % of this video* (Horizontal/Vertical, arrow tip X/Y, Width/Height).
- **Multi-clip**: "Selection properties"; glyph layers "N clips selected" / "Shift-click to change selection"; *Edit together* (list of up to 8 names with icons in a bg box; Copy / Duplicate / Group); *Shared fades* (None / 0.5s in & out / 1s in & out); *Shared color* filter grid; *Sound* Mute all / Unmute all.

---

## 6. Timeline

`.timeline{display:flex;flex-direction:column;background:var(--panel)}`.

### 6.1 Splitter (between workspace and timeline)
`editor.css: ".resize-bar{background:var(--panel);cursor:ns-resize;border-top:1px solid var(--line);border-bottom:1px solid var(--line)"`, 8px row; grip `":after{…top:2px;left:calc(50% - 21px);height:2px;width:42px;border-radius:2px;background:var(--muted);opacity:.5}"`. role=separator, focusable, ArrowUp/Down ±25px (`editor.js: "(ev.key==='ArrowUp'?25:-25),210,Math.min(540,innerHeight-260)"`), drag range 170–540 (≤ innerHeight−370).

### 6.2 Toolbar (44px)
`workspace-ui.css: ".timeline-toolbar{height:44px;gap:6px;padding:0 12px}"`, border-bottom. Left→right (`shell.html` + workspace-ui):
1. "Timeline" label 10px muted (`".timeline-toolbar>.name{font-size:10px;color:var(--muted);margin-right:5px}"`).
2. undo icon-btn, redo icon-btn (disabled when history empty → opacity .4).
3. divider.
4. **Split** — scissors + "Split" (11px, min-height 30; disabled without a selected clip).
5. trash icon-btn (Delete selection).
6. `<select>` **Delete: leave gap / Delete: close gap** (`"#deleteMode{font-size:10px;max-width:none;width:136px;min-height:30px}"`, bg panel).
7. bookmark icon button (Add chapter marker, M).
8. **Edit actions** — more icon + "Edit actions", `border-left:1px solid var(--line);border-radius:0 6px 6px 0;margin-left:3px`, color sub; opens the selection's context menu (Shift+F10).
9. divider.
10. **Snap** — magnet + "Snap"; active: `"#snapButton.active{color:var(--accent);background:var(--accent-bg)}"`, no border.
11. Zoom group `margin-left:auto`: zoomOut icon-btn, range 66px (`".timeline-zoom input{width:66px}"`, 1–32 step .25), zoomIn icon-btn, text button **Fit**.

### 6.3 Header row + ruler (32px, sticky top)
`".ruler-row{height:32px;position:sticky;top:0;z-index:9;background:var(--panel);display:flex;border-bottom:1px solid var(--line)}"`.
- Label cell (sticky left, width --label, border-right, padding 0 12, space-between): **plus "Add track"** button 10px (opens track menu: add video / add audio track) and tiny muted "Layers ↓".
- Ruler (role=slider, ew-resize, click/drag scrubs): ticks `".ruler-tick{position:absolute;top:3px;color:var(--muted);font:9px ui-monospace,monospace"` with a 7px tick line below (`":after{…height:7px;border-left:1px solid var(--line);margin-top:5px}"`). Label format `MM:SS` (`MM:SS.d` if step <1s). Step = first of `[.1,.2,.5,1,2,5,10,15,30,60,…]` where `step·pps ≥ 70` (`session-safety.js: "return steps.find(n => n * scale >= 70)"`); pps = `(scrollWidth − label − 26) / max(duration,15s) × zoom` → 2 s ticks at 1600w, 5 s at 960w.
- Chapter markers: gold "◆" 11px buttons, 24×24 hit area, `top:12px` (`".timeline-marker{min-width:24px;min-height:24px;top:12px;line-height:24px}"`), centered on their time, color gold.

### 6.4 Track rows
- Row `".track-row{display:flex;min-height:58px;position:relative;border-bottom:1px solid var(--line)}"`; actual height ≈68px because the header content is 8+24+1+26+8 (title min-height 24, controls 26 — session-safety.css). Lane `".track-lane{…min-height:57px;background-image:linear-gradient(to right,var(--line) 1px,transparent 1px);background-size:var(--grid-size,120px) 100%"` — vertical 1px grid lines at every ruler tick.
- **Track header** (sticky left, width --label 196, bg panel, border-right, padding 8 10, gap 8): 
  - Badge 28×28 radius 6 mono 10px: video `V3/V2/V1` (top→bottom, numbered descending) on video-bg/video; audio `A1…` on audio-bg/audio (`".track-badge{width:28px;height:28px;…border-radius:6px;color:var(--video);background:var(--video-bg);font:10px ui-monospace,monospace"`). Click → track properties.
  - Title 11px/550 ellipsis (button; click selects track).
  - Controls row gap 4, buttons 26×26 radius 4 10px (`".track-controls button{min-height:26px!important;min-width:26px!important}"`): video only **eye** (eyeOff when hidden); **M** (mute), **S** (solo), **lock/unlock** icon 12px. Active state = gold: `".track-controls button.active{background:var(--gold-bg);color:var(--gold);border-color:transparent}"` — visibility button is "active" when the track is HIDDEN. (Screen 02: V3 and V2 show M active because their sample video has no audio.)
  - Row states: `.locked` lane gets a diagonal hatch `repeating-linear-gradient(135deg,transparent 0 8px,#8888880b 8px 10px)`; `.disabled` (hidden video) clips at opacity .4; drop target lane bg accent-bg; empty lane text "Drop video here · or add from Media" 10px muted at left 16 top 18.
- **Teaching layers row** (top, above V tracks): `".track-row.fx{min-height:44px;background:color-mix(in srgb,var(--accent-bg) 14%,var(--panel))}"`, height `max(44, lanes×22+5)` (49 in sample); badge = text icon on accent-bg/accent-ink; title "Teaching layers", sub tiny muted "Attached to video". Cues pack into rows 22px apart: `".fx-clip{position:absolute;height:19px;top:3px;border:1px solid color-mix(in srgb,var(--accent) 40%,var(--line));border-radius:4px;background:var(--accent-bg);color:var(--accent-ink);font-size:9px;padding:2px 6px"` (top = lane·22+3); label = cue text or kind; zoom cues gold (`".fx-clip.focus-effect{background:var(--gold-bg);color:var(--gold);border-color:var(--gold)}"`, label "1.65× Focus"); 7px edge grips for trimming; selected outline 2px accent.
- **Captions row** (only when cues exist, inserted after the ruler): 35px, label gold captions icon "Captions  N", cues gold-bg / gold 50% border, 24px tall, 9px (`".caption-cue{…height:24px;…background:var(--gold-bg)"`).

### 6.5 Clips
- `".clip{position:absolute;top:5px;height:47px;border:1px solid color-mix(in srgb,var(--video) 48%,var(--video-bg));border-radius:5px;background:var(--video-bg);color:var(--video);…cursor:grab;min-width:5px;z-index:2}"` — note: top-anchored at 5px, not centered in the 68px row.
- Filmstrip: `".clip-film{position:absolute;inset:0;background-size:auto 47px;opacity:.32;filter:saturate(.7)}"` repeating the asset thumbnail horizontally.
- Name: bottom-left `".clip-name{position:absolute;left:9px;right:6px;bottom:5px;…font-size:10px;font-weight:550;color:var(--ink);text-shadow:0 1px var(--panel)"` with a 10px video/music glyph.
- Duration badge (video, width >140px): `".clip-duration{position:absolute;right:7px;top:4px;font-size:8px;padding:1px 4px;background:#16141a80;border-radius:3px;color:white}"` "9.5s".
- Badges (grouped link / speed "2×" / "STILL"): top-left `".clip-badges{…left:8px;top:3px;…background:#24202dc9;…font-size:8px"`.
- **Fade shape**: SVG over the clip, viewBox 0 0 100 47 non-scaling: gold triangles `fill="var(--gold)" opacity=".12"` from bottom-left corner up to the fade-in knee and down from the fade-out knee, plus 1.2px gold diagonal stroke (`editor.js: "<path d=\"M0 47L${fi} 2 M${fo} 2L100 47\" stroke=\"var(--gold)\""`).
- **Fade handles**: 13px gold circles with 2px panel border on the TOP edge at the knee positions (`".fade-handle{position:absolute;top:-4px;width:13px;height:13px;background:var(--gold);border:2px solid var(--panel);border-radius:50%;z-index:8;cursor:ew-resize;transform:translateX(-50%)"`); invisible unless clip selected/hovered.
- **Trim handles**: 9×33 at top 10 on each edge with a 2px ink bar inset 2px (`".trim-handle:after{…top:6px;bottom:6px;width:2px;…background:var(--ink)}"`), visible on selected/hover, cursor ew-resize.
- **Selected**: `".clip.selected{outline:2px solid var(--accent);outline-offset:0;z-index:4}"` (+ aria-pressed).
- **Audio clip**: `".audio .clip{background:var(--audio-bg);border-color:color-mix(in srgb,var(--audio) 46%,var(--audio-bg));color:var(--audio)}"`; name at TOP (`top:3px`) in audio color, no text-shadow; waveform SVG `".waveform{position:absolute;left:0;top:18px;width:100%;height:27px;opacity:.65}"` = vertical rounded 1.5px bars, ≤130 bars (~every 5px), centered on y=13 (`editor.js: function waveform`); no-peaks fallback text "waveform unavailable · audio still plays".
- **Crossfade chip**: gold-bg/gold border, 21px tall at top 30, "◇" (`.transition-chip`).
- **Gap hint**: dashed button in gaps wider than 70px (`".gap-action{position:absolute;top:16px;height:25px;…background:var(--panel);color:var(--muted);font-size:9px;border:1px dashed var(--line);border-radius:4px"`), gap icon + "Close gap", `opacity:.16` until the lane is hovered/focused (then 1; hover accent-ink/accent border/accent-bg).
- **Playhead**: `".playhead{position:absolute;top:26px;bottom:0;width:1px;background:var(--accent);pointer-events:none;z-index:7}"` with pentagon head 11×10 `clip-path:polygon(0 0,100% 0,100% 60%,50% 100%,0 60%)` at top −4 (sits in the ruler's lower band). Auto-scrolls while playing.
- **Snap guide**: dashed gold 1px vertical line from the ruler down while dragging.
- Locked-track hatch and selection outline must remain distinguishable in forced colors.

### 6.6 Row heights summary (1600×1000, screen 02)
toolbar 44 → ruler 32 → teaching layers 49(+1) → V3 68 → V2 68 → V1 68 → A1 … (scrolls). Clip body 47; cue 19; caption row 35.

---

## 7. Bottom bars

- **Timeline footer (27px)** — inside the timeline section: `".timeline-footer{height:27px;min-height:27px}"`, border-top, padding 0 13, 10px muted, space-between. Left: link icon + `#editHint` "Callouts follow their clip." — a live hint line that changes during drags ("Moving 3 clips together · tracks stay fixed · Esc cancels"). (Key hints `S split · Space play · ? help` are hidden.) Right: button sliders icon + "Audio mixer" + pill with the audio-track count "2" → opens the mixer.
- **Status bar (25px)** — `".statusbar{display:flex;align-items:center;justify-content:space-between;font-size:9px;color:var(--muted);padding:0 13px;border-top:1px solid var(--line);background:var(--bg)"`, icons 12px. Left: shield + "Local only. No media is uploaded." Center: `#storageInfo` (clickable → session dialog) "Recovery unavailable · save a project file before closing" / "Recovery cached in this browser · save a project file for a durable copy" — **BROWSER-ONLY** copy (native: journal/recovery state). Right: "Browser reference · downloads stay local" — **BROWSER-ONLY**.

---

## 8. Context menu (screen 03)

Generic panel used for right-click and all command menus (`direct-edit.js: function openMenuPanel`).
- Panel: `".context-panel{position:fixed;width:282px;max-width:calc(100vw - 16px);max-height:calc(100vh - 20px);overflow:auto;…background:var(--panel);border:1px solid color-mix(in srgb,var(--line) 65%,var(--muted));box-shadow:0 16px 55px #0006,0 2px 6px #0003;border-radius:10px;padding:6px"`; submenu level 1 width 245. Positioned at pointer (or anchor bottom+4/+6) and clamped 8px inside the viewport.
- **Heading** (sticky): `".context-heading{padding:8px 10px 11px;…gap:4px;…border-bottom:1px solid var(--line);margin-bottom:4px}"`, b 11px/600 = target name ("Presenter · demo", "3 selected clips", track/asset name, "Timeline gap", "Editor actions"), small mono 9px muted subtitle "Direct editing · originals unchanged".
- **Item**: `role=menuitem|menuitemcheckbox`, `"display:flex;width:100%;align-items:center;justify-content:flex-start;gap:9px;min-height:31px;height:31px;font-size:11px;line-height:1.4;padding:6px 9px;border-radius:5px"`; glyph slot 17px wide, icon 14px in sub color (checked items show a check instead of their icon); label flex:1 ellipsis; right side = shortcut `kbd` (9px mono muted, no border) or a chevronRight for submenus. Hover/focus: `background:var(--accent-bg);color:var(--accent-ink)` (screen 03: first item highlighted). Disabled: opacity .5, tooltip = reason, hint line shows the reason. Danger: `.danger-action` text + glyph `var(--danger)`.
- **Separator**: `".context-separator{height:1px;background:var(--line);margin:5px 8px}"`.
- **Hint footer** (aria-live): `".context-hint{font-size:9px;color:var(--muted);padding:9px 9px 4px;…min-height:31px;border-top:1px solid var(--line);margin-top:5px}"` default "↑ ↓ navigate · Enter choose · Esc close"; on focus shows the reason / "→ Open options" / "Removes from this edit. You can undo." / "Enter to apply · Esc to dismiss".
- Submenus open on 180 ms hover or Enter/→, placed at item right+4 (or left−252 if it would overflow).

**Clip menu (single clip)** — `direct-edit.js: return[I('Go to this clip'` + `workspace-ui.js` "Clear selection":
1. locate — Go to this clip
2. zoomIn — Fit this clip
— separator —
3. copy — Copy clip — Ctrl+C
4. scissors — Cut clip — Ctrl+X
5. copy — Duplicate — Ctrl+D
6. edit — Rename… — F2
— separator —
7. scissors — Split at 00:17.4 — S (disabled outside the clip / with crossfade)
8. scissors — Trim to pointer ▸ {Trim start to here, Trim end to here}
9. speed — Speed ▸ {0.25×, 0.5×, 1× · normal ✓, 1.5×, 2×, 4×, —, Speed & timing options…}
10. (video) rotate — Transform ▸ {Fit entire source ✓, Fill frame · crop, Rotate 90° clockwise, Flip horizontally, Flip vertically, Center in canvas, Full frame, Reset crop & orientation}
11. (video) palette — Color treatment ▸ {Original ✓, Clear, Warm, Soft, Mono, Adjust color…}
12. (video) shield — Cover private information
13. fade — Fades ▸ {Remove edge fades, 0.25s · both edges, 0.5s · both edges, 1s · both edges, Fades & transitions…}
14. volume — Audio ▸ {Mute clip / Unmute clip, (video) Detach audio · keep aligned, Audio properties…}
15. palette — Copy / paste look ▸ {Copy layout, color & fades, Paste look & fades}
16. captions — Add caption here…
— separator —
17. cursor — Clear selection — V (inserted before the first destructive item)
18. trash — **Delete · leave gap** — Delete (danger)
19. gap — **Delete · ripple this track** (danger)

**Multi-clip**: Copy selection Ctrl+C, Cut selection Ctrl+X, Duplicate selection Ctrl+D, —, group Group selection Ctrl+G, unlock Ungroup Ctrl+Shift+G, locate Fit selection, fade Fades ▸ (Remove edge fades / 0.25s · 0.5s · 1s · fade both edges), palette Color treatment ▸, muted Mute selection, volume Unmute selection, paste Paste look & fades, —, Clear selection V, trash Delete selection · leave gaps (danger, Delete).

**Track header**: group Select clips on this track; edit Rename track…; —; (video) eye Visible video ☑; muted Mute audio ☐; volume Solo audio ☐; lock Lock track ☐; —; up Move track up; down Move track down; gap Close gaps on this track; Clear selection V; trash Remove track… (danger).

**Empty lane / gap / timeline** (heading "Timeline gap"/"Editor actions"): cursor Move playhead to 00:12.0; paste Paste clips here Ctrl+V; —; gap Close this gap · this track; gap Close all gaps · this track; text Add title here; layers Insert intro · shift all tracks; video Add video track; music Add audio track; zoomOut Fit timeline F; magnet Snapping N ☑.

**Teaching cue**: edit Edit annotation; copy Duplicate annotation; link Select attached clip; Clear selection V; trash Delete annotation (danger).

**Media asset (library row)**: plus Add at playhead; layers Add on a new track; edit Rename library label…; (missing) link Reconnect original…; trash Remove from library (danger, disabled while used).

---

## 9. Dialogs and popovers

Shared dialog chrome (`editor.css: ".dialog{padding:0;border:1px solid var(--line);border-radius:13px;background:var(--panel);color:var(--ink);box-shadow:var(--shadow);width:min(560px,95vw);max-height:90dvh"`): backdrop `"#080711aa"` + `backdrop-filter:blur(4px)`; header `"padding:18px 20px 13px;…border-bottom:1px solid var(--line)"` with h2 16px/600 (lh 1.4) + optional `p.small.muted` subtitle (11px) and a 32px x icon-btn on the right; body `"padding:20px;display:flex;flex-direction:column;gap:16px"` scrolls; footer `"padding:13px 20px;display:flex;justify-content:flex-end;gap:8px;border-top:1px solid var(--line)"`, sticky with a fade shadow (`".dialog-footer{position:sticky;bottom:0;…box-shadow:0 -7px 12px color-mix(in srgb,var(--panel) 85%,transparent)}"`). Header and footer never scroll (`session-safety.css: ".dialog[open]{display:flex;flex-direction:column;overflow:hidden;max-height:92dvh}"`). Inline boxes: `.callout` (accent-bg, 22% accent border, radius 7, padding 13, 11px sub, b accent-ink), `.callout.warning`, `.field-help.warning` (gold).

### 9.1 Welcome invitation (screen 01) — nonmodal, never steals focus
`onboarding.css: ".guide-invite{position:fixed;z-index:65;right:24px;top:76px;width:350px;…padding:25px 24px 18px;background:var(--panel);border:1px solid var(--guide-edge);border-radius:14px;box-shadow:var(--guide-shadow)"`. Close x top-right (9/8). Overline "NEW HERE? START HERE." (`".guide-overline{…font-size:10px;font-weight:700;letter-spacing:1.7px;color:var(--accent)"`). Title row gap 14, margin 17 0 10: icon tile 46×50 radius 13 accent-bg with compass 27px; h2 22px/650 lh 1.18 "A little guidance.<br>A clearer first edit." Body 13px sub lh 1.65 "Get to know the editor, one useful step at a time. You can pause and pick up later." Actions gap 18: primary "Show me around ›" (min-height 39), link "Not now" 12px accent-ink. Footer small 10px muted with border-top: "Always available from **Help** · No editing required".

### 9.2 Guided coach (screen 10)
- Layer: `#guideLayer{position:fixed;inset:0;z-index:80;pointer-events:none}`. **Shade**: 4 rectangles in `--guide-dim` around the target (5px gap), only when "Dim" preference is on and focus is not elsewhere. **Ring**: `"#guideRing{position:fixed;…border:2px solid var(--guide-edge);border-radius:8px;box-shadow:0 0 0 4px color-mix(in srgb,var(--guide-edge) 15%,transparent)}"` placed at target rect −4px/+8px. **Target label chip**: `"#guideTargetLabel{…max-width:250px;padding:4px 9px;font-size:10px;font-weight:550;color:var(--accent-ink);background:var(--accent-bg);border:1px solid var(--guide-edge);border-radius:5px"`, at target left, 30px above its top (below if near top); hidden <700px wide or when a menu is open. Screen 10 label: "Fade controls" above the inspector.
- **Placement** (`onboarding.js: function layout`): card width `min(362, innerWidth−24)`, max-height `innerHeight−24`; candidates in order **right** of target (gap 18, vertically centered, clamped to 12px pad), **left**, **below** (horizontally centered), **above**; first candidate fully inside the viewport (12px pad) wins, preferring one that doesn't overlap an open menu; if none fits it shrinks to the larger free band above/below (≥240px) and scrolls only its copy ("compact-scroll"), else pins top/bottom ("compact"). No target → top-right `x=innerWidth−w−12, y=min(95,…)`. Suspends (hides) while any modal dialog, context menu or export is open, and prepends a `.guide-dialog-note` ("Walkthrough is waiting. Close this window to continue." + "Pause guide") into that dialog.
- **Card**: `".guide-coach{position:fixed;…border:1px solid var(--guide-edge);border-radius:13px;background:var(--guide-paper);box-shadow:var(--guide-shadow)"`, flex column, copy area scrolls.
  - Top bar `padding:8px 9px 8px 14px`, border-bottom: book icon (accent 15px) + chapter title button 11px muted ("Smooth the edges", opens learning center); right: minus (minimize) and x (pause/dismiss) 27×28.
  - Copy `padding:17px 21px 7px`: meta row "GUIDED WALKTHROUGH" 9px/600 letter-spacing 1.15 muted + step counter mono 11px accent "13 / 22"; h2 23px/650 lh 1.18 letter-spacing −.55 (focused on show; focus ring offset 5); body 13px sub lh 1.7; optional diagram (`.guide-flow` 3 boxes with arrows, current box accent-bg); tip row (border-top, info icon muted, 11px muted); task box `.guide-task` (accent-bg/accent-ink, radius 8, padding 11 12, 11px; `.tried` teal audio; `.is-edit` gold: "Optional edit: try a 0.5-second fade. Undo remains available.") with cursor/edit/check icon; status line gold-bg (`.guide-step-status`) when the control is unavailable/out of view; row with link "Focus control" / "Show control" + kbd hints.
  - Nav `padding:10px 16px 13px`, border-top: link "Pause guide" (muted 11px) left; right Back (`.btn`, disabled on step 1) + Next (`.primary`, "Next ›" / "Finish guide ✓"), both min-height 35, 12px, padding 7 13.
  - Foot 9px muted: storage text ("Session only · back up progress in Help" — BROWSER-ONLY wording; "Progress remembered on this device") and "F6 to focus control".
  - Progress line 3px: line bg with accent fill = (step/22).
- **Mini (minimized)**: `".guide-mini{position:fixed;right:20px;bottom:26px;…border:1px solid var(--guide-edge);border-radius:10px;background:var(--panel);…padding:3px"`: button book icon + "13/22 · {title}" + up chevron, divider, x.
- Completion dialog (`.guide-completion`, 500px, padding 36, centered: 59px teal check tile, overline "LEARNING AT YOUR PACE", h2 27px, project diagram, Browse lessons / Back to my edit ›) and restart confirm (`.guide-reset`: "Start the walkthrough again?" + Keep my progress / Start over).

### 9.3 Help & learning center (screen 11)
`".guide-hub{…border:1px solid var(--line);padding:0;border-radius:15px;width:870px;max-width:calc(100vw - 40px);max-height:calc(100dvh - 40px);box-shadow:var(--guide-shadow)"`, backdrop `rgba(10,10,19,.66)`, scrolls as a whole.
- Header `padding:13px 21px` border-bottom, 12px sub: book icon (accent) "Help & learning center", x close.
- Hero `padding:31px 34px 25px`, `background:linear-gradient(108deg,color-mix(in srgb,var(--accent-bg) 53%,var(--panel)),var(--panel))`, border-bottom: overline "YOUR EDITOR, EXPLAINED" (completed: "WALKTHROUGH REVIEWED"); h2 29px/650 lh 1.13 max-width 425 "From recording to a clear tutorial."; p 13px sub "One explained control at a time. Skip anything. Come back anytime."; primary min-height 40 12px: play "Start walkthrough" / bookmark "Resume walkthrough" / "Revisit walkthrough"; small 11px muted "7 chapters · 22 short steps · no setup needed" / "Continue at step N · {title}". Right: **progress ring** 128px `conic-gradient(var(--accent) p, var(--line) p)`, 4px ring, inner panel disc with "0%" 31px/600 and "0 / 22 reviewed" 10px muted.
- Tabs row padding 0 34, gap 23, border-bottom: **Walkthrough / Quick answers / Shortcuts** 12px/550 muted, selected accent-ink with 2px accent underline at bottom −1.
- Walkthrough body (padding 24 34): heading "Pick up a skill." 15px + p 12px muted "Start at the beginning or jump straight to what you need." + link "Start over" (muted, right). Chapters grid 2 cols gap 11, last card spans both (`".guide-chapter:last-child{grid-column:1/-1}"`); card radius 10 bg var(--bg) border line; main button (padding 13 12 9, min-height 64, hover var(--hover)): icon tile 34×37 radius 9 accent-bg accent icon 19px (reviewed → teal check on audio-bg), title 12px/600, subtitle 10px muted, count "0/4" 10px muted, chevronRight 13px muted; `<details>` summary "See 4 steps +" 10px muted indented 58px → step list (border-top, rows 35px 11px, mono checkmark "01"/"✓" accent, current = accent-bg, "Resume here" 9px accent). Chapters: compass Find your way (Media, preview & timeline, 4), scissors Make the first edit (Select, split, undo & arrange, 5), layers Build your scene (Tracks, webcam & layout, 3), fade Smooth the edges (Fades & audio levels, 2), text Make it easy to follow (Callouts, captions & chapters, 3), save Save your work. Share a video. (Checks, editable projects & outputs, 3), bookmark Come back anytime (Your projects & this guide, 2). Safety box (border, radius 9, padding 15, lock icon accent): "**Your edit stays yours.** The guide does not automatically cut, record or render. …"
- Quick answers: search field (search icon, input 13px min-height 39, placeholder "Search audio, saving, camera…") + `<details>` answers (13px summary, 12px sub body, "Show me in the editor ›" link). Shortcuts: rows (padding 11 0, border-bottom, 12px sub text left, kbd 11px right) for F1 or ?, Esc, F6, Tab/Shift Tab, Enter/Space, Space, S, Ctrl/⌘ Z, Ctrl/⌘ S, Shift F10, Shift + click; `.btn` "All editor shortcuts".
- Footer (bg var(--bg), padding 17 34 20, border-top): `<details>` "Progress & preferences" with right muted "Local to this device" (BROWSER-ONLY wording); checkbox "Dim the surrounding editor during a step"; save icon "Save progress file", folder icon "Load progress file"; small note.

### 9.4 Before you share / Checks (screen 07)
`.checks-dialog{width:min(680px,95vw)}`. Header "Before you share" / "Actionable checks, not a quality score." Body: summary box (`".checks-summary{…border:1px solid var(--line);background:var(--bg);padding:14px;border-radius:9px}"`, b 13px "0 blockers · 0 review warnings", 11px muted "These checks inspect the edit, not the meaning of your tutorial."); issue rows (`".issue-row{display:flex;gap:14px;padding:14px 0;border-bottom:1px solid var(--line)}"`): kind chip mono 9px letter-spacing .5 min-width 48 padding 4 6 radius 4 — **NOTE** accent-bg/accent-ink, **REVIEW** gold-bg/gold, **FIX** danger 14% / danger; h3 12px/600; p 11px muted lh 1.7; action `.btn` 10px min-height 30 "Save project ›" / "Open Captions ›" (reveals the object). Empty: callout "No structural issues found…". Closing help 10px: "No automatic speech transcription, content review or privacy detection is performed. This is not an accessibility certification." Footer: Export diagnostics · Back to edit · **Continue to render** (primary). (First NOTE in screen 07 is BROWSER-ONLY: "Editable project has not been downloaded with these changes".)

### 9.5 Save project (screen 08)
Default 560 width. Header "Save project" / "Keep the workspace. Continue whenever you are ready." Body:
- Intro row gap 16 mb 22: icon tile (`".save-project-icon{padding:12px;background:rgba(151,114,213,.16);border:1px solid var(--line);border-radius:12px;color:var(--accent)}"`, folder 28px) + h3 15px "Your workspace. Ready to continue." + p small sub "Save the editable project without rendering. Nothing is flattened, and your original media is never changed."
- Field "Project name".
- Format options gap 9: `.save-option` (border line radius 9 padding 14 gap 12, radio 18px) — selected `":has(input:checked){background:rgba(151,114,213,.10);border-color:var(--accent)}"`: **Portable project** + success pill "Recommended" / small 11px muted "Workspace + available originals, including retained render revisions, in one ZIP. Reopen directly in this editor."; **Lightweight project file** / "Edits and workspace as JSON. Keep originals; relink them when needed."
- Checkbox "Include available rendered videos in the portable project"; help "Render history and each render's editable snapshot are always preserved. 0 products · 0 originals (0 bytes)."; missing-originals callout.warning when applicable.
- Checklist 2×2 gap 12 margin 22 0, 11px, teal check 15px: All tracks & clips · Fades & teaching layers · Mixer & chapter markers · Playhead & workspace layout.
- BROWSER-ONLY help "This browser editor downloads a project file; it does not overwrite…".
- Footer: Keep editing · primary download icon "Download project" (BROWSER-ONLY verb → native "Save project").

### 9.6 Render a video (screen 09)
Header "Render a video" / "A finished output from your editable workspace." Body top→bottom:
- `.range-export` box (radius 9, bg, padding 14): checkbox 12px "Render a short review range"; help "Test a section before rendering everything…"; fieldpair From (seconds) / To (seconds).
- `.render-parent` (border radius 9 padding 17 gap 10): accent pill "PROJECT · r1", b 14px project title, small muted "This render becomes a product. The project stays editable."
- Field "Rendered video name" ("Create your first project · Review 1"); fieldpair Destination vault / Folder inside vault.
- `.render-profile` (border radius 9 padding 16 gap 8): video icon 16px + "Browser review" 12px (BROWSER-ONLY label), b 13px "1280 × 720 · 30 fps · WebM", small muted "00:33.5 · real-time rendering · maximum 3 minutes" (BROWSER-ONLY limits).
- Callout accent: b "A new output, never an overwrite." + "The ZIP includes a video, companion note, poster and the exact edit snapshot used." (warning variant when blocked, with the reason).
- Gap/solo warnings (gold), BROWSER-ONLY help lines.
- Footer: Save project instead · primary video icon "Render video" (disabled when blocked).

### 9.7 Webcam (screen 05)
`".webcam-dialog{width:min(960px,calc(100vw - 30px));max-width:960px}"`. Header "Bring yourself into the tutorial" / "Your camera. A separate, editable layer." Body grid `minmax(0,1fr) 230px` gap 24:
- Left: view 16:9 `background:#18161d;border:1px solid var(--line);border-radius:10px`; empty state radial `#292137→#18161d`, webcam icon 38px `#b7a0df`, b 15px `#eee7f7` "Camera is off", p 11px `#a89cb7` "Enable your camera when you are ready.<br>No permission is requested until you do."; countdown 80px/700 white on `#0005`; timer chip top-right `#17121fce` 12px. Status row: 6px dot (muted / teal live / `#f48796` recording) + "Camera off" 11px muted; right small muted "720p target · local recording". Message help "Your camera and microphone are off…".
- Right `.webcam-settings`: h3 12px "Set up your take"; Camera select "System default"; checkbox Include microphone; Microphone select (disabled until checked); 5px mic meter (teal); help "Mic level preview. Live audio is never played through your speakers."; "Insert at timeline time (seconds)" number; checkbox "Mirror preview & overlay"; help paragraph.
- Privacy strip full width (`".webcam-privacy{…padding:13px 15px;margin-top:20px;background:var(--bg);border:1px solid var(--line);border-radius:7px;font-size:11px"`, teal shield): "Permission is explicit. Camera and microphone stop after recording or closing. Save the project to keep the original take; rendering is optional."
- Footer by phase: idle — Try demo overlay (BROWSER-ONLY demo) · primary webcam "Enable camera"; requesting — note + Cancel request; preview — Cancel · primary record-dot "Start recording"; countdown — Cancel countdown; recording — "Local recording · maximum 3 minutes" · primary stop "Stop & review"; review — Discard & retake · download "Save raw take" · primary layers "Add as overlay"; close-guard during review — "This take has not been added to your project." · Keep reviewing · danger "Discard & close".

### 9.8 Audio mixer (no screenshot; `shell.html: id="mixerDialog"`)
Header "Audio mixer" / "Track levels affect both preview and export." Rows `".mixer-row{display:grid;grid-template-columns:125px 1fr 52px;gap:10px;align-items:center;padding:12px 0;border-bottom:1px solid var(--line)}"`: name b 11px + M / S 27×26 toggles (gold active) + tiny kind; range 0–2 per track; value mono "−2.5 dB" (−∞ at 0). Then "Master output" range + mono "80%"; help "M = mute. S = solo. The output includes a safety compressor, not a loudness-normalization pass…"; `.btn` play "Play / pause preview". Opened from the timeline footer, the Audio tab, View menu.

### 9.9 Project files & session (recovery/status; `session-safety.js: id="sessionDialog"`)
`.session-dialog{width:min(660px,95vw)}`. Header "Project files & session" / "Your editable source. Its rendered products." Lead row: `.session-icon` (padding 13, accent-bg, radius 10, shield 23px) + h3 15px "Keep an editable copy before closing" / "This edit matches a project file" + p 12px sub. Facts grid 2×2 gap 10: article (border radius 9 padding 15 bg): 11px muted label, 18px/600 value, 11px sub note — Working project "Revision N"; Original media "x / y loaded"; Rendered products "N"; Browser recovery "Session only"/"Best-effort cache" (BROWSER-ONLY). Sections (border-top, h3 12px, p 12px sub lh 1.75): "What each file preserves", "Before sharing". Footer: Reconnect originals · Back to edit · primary save "Save project".

### 9.10 Small dialogs
Rename (`editDialog`: "Rename tutorial", Title field, Cancel / Apply), Confirm (`confirmDialog`), Edit caption (Caption text textarea + counter "N/500 characters · x.x characters/second", Starts/Ends at (seconds), Save & split cue · Cancel · Save caption), Companion note (mono `pre` preview + download), Reconnect original media (relink rows `.source-row`).

### 9.11 Frame your tutorial (ratio)
Grid 4 cols gap 10 of `.canvas-choice` (min-height 175, border radius 9, bg; active accent border + accent-bg): ratio diagram (2px accent border, raised fill, max 65×68), b "Landscape · 16:9", small "1280 × 720". Formats: Landscape 16:9, Portrait 9:16, Square 1:1, Classic 4:3. Help + footer "Keep current format".

### 9.12 Toast
`".toast{position:fixed;left:50%;bottom:43px;…border:1px solid var(--line);border-radius:8px;padding:11px 17px;max-width:min(650px,90vw);background:var(--raised);…font-size:12px;transition:.16s}"`, slides up 15px + fades in. Screen 12 shows "Guide paused. Help → Resume walkthrough brings you back here."

### 9.13 Startup failure (screen 15)
`handover-polish.css: ".startup-failure{position:fixed;inset:0;z-index:20000;…justify-content:center;align-items:flex-start;gap:20px;padding:max(30px,10vw);background:var(--bg)"`: h1 (focused) "The editor could not start", p 15px lh 1.7 max 60ch, primary "Reload editor" (BROWSER-ONLY wording).

---

## 10. Icons

All icons are inline SVG, 24×24 viewBox, stroke-only: `drawing-primitives.js: "const svg=n=>`<svg class=\"icon\" viewBox=\"0 0 24 24\" aria-hidden=\"true\">${ICONS[n]||ICONS.box}</svg>`"` styled by `editor.css: ".icon{height:17px;width:17px;fill:none;stroke:currentColor;stroke-width:1.7;stroke-linecap:round;stroke-linejoin:round;flex-shrink:0}"`. Color always `currentColor` (inherits the button's text color; a few are tinted: Help book = accent, chapter tiles = accent, webcam privacy shield = audio teal, checklist checks = success). Unknown names fall back to `box`. Sizes by context: 17 default, 15 toolstrip/guide, 14 context menu/search, 13 transform/selection, 12 track controls/status bar/Project chevron, 10 clip-name glyph and More-tools chevron, 19 learning chapter, 27 invite, 38 webcam empty.

74 icons are defined (base set in drawing-primitives.js, extended by `Object.assign(ICONS,…)` in editor.js — which overrides `video` and `sun` —, project-lifecycle.js, webcam.js, editing-features.js, onboarding.js). Full markup + usage: **`docs/superpowers/specs/2026-09-26-tutorial-editor-visual-parity-icons.json`** (`{name:{viewBox,svgInnerMarkup,strokeWidth,fill,stroke,strokeLinecap,strokeLinejoin,definedIn,usedFor}}`).

Name → main use: folder (Project trigger), chevronDown (Project/ratio/More tools), edit (title pencil, Rename), book (Help, coach, learning center), check (Checks, checked items), save (Save project), video (Render video, video clips) — `<rect x="3" y="5" width="13" height="14" rx="2"/><path d="m16 9 5-3v12l-5-3"/>`, upload (Import media), webcam (Webcam), search (media search), plus (media add, Add track, More tools, New project), music (audio thumb/clip), panelLeft (library toggle), text/arrowUpRight/box/zoomIn (tools), spotlight/step/shield/layers (More tools), play/pause (Review, transport), more (⋯ View, Edit actions), sliders (properties toggle, Audio mixer), volume/muted (monitor), skipBack/skipForward (transport), undo/redo, scissors (Split), trash (delete), bookmark (chapter marker), magnet (Snap), zoomOut/zoomIn (timeline zoom), eye/eyeOff/lock/unlock (track controls), link (footer hint), shield (status bar), x (close), locate/copy/paste/speed/rotate/palette/fade/captions/gap/group/cursor/up/down/stop/sun/camera/info/minus/compass (menus, guide, webcam).

Non-icon glyphs drawn with text or CSS: brand mark (`··` + CSS ears), chapter markers `◆`, crossfade chip `◇`, "Layers ↓", M / S track buttons (text), corner-placement glyphs (CSS `.corner-icon`), playhead head (CSS clip-path), fade handles (CSS circles), fade graph (inline SVG built per clip), waveform (inline SVG path).

---

## 11. Interaction details that affect layout

- **Panel toggles** change grid columns only (never the canvas ratio): library-hidden / properties-hidden / focus-view; ≤1080 inspector becomes an overlay drawer (`.inspector-open`, 276px, shadow), ≤860 library becomes a drawer (`.library-drawer`, 250px). Toggle buttons expose aria-pressed and titles "Hide/Show media library|properties". Selecting an inspector category on narrow screens opens the drawer.
- **Review mode** (`.app.review-mode`): sidebar/inspector hidden, preview header keeps only "Preview" + actions (toolstrip, ratio, panel toggles hidden), Review button → "Back to edit" active, stage padding `0 8vw`, preview badge hidden.
- **Preview header density** via ResizeObserver: wide ≥800, medium 520–799 (hide "Preview", "More tools"→"More", tool padding 5 6, 10px), compact <520 (hide Highlight/Zoom into More, Review icon-only, padding 0 6 gap 3). Toolbar is one tab stop with arrow/Home/End roving focus.
- **Splitter**: pointer drag ns-resize; keyboard ±25px; clamps 170/210–540 and ≤ innerHeight−370; persisted in workspace (`timeline_height`).
- **Clip hover/selected**: fade + trim handles appear (opacity 0→1); selected = 2px accent outline; cursor grab on body, ew-resize on handles; drag shows dashed gold snap guide; Alt bypasses snap; Escape cancels drag; Shift/Ctrl-click multi-selects; locked lanes hatched and uneditable.
- **Gap hints**: dashed "Close gap" at 16% opacity, full on lane hover/focus-within (`transition:opacity .12s`); coarse pointer 70%.
- **Menus**: open below anchor (+6) or at pointer, clamp to viewport, submenu after 180 ms hover, focus first item, Escape closes and restores focus to the opener. Clicking a menu trigger again closes it.
- **Buttons**: hover bg `--hover`; primary hover `#9475d8`; disabled opacity .4 with `title` reason; pressed toggles use `.active` (accent-bg); track toggles use gold active.
- **Document title**: pencil appears on hover/focus only.
- **Media rows**: draggable onto lanes (lane shows accent-bg drop highlight); keyboard Enter/Space adds at playhead.
- **Precision disclosures** (Frame & crop, Transform source) remember open state per selected object.
- **Invite** is nonmodal (no focus theft); coach never auto-advances; clicking the highlighted control marks "explored" (task box turns teal).
- **Toast** slides in (.16s). All motion disabled under reduced-motion.
