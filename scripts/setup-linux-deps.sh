#!/usr/bin/env bash
# Install the system libraries the Tauri shell crate needs to COMPILE on
# Linux. The core/capture/transcribe crates already build on Linux; only the
# shell (src-tauri/src/*.rs) needs the WebView + GTK stack. This is the single
# source of truth for that package list — CI and humans/agents both call it,
# so the list never drifts. See
# docs/superpowers/specs/2026-07-09-linux-build-for-container-testing-design.md
set -euo pipefail

# Fast no-op on a warm container: if the WebView headers are already present,
# everything else in the list came with them.
if pkg-config --exists webkit2gtk-4.1 2>/dev/null; then
  echo "setup-linux-deps: webkit2gtk-4.1 already present — nothing to do"
  exit 0
fi

# Use sudo only when not already root (CI images run as root; a container
# agent may not).
SUDO=""
if [ "$(id -u)" -ne 0 ]; then
  SUDO="sudo"
fi

# --allow-releaseinfo-change: a pre-existing third-party apt repo in the base
# image can change its Label/Suite between our runs (e.g. a PPA relabel),
# which otherwise makes `apt-get update` exit non-zero and abort us under
# `set -e`, even though none of our packages come from it.
$SUDO apt-get update --allow-releaseinfo-change
$SUDO apt-get install -y \
  libwebkit2gtk-4.1-dev `# the WebView — the actual blocker` \
  libgtk-3-dev `# GTK windowing/toolkit layer Tauri links on Linux` \
  libayatana-appindicator3-dev `# system tray (tray-icon on Linux)` \
  librsvg2-dev `# SVG icon rendering` \
  libxdo-dev `# input synthesis Tauri links on Linux` \
  libsoup-3.0-dev `# HTTP stack behind webkit2gtk-4.1` \
  libssl-dev `# TLS for updater/network crates` \
  libasound2-dev `# ALSA headers for cpal (capture)` \
  build-essential pkg-config `# C toolchain + lib discovery` \
  cmake clang `# whisper-rs-sys: bindgen + whisper.cpp`

echo "setup-linux-deps: done"
