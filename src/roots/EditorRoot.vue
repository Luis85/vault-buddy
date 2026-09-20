<script setup lang="ts">
/**
 * The editor window's root (spec 8). Unlike the three companion roots this
 * one mirrors NO Rust state and installs no store: it is handed one staged
 * capture and edits it locally, persisting through `save_capture_timeline`.
 */
</script>

<template>
  <main class="flex h-screen w-screen items-center justify-center bg-slate-900 text-fg">
    <p class="text-sm text-fg-muted">
      Loading capture…
    </p>
  </main>
</template>
