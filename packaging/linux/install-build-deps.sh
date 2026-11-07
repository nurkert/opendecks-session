#!/bin/sh
# Build dependencies on Debian 12+, Ubuntu 22.04+ and Raspberry Pi OS (64-bit).
set -eu
SUDO=""
[ "$(id -u)" -eq 0 ] || SUDO=sudo
$SUDO apt-get update
$SUDO apt-get install -y --no-install-recommends \
  build-essential pkg-config ca-certificates curl git file python3 \
  libasound2-dev librubberband-dev libjack-jackd2-dev libpipewire-0.3-dev libclang-dev \
  libfontconfig1-dev libfreetype-dev libxkbcommon-dev libudev-dev libinput-dev libgbm-dev libdrm-dev \
  libgtk-3-dev libwayland-dev libx11-dev libxcursor-dev libxrandr-dev libxi-dev libgl-dev libegl-dev
