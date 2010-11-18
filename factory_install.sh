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
      $firmware_fw_versions $firmware_reserved

  kernel_index="0x1008"
  kernel_struct_version="1"
  kernel_uid="4c 57 52 47"
  kernel_kernel_versions="1 0 1 0"
  kernel_reserved="0 0 0 0"

  tpmc write $kernel_index $kernel_struct_version $kernel_uid \
      $kernel_kernel_versions $kernel_reserved

  log "Done clearing TPM"
}

log "Starting Factory Installer."

log "Checking for Firmware Write Protect"

# Only ChromeOS machines have this setting.
if [ -e /sys/devices/platform/chromeos_acpi/CHSW ]; then
  # Check for physical firmware write protect. We'll only
  # clear this stuff if the case is open.
  CHROME_SWITCHES=/sys/devices/platform/chromeos_acpi/CHSW
  if [ $(( $(cat "${CHROME_SWITCHES}") & 512 )) = 512 ]; then
    # Ensure that flash chips are in a known good state.
    clear_fwwp

    # Ensure that we can wipe TPM if necessary.
    clear_tpm
  fi
fi

log "Pause to ensure ethernet connectivity"
while ! ifconfig eth0 | grep -q "inet addr"; do
  sleep 1
done
log "$(ifconfig eth0 | grep 'inet addr')"

# TODO(adlr): pick an install device in a matter that works on x86 and ARM.
# This works on x86 only, afaik.
DST_DRIVE=/dev/sda
DST_FACTORY_PART=3
DST_RELEASE_PART=5
DST_OEM_PART=8
DST_EFI_PART=12
DST_STATE_PART=1
DST_FIRMWARE=/tmp/firmware.sh

# Light up screen in case you can't see our splash image.
LIGHTUP_SCREEN="/usr/sbin/lightup_screen"
if [ -x /usr/sbin/lightup_screen ]; then
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
    DST="${DST_DRIVE}${PART}"
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
  dd if=/dev/zero of=${DST_DRIVE}$((${DST_RELEASE_PART} - 1))
  dd if=/dev/zero of=${DST_DRIVE}$((${DST_FACTORY_PART} - 1))
  exit 1
fi

log "All done installing."

sleep 3
shutdown -r now
sleep 1d  # sleep indefinitely to avoid respawning rather than shutting down
