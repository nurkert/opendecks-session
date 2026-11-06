#!/bin/sh
# Build dependencies on Debian, Ubuntu and Raspberry Pi OS (64-bit).
set -eu
sudo apt-get update
sudo apt-get install -y --no-install-recommends \
  build-essential pkg-config \
  libasound2-dev librubberband-dev \
  libfontconfig1-dev libfreetype-dev libxkbcommon-dev libudev-dev libinput-dev libgbm-dev libdrm-dev \
  libgtk-3-dev libwayland-dev libx11-dev libxcursor-dev libxrandr-dev libxi-dev libgl-dev libegl-dev
