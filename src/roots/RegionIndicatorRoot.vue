<script setup lang="ts">
/**
 * The region-capture indicator (GAP-165, spec
 * `2026-09-20-region-capture-indicator-design.md` as amended 2026-09-21).
 *
 * This root runs in the `region-indicator` window, which Rust has already
 * sized and positioned over the recorded rectangle while it was still
 * hidden — so this viewport IS the region, and the border below simply
 * traces its own edges. There is deliberately no arithmetic here: the
 * translation from a monitor-local rect to a virtual-desktop position is
 * `core::screen_geometry::indicator_bounds`, where Linux can test it, and
 * nothing about this feature is testable off Windows twice.
 *
 * **Why a border drawn over the recorded area is free.** The window is in
 * `capture_exclusion::EXCLUDED_LABELS`, so it is invisible to screen
 * capture while fully visible to the user. That membership is hardware
 * evidence rather than a declaration — see that module and GAP-165.
 *
 * Like `RegionRoot`, this root wires NO Pinia store, and that is not an
 * omission: AGENTS.md's per-window `init()` rule exists because a store
 * mirroring Rust state is dead in any webview that never subscribed. This
 * root mirrors one boolean with no derived state, which is less machinery
 * than the wiring a store would need.
 */
</script>

<template>
  <!-- `pointer-events-none` is belt-and-braces only. The real click-through
       is Rust's `set_ignore_cursor_events(true)`, applied before every show:
       a CSS rule cannot stop a transparent always-on-top window from taking
       the click at the OS level, and a window that swallowed every click
       over the whole region for a whole capture would be strictly worse
       than no indicator at all. -->
  <div
    data-testid="region-indicator-border"
    class="pointer-events-none fixed inset-0 border-2 border-recording"
  />
</template>
