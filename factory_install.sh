#!/bin/sh -ex

# Copyright (c) 2011 The Chromium OS Authors. All rights reserved.
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.

. "$(dirname "$0")/chromeos-common.sh"
. "/opt/google/memento_updater/memento_updater_logging.sh"
. "/opt/google/memento_updater/find_omaha.sh"

# Definition of ChromeOS partition layout
DST_FACTORY_PART=3
DST_RELEASE_PART=5
DST_OEM_PART=8
DST_EFI_PART=12
DST_STATE_PART=1

# Override this if we need to perform additional commands
COMPLETE_SCRIPT=""

# Override this if we want to install with a board different from installer
BOARD=""

# Override this with console device for I/O
TTY="/dev/tty1"

# Global variables
DST_DRIVE=""

# Starting
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
on_die() {
  set +x  # prevent extra log in console
  colorize "red"
  log "
    ERROR: Factory installation has been stopped.
    Press Ctrl-Alt-F3 (Refresh) to get the detail information."
}

exit_success() {
  trap - EXIT
  exit 0
}

trap on_die EXIT

die() {
  set +x  # prevent extra log in console
  colorize "red"
  log "ERROR: $*"
  exit 1
}

clear_fwwp() {
  log "Firmware Write Protect disabled, clearing status registers."
  flashrom -p internal:bus=lpc --wp-disable
  flashrom -p internal:bus=spi --wp-disable
  log "WP registers should be cleared now"
}

clear_tpm() {
  log "Clearing TPM"

  # Reset TPM. tcsd needs to have not been run because it locks the TPM.
  tpmc ppon
  tpmc clear
  tpmc enable
  tpmc activate

  local firmware_index="0x1007"
  local firmware_struct_version="1"
  local firmware_flags="0"
  local firmware_fw_versions="1 0 1 0"
  local firmware_reserved="0 0 0 0"

  tpmc write $firmware_index $firmware_struct_version $firmware_flags \
      $firmware_fw_versions $firmware_reserved

  local kernel_index="0x1008"
  local kernel_struct_version="1"
  local kernel_uid="4c 57 52 47"
  local kernel_kernel_versions="1 0 1 0"
  local kernel_reserved="0 0 0 0"

  tpmc write $kernel_index $kernel_struct_version $kernel_uid \
      $kernel_kernel_versions $kernel_reserved

  log "Done clearing TPM"
}

set_time() {
  log "Setting time from:"
  # Extract only the server and port.
  local server_url="$(findLSBValue "CHROMEOS_AUSERVER" |
                      sed "s|/update||; s|http://||")"
  log " Server $server_url."
  local result="$(htpdate -s -t "$server_url" 2>&1)"
  if ! echo "$result" | grep -Eq "(failed|unavailable)"; then
    log "Success, time set to $(date)"
    hwclock -w 2>/dev/null
    return 0
  fi

  log "Failed to set time: $(echo "$result" | grep -E "(failed|unavailable)")"
  return 1
}

list_ethernet_interface() {
  local candidate
  local candidates="$(ifconfig | grep 'Link encap:Ethernet' | cut -d ' ' -f 1)"

  for candidate in $candidates; do
    # output if it is not a wifi interface
    if ! iw $candidate info >/dev/null 2>&1; then
      echo "$candidate"
    fi
  done
}

check_ethernet_status() {
  local ethernet_interface

  for ethernet_interface in $(list_ethernet_interface); do
    if ifconfig $ethernet_interface | grep -q "inet addr"; then
      log "$(ifconfig $ethernet_interface | grep 'inet addr')"
      return 0
    fi
  done

  return 1
}

reset_chromeos_device() {
  # Only ChromeOS machines have meaninful output here.
  if ! crossystem hwid >/dev/null 2>&1; then
    return 0
  fi

  log "Checking for Firmware Write Protect"
  # Check for physical firmware write protect. We'll only
  # clear this stuff if the case is open.
  if [ "$(crossystem wpsw_cur)" = "0" ]; then
    # Clear software firmware write protect.
    clear_fwwp
  fi

  log "Checking if TPM should be cleared"
  # To clear TPM, we need both software firmware write protect to be off, and
  # boot type as "recovery". Booting with USB in developer mode (Ctrl-U) does
  # not work.
  local tpm_is_cleared=""
  if ! crossystem "mainfw_type?recovery"; then
    mainfw_type="$(crossystem mainfw_type)"
    log " - System was not booted in recovery mode (current: $mainfw_type)."
  elif flashrom -p internal:bus=spi --wp-status 2>/dev/null |
       grep -q "write protect is enabled"; then
    log " - Firmware write protection is not disabled."
  else
    if ! clear_tpm; then
      die "Failed to clear TPM. Installation is stopped."
    else
      tpm_is_cleared=True
    fi
  fi

  if [ -z "$tpm_is_cleared" ]; then
    colorize "yellow"
    log "

    WARNING: TPM won't be cleared. To force clearing TPM, ensure firmware write
    protection is disabled, hold recovery button and reboot the system again.
    "
  fi
}

get_dst_drive() {
  case "$(crossystem arch)" in
    x86 )
      DST_DRIVE=/dev/sda
      ;;
    arm )
      DST_DRIVE=/dev/mmcblk0
      ;;
    * )
      die "Failed to auto detect architecture."
      ;;
  esac

  # Prevent writing to removable devices. (rootdev does not work here)
  local removable="/sys/block/$(basename $DST_DRIVE)/removable"
  if [ -f "$removable" ] && [ "$(cat $removable)" = 1 ]; then
    die "Cannot install to a removable device ($DST_DRIVE)."
  fi
}

lightup_screen() {
  # Light up screen in case you can't see our splash image.
  local script="/usr/sbin/lightup_screen"
  if [ -x "$script" ]; then
    $script
  else
    log "$script does not exist or not executable"
  fi
}

prepare_disk() {
  log "Factory Install: Setting partition table"

  local gpt_layout="/root/.gpt_layout"
  local pmbr_code="/root/.pmbr_code"
  local inst_flags="--dst $DST_DRIVE --skip_rootfs --run_as_root --yes"

  # In current design, we need both gpt layout and "pmbr code" always generated.
  [ -s $gpt_layout ] || die "Missing $gpt_layout; please rebuild image."
  [ -r $pmbr_code ] || die "Missing $pmbr_code; please rebuild image."
  inst_flags="$inst_flags --gpt_layout $gpt_layout --pmbr_code $pmbr_code"

  /usr/sbin/chromeos-install $inst_flags >>"$MEMENTO_AU_LOG" 2>&1

  # Informs OS of partition table changes. The autoupdater has trouble with loop
  # devices.
  log "Reloading partition table changes..."
  sync
  echo 3 >/proc/sys/vm/drop_caches
  # sfdisk works fine on x86, and partprobe works for ARM.
  sfdisk -R "$DST_DRIVE"
  partprobe "$DST_DRIVE"

  log "Done preparing disk"
}

select_board() {
  # Prompt the user if USER_SELECT is true.
  local user_select="$(findLSBValue "USER_SELECT" | tr '[:upper:]' '[:lower:]')"
  if [ "$user_select" = "true" ]; then
    echo -n "Enter the board you want to install (ex: x86-mario): " >"$TTY"
    read BOARD <"$TTY"
  fi
}

disable_release_partition() {
  # Release image is not allowed to boot unless the factory test is passed
  # otherwise the wipe and final verification can be skipped.
  # TODO(hungte) do this in memento_updater or postinst may be better
  if ! cgpt add -i $(($DST_RELEASE_PART - 1)) -P 0 -T 0 -S 0 "$DST_DRIVE"
  then
    # Destroy kernels otherwise the system is still bootable.
    dst="$(make_partition_dev $DST_DRIVE $(($DST_RELEASE_PART - 1)))"
    dd if=/dev/zero of=$dst bs=1M count=1
    dst="$(make_partition_dev $DST_DRIVE $(($DST_FACTORY_PART - 1)))"
    dd if=/dev/zero of=$dst bs=1M count=1
    die "Failed to lock release image. Destroy all kernels."
  fi
}

factory_on_complete() {
  if [ ! -s "$COMPLETE_SCRIPT" ]; then
    return 0
  fi

  log "Executing completion script... ($COMPLETE_SCRIPT)"
  if ! sh "$COMPLETE_SCRIPT" "$DST_DRIVE" >"$MEMENTO_AU_LOG" 2>&1; then
    die "Failed running completion script $COMPLETE_SCRIPT."
  fi
  log "Completion script executed successfully."
}


factory_reset() {
  log "Performing factory reset"
  if ! /usr/sbin/factory_reset.sh; then
    die "Factory reset failed."
  fi

  log "Done."
  # TODO(hungte) shutdown or reboot once we decide the default behavior.
  exit_success
}

factory_omaha_install() {
  local i=""
  local result=""
  local return_code=""
  local dst=""
  local dst_arg=""

  # Channels defined by memento updater
  FACTORY_CHANNEL_ARG='--force_track=factory-channel'
  RELEASE_CHANNEL_ARG='--force_track=release-channel'
  OEM_CHANNEL_ARG='--force_track=oempartitionimg-channel'
  EFI_CHANNEL_ARG='--force_track=efipartitionimg-channel'
  STATE_CHANNEL_ARG='--force_track=stateimg-channel'
  FIRMWARE_CHANNEL_ARG='--force_track=firmware-channel'
  HWID_CHANNEL_ARG='--force_track=hwid-channel'
  COMPLETE_CHANNEL_ARG='--force_track=complete-channel'

  # Special channels for execution
  DST_FIRMWARE_PART="$(mktemp --tmpdir "fw_XXXXXXXX")"
  DST_HWID_PART="$(mktemp --tmpdir "hwid_XXXXXXXX")"
  DST_COMPLETE_PART="$(mktemp --tmpdir "complete_XXXXXXXX")"

  # Install the partitions
  for i in EFI OEM STATE RELEASE FACTORY FIRMWARE HWID COMPLETE; do
    # DST_*_PART can be a numeric partition number or plain file.
    local part="$(eval 'echo $DST_'${i}'_PART')"

    if [ -z "$part" ]; then
      die "INVALID CHANNEL: $i."
    elif echo "$part" | grep -qs "^[0-9]*$"; then
      dst="$(make_partition_dev $DST_DRIVE $part)"
      dst_arg=""
    else
      dst="$part"
      dst_arg="--nocheck_block_device"
    fi

    log "Factory Install: Installing $i image to $dst"

    local channel_arg="$(eval 'echo $'${i}'_CHANNEL_ARG')"
    local kpart="none"
    if [ "$i" = "FACTORY" -o "$i" = "RELEASE" ]; then
      # Set up kernel partition
      kpart=""
    fi

    local extra_arg="--skip_postinst"
    if [ "$i" = "FACTORY" ]; then
      # Do postinst after update
      extra_arg=""
    fi

    if [ -n "$BOARD" ]; then
      extra_arg="$extra_arg --board=$BOARD"
    fi

    return_code=0
    result="$(IS_FACTORY_INSTALL=1 \
      /opt/google/memento_updater/memento_updater.sh \
      --dst_partition "$dst" --kernel_partition "$kpart" \
      --allow_removable_boot $channel_arg $dst_arg $extra_arg)" ||
      return_code="$?"

    if [ "$i" = "RELEASE" ]; then
      disable_release_partition
    fi

    # Check the result
    if [ "$return_code" != "0" ]; then
      # memento update has encountered a fatal error.
      die "Factory install of target $dst has failed with error $return_code."
    elif [ "$result" != "UPDATED" -a "$i" = "FACTORY" ]; then
      # Only updating the primary root/kernel partition is strictly required.
      # If the omahaserver is configured to not update others that's fine.
      die "AU failed."
    fi
  done

  # Post-processing channels in self-executable file.
  if [ -s "$DST_FIRMWARE_PART" ]; then
    log "Execute firmware-install script"
    dst="$DST_FIRMWARE_PART"
    dst_arg="--force --mode=factory_install"
    if ! sh "$dst" $dst_arg >"$MEMENTO_AU_LOG" 2>&1; then
      die "Firmware updating failed."
    fi
  fi
  if [ -s "$DST_HWID_PART" ]; then
    log "Execute HWID component list updater script"
    dst="$DST_HWID_PART"
    dst_arg="$(make_partition_dev $DST_DRIVE $DST_STATE_PART)"
    if ! sh "$dst" "$dst_arg" >"$MEMENTO_AU_LOG" 2>&1; then
      die "HWID component list updating failed."
    fi
  fi
  if [ -s "$DST_COMPLETE_PART" ]; then
    log "Found completion script."
    COMPLETE_SCRIPT="$DST_COMPLETE_PART"
  fi
}

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

main() {
  if [ "$(id -u)" -ne 0 ]; then
    echo "You must run this as root."
    exit 1
  fi

  log "Starting Factory Installer."
  lightup_screen

  colorize "green"
  reset_chromeos_device

  colorize "yellow"
  log "Waiting for ethernet connectivity to install"
  log "Or disable developer mode to factory reset."
  while true; do
    if check_ethernet_status; then
      break
    elif [ "$(crossystem devsw_cur)" = "0" ]; then
      factory_reset
    else
      sleep 1
    fi
  done

  # TODO(hungte) how to set time in RMA?
  colorize "green"
  set_time || die "Please check if the server is configured correctly."

  get_dst_drive
  prepare_disk
  select_board

  # TODO(hungte) Support installing from USB
  factory_omaha_install

  log "Factory Installer Complete."
  factory_on_complete

  # Default action after installation: reboot.
  trap - EXIT
  sleep 3
  shutdown -r now

  # sleep indefinitely to avoid re-spawning rather than shutting down
  sleep 1d
}

main "$@"
