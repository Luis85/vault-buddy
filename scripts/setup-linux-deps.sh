#!/usr/bin/env bash
# Install the system libraries the Tauri shell crate needs to COMPILE on
# Linux. The core/capture/transcribe crates already build on Linux; only the
# shell (src-tauri/src/*.rs) needs the WebView + GTK stack. This is the single
# source of truth for that package list — CI and humans/agents both call it,
# so the list never drifts. See
# docs/superpowers/specs/2026-07-09-linux-build-for-container-testing-design.md
set -euo pipefail

# Fast no-op on a warm container — but ONLY when EVERY load-bearing dependency
# group is already present, not just the WebView. A container can ship
# webkit2gtk yet lack the repo-specific extras (ALSA for cpal, cmake/clang for
# whisper-rs-sys); keying the no-op on webkit alone would make this report
# success while `tauri build --no-bundle` still fails to build. So probe one
# representative per apt package below (its pkg-config `.pc`, or the binary for
# the build tools) and fall through to a full install if any is missing.
have_pc() { pkg-config --exists "$1" 2>/dev/null; }
if have_pc webkit2gtk-4.1 `# libwebkit2gtk-4.1-dev` \
  && have_pc gtk+-3.0 `# libgtk-3-dev` \
  && have_pc ayatana-appindicator3-0.1 `# libayatana-appindicator3-dev` \
  && have_pc librsvg-2.0 `# librsvg2-dev` \
  && have_pc libsoup-3.0 `# libsoup-3.0-dev` \
  && have_pc openssl `# libssl-dev` \
  && have_pc alsa `# libasound2-dev` \
  && [ -e /usr/include/xdo.h ] `# libxdo-dev (no pkg-config .pc; probe its header)` \
  && command -v cmake >/dev/null `# cmake` \
  && command -v clang >/dev/null `# clang`; then
  echo "setup-linux-deps: all build deps already present — nothing to do"
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
