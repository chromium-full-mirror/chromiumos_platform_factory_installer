#!/bin/sh -ex

# Copyright (c) 2011 The Chromium OS Authors. All rights reserved.
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.

. "$(dirname "$0")/chromeos-common.sh"
. "/opt/google/memento_updater/memento_updater_logging.sh"
. "/opt/google/memento_updater/find_omaha.sh"

if [ $(id -u) -ne 0 ]; then
  echo "You must run this as root."
  exit 1
fi


clear_fwwp() {
  log "Firmware Write Protect disabled, clearing status registers."
  flashrom -p internal:bus=lpc --wp-disable
  flashrom -p internal:bus=spi --wp-disable
  log "WP registers should be cleared now"
}

clear_tpm() {
  log "Clearing TPM"

  # Reset TPM
  # tcsd needs to have not been run. It locks the TPM.
  # Add tpm startup command. Second startup won't hurt.
  tpmc startup || true
  tpmc ppon
  tpmc clear
  tpmc enable
  tpmc activate

  firmware_index="0x1007"
  firmware_struct_version="1"
  firmware_flags="0"
  firmware_fw_versions="1 0 1 0"
  firmware_reserved="0 0 0 0"

  tpmc write $firmware_index $firmware_struct_version $firmware_flags \
      $firmware_fw_versions $firmware_reserved || true

  kernel_index="0x1008"
  kernel_struct_version="1"
  kernel_uid="4c 57 52 47"
  kernel_kernel_versions="1 0 1 0"
  kernel_reserved="0 0 0 0"

  tpmc write $kernel_index $kernel_struct_version $kernel_uid \
      $kernel_kernel_versions $kernel_reserved || true

  log "Done clearing TPM"
}

set_time() {
  log "Setting time from:"
  # Extract only the server and port.
  AUSERVER_URL=$(findLSBValue "CHROMEOS_AUSERVER" |
      sed "s|/update||; s|http://||")
  log "Server $AUSERVER_URL."
  echo "Server $AUSERVER_URL."
  RESULT=$(htpdate -s "${AUSERVER_URL}" 2>&1)
  if ! echo "${RESULT}" | grep -Eq "(failed|unavailable)"; then
    log "Success, time set to $(date)"
    return 0
  fi

  log $(echo "${RESULT}" | grep -E "(failed|unavailable)")
  log "Failed to set time"
  return 1
}

list_ethernet_interface() {
  local candidates="$(ifconfig | grep 'Link encap:Ethernet' | cut -d ' ' -f 1)"
  for candidate in $candidates; do
    # output if it is not a wifi interface
    if ! iw $candidate info >/dev/null 2>&1; then
      echo $candidate
    fi
  done
}

log "Starting Factory Installer."

log "Checking for Firmware Write Protect"

# Only ChromeOS machines have meaninful output here.
if crossystem hwid >/dev/null; then
  # Check for physical firmware write protect. We'll only
  # clear this stuff if the case is open.
  if [ "$(crossystem wpsw_cur)" = "0" ]; then
    # Clear software firmware write protect.
    clear_fwwp
  fi

  # Check for software firmware write protect.  We only clear the TPM if
  # software write protect is disabled.
  if flashrom -p internal:bus=spi --wp-status 2>/dev/null |
      grep -q "write protect is disabled"; then
    # Ensure that we can wipe TPM if necessary.
    clear_tpm
  fi
fi

#
# factory_install.sh implements two operations for assembly line
# operators: install (obviously) and reset.
#
# Somehow the way that operators switch between the two operations
# is by plugging in a Ethernet cable.
#
# The operation is:
# * Install if it is connected to Ethernet;
# * Reset if developer switch is toggled to consumer mode.
#
# So we have to detect a possible ethernet connection here.
#

log "Waiting for ethernet connectivity to install"
log "Or disable developer mode to factory reset."
while true; do
  connected=0
  for ethernet_interface in $(list_ethernet_interface); do
    if ifconfig $ethernet_interface | grep -q "inet addr"; then
      log "$(ifconfig $ethernet_interface | grep 'inet addr')"
      connected=1
      break
    fi
  done
  if [ $connected -eq 1 ]; then
    break
  fi

  # If developer switch is flipped, go to "reset mode" instead of
  # network install mode. Make sure gpio can be read (gpio_setup may
  # fail if the device is not ready).
  if [ "$(crossystem devsw_cur)" = "0" ]; then
    log "Performing factory reset"
    if ! /usr/sbin/factory_reset.sh; then
      log "Factory reset failed."
      exit 1
    fi

    log "Done"
    exit 0
  fi
  sleep 1
done

set_time || exit 1

# TODO(crosbug:10680): replace arch detection with crossystem?
if uname -m | grep -q "^i.86\$"; then
  ARCH="INTEL"
elif [ $(uname -m ) = "x86_64" ]; then
  ARCH="INTEL"
elif [ $(uname -m ) = "armv7l" ]; then
  ARCH="ARM"
else
  log "Error: Failed to auto detect architecture"
  exit 1
fi

if [ "$ARCH" = "INTEL" ]; then
  DST_DRIVE=/dev/sda
else
  DST_DRIVE=/dev/mmcblk0
fi
DST_FACTORY_PART=3
DST_RELEASE_PART=5
DST_OEM_PART=8
DST_EFI_PART=12
DST_STATE_PART=1
DST_FIRMWARE=/tmp/firmware.sh

# Light up screen in case you can't see our splash image.
LIGHTUP_SCREEN="/usr/sbin/lightup_screen"
if [ -x "${LIGHTUP_SCREEN}" ]; then
  ${LIGHTUP_SCREEN}
else
  log "${LIGHTUP_SCREEN} does not exist or not executable"
fi

log "Factory Install: Setting partition table"

INST_FLAGS="--dst ${DST_DRIVE} --skip_rootfs --run_as_root --yes"

# in factory mode, we need both GPT_LAYOUT and PMBR_CODE always generated.
FACTORY_GPT_LAYOUT=/root/.gpt_layout
FACTORY_PMBR_CODE=/root/.pmbr_code
if [ -s $FACTORY_GPT_LAYOUT ]; then
  INST_FLAGS="${INST_FLAGS} --gpt_layout $FACTORY_GPT_LAYOUT"
else
  log "ERROR: MISSING $FACTORY_GPT_LAYOUT ; PLEASE REBUILD FACTORY IMAGE."
  log "FACTORY INSTALLER STOPPED."
  exit 1
fi
if [ -r $FACTORY_PMBR_CODE ]; then
  INST_FLAGS="${INST_FLAGS} --pmbr_code $FACTORY_PMBR_CODE"
else
  log "ERROR: MISSING $FACTORY_PMBR_CODE ; PLEASE REBUILD FACTORY IMAGE."
  log "FACTORY INSTALLER STOPPED."
  exit 1
fi

/usr/sbin/chromeos-install $INST_FLAGS 2>&1 | cat >> "$MEMENTO_AU_LOG"

# Load the new partition table. The autoupdater has trouble with loop devices.
sync
echo 3 > /proc/sys/vm/drop_caches
/sbin/sfdisk -R "$DST_DRIVE"
partprobe "$DST_DRIVE" # inform the OS of partition table changes

log "Done preparing disk"

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
    DST="$(make_partition_dev ${DST_DRIVE} ${PART})"
  fi

  log "Factory Install: Installing $i image to $DST"

  CHANNEL_ARG=$(eval "echo \$${i}_CHANNEL_ARG")
  KPART="none"
  if [ "$i" = "FACTORY" -o "$i" = "RELEASE" ]; then
    # Set up kernel partition
    KPART=""
  fi

  EXTRA_ARG="--skip_postinst"
  if [ "$i" = "FACTORY" ]; then
    # Do postinst after update
    EXTRA_ARG=""
  fi

  RESULT="$(IS_FACTORY_INSTALL=1 \
    /opt/google/memento_updater/memento_updater.sh \
    --dst_partition "${DST}" --kernel_partition "${KPART}" \
    --allow_removable_boot $CHANNEL_ARG $EXTRA_ARG)"
  RETURNCODE="$?"

  # memento update has encountered a fatal error.
  if [ "${RETURNCODE}" != "0" ]; then
    log "Factory install of target ${DST} has failed."
    exit 1
  fi

  # Only updating the primary root/kernel partition is strictly required.
  # If the omahaserver is configured to not update others that's fine.
  if [ "${RESULT}" != "UPDATED" -a "$i" = "FACTORY" ]; then
    log "Factory Install: AU failed"
    exit 1
  fi
done

# Release image is not allowed to boot unless the factory test is passed
# otherwise the wipe and final verification can be skipped.
# TODO(hungte) do this in memento_updater or postinst may be better
# TODO(hungte) make a better way to find location of cgpt
if ! cgpt add -i $((${DST_RELEASE_PART} - 1)) -P 0 -T 0 -S 0 ${DST_DRIVE}; then
  log "Factory Install: failed to lock release image. Destroy all kernels."
  # Destroy kernels otherwise the system is still bootable.
  DST_RELEASE=$(make_partition_dev ${DST_DRIVE} $((${DST_RELEASE_PART} - 1)))
  DST_FACTORY=$(make_partition_dev ${DST_DRIVE} $((${DST_FACTORY_PART} - 1)))
  dd if=/dev/zero of=$DST_RELEASE
  dd if=/dev/zero of=$DST_FACTORY
  exit 1
fi

log "All done installing."

sleep 3
shutdown -r now
sleep 1d  # sleep indefinitely to avoid respawning rather than shutting down
