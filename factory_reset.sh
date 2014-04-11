#!/bin/sh

# Copyright (c) 2011 The Chromium OS Authors. All rights reserved.
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.

# This runs from the factory install/reset shim. This MUST be run
# from USB, in developer mode. This script will wipe OQC activity and
# put the system back into factory fresh/shippable state.

# Preserve files in the CRX cache.  Largely copied from clobber-state.
# TODO(dgarrett,jsalz): Consolidate.
#

. "/usr/share/misc/chromeos-common.sh"

# Note that some variables are shared with restore_crx_cache.
preserve_crx_cache() {
  PRESERVED_TAR="/tmp/preserve.tar"
  PRESERVED_FILES=""
  STATE_PATH="/mnt/stateful_partition"
  mkdir -p "${STATE_PATH}"
  mount "${STATE_DEV}" "${STATE_PATH}"
  IMPORT_FILES="$(cd ${STATE_PATH};
                  echo unencrypted/import_extensions/extensions/*.crx)"
  if [ "${IMPORT_FILES}" != \
       "unencrypted/import_extensions/extensions/*.crx" ]; then
    PRESERVED_FILES="${PRESERVED_FILES} ${IMPORT_FILES}"
  fi
  PRESERVED_LIST=""
  if [ -n "${PRESERVED_FILES}" ]; then
    # We want to preserve permissions and recreate the directory structure
    # for all of the files in the PRESERVED_FILES variable. In order to do
    # so we run tar --no-recurison and specify the names of each of the
    # parent directories. For example for home/.shadow/install_attributes.pb
    # we pass to tar home home/.shadow home/.shadow/install_attributes.pb
    for file in ${PRESERVED_FILES}; do
      if [ ! -e "${STATE_PATH}/${file}" ]; then
        continue
      fi
      path="${file}"
      while [ "${path}" != '.' ]; do
        PRESERVED_LIST="${path} ${PRESERVED_LIST}"
        path=$(dirname "${path}")
      done
    done
    tar cf ${PRESERVED_TAR} -C ${STATE_PATH} --no-recursion -- ${PRESERVED_LIST}
  fi
  # Try a few times to unmount the stateful partition.
  local unmounted=false
  for i in $(seq 5); do
    if umount ${STATE_DEV}; then
      unmounted=true
      break
    fi
    sleep 1
  done
  if ! ${unmounted}; then
    # Bail out: we may not be able to successfully mkfs.
    echo "Unable to unmount stateful partition. Aborting."
    exit 1
  fi
}

# Restores files previously preserved by preserve_crx_cache.
restore_crx_cache() {
  if [ -n "${PRESERVED_LIST}" ]; then
    # Copy files back to stateful partition
    mount ${STATE_DEV} ${STATE_PATH}
    tar xfp ${PRESERVED_TAR} -C ${STATE_PATH}
    sync  # Try as best we can, in case umount fails
    umount ${STATE_PATH}
    # Sleep for a bit, since we will shut down soon and want to give
    # the drive a chance to flush everything
    sleep 3
  fi
}

echo "Factory reset"

if [ "$#" -lt 1 ]; then
  echo "Usage: $0 disk [wipe]."
  echo "ex, $0 /dev/sda."
  exit 1
fi

DEV="$1"
if [ ! -b "${DEV}" ]; then
  echo "Invalid root disk ${DEV}."
  exit 1
fi
STATE_DEV=$(make_partition_dev "${DEV}" "1")
shift

# Tcsd will bring up the tpm and de-own it,
# as we are in developer/recovery mode.
start tcsd

if [ $# -eq 1 -a "$1" = "wipe" ]; then
  # Nuke the disk
  DEV_SIZE=$(blockdev --getsize64 ${DEV})
  pv -etpr -s ${DEV_SIZE} -B 8M /dev/zero 2>/dev/tty1 | \
      dd bs=8M of=${DEV} oflag=dsync iflag=fullblock
else
  if [ ! -b "${STATE_DEV}" ]; then
    echo "Failed to find stateful partition."
    exit 1
  fi

  preserve_crx_cache

  # Just wipe the start of the partition and remake the fs on
  # the stateful partition.
  dd bs=4M count=1 if=/dev/zero of=${STATE_DEV}
  /sbin/mkfs.ext4 "${STATE_DEV}"

  restore_crx_cache
fi

# Do any board specific resetting here.
# board_factory_reset.sh will be installed by the board overlay if necessary.
BOARD_RESET=/usr/sbin/board_factory_reset.sh
if [ -x "${BOARD_RESET}" ]; then
  echo "Running board specific factory reset: ${BOARD_RESET}"
  ${BOARD_RESET} || exit 1
fi

echo "Done"
