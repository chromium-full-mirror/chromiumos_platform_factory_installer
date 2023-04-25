#!/bin/sh

# Copyright 2011 The ChromiumOS Authors
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.

# This runs from the factory install/reset shim. This MUST be run
# from USB, in developer mode. This script will wipe OQC activity and
# put the system back into factory fresh/shippable state.

# Preserve files in the CRX cache.  Largely copied from clobber-state.
# TODO(dgarrett,jsalz): Consolidate.
#

. "/usr/share/misc/chromeos-common.sh"
. "$(dirname "$0")/factory_common.sh"
. "$(dirname "$0")/secure-wipe.sh"

# CUTOFF_DIR is provided from platform/factory/sh/cutoff, repacked by ebuild.
CUTOFF_DIR="/usr/share/cutoff"
CUTOFF_SCRIPT="${CUTOFF_DIR}/cutoff.sh"
INFORM_SHOPFLOOR_SCRIPT="${CUTOFF_DIR}/inform_shopfloor.sh"

usage() {
  echo "Usage: $0 disk [wipe|secure|verify]."
  echo "disk: device to operate on [for instance /dev/sda]"
  echo "If no argument, do a factory reset. Otherwise:"
  echo "wipe: write 0's on every LBA [backward compatibility]"
  echo "secure: use internal erase command in the device"
  echo "        and write a pattern on the disk"
  echo "verify: verify the disk has been erased properly"
  exit 1
}

do_cutoff() {
  # inform_shopfloor will load shopfloor URL from lsb-factory, and ignore the
  # request if SHOPFLOOR_URL is not set.
  "${INFORM_SHOPFLOOR_SCRIPT}" "" "factory_reset" || exit 1

  "${CUTOFF_SCRIPT}"
}

main() {
  if [ "$#" -lt 1 ]; then
    usage
  fi

  DEV="$1"
  if [ ! -b "${DEV}" ]; then
    echo "Invalid root disk ${DEV}."
    exit 1
  fi
  STATE_DEV=$(make_partition_dev "${DEV}" "1")
  STATE_DEV="$(get_path_to_lvm_stateful "${STATE_DEV}")"
  DEV_SIZE=$(blockdev --getsize64 "${DEV}")
  shift

  # Tcsd will bring up the tpm and de-own it,
  # as we are in developer/recovery mode.
  start tcsd || true

  if [ $# -eq 1 ]; then
    case "$1" in
      wipe)
        # Nuke the disk.
        pv -etpr -s ${DEV_SIZE} -B 8M /dev/zero |
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

    ROOT_DEV="$(make_partition_dev "${DEV}" "5")"
    echo "Running clobber-state: ROOT_DEV=${ROOT_DEV}, ROOT_DISK: ${DEV}"

    # clobber-state preserves the crx and factory installed dlc files under
    # `/mnt/stateful_partition`. We need to mount `${STATE_DEV}` to that
    # directory, so that the files can be correctly preserved.
    STATE_MOUNT_POINT=/mnt/stateful_partition
    mount -o ro "${STATE_DEV}" "${STATE_MOUNT_POINT}"

    ROOT_DEV="${ROOT_DEV}" ROOT_DISK="${DEV}" \
      clobber-state factory fast

    umount "${STATE_MOUNT_POINT}"
  fi

  if [ "$(findLSBValue DISPLAY_QRCODE)" = true ]; then
    local frecon_pid
    frecon_pid="$(cat /run/frecon/pid)"
    local qrcode_path
    qrcode_path="/proc/${frecon_pid}/root"
    local sn
    sn="$(vpd -g serial_number)"
    local mlb_sn
    mlb_sn="$(vpd -g mlb_serial_number)"
    local hwid
    hwid="$(crossystem hwid)"
    declare -A info=( ["sn"]="${sn}" ["mlb_sn"]="${mlb_sn}" ["hwid"]="${hwid}" )

    # Display the info of the DUT as QR codes. Use comma to separate info
    # between QR codes. Use space to display multiple info in one QR code.
    IFS="," read -r -a needed_info <<< "$(findLSBValue DISPLAY_INFO)"
    clear
    local i=0
    while [ "${i}" -lt ${#needed_info[@]} ]; do
      local arr=(${needed_info[${i}]})
      local display_arr=()
      local j=0
      while [ "${j}" -lt ${#arr[@]} ]; do
        display_arr[${j}]="${info[${arr[${j}]}]}"
        j=$((j+1))
      done
      echo "display info=${needed_info[${i}]}"
      echo "display string=${display_arr[*]}"
      qrencode -s 5 -o "${qrcode_path}/qrcode.png" \
                       "$(IFS=, ; echo "${display_arr[*]}")"
      printf "\033]image:file=/qrcode.png\033\\" > /run/frecon/vt0
      if [[ ${i} -lt $(((${#needed_info[@]})-1)) ]]; then
        read -r -N 1 -p "Press any key to continue... "
      fi
      echo
      i=$((i+1))
    done
  fi

  do_cutoff
  echo "Done"
}
main "$@"
