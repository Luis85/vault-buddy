---
type: UseCase
status: shipped
domain: platform
shipped_in: v0.3.0
source_prd: "docs/prds/platform-and-cross-cutting.md"
tags: [use-case, platform]
---

# Diagnostics, Crash Reporting & Recovery

> Vault Buddy can tell, on its next launch, whether it exited cleanly last time — including exits a Rust panic hook can't see (native faults, kills, power loss) — and lets the user open the logs folder from inside the app.

## Source

[Platform & Cross-Cutting Capabilities PRD](../prds/platform-and-cross-cutting.md) § Capability: Diagnostics, Crash Reporting & Recovery. Previously undocumented in any PRD, same as [Software Auto-Update](software-auto-update.md) — surfaced only in `AGENTS.md`'s § Diagnostics invariants and scattered through the window-system invariants that depend on it.

## Status: Shipped (v0.3.0, hardened through later releases)

## Implementation

- `core::app_diagnostics` owns the clean-shutdown marker and run-heartbeat logic; `src-tauri/src/diagnostics.rs` wires it to the app lifecycle.
- The metronome thread in `lib.rs` (the same 1s loop that does always-on-top re-assertion and position checkpointing) heartbeat-refreshes the run marker — a structural detector for unclean shutdowns the panic hook alone cannot see.
- Every graceful exit path (tray/buddy quit, Alt+F4 close, `prepare_update_install`) calls `diagnostics::mark_clean_shutdown()`; any code that terminates via `std::process::exit` must do the same or the next launch reports a crash.
- The panic hook and native crash handler are installed before the Tauri builder — nothing may run ahead of them. A crash before logging is initialized is written to a temp fallback and adopted into the real log directory once available (`adopt_stray_crash_log`).
- IPC: `open_logs_folder` (opens the log directory in the OS file explorer), `rearm_crash_detection` (re-arms the marker after an update-install attempt latches it off; see [Software Auto-Update](software-auto-update.md)).
- `tauri-plugin-single-instance` is registered first in the builder — a second launch exits immediately and the surviving instance reveals its buddy instead.

## Related use-cases

- [Software Auto-Update](software-auto-update.md) — shares the clean-shutdown marker; `prepare_update_install` must stamp it before installing, and `rearm_crash_detection` re-arms it on failure.
- [Desktop Companion](desktop-companion.md) — the window-state save/position-checkpoint invariants this diagnostics system protects against (main-thread-only saves, drag-crash avoidance) live there.
