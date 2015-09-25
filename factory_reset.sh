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
. "$(dirname "$0")/factory_verify.sh"
. "/opt/google/memento_updater/find_omaha.sh"

usage() {
  echo "Usage: $0 disk [wipe|erase|verify]."
  echo "disk: device to operate on [for instance /dev/sda]"
  echo "If no argument, do a factory reset. Otherwise:"
  echo "wipe: write 0's on every LBA [backward compatibility]"
  echo "secure: use internal erase command in the device"
  echo "        and write a pattern on the disk"
  echo "verify: verify the disk has been erased properly"
  exit 1
}

show_banner_message() {
  local msg="$1"
  figlet "$msg"
}

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

reset_activate_date() {
  /usr/sbin/activate_date --clean
}

has_battery() {
  power_supply_info | grep -q 'Device: Battery'
}

get_battery_percentage() {
  power_supply_info 2>/dev/null |
    awk '/percentage/ && !/display/ {print int($2)}'
}

get_battery_voltage() {
  ectool battery 2>/dev/null | awk '/Present voltage/ {print int($3)}'
}

is_ac_present() {
  ectool battery | grep -q AC_PRESENT
}

require_ac() {
  if ! is_ac_present; then
    show_banner_message "Connect AC"
    while ! is_ac_present; do
      sleep 0.5
    done
  fi
}

require_remove_ac() {
  if is_ac_present; then
    show_banner_message "Remove AC"
    while is_ac_present; do
      sleep 0.5
    done
  fi
}

check_battery_value() {
  local min_battery_value="" max_battery_value=""
  local get_value_cmd=""
  local battery_value=""
  local prev_battery_value=""

  min_battery_value="$1"
  max_battery_value="$2"
  get_value_cmd="$3"
  battery_value=$($get_value_cmd)

  if [ -n "$min_battery_value" ] &&
     [ "$battery_value" -lt "$min_battery_value" ]; then
    require_ac
    show_banner_message "Charging..."

    # Wait for battery to charge to min_battery_value
    prev_battery_value="-1"
    while [ "$battery_value" -lt "$min_battery_value" ]; do
      # Only print battery info when it changes.
      if [ "$battery_value" -ne "$prev_battery_value" ]; then
        echo "current: $battery_value, target: $min_battery_value"
        prev_battery_value="$battery_value"
      fi
      sleep 1
      battery_value=$($get_value_cmd)
    done
  fi

  if [ -n "$max_battery_value" ] &&
     [ "$battery_value" -gt "$max_battery_value" ]; then
    require_remove_ac
    show_banner_message "Discharging..."

    # Wait for battery to discharge to max_battery_value
    prev_battery_value="-1"
    while [ "$battery_value" -gt "$max_battery_value" ]; do
      # Only print battery info when it changes.
      if [ "$battery_value" -ne "$prev_battery_value" ]; then
        echo "current: $battery_value, target: $max_battery_value"
        prev_battery_value="$battery_value"
      fi
      sleep 1
      battery_value=$($get_value_cmd)
    done
  fi
}

check_ac_state() {
  local cutoff_ac_state="$1"
  if [ "$cutoff_ac_state" = "ac_connect" ]; then
    require_ac
  elif [ "$cutoff_ac_state" = "ac_disconnect" ]; then
    require_remove_ac
  fi
}

default_cutoff_flow() {
  local cutoff_method=""
  local cutoff_ac_state=""
  local min_battery_percent="" max_battery_percent=""
  local min_battery_voltage="" max_battery_voltage=""

  cutoff_method=$(findLSBValue CUTOFF_METHOD)
  cutoff_ac_state=$(findLSBValue CUTOFF_AC_STATE)

  reset_activate_date

  if has_battery; then
    # Needed by 'ectool battery'.
    mkdir -p /var/lib/power_manager
    modprobe i2c_dev

    min_battery_percent=$(findLSBValue CUTOFF_BATTERY_MIN_PERCENTAGE)
    max_battery_percent=$(findLSBValue CUTOFF_BATTERY_MAX_PERCENTAGE)
    min_battery_voltage=$(findLSBValue CUTOFF_BATTERY_MIN_VOLTAGE)
    max_battery_voltage=$(findLSBValue CUTOFF_BATTERY_MAX_VOLTAGE)
    check_battery_value "$min_battery_percent" "$max_battery_percent" \
      "get_battery_percentage"
    check_battery_value "$min_battery_voltage" "$max_battery_voltage" \
      "get_battery_voltage"
  fi

  case "$cutoff_method" in
    SHUTDOWN )
      shutdown -h now
      ;;
    REBOOT )
      reboot
      ;;
    EC_BATTERY_CUTOFF )
      check_ac_state "$cutoff_ac_state"
      ectool batterycutoff
      ;;
    EC_BATTERY_CUTOFF_AT_SHUTDOWN )
      check_ac_state "$cutoff_ac_state"
      ectool batterycutoff at-shutdown && shutdown -h now
      ;;
    * )
      # By default we shutdown the device without doing anything.
      shutdown -h now
  esac

  show_banner_message "Please wait for shutdown"

  sleep 15
  show_banner_message "Cutoff failed"
  sleep 1d
  exit 1
}

if [ "$#" -lt 1 ]; then
  usage
fi

DEV="$1"
if [ ! -b "${DEV}" ]; then
  echo "Invalid root disk ${DEV}."
  exit 1
fi
STATE_DEV=$(make_partition_dev "${DEV}" "1")
DEV_SIZE=$(blockdev --getsize64 "${DEV}")
shift

# Tcsd will bring up the tpm and de-own it,
# as we are in developer/recovery mode.
start tcsd

if [ $# -eq 1 ]; then
  case "$1" in
    wipe)
      # Nuke the disk.
      pv -etpr -s ${DEV_SIZE} -B 8M /dev/zero 2>/dev/tty1 | \
        dd bs=8M of="${DEV}" oflag=dsync iflag=fullblock
      ;;
    secure)
      # Erase using firmware feature first.
      secure_erase ${DEV} || exit 1
      perform_fio_op "${DEV}" "${DEV_SIZE}" "write" || exit 1
      ;;
    verify)
      perform_fio_op "${DEV}" "${DEV_SIZE}" "verify" || exit 1
      ;;
    *)
      usage
  esac
else
  echo "Factory reset"
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
# Otherwise, settings in lsb-factory will be loaded and default cutoff flow
# will be applied.
BOARD_RESET=/usr/sbin/board_factory_reset.sh
if [ -x "${BOARD_RESET}" ]; then
  echo "Running board specific factory reset: ${BOARD_RESET}"
  ${BOARD_RESET} || exit 1
else
  default_cutoff_flow
fi

echo "Done"
