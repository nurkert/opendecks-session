#!/bin/bash -e
# pi-gen stage: installs the OpenDecks bundle into the image.
# Usage: copy this stage into pi-gen, put opendecks-pi-*.tar.gz into
# files/, and add "stage-opendecks" after stage2 in STAGE_LIST.
BUNDLE=$(ls files/opendecks-pi-*.tar.gz | head -1)
tar -xzf "$BUNDLE" -C "${ROOTFS_DIR}/tmp"
DIR=$(basename "$BUNDLE" .tar.gz)
on_chroot << CHROOT
/tmp/$DIR/install.sh dj
rm -rf /tmp/$DIR
CHROOT
