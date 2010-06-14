#!/bin/sh -e

# Copyright (c) 2010 The Chromium OS Authors. All rights reserved.
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.

. "$(dirname "$0")/chromeos-common.sh"
. "/opt/google/memento_updater/memento_updater_logging.sh"

if [ $(id -u) -ne 0 ]; then
  echo "You must run this as root."
  exit 1
fi
log "Pause to ensure ethernet connectivity"
sleep 10

# TODO(adlr): pick an install device in a matter that works on x86 and ARM.
# This works on x86 only, afaik.
DST_DRIVE=/dev/sda
DST_FACTORY_PART=3
DST_RELEASE_PART=5
DST_OEM_PART=8
DST_EFI_PART=12
DST_STATE_PART=1
DST_FIRMWARE=/tmp/firmware.sh

log "Factory Install: Setting partition table"

/usr/sbin/chromeos-install --dst "${DST_DRIVE}" --skip_rootfs --run_as_root \
  --yes 2>&1 | cat >> "$MEMENTO_AU_LOG"

# Load the new partition table. The autoupdater has trouble with loop devices.
sync
echo 3 > /proc/sys/vm/drop_caches
/sbin/sfdisk -R "$DST_DRIVE"

FACTORY_CHANNEL_ARG='--force_track=factory-channel'
RELEASE_CHANNEL_ARG='--force_track=release-channel'
OEM_CHANNEL_ARG='--force_track=oempartitionimg-channel'
EFI_CHANNEL_ARG='--force_track=efipartitionimg-channel'
STATE_CHANNEL_ARG='--force_track=stateimg-channel'
FIRMWARE_CHANNEL_ARG='--force_track=firmware-channel'

# Install the partitions
for i in EFI OEM STATE RELEASE FACTORY FIRMWARE; do
  if [ "$i" = "FIRMWARE" ]; then
    DST="${DST_FIRMWARE}"
  else
    PART=$(eval "echo \$DST_${i}_PART")
    DST="${DST_DRIVE}${PART}"
  fi

  log "Factory Install: Installing $i image to $DST"

  CHANNEL_ARG=$(eval "echo \$${i}_CHANNEL_ARG")
  SKIP_POSTINST_ARG="--skip_postinst"
  KPART="none"
  if [ "$i" = "FACTORY" -o "$i" = "RELEASE" ]; then
    # Set up kernel partition
    SKIP_POSTINST_ARG=""
    KPART=""
  fi

  RESULT="$(/opt/google/memento_updater/memento_updater.sh --dst_partition \
    "${DST}" --kernel_partition "${KPART}" \
    --allow_removable_boot $CHANNEL_ARG $SKIP_POSTINST_ARG)"

  # Firmware update is optional; still OK if it does not be updated.
  if [ "$RESULT" != "UPDATED" -a "$i" != "FIRMWARE" ]; then
    log "Factory Install: AU failed"
    exit 1
  fi
done

log "All done installing."

shutdown -r now
sleep 1d  # sleep indefinitely to avoid respawning rather than shutting down
