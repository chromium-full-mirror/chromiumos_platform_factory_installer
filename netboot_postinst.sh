#!/bin/sh -ex

# Copyright (c) 2013 The Chromium OS Authors. All rights reserved.
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.

# This is script is used to install HWID bundle on the installed device.
# After stateful partition and root partition are installed, this script is
# copied to stateful partition. Installer then chroot to the installed
# partitions and executes this script.
# This script sets up environment required for HWID bundle and installs the
# HWID bundle. Before exiting, it also cleans up itself and the HWID bundle.

POST_PROCESS_SCRIPT=$(readlink -f "$0")
HWID_BUNDLE_PATH=/mnt/stateful_partition/hwid_bundle.sh
FIRMWARE_PATH=/mnt/stateful_partition/firmware_install

# Change color in console by ANSI escape sequence code
colorize() {
  local code="$1"
  case "$code" in
    "red" )
      code="1;31"
      ;;
    "green" )
      code="1;32"
      ;;
    "yellow" )
      code="1;33"
      ;;
  esac
  printf "\033[%sm" "$code"
}

# Error message for any unexpected error.
die() {
  set +x  # prevent extra log in console
  colorize "red"
  echo "
    ERROR: Factory installation has been stopped.
    Press Ctrl-Alt-F3 (Refresh) to get the detail information."
  sleep 1d
}

setup_environment() {
  # This script was executed after chroot so we have to re-create all run state
  # folders. In future we should migrate to using "mount -o move" by moving the
  # chroot call into this script.

  # We need /tmp for mktemp and /run/[lock] for flashrom / mosys.
  mount -t tmpfs -o exec none /tmp
  mount -t tmpfs -o noexec none /run
  mkdir -p /run/lock

  # Copied from platform/initramfs/common/init.sh#init_mounts
  mount -n -t proc -o nodev,noexec,nosuid proc /proc
  mount -n -t sysfs -o nodev,noexec,nosuid sysfs /sys

  mount -t devtmpfs -o mode=0755,nosuid devtmpfs /dev
  ln -sf /proc/self/fd /dev/fd || :
  ln -sf fd/0 /dev/stdin || :
  ln -sf fd/1 /dev/stdout || :
  ln -sf fd/2 /dev/stderr || :

  mkdir -p /dev/pts
  mount -n -t devpts -o noexec,nosuid devpts /dev/pts || :

  mount -n -t debugfs debugfs /sys/kernel/debug
  # End of platform/initramfs/common/init.sh#init_mounts

  # Set up /usr/local
  mount --bind /mnt/stateful_partition/dev_image /usr/local || die
}

setup_environment
if [ -s "$FIRMWARE_PATH" ]; then
  echo "Execute firmware-install script"
  sh "$FIRMWARE_PATH" --force --mode=factory_install || die
fi
if [ -s "$HWID_BUNDLE_PATH" ]; then
  echo "Execute HWID component list updater script"
  sh "$HWID_BUNDLE_PATH" || die
fi
rm -f "$POST_PROCESS_SCRIPT" "$HWID_BUNDLE_PATH" "$FIRMWARE_PATH"
