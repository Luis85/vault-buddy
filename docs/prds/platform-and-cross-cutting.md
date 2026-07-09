# Platform & Cross-Cutting Capabilities — Capability PRD

- **Status:** Draft
- **Version:** 0.1
- **Parent Product:** [Vault Buddy](../PRD%20-%20Product%20Vision.md)

Use cases extracted from this PRD, with shipping status: [Software
Auto-Update](../use-cases/software-auto-update.md), [Diagnostics, Crash
Reporting & Recovery](../use-cases/diagnostics-and-crash-reporting.md) (both
shipped). See [docs/use-cases/](../use-cases/README.md) for the full
catalog.

---

## Vision

Some product behaviors don't belong to any one capability domain — they
apply uniformly regardless of whether the user is browsing a vault,
recording a meeting, or managing tasks. Vault Buddy should be trustworthy
*as software*, not just useful as a knowledge tool: it starts fast, never
silently loses work, tells the truth about its own crashes, and keeps
itself current without asking the user to think about installers.

This PRD is the home for those behaviors. It exists because the other
capability PRDs are shaped around the [Knowledge
Lifecycle](knowledge-lifecycle.md) (Capture → Process → Organize → Act →
Retrieve → Automate → Learn), and platform mechanics like self-updating or
crash diagnostics don't map onto any lifecycle stage — they were shipping
in the product with no PRD to call home, discovered only by reading
`AGENTS.md`'s implementation notes and the source directly. See
[docs/use-cases/README.md § Known documentation gaps](../use-cases/README.md#known-documentation-gaps-found-during-this-extraction)
for how that was found.

## Mission

Give every product-wide quality attribute and platform mechanic exactly one
documented home, so "the app updates itself" or "the app reports its own
crashes" are never again shipped features nobody wrote down.

## Scope: what counts as cross-cutting

A capability belongs here if it is **true across every domain** rather than
specific to one — it would need to be restated in every other capability
PRD if it lived there instead. In practice:

- **In scope:** startup/performance targets, reliability guarantees, the
  security/permission model, accessibility, self-updating, crash
  detection/reporting, application lifecycle (single-instance, startup,
  shutdown).
- **Out of scope, stays in its capability PRD:** anything that only makes
  sense inside one domain's workflow, even if it *sounds* like reliability —
  e.g. "no recording loss" is Knowledge Intake's own reliability
  requirement ([Knowledge Intake PRD § Non-Functional
  Requirements](knowledge-intake.md)), not restated here. This PRD sets the
  product-wide floor; a capability PRD may add stricter, domain-specific
  requirements on top of it.
- **Out of scope, belongs to Desktop Companion:** the buddy character,
  window placement/dragging, tray, and speech bubble are user-facing
  *experience* — tracked under the main PRD's Core Capabilities and
  [Desktop Companion](../use-cases/desktop-companion.md), not here. This PRD
  only covers the *lifecycle* mechanics underneath a window (single-instance
  enforcement, crash-safe shutdown) rather than how it looks or moves.

---

## Non-Functional Requirements

*(Extracted verbatim from the main PRD's former §15 — see that PRD's
history for the original. Capability PRDs may declare stricter,
domain-specific targets; those specializations live in their own PRD, e.g.
Knowledge Intake's recording-startup target or Task Management's
modal-open target.)*

### Performance

- Startup < 2 seconds
- Command latency < 500 ms
- Memory usage < 250 MB
- Idle CPU < 1%

### Reliability

- Offline capable
- Automatic recovery
- Graceful failures
- Crash reporting (optional)

### Security

- Local-first
- Encrypted configuration
- Permission model
- Command allowlists
- Confirmation dialogs
- Audit log
- Read-only mode
- Secrets management

### Accessibility

- Keyboard navigation
- Screen reader compatibility
- High contrast
- Configurable scaling
- Reduced motion mode

---

## Capability: Software Auto-Update

Vault Buddy checks GitHub Releases for a newer signed build, downloads it,
and relaunches itself into the new version, entirely from Settings.

### Status: Shipped (v0.3.0)

### Requirements

- Check for updates on demand from Settings; surface a clear phase
  (checking / up to date / available / installing / error).
- Download while keeping the panel open so progress and errors stay
  visible.
- Save window state and stamp a clean-shutdown marker before installing, so
  a mid-update crash is never mistaken for an unclean shutdown of the *old*
  version.
- On failure, reopen the panel on the settings view with the error and keep
  the install button available for retry; never leave the app in a state
  where the user can't see what happened.
- Installer artifacts are signed; CI must still succeed (without updater
  artifacts) when signing secrets are unavailable, such as on forked PRs.

### Non-Functional Requirements

- No update check or download may block the buddy's normal window/tray
  behavior.
- A failed or interrupted update must never corrupt the running
  installation — the user can always retry or continue using the current
  version.

### Implementation reference

See [Software Auto-Update](../use-cases/software-auto-update.md) for the
concrete implementation (`updates` Pinia store, `prepare_update_install`,
`tauri-plugin-updater`, the release/signing workflow).

---

## Capability: Diagnostics, Crash Reporting & Recovery

Vault Buddy must be able to tell, on its *next* launch, whether it exited
cleanly last time — including exits a Rust panic hook cannot see (native
faults, kills, power loss) — and give the user a way to inspect what
happened without leaving the app.

### Status: Shipped (v0.3.0, hardened through later releases)

### Requirements

- Every graceful exit path (tray/buddy quit, Alt+F4 close, update install)
  stamps a clean-shutdown marker; any unclean shutdown is detectable on the
  next launch even when no Rust panic occurred.
- A background heartbeat refreshes a run marker so a native crash, a kill,
  or a power loss is distinguishable from a graceful exit.
- The panic hook and a native crash handler are installed before anything
  else in the app, so no earlier startup code can fail unrecorded.
- A crash occurring before logging is initialized is still captured (to a
  temp location) and folded into the real log directory once logging comes
  up, rather than being lost.
- The user can open the logs folder directly from the app (no manual
  `%APPDATA%` navigation) to inspect or share a crash record.
- Exactly one instance of the app may run at a time; a second launch exits
  immediately and reveals the existing instance's buddy instead of starting
  a competing process.

### Non-Functional Requirements

- No swallowed errors: anything caught-and-hidden is logged, never silent.
- Diagnostics must never depend on the very subsystem they're diagnosing
  (e.g. crash detection cannot depend on the window it's meant to detect
  the loss of).

### Implementation reference

See [Diagnostics, Crash Reporting &
Recovery](../use-cases/diagnostics-and-crash-reporting.md) for the concrete
implementation (`diagnostics.rs`, `core::app_diagnostics`, the panic hook +
native crash handler, `open_logs_folder`, `rearm_crash_detection`,
`tauri-plugin-single-instance`).

---

## Roadmap

### Now (shipped, this PRD backfills documentation for it)

- Software Auto-Update
- Diagnostics, Crash Reporting & Recovery

### Next

- Formalize the Security NFRs above into an actual permission/audit model —
  today's "audit log" is the vault domain's URI-launch log
  (`uri::launch`); there is no unified cross-domain audit trail, encrypted
  configuration, or command allowlist yet.
- Accessibility: none of the five NFR bullets above have a corresponding
  implementation today; this PRD is where that work should be scoped when
  it starts.

## Success Metrics

- Percentage of sessions with a clean-shutdown marker on next launch.
- Update adoption rate (time from release to majority of active installs
  updated) — measured only through local, user-inspectable means per the
  product's no-telemetry principle, consistent with every other capability
  PRD's Success Metrics section.
- Crash-free sessions (already a named metric in the main PRD's § Success
  Metrics — this PRD is where the underlying mechanism is specified).
