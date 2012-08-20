#!/bin/sh -x

# Copyright (c) 2011 The Chromium OS Authors. All rights reserved.
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.

# This runs from the factory install/reset shim. This MUST be run
# from USB, in developer mode. This script will wipe OQC activity and
# put the system back into factory fresh/shippable state.
echo "Factory reset"

# TODO(crosbug:10680): replace arch detection with crossystem?
if uname -m | grep -q "^i.86\$"; then
  ARCH="INTEL"
elif [ $(uname -m ) = "x86_64" ]; then
  ARCH="INTEL"
elif [ $(uname -m ) = "armv7l" ]; then
  ARCH="ARM"
else
  echo "Failed to auto detect architecture"
  exit 1
fi

if [ "$ARCH" = "INTEL" ]; then
  # This is a Parrot specific issue and applied on Parrot factory branch only.
  # On Parrot, the internal storage(HDD) is recognized as sdb instead of sda
  # when it boots from USB memory like factory install or recovery. So before
  # wiping the stateful partition, we should check if it is removable or not.
  # If HDD is recognized first, then it will be sda, but if USB memory first,
  # then HDD will be sdb.
  #
  # Prevent writing to removable devices. Try sda and then try sdb.
  STATE_DEV="/dev/sda"
  REMOVABLE="/sys/block/$(basename $STATE_DEV)/removable"
  if [ -f "$REMOVABLE" ] && [ "$(cat $REMOVABLE)" = 1 ]; then
    echo "Cannot install to a removable device ($STATE_DEV)."
    STATE_DEV="/dev/sdb"
    REMOVABLE="/sys/block/$(basename $STATE_DEV)/removable"
    if [ -f "$REMOVABLE" ] && [ "$(cat $REMOVABLE)" = 1 ]; then
      echo "Cannot install to a removable device ($STATE_DEV)."
      echo "Cannot find internal storage."
      exit 1
    fi
  fi
  STATE_DEV="$STATE_DEV"1
elif [ "$ARCH" = "ARM" ]; then
  STATE_DEV="/dev/mmcblk0p1"
else
  STATE_DEV=""
fi
if [ ! -b "$STATE_DEV" ]; then
  echo "Failed to find root SSD."
  exit 1
fi

# Tcsd will bring up the tpm and de-own it,
# as we are in developer/recovery mode.
start tcsd

# Just wipe the start of the partition and remake the fs on
# the stateful partition.
dd bs=4M count=1 if=/dev/zero of=${STATE_DEV}
/sbin/mkfs.ext4 "$STATE_DEV"

# For factory BIOS update it should return and complete the process.
if [ "$1" = "--bios_update" ]; then
  exit 0
fi

# This should be executed only for keyboard-controlled recovery/developer mode.
crossystem disable_dev_request=1

# Do any board specific resetting here.
# board_factory_reset.sh will be installed by the board overlay if necessary.
BOARD_RESET=/usr/sbin/board_factory_reset.sh
if [ -x "${BOARD_RESET}" ]; then
  echo "Running board specific factory reset: ${BOARD_RESET}"
  ${BOARD_RESET} || exit 1
fi

echo "Done"
