# RPG Buddy Characters & Settings View — Design

- **Date:** 2026-07-04
- **Status:** Approved for implementation
- **Branch:** `claude/vault-buddy-rpg-characters-lemi01`

## Goal

Make the buddy more charming: let the user pick an animated pixel-art RPG
character as their companion, chosen from a settings view inside the panel.
The original SVG blob stays available as the "Classic" nostalgia option.

## Asset research

Requirements: free to use commercially, redistributable (the assets live in
this public MIT repo), pixel-art style, multiple characters, and **animated**
(real sprite frames, not just CSS motion).

| Pack | License | Verdict |
| --- | --- | --- |
| Tiny Swords (Pixel Frog) | Free, but "may not redistribute … even if modified" | Rejected — bundling PNGs in a public repo is redistribution |
| Medieval Fantasy Character Pack (OcO) | Free commercial, informal terms, attribution requested | Rejected — no clear redistribution grant |
| PIPOYA Free RPG Characters | Free commercial, redistribution prohibited | Rejected |
| **Dungeon Tileset II (0x72), v1.7** | **CC0** ("You can use this tileset for whatever you like") | **Chosen** |

Source: <https://0x72.itch.io/dungeontileset-ii> — CC0 means we can crop,
recolor, and commit the sprites with no legal risk to the MIT license.
Attribution is optional but we credit 0x72 anyway in an asset LICENSE file.

The pack ships per-frame PNGs for hero characters at 16×28 px, 4-frame idle
and 4-frame run cycles each.

## Character roster

| id | Name | Source frames |
| --- | --- | --- |
| `classic` | Classic | existing inline SVG (unchanged) |
| `knight` | Knight | `knight_m_{idle,run}_anim_f0..3` |
| `wizard` | Wizard | `wizzard_f_{idle,run}_anim_f0..3` |
| `elf` | Elf | `elf_f_{idle,run}_anim_f0..3` |
| `dwarf` | Dwarf | `dwarf_m_{idle,run}_anim_f0..3` |
| `lizard` | Lizard | `lizard_m_{idle,run}_anim_f0..3` |

Frames are composed into one horizontal 4-frame strip per animation
(`<id>-idle.png`, `<id>-run.png`, each 64×28) under `src/assets/buddies/`,
next to `LICENSE.md` with the attribution.

## Architecture

- **`src/characters.ts`** — character registry. `BuddyCharacter` describes
  either the classic SVG (`kind: "classic"`) or a sprite character
  (`kind: "sprite"` with idle/run strip URLs and frame geometry).
  `getCharacter(id)` falls back to Classic for unknown ids, so a stale
  localStorage value can never break rendering.
- **`src/components/BuddyAvatar.vue`** — presentational renderer used by both
  the companion and the settings previews. Renders the classic SVG or a
  sprite `<div>` animated via CSS `steps(4)` over the strip
  (`image-rendering: pixelated`, 2× scale → 32×56). Idle is not a constant
  loop: the sprite stands still and, at random moments (3 s minimum delay +
  up to 4 s jitter), either plays one quick cycle (re-armed via
  `animationend`) or glances the other way via a `scaleX(-1)` mirror for
  0.7–1.5 s before snapping back — so it reads as a creature shifting its
  weight and looking around rather than fidgeting. The home view direction
  is a setting (`vault-buddy.facing`, right by default) with a Left/Right
  control in the settings view; anything that interrupts idling (work,
  animations off) also ends a glance. `working`
  swaps to a continuous run loop; `animated: false` freezes everything
  (existing "still" behavior, sprites frozen on frame 0).
- **`src/components/CompanionCharacter.vue`** — keeps all gesture logic
  (click/drag/context menu) and delegates drawing to `BuddyAvatar`. Gains a
  `character` prop.
- **`src/stores/settings.ts`** — new persisted `character` state
  (`vault-buddy.character` in localStorage, default `classic`) and
  `setCharacter(id)` action that validates against the registry.
- **`src/components/BuddySettings.vue`** — the settings view: a character
  grid (live animated previews, selected one highlighted) plus the existing
  animations toggle as a visible control, and a dragging toggle
  (`vault-buddy.dragging`, default on) that pins the buddy in place —
  `CompanionCharacter` skips the drag gesture entirely so a press stays a
  plain click, and the cursor/tooltip drop the drag affordance.
- **`src/components/ActionPanel.vue`** — header gains a gear button that
  switches the panel body between the vault list and the settings view.

## Data flow

`BuddySettings` writes to the settings store → `App.vue` passes
`settings.character` into `CompanionCharacter` → `BuddyAvatar` renders it.
The native right-click "animated" toggle keeps working since it flows through
the same store.

## Error handling

- Unknown/legacy `character` value in localStorage → `getCharacter` falls
  back to Classic.
- Sprite image failing to load only blanks the buddy visually; gestures are
  on the button, not the image, so the app remains operable.

## Testing

- settings store: character defaults to classic, persists, invalid ids fall
  back.
- characters registry: classic first, sprite entries carry both strips.
- BuddyAvatar: classic renders SVG; sprite renders strip; working uses run
  strip; `animated: false` adds `still`.
- CompanionCharacter: existing gesture tests unchanged (classic default);
  sprite character renders through the same button.
- ActionPanel/BuddySettings: gear toggles the view; picking a character
  updates the store; toggle mirrors `animationsEnabled`.
