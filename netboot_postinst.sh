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

# Change color in tty1 by ANSI escape sequence code
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
  printf "\033[%sm" "$code" >/dev/tty1
}

# Error message for any unexpected error.
die() {
  set +x  # prevent extra log in console
  colorize "red"
  echo "
    ERROR: Factory installation has been stopped.
    Press Ctrl-Alt-F3 (Refresh) to get the detail information." >/dev/tty1
  sleep 1d
}

setup_environment() {
  # We need /tmp for mktemp
  mount -t tmpfs -o exec none /tmp

  # Copied from platform/initramfs/init
  mount -t proc -o nodev,noexec,nosuid none /proc
  mount -t sysfs -o nodev,noexec,nosuid none /sys
  if ! mount -t devtmpfs -o mode=0755 none /dev; then
    mount -t tmpfs -o mode=0755 none /dev
    mknod -m 0600 /dev/console c 5 1
    mknod -m 0601 /dev/tty1 c 4 1
    mknod -m 0601 /dev/tty2 c 4 2
    mknod -m 0601 /dev/tty3 c 4 3
    mknod -m 0600 /dev/tpm0 c 10 224
    mknod /dev/null c 1 3
    ln -s /proc/self/fd /dev/fd
  fi
  mkdir -p /dev/pts
  mount -t devpts -o noexec,nosuid none /dev/pts || true
  mount -n -t debugfs debugfs /sys/kernel/debug

  # Set up /usr/local
  mount --bind /mnt/stateful_partition/dev_image /usr/local || die
}

setup_environment
if [ -s "$FIRMWARE_PATH" ]; then
  echo "Execute firmware-install script" >/dev/tty1
  sh "$FIRMWARE_PATH" --force --mode=factory_install >/dev/tty1 || die
fi
if [ -s "$HWID_BUNDLE_PATH" ]; then
  echo "Execute HWID component list updater script" >/dev/tty1
  sh "$HWID_BUNDLE_PATH" >/dev/tty1 || die
fi
rm -f "$POST_PROCESS_SCRIPT" "$HWID_BUNDLE_PATH" "$FIRMWARE_PATH"
