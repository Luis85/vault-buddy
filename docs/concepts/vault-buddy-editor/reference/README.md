# Browser behavior reference

Build with `python reference/build.py` from the handover root. No package installation or network is required. The output `vault-buddy-editor.html` contains its CSS, SVG icons, JavaScript and procedural sample media.

The modules separate drawing, core editor behavior, project lifecycle, webcam, editing features, captions, direct/contextual editing, preflight/recovery, onboarding, session safety and workspace composition. `handover-polish.js` handles startup state and document identity. The builder lists the exact dependency order. It preserves existing interchange/storage format IDs; these are data-compatibility identifiers, not user-facing edition labels.

This is an executable behavior reference with test seams and imperative DOM code. **Do not copy its globals, wrapper overrides or repeated HTML rendering into Vue as the production architecture.** Port to components, typed stores, registered actions, lifecycle-bound controllers and native-authoritative transactions as described in the architecture guide. The standalone inline bundle is not compatible with the application's current production CSP without a normal Vue build; do not weaken script policy to embed it.

No CDN, external fonts or remote media are used by the editor. Local import/recording requires supported browser capabilities and explicit user action. Browser rendering is real-time review output, not production encoding. Native vault writing is not connected.
