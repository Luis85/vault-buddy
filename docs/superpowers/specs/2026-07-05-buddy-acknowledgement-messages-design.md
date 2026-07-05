# Buddy acknowledgement messages — design

The buddy communicates with the user through its speech bubble: short, playful
acknowledgements of actions and background progress. Extends the existing
launch-greeting bubble into a general, event-driven message channel. The app
never gains a second bubble window — this reuses the whole existing stack
(`SpeechBubble.vue`, `BubbleRoot.vue`, Rust `show_bubble`/`place_beside_buddy`).

## Goals / non-goals

- **Goal:** acknowledge exactly seven moments (below) with a brief bubble.
- **Goal:** a dedicated, user-facing on/off setting (chatter can be silenced).
- **Non-goal:** narrating every micro-action. Pause/resume are *not* announced
  (not a start/end; keeps the buddy from getting chatty). Vault-open *failures*
  are not announced — the panel already shows an inline error banner there; a
  bubble would duplicate it.

## Architecture — Rust owns the window, the frontend owns the words

The bubble is its own webview, so a message must round-trip through Rust (only
Rust can show/position another window):

1. A frontend announcer composes the (playful) text and calls a new
   `announce(text)` command — but only if the setting is on.
2. `announce` positions + shows the bubble window beside the buddy (reusing the
   existing `show_bubble` / `place_beside_buddy` path) and emits a
   `bubble-message` event carrying the text.
3. `BubbleRoot` receives `bubble-message`, shows the text, and starts an
   auto-dismiss timer. On expiry it sets the bubble invisible, which already
   triggers `close_bubble` (Rust hides the window).

**Single announcer per event** (both the buddy and panel windows call
`capture.init()`, so both *see* capture events — announcing from both would
double the bubble):

- **Buddy window** is the announcer for capture-driven progress (recording
  start/saved/failed, transcription start/done/failed). It is always alive.
- **Panel window** is the announcer for its own user actions (vault opened,
  daily note opened) from the vaults store's `runAction`.

This is an invariant: capture announcements are wired *only* in the buddy
window; action announcements *only* in the vaults store.

## The message channel (`BubbleRoot`)

`BubbleRoot` becomes a **latest-wins** channel: one current message + one
auto-dismiss timer. A new message replaces the current text and resets the
timer, so rapid actions (start → stop) don't stack or queue. The launch
greeting is simply the first message through this channel (still 5 s); action
acknowledgements show ~3.2 s. Opening the panel dismisses a lingering bubble to
avoid overlapping the panel card.

The greeting timer logic (`useGreeting`) generalizes into a `useBuddyBubble`
composable exposing `show(text, ms)` / `visible` / `text`; the greeting calls
it on mount, the `bubble-message` listener calls it per message.

## Triggers → copy

Copy lives in a pure, unit-tested `src/buddyMessages.ts` module (no Vue, no IPC).

| Moment | Detected via | Line |
| --- | --- | --- |
| Vault opened | vaults store `runAction("open_vault")` success | `Opening <name> ✨` |
| Daily note opened | vaults store `runAction("open_daily_note")` success | `Here's today's note 📅` |
| Recording started | capture `status` → `recording` | `Listening… 🎙️` |
| Recording saved | `capture:saved` (`lastSavedFile` set) | `Got it — saved! 🎧` |
| Transcription started | `capture:transcribing` (`transcribing` → true) | `Writing it down… ✍️` |
| Transcription done | `capture:transcribed` (`lastTranscribed` set) | `Transcript ready! ✨` |
| Recording / transcription failed | `capture:failed` / `capture:transcribeFailed` | `Hmm, that didn't work 😕` |

The vault name is looked up from the vaults store and truncated by the bubble's
existing `max-width`.

## The setting

`buddyMessagesEnabled: boolean`, default **on**, added to the settings store
(localStorage-persisted, cross-window synced by `useSettingsStorageSync`). A
"Buddy messages" toggle joins the existing controls in `BuddySettings.vue`.
Every announce path checks it first, in whichever window originates the message
(both windows hold a synced copy).

## Components / boundaries

- `src/buddyMessages.ts` — pure copy functions. Testable everywhere.
- `src/composables/useBuddyBubble.ts` — the current-message + timer channel
  (generalized from `useGreeting`). Testable with fake timers.
- `src/composables/useBuddyAnnouncements.ts` — buddy-window watchers on the
  capture store → `announce`. Testable by driving the store + asserting the
  `announce` IPC.
- `BubbleRoot.vue` — wires `useBuddyBubble` + the `bubble-message` listener +
  greeting.
- vaults store `runAction` — announces vault/note opens.
- settings store + `BuddySettings.vue` — the toggle.
- Rust `announce` command (`commands.rs`) — show/position + emit
  `bubble-message`; registered in `lib.rs` and allowed in
  `capabilities/default.json`. Shell crate → Windows CI is its compile gate.

## Testing

- `buddyMessages` copy — pure unit tests.
- `useBuddyBubble` — show/replace/auto-dismiss with fake timers.
- `useBuddyAnnouncements` — each capture transition invokes `announce` with the
  right line; silent when the setting is off.
- vaults store — `runAction` announces on success (enabled) / stays silent
  (disabled), and never on vault-open failure.
- `BuddySettings` — the toggle flips the setting.
- Visual check of a real bubble via the headless-Chromium render.
- The Rust `announce` command compiles on CI's Windows job.
