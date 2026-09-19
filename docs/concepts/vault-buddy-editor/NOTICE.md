# Bundle contents and execution boundary

The editor includes inline icons, CSS, JavaScript and procedurally drawn sample tutorial/presenter content. The sample cues and ambient bed are synthesized, not a human voice recording. The package does not include font files or remotely fetched stock media.

Test fixtures are included solely for repeatable validation of local image/video/audio workflows. Test execution uses externally installed Playwright/Chromium, ffmpeg/ffprobe and Python packages; none of those executables are bundled as application dependencies. The application implementation remains Rust/Tauri/TypeScript/Vue/Pinia.

Repository source was inspected read-only to guide integration. This archive does not claim to be a merged repository change, a native installer, or a completed media engine. The TypeScript/Vue/Pinia/Rust starter demonstrates explicit boundaries; its verification limits are stated separately. All product behaviors and native acceptance gates are documented within this handover.
