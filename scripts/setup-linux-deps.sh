#!/usr/bin/env bash
# Install the system libraries the Tauri shell crate needs to COMPILE on
# Linux. The core/capture/transcribe crates already build on Linux; only the
# shell (src-tauri/src/*.rs) needs the WebView + GTK stack. This is the single
# source of truth for that package list — CI and humans/agents both call it,
# so the list never drifts. See
# docs/superpowers/specs/2026-07-09-linux-build-for-container-testing-design.md
set -euo pipefail

# The apt packages the Linux shell build needs. ONE list, used for both the
# already-installed check and the install — so the "already present?" probe
# can never drift from what we actually install. (An earlier revision probed a
# representative pkg-config file / binary per group, which kept missing
# packages the proxy didn't cover, e.g. build-essential's gcc/g++/make; keying
# the check on the exact package set removes that whole failure class.)
PACKAGES=(
  libwebkit2gtk-4.1-dev         # the WebView — the actual blocker
  libgtk-3-dev                  # GTK windowing/toolkit layer Tauri links on Linux
  libayatana-appindicator3-dev  # system tray (tray-icon on Linux)
  librsvg2-dev                  # SVG icon rendering
  libxdo-dev                    # input synthesis Tauri links on Linux
  libsoup-3.0-dev               # HTTP stack behind webkit2gtk-4.1
  libssl-dev                    # TLS for updater/network crates
  libasound2-dev                # ALSA headers for cpal (capture)
  build-essential               # C/C++ toolchain (gcc, g++, make) for whisper-rs-sys
  pkg-config                    # lib discovery
  cmake                         # whisper-rs-sys build (drives whisper.cpp's CMake)
  clang                         # whisper-rs-sys bindgen (libclang)
)

# Fast no-op on a warm container: skip the slow apt-get update/install only
# when EVERY package above is already installed. dpkg-query is authoritative —
# it checks the exact packages we install, with no proxy files to drift.
all_installed() {
  local pkg
  for pkg in "${PACKAGES[@]}"; do
    dpkg-query -W -f='${Status}' "$pkg" 2>/dev/null \
      | grep -q "install ok installed" || return 1
  done
}
if all_installed; then
  echo "setup-linux-deps: all ${#PACKAGES[@]} packages already installed — nothing to do"
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
$SUDO apt-get install -y "${PACKAGES[@]}"

echo "setup-linux-deps: done"
