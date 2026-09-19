# References and evidence provenance

Reviewed September 19, 2026. Product behavior is grounded in the supplied executable reference and newly executed checks. The links below support technology facts, not claims that the native integration has been tested.

## Repository evidence

Pinned source revision: `3f330a970ec26ca8311af2215d8df0dc8f17a737`. Declared dependency ranges are not lockfile-resolved versions. Source inspection was read-only; no repository files or pull requests were modified by this handover.

- [`package.json`](https://github.com/Luis85/vault-buddy/blob/3f330a970ec26ca8311af2215d8df0dc8f17a737/package.json)
- [`src/roots/index.ts`](https://github.com/Luis85/vault-buddy/blob/3f330a970ec26ca8311af2215d8df0dc8f17a737/src/roots/index.ts)
- [`src/stores/screenCapture.ts`](https://github.com/Luis85/vault-buddy/blob/3f330a970ec26ca8311af2215d8df0dc8f17a737/src/stores/screenCapture.ts)
- [`src-tauri/src/screen_commands.rs`](https://github.com/Luis85/vault-buddy/blob/3f330a970ec26ca8311af2215d8df0dc8f17a737/src-tauri/src/screen_commands.rs)
- [`src-tauri/screen/Cargo.toml`](https://github.com/Luis85/vault-buddy/blob/3f330a970ec26ca8311af2215d8df0dc8f17a737/src-tauri/screen/Cargo.toml)
- [`src-tauri/screen/src/staging.rs`](https://github.com/Luis85/vault-buddy/blob/3f330a970ec26ca8311af2215d8df0dc8f17a737/src-tauri/screen/src/staging.rs)
- [`src-tauri/tauri.conf.json`](https://github.com/Luis85/vault-buddy/blob/3f330a970ec26ca8311af2215d8df0dc8f17a737/src-tauri/tauri.conf.json)

The capture store/command files establish existing events, DTOs and resync behavior. The staging file establishes native `vaultId` metadata and current direct sidecar-write semantics. Root/config establish the editor window and media/security configuration still required at the inspected revision. The screen crate establishes reuse of native Windows capture/Media Foundation dependencies. None of these observations imply that a particular Windows test was rerun here.

## Official guidance

- [Tauri: calling Rust from the frontend](https://v2.tauri.app/develop/calling-rust/) — command arguments/results, async boundaries and channels.
- [Tauri: calling the frontend](https://v2.tauri.app/develop/calling-frontend/) — events/channels and message lifecycle.
- [Tauri: capabilities](https://v2.tauri.app/security/capabilities/) — per-window grants, union of grants and app-command manifest restrictions.
- [Tauri: runtime authority](https://v2.tauri.app/security/runtime-authority/) — permission enforcement boundary and limits.
- [Tauri: asset protocol](https://v2.tauri.app/security/asset-protocol/) — enabling and scoping filesystem-backed media URLs.
- [Pinia: state](https://pinia.vuejs.org/core-concepts/state.html) — typed option stores, patches and subscriptions.
- [Pinia: defining stores](https://pinia.vuejs.org/core-concepts/) — store composition and reactive references.
- [Vue: performance](https://vuejs.org/guide/best-practices/performance) — list virtualization and shallow boundaries for large immutable state.
- [WAI-ARIA: toolbar pattern](https://www.w3.org/WAI/ARIA/apg/patterns/toolbar/) — toolbar focus and keyboard behavior.
- [WAI-ARIA: menu button](https://www.w3.org/WAI/ARIA/apg/patterns/menu-button/) — named trigger, role and keyboard patterns.

Source-derived design guidance is paraphrased. Security and accessibility rules require actual implementation tests; referencing documentation is not evidence of compliance. Target hardware, durability and render acceptance must be established by the native release checklist.
