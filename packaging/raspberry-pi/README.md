# OpenDecks Session on a Raspberry Pi

Standalone setup: a Raspberry Pi 4 or 5 (64-bit Raspberry Pi OS Lite) with a
touchscreen, attached to a DDJ-FLX10 or DDJ-FLX4. The Pi boots straight into
Perform; the controller drives everything, including the library browser.

## Install on an existing Raspberry Pi OS Lite (64-bit)

```sh
# on the Pi (or an arm64 build machine)
git clone https://github.com/nurkert/opendecks-session && cd opendecks-session
packaging/linux/install-build-deps.sh
packaging/raspberry-pi/build-bundle.sh
tar -xzf target/dist/opendecks-pi-*.tar.gz -C /tmp
sudo /tmp/opendecks-pi-*/install.sh dj
sudo reboot
```

## Build an SD card image

Use [pi-gen](https://github.com/RPi-Distro/pi-gen) (arm64 branch): copy
`stage-opendecks` into the pi-gen tree, put the bundle into
`stage-opendecks/files/`, and set `STAGE_LIST="stage0 stage1 stage2 stage-opendecks"`.

## What the installer sets up

- `/opt/opendecks` with the app, `od-cli` and the controller mappings
- `opendecks-kiosk.service`: full screen via `linuxkms` (no desktop), Lite
  profile, real-time priority for audio, restarts automatically
- USB sticks mount at `/media/opendecks/<device>` and can be ejected from the app
- performance CPU governor

## Music

Prepare sticks on your Mac or Linux machine: every registered stick carries
the OpenDecks format next to the Pioneer export, and the Pi plays it directly.
For a library on the Pi itself, copy music to the Pi and run
`/opt/opendecks/od-cli import <folder> && /opt/opendecks/od-cli analyze --lite`.
