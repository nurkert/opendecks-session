#!/bin/sh
# Installs OpenDecks Session as a kiosk on Raspberry Pi OS Lite (64-bit),
# Raspberry Pi 4 or 5. Run from the unpacked bundle as root:
#   sudo ./install.sh [user]
set -eu
USER_NAME=${1:-dj}
HERE=$(cd "$(dirname "$0")" && pwd)

apt-get update
apt-get install -y --no-install-recommends \
  librubberband2 libasound2 libinput10 libgbm1 libdrm2 libegl1 libgles2 libfontconfig1 libxkbcommon0 \
  udisks2 exfatprogs dosfstools

id "$USER_NAME" >/dev/null 2>&1 || useradd -m -s /bin/bash "$USER_NAME"
usermod -aG audio,video,input,render,plugdev "$USER_NAME"

install -d /opt/opendecks
install -m755 "$HERE/opendecks" "$HERE/od-cli" /opt/opendecks/
rm -rf /opt/opendecks/mappings && cp -R "$HERE/mappings" /opt/opendecks/mappings

# Real-time audio for the audio group.
cat > /etc/security/limits.d/95-opendecks-audio.conf <<LIMITS
@audio - rtprio 95
@audio - memlock unlimited
LIMITS

# USB sticks are mounted under /media/opendecks/<device> automatically and
# may be ejected by the kiosk user.
install -m644 "$HERE/99-opendecks-usb.rules" /etc/udev/rules.d/
install -d /etc/polkit-1/rules.d
install -m644 "$HERE/50-opendecks-udisks.rules" /etc/polkit-1/rules.d/
install -d /media/opendecks

# Kiosk service: starts at boot straight into Perform.
sed "s/@USER@/$USER_NAME/g" "$HERE/opendecks-kiosk.service" > /etc/systemd/system/opendecks-kiosk.service
systemctl daemon-reload
systemctl enable opendecks-kiosk.service
systemctl set-default multi-user.target
# The kiosk owns the display: no login prompt on tty1.
systemctl disable getty@tty1.service || true

# Performance CPU governor (audio stability).
if [ -w /etc/default/cpufrequtils ] || [ ! -e /etc/default/cpufrequtils ]; then
  echo 'GOVERNOR="performance"' > /etc/default/cpufrequtils
fi
udevadm control --reload
echo "Installed. Reboot to start OpenDecks Session."
