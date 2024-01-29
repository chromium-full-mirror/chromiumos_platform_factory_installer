#!/bin/bash

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

  # Apply the cutoff settings in toolkit config.
  cros_payload_patch_lsb_factory

  "${CUTOFF_SCRIPT}"
}

main() {
  if [ "$(findLSBValue DISPLAY_QRCODE)" = true ]; then
    local frecon_pid
    frecon_pid="$(cat /run/frecon/pid)"
    local qrcode_path
    qrcode_path="/proc/${frecon_pid}/root"

    # Display the info of the DUT as QR codes. Use comma to separate info
    # between QR codes. Use space to display multiple info in one QR code.
    IFS="," read -r -a needed_info <<< "$(findLSBValue DISPLAY_INFO)"
    clear
    local i=0
    while [ "${i}" -lt ${#needed_info[@]} ]; do
      for info in ${needed_info[${i}]}; do
        value=
        if [ "${info}" = hwid ]; then
          value="$(crossystem hwid)"
        elif [ "${info}" = serial_number ]; then
          value="$(vpd -g serial_number)"
        elif [ "${info}" = mlb_serial_number ]; then
          value="$(vpd -g mlb_serial_number)"
        elif [ "${info}" = wifi_mac0 ]; then
          value="$(vpd -g wifi_mac0)"
        elif [ "${info}" = service_tag ]; then
          value="$(vpd -g service_tag)"
        fi
        display_string="${display_string:+${display_string} }${value}"
      done
      echo "display info=${needed_info[${i}]}"
      echo "display string=${display_string}"
      qrencode -s 5 -o "${qrcode_path}/qrcode.png" \
                       "$(IFS=, ; echo "${display_string}")"
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
