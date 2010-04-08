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

# If we get in a respawn loop, sleeping for 1 minute before starting is nice.
sleep 60

# TODO(adlr): pick an install device in a matter that works on x86 and ARM.
# This works on x86 only, afaik.
DST=/dev/sda
DST_FACTORY_PART=3
DST_RELEASE_PART=5

log "Factory Install: Setting partition table"

/usr/sbin/chromeos-install --dst "${DST}" --skip_rootfs --run_as_root --yes \
  2>&1 | cat >> "$MEMENTO_AU_LOG"

cleanup_loop() {
  sleep 1  # TODO(adlr): figure out why this sleep seems to be required
  losetup -d "$LOOP_DEV" 2>&1 | cat >> "$MEMENTO_AU_LOG"
}

# Call as: setup_loop loop_dev device partition#
setup_loop() {
  OFFSET=$(partsize "$2" "$3")
  losetup -o "$OFFSET" "$1" "$2" 2>&1 | cat >> "$MEMENTO_AU_LOG"
}

# Install the two partitions
for i in FACTORY RELEASE; do
  PART=$(eval "echo \$DST_${i}_PART")
  if [ "$i" = "FACTORY" ]; then
    CHANNEL_ARG='--force_track=factory-channel'
  else
    CHANNEL_ARG=""
  fi
  
  LOOP_DEV=$(losetup -f)
  setup_loop "$LOOP_DEV" "$DST" "$PART" || exit 1
  trap cleanup_loop INT TERM EXIT

  log "Factory Install: Installing $i partition to $PART"

  RESULT="$(/opt/google/memento_updater/memento_updater.sh --dst_partition \
    "${DST}${PART}" --allow_removable_boot $CHANNEL_ARG)"

  if [ "$RESULT" != "UPDATED" ]; then
    log "Factory Install: AU failed"
    exit 1
  fi
  cleanup_loop
  trap - INT TERM EXIT
done

log "All done installing."

shutdown -r now
sleep 1d  # sleep indefinitely to avoid respawning rather than shutting down
