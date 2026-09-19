# Vault Buddy · Tutorial Editor
## Implementation handover

**Purpose:** turn a screen capture into a clear, short tutorial without losing the editable workspace. The project is the working document; every rendered video is a separate product of that project.

Open **[vault-buddy-editor.html](vault-buddy-editor.html)** in a desktop browser. It is self-contained: CSS, icons, JavaScript and procedural sample media are included. Choose **Show me around**, or open **Help** at any time to start/resume the walkthrough. Camera access and file operations happen only after explicit actions.

This handover is the complete product-design and behavior reference. It does not require another document bundle or a chat transcript. The HTML is a working browser reference, **not a compiled Rust/Tauri application**. The native-stack integration requirements are explicit and remain separate from browser verification.

A visual entry point is also available at **[START-HERE.html](START-HERE.html)**.

## Read in this order

| Need | Start here |
|---|---|
| Understand the product, scope and success conditions | [Product specification](docs/PRODUCT-SPEC.md) |
| Find every retained capability | [Feature catalog](docs/FEATURE-CATALOG.md) |
| Learn the finished experience | [User guide](docs/USER-GUIDE.md), [screen specifications](docs/SCREENS-AND-INTERACTIONS.md) |
| Implement Vue, TypeScript and Pinia correctly | [Architecture and stack review](docs/ARCHITECTURE-AND-STACK.md) |
| Implement edit data and native communication | [Data model](docs/DATA-MODEL.md), [IPC contracts](docs/IPC-CONTRACTS.md) |
| Implement media, persistence and vault writes | [Media pipeline](docs/NATIVE-MEDIA.md), [Persistence and security](docs/PERSISTENCE-AND-SECURITY.md) |
| Implement the full onboarding experience | [Onboarding specification](docs/ONBOARDING.md), [machine-readable steps](contracts/onboarding.steps.json) |
| Plan and execute work | [Implementation plan](docs/IMPLEMENTATION-PLAN.md), [agent brief](docs/IMPLEMENTER-BRIEF.md) |
| Define acceptance across all capabilities | [Acceptance scenarios](docs/ACCEPTANCE-SCENARIOS.md), [feature evidence matrix](docs/FEATURE-ACCEPTANCE-MATRIX.md) |
| Check evidence and release gates | [Review](docs/PRODUCT-REVIEW.md), [verification](docs/VERIFICATION.md), [release checklist](docs/RELEASE-CHECKLIST.md) |

## Package contents

`reference/` contains the complete browser sources and a standard-library-only Python builder. `contracts/` contains the example workspace, schema, design tokens, onboarding content and acceptance scenarios. `implementation-starter/` contains typed TypeScript/Vue/Pinia/native-adapter examples, not a replacement application. `tests/` contains executable browser tests and original test-media fixtures. `screens/` contains current reference captures. `MANIFEST.json` records file hashes.

Build the HTML with `python reference/build.py`. Run browser checks with `python tests/run-all.py`; see `tests/README.md` for environment dependencies. The native integration starter is separately scoped and verified as stated in its README and the verification report.

## Fixed decisions

Keep one preview-header row. Keep Save project separate from Render video. Keep multi-track media and teaching tools. Keep editing non-destructive. Keep source-linked annotations/captions/chapters attached during timeline edits. Keep the walkthrough optional, dismissible and resumable. Keep all media local by default. Use the existing Rust + Tauri + TypeScript + Vue + Pinia application, not another framework.

## Implementation boundary

The browser supports real local-media editing, webcam takes, portable project files and real-time review rendering. It downloads files; it has no native vault connection. Browser render limits, archive limits, simulated-device tests and browser-cache checks are not native product guarantees. The native release requires bounded-memory encoding, crash-safe file transactions, camera/device verification, scoped IPC and Windows accessibility acceptance. The release checklist names these gates rather than treating them as already complete.
