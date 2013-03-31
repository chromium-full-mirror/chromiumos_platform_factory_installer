#!/bin/sh -ex

# Copyright (c) 2012 The Chromium OS Authors. All rights reserved.
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.

. "$(dirname "$0")/chromeos-common.sh"
. "$(dirname "$0")/ping_shopfloor.sh"
. "/opt/google/memento_updater/memento_updater_logging.sh"
. "/opt/google/memento_updater/find_omaha.sh"

# Definition of ChromeOS partition layout
DST_FACTORY_KERNEL_PART=2
DST_FACTORY_PART=3
DST_RELEASE_KERNEL_PART=4
DST_RELEASE_PART=5
DST_OEM_PART=8
DST_EFI_PART=12
DST_STATE_PART=1

# Override this if we need to perform additional commands
COMPLETE_SCRIPT=""

# Override this if we want to install with a board different from installer
BOARD=""

# Override this if we want to install with a different omaha server.
OMAHA="$(findLSBValue CHROMEOS_AUSERVER)"

# Override this with console device for I/O
TTY="/dev/tty1"

# Global variables
DST_DRIVE=""
EC_PRESENT=0
DEVSW_PRESENT=1
NETBOOT_RAMFS=""

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

config_tty() {
  stty -F "$TTY" opost
}

clear_fwwp() {
  log "Firmware Write Protect disabled, clearing status registers."
  if [ $EC_PRESENT -eq 1 ]; then
    flashrom -p internal:bus=lpc --wp-disable
  fi
  flashrom -p internal:bus=spi --wp-disable
  log "WP registers should be cleared now"
}

ensure_fwwp_consistency() {
  local ec_wp main_wp

  if [ $EC_PRESENT -eq 0 ]; then
    return
  fi

  ec_wp="$(flashrom -p internal:bus=lpc --wp-status 2>/dev/null)" || return
  main_wp="$(flashrom -p internal:bus=spi --wp-status 2>/dev/null)"
  ec_wp="$(echo "$ec_wp" | sed -nr 's/WP.*(disabled|enabled).$/\1/pg')"
  main_wp="$(echo "$main_wp" | sed -nr 's/WP.*(disabled|enabled).$/\1/pg')"
  if [ "$ec_wp" != "$main_wp" ]; then
    die "Inconsist firmware write protection status: main=$main_wp, ec=$ec_wp."\
        "Please disable Hardware Write Protection and restart again."
  fi
}

check_fwwp() {
  flashrom -p internal:bus=spi --wp-status 2>/dev/null |
    grep -q "write protect is enabled"
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
  local time_server_url="$(echo "$OMAHA" | sed "s|/update||; s|http://||")"

  log " Server $time_server_url."
  local result="$(htpdate -s -t "$time_server_url" 2>&1)"
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
  log "Clearing NVData."
  if ! mosys nvram clear; then
    # Not every platforms really need this - OK if nvram is not cleared.
    log "Warning: NVData not cleared."
  fi

  if grep -q cros_netboot /proc/cmdline; then
    log "Device is network booted."
    return
  fi

  if crossystem "mainfw_type?nonchrome"; then
    # Non-ChromeOS firmware devices can stop now.
    log "Device running Non-ChromeOS firmware."
    return
  fi

  log "Checking for Firmware Write Protect"
  # Check for physical firmware write protect. We'll only
  # clear this stuff if the case is open.
  if [ "$(crossystem wpsw_cur)" = "0" ]; then
    # Clear software firmware write protect.
    clear_fwwp
  fi

  log "Checking if TPM should be cleared (for version and owner)"
  # To clear TPM, we need it unlocked (only in recovery boot).
  # Booting with USB in developer mode (Ctrl-U) does not work.
  if crossystem "mainfw_type?recovery"; then
    clear_tpm
  else
    mainfw_type="$(crossystem mainfw_type)"
    colorize "yellow"
    log " - System was not booted in recovery mode (current: $mainfw_type).

    WARNING: TPM won't be cleared. To enforce clearing TPM, make sure you are
    using correct image signed with same key (MP, Pre-MP, or DEV), turn on
    developer switch if you haven't, then hold recovery button and reboot the
    system again.  Ctrl-U won't clear TPM.
    "
    # Alert for a while
    sleep 3
  fi
  ensure_fwwp_consistency
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

load_modules() {
  # Required kernel modules might not be loaded. Load them now.
  modprobe i2c-dev || true
}

prepare_disk() {
  log "Factory Install: Setting partition table"

  local pmbr_code="/root/.pmbr_code"
  [ -r $pmbr_code ] || die "Missing $pmbr_code; please rebuild image."

  . "/usr/sbin/write_gpt.sh"
  write_base_table "${DST_DRIVE}" "${pmbr_code}" >>"$MEMENTO_AU_LOG" 2>&1

  # Informs OS of partition table changes. The autoupdater has trouble with loop
  # devices.
  log "Reloading partition table changes..."
  sync
  sleep 3  # Wait for sync to take place.
  partprobe "$DST_DRIVE"

  log "Done preparing disk"
}

select_board() {
  # Prompt the user if USER_SELECT is true.
  local user_select="$(findLSBValue USER_SELECT | tr '[:upper:]' '[:lower:]')"
  if [ "$user_select" = "true" ]; then
    echo -n "Enter the board you want to install (ex: x86-mario): " >"$TTY"
    read BOARD <"$TTY"
  fi
}

find_var() {
  # Check kernel commandline for a specific key value pair.
  # Usage: omaha=$(find_var omahaserver)
  # Assume values are space separated, keys are unique within the commandline,
  # and that keys and values do not contain spaces.
  local key="$1"

  for item in $(cat /proc/cmdline); do
    if echo "$item" | grep -q "$key"; then
      echo "$item" | cut -d'=' -f2
      return 0
    fi
  done
  return 1
}

override_from_firmware() {
  # Check for Omaha URL or Board type from kernel commandline.
  # OMAHA and BOARD are override env variables used when calling
  # memento_updater.
  local omaha=""
  if omaha="$(find_var omahaserver)"; then
    log " Kernel cmdline OMAHA override to $omaha"
    OMAHA="$omaha"
  fi

  local board=""
  if board="$(find_var cros_board)"; then
    log " Kernel cmdline BOARD override to $board"
    BOARD="$board"
  fi
}

override_from_board() {
  # Call into any board specific configuration settings we may need.
  # These will be provided by chromeos-bsp-[board] build in the private overlay.
  local lastboard="$BOARD"
  if [ -f "/usr/sbin/board_customize_install.sh" ]; then
    . /usr/sbin/board_customize_install.sh
  fi

  # Let's notice if BOARD has changed and print a message.
  if [ "$lastboard" != "$BOARD" ]; then
    colorize "red"
    log " Private overlay customization BOARD override to $BOARD"
    sleep 1
  fi
}

overrides() {
  override_from_firmware
  override_from_board
}

bringup_network() {
  # Probe USB Ethernet devices.
  local module
  local module_name
  for module in $(find /lib/modules/ -name "*.ko"); do
    module_name="$(basename ${module%.ko})"
    modprobe ${module_name}
  done

  # Try to bring up network and get an IP address.
  ifconfig eth0 up
  udhcpc -t 5 -f -q -s /etc/udhcpc.script
}

disable_release_partition() {
  # Release image is not allowed to boot unless the factory test is passed
  # otherwise the wipe and final verification can be skipped.
  # TODO(hungte) do this in memento_updater or postinst may be better
  if ! cgpt add -i "$DST_RELEASE_KERNEL_PART" -P 0 -T 0 -S 0 "$DST_DRIVE"
  then
    # Destroy kernels otherwise the system is still bootable.
    dst="$(make_partition_dev $DST_DRIVE $DST_RELEASE_KERNEL_PART)"
    dd if=/dev/zero of=$dst bs=1M count=1
    dst="$(make_partition_dev $DST_DRIVE $DST_FACTORY_KERNEL_PART)"
    dd if=/dev/zero of=$dst bs=1M count=1
    die "Failed to lock release image. Destroy all kernels."
  fi
}

run_postinst() {
  local install_dev="$1"
  local mount_point="$(mktemp -d)"
  local result=0

  mount -t ext2 -o ro "$install_dev" "$mount_point"
  IS_FACTORY_INSTALL=1 "$mount_point"/postinst \
    "$install_dev" >>"$MEMENTO_AU_LOG" 2>&1 || result="$?"

  umount "$install_dev" || true
  rmdir "$mount_point" || true
  return $result
}

run_netboot_postinst()
{
  # For netboot, handle post-processing channels in chroot in installed
  # environment.
  INSTALLED_ROOT="$1"
  INSTALLED_STATEFUL="$2"
  HWID_SCRIPT="$3"
  FIRMWARE_SCRIPT="$4"

  CHROOT_PATH="$(mktemp -d /tmp/chroot_XXXXXXXX)"
  POST_PROCESS_SCRIPT=/mnt/stateful_partition/netboot_postinst.sh
  HWID_COPIED_PATH=/mnt/stateful_partition/hwid_bundle.sh
  FIRMWARE_COPIED_PATH=/mnt/stateful_partition/firmware_install
  mount -o ro -t ext2 "$INSTALLED_ROOT" "$CHROOT_PATH"
  mount "$INSTALLED_STATEFUL" "$CHROOT_PATH"/mnt/stateful_partition
  if [ -s "$HWID_SCRIPT" ]; then
    cp "$HWID_SCRIPT" "${CHROOT_PATH}${HWID_COPIED_PATH}"
  fi
  if [ -s "$FIRMWARE_SCRIPT" ]; then
    cp "$FIRMWARE_SCRIPT" "${CHROOT_PATH}${FIRMWARE_COPIED_PATH}"
  fi
  cp -p /bin/netboot_postinst.sh "${CHROOT_PATH}${POST_PROCESS_SCRIPT}"
  chroot "$CHROOT_PATH" "$POST_PROCESS_SCRIPT"
  umount "$CHROOT_PATH"/mnt/stateful_partition || true
  umount "$CHROOT_PATH" || true
  sync
}

run_firmware_update() {
  local install_drive="$1"
  local install_dev=""
  local mount_point="$(mktemp -d)"
  local result=0
  local updater="$(findLSBValue FACTORY_INSTALL_FIRMWARE)"
  local stateful_updater="${updater#/mnt/stateful_partition/}"
  local mount_opt=""

  # If there's nothing assigned, we should load firmware from release rootfs;
  # otherwise follow the assigned location (currently only stateful partition is
  # supported).
  if [ -z "$updater" ]; then
    updater="$mount_point/usr/sbin/chromeos-firmwareupdate"
    install_dev="$(make_partition_dev "$install_drive" "$DST_RELEASE_PART")"
    mount_opt="-t ext2 -o ro"
  elif [ "$updater" != "$stateful_updater" ]; then
    updater="$mount_point/$stateful_updater"
    install_dev="$(make_partition_dev "$install_drive" "$DST_STATE_PART")"
    # Stateful partition may be ext4 and turned off ext2 compatibility mode.
    mount_opt="-o ro"
  else
    die "Unknown firmware updater location: $updater"
  fi

  log "Running firmware updater from $install_dev ($updater)..."
  mount $mount_opt "$install_dev" "$mount_point"
  # If write protection is disabled, perform factory (RO+RW) firmware setup;
  # otherwise run updater in recovery (RW only) mode.
  if ! check_fwwp; then
    "$updater" --force --mode=factory_install >>"$MEMENTO_AU_LOG" 2>&1 ||
      result="$?"
  else
    # We need to recover the firmware and then enable developer firmware.
    "$updater" --force --mode=recovery >>"$MEMENTO_AU_LOG" 2>&1 || result="$?"
    # For two-stop firmware, todev is a simple crossystem call; but for other
    # old firmware (alex/zgb), todev will perform flashrom and then reboot.
    # So this must be the very end command.
    "$updater" --force --mode=todev >>"$MEMENTO_AU_LOG" 2>&1 || result="$?"
  fi

  umount "$mount_point" || true
  rmdir "$mount_point" || true
  return $result
}

save_omaha_url() {
  log "Save active Omaha server URL to stateful partition"
  local stateful_dev="$1"
  local mount_point="$(mktemp -d)"
  local output_file="$mount_point/dev_image/etc/lsb-factory"

  mount "$stateful_dev" "$mount_point"
  mkdir -p "$(dirname "$output_file")"
  echo "FACTORY_OMAHA_URL=$OMAHA" >> "$output_file"

  # Sometimes lsb-factory is not written yet before umount.
  sync;sync;sync;

  umount "$mount_point" || true
  rmdir "$mount_point" || true
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
  # Turn off developer mode on devices without physical developer switch.
  if [ $DEVSW_PRESENT -eq 0 ]; then
    crossystem disable_dev_request=1
  fi

  log "Performing factory reset"
  if ! /usr/sbin/factory_reset.sh; then
    die "Factory reset failed."
  fi

  log "Done."
  # TODO(hungte) shutdown or reboot once we decide the default behavior.
  exit_success
}

factory_install_usb() {
  local i=""
  local src_offset="$(findLSBValue FACTORY_INSTALL_USB_OFFSET)"
  local src_drive="$(findLSBValue REAL_USB_DEV)"
  # REAL_USB_DEV is optional on systems without initramfs (ex, ARM).
  [ -n "$src_drive" ] || src_drive="$(rootdev -s 2>/dev/null)"
  [ -n "$src_drive" ] || die "Unknown media source. Please define REAL_USB_DEV."

  # Finds the real drive from sd[a-z][0-9]* or mmcblk[0-9]*p[0-9]*
  src_drive="${src_drive%[0-9]*}"
  src_drive="$(echo $src_drive | sed 's/\(mmcblk[0-9]*\)p/\1/')"

  colorize "yellow"
  for i in EFI OEM STATE FACTORY FACTORY_KERNEL RELEASE RELEASE_KERNEL; do
    # The source media must have exactly the same layout.
    local part="$(eval 'echo $DST_'${i}'_PART')"
    local src="$(make_partition_dev $src_drive $part)"
    local dst="$(make_partition_dev $DST_DRIVE $part)"

    # Factory/Release may be shifted on source media.
    if [ -n "$src_offset" ]; then
      case "$i" in
        FACTORY* | RELEASE* )
          src="$(make_partition_dev $src_drive $((part + src_offset)) )"
          true
          ;;
      esac
    fi

    # Detect file system size
    local dd_param="bs=1M"
    local count="$(dumpe2fs -h "$src" 2>/dev/null |
                   grep "^Block count" |
                   sed 's/.*: *//')"

    if [ -n "$count" ]; then
      local bs="$(dumpe2fs -h "$src" 2>/dev/null |
            grep "^Block size" |
            sed 's/.*: *//')"

      # Optimize copy speed, with restriction: bs<10M
      while [ "$(( (count > 0) &&
                   (count % 2 == 0) &&
                   (bs / 1048576 < 10) ))" = "1" ]; do
        count="$((count / 2))"
        bs="$((bs * 2))"
      done
      dd_param="bs=$bs count=$count"
    fi

    log "Copying: [$i] $src -> $dst ($dd_param)"
    # TODO(hungte) Detect copy failure
    pv -B 1M "$src" 2>"$TTY" |
      dd $dd_param of="$dst" iflag=fullblock oflag=dsync
  done
  colorize "green"

  # Disable release partition and activate factory partition
  disable_release_partition
  run_postinst "$(make_partition_dev "$DST_DRIVE" "$DST_FACTORY_PART")"
  run_firmware_update "$DST_DRIVE"
}

factory_install_omaha() {
  local i=""
  local result=""
  local return_code=""
  local dst=""
  local dst_arg=""
  local src_url=""
  local src_checksum=""
  local ping_result=""

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
  DST_FIRMWARE_PART="$(mktemp "/tmp/fw_XXXXXXXX")"
  DST_HWID_PART="$(mktemp "/tmp/hwid_XXXXXXXX")"
  DST_COMPLETE_PART="$(mktemp "/tmp/complete_XXXXXXXX")"

  # Ping shopfloor server
  ping_result=$(ping_shopfloor 2>/dev/null) || return_code="$?"
  if [ "$return_code" = "" ]; then
    eval "$ping_result"
  fi

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

    if [ -n "$OMAHA" ]; then
      extra_arg="$extra_arg --omaha_url=$OMAHA"
    fi

    if [ "$SHOPFLOOR_INSTALL" = "1" ]; then
      src_url="$(eval 'echo $SRC_'${i}'_URL')"
      src_checksum="$(eval 'echo $SRC_'${i}'_CHECKSUM')"
      extra_arg="$extra_arg --install_url=$src_url"
      extra_arg="$extra_arg --install_url_checksum=$src_checksum"
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
  if [ -n "$NETBOOT_RAMFS" ]; then
    run_netboot_postinst \
      "$(make_partition_dev $DST_DRIVE 3)" \
      "$(make_partition_dev $DST_DRIVE 1)" \
      "${DST_HWID_PART}" \
      "${DST_FIRMWARE_PART}"
  else
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
  fi

  # Update omaha information into stateful partition.
  save_omaha_url "$(make_partition_dev $DST_DRIVE $DST_STATE_PART)" \
    >"$MEMENTO_AU_LOG" 2>&1 || die "Failed to save OMAHA server information."

  if [ -s "$DST_COMPLETE_PART" ]; then
    log "Found completion script."
    COMPLETE_SCRIPT="$DST_COMPLETE_PART"
  fi
}

test_ec_flash_presence() {
  # If "flashrom -p internal:bus=lpc --get-size" command succeeds (returns 0),
  # then EC flash chip is present in system. Otherwise, assume EC flash is not
  # present or supported.
  if flashrom -p internal:bus=lpc --get-size 1>&2; then
    EC_PRESENT=1
    log "EC flash is present"
  else
    EC_PRESENT=0
    log "EC flash is not present or supported"
  fi
}

test_devsw_presence() {
  local VBSD_HONOR_VIRT_DEV_SWITCH="0x400"
  local vdat_flags="$(crossystem vdat_flags || echo 0)"

  if [ "$((vdat_flags & VBSD_HONOR_VIRT_DEV_SWITCH))" = "0" ]; then
    DEVSW_PRESENT=1
    log "Physical developer switch is present"
  else
    DEVSW_PRESENT=0
    log "Using virtual developer switch (no physical switch)"
  fi
}

# Prints the two byte hex code of the matched char or
# exists non-zero on timeout.  It reads from the current tty.
# To override, call it as follows,
#   match_char_timeout args < "$STDOUT"
#
# Arguments: time_in_seconds two_byte_hex_match_1 two_byte_hex_match_2 ...
match_char_timeout() {
  local delay_secs="$1"
  shift

  local input=''
  local match=''
  local start_time=$(date +%s)
  local stop_time=$((start_time + delay_secs))
  local tty_config=$(stty -g)
  stty raw -echo
  while [ $delay_secs -gt 0 ]; do
    local ret_code=0
    input=$(timeout -s KILL ${delay_secs}s head -c 1) || ret_code="$?"
    [ $ret_code -eq 137 ] && break  # Timed out.
    input=$(printf "%02x" "'$input")
    for char in "$@"; do
      if [ "$input" = "$char" ]; then
        match="$input"
        break
      fi
    done
    [ -n "$match" ] && break
    delay_secs=$((stop_time - $(date +%s) ))
  done
  # Restores the tty's settings.
  stty "$tty_config"

  [ -z "$match" ] && return 1
  printf "$match"
  return 0
}

init_tps65090() {
  ectool i2cwrite 8 0 0x90 0x5 0x3e
  ectool i2cwrite 8 0 0x90 0x6 0x5e
  ectool i2cwrite 8 0 0x90 0x7 0xbe
  ectool i2cwrite 8 0 0x90 0x8 0xfe
  ectool i2cwrite 8 0 0x90 0x9 0xff
  # log "============================================="
  # log "========== Re-plug AC adapter now. =========="
  # log "============================================="
  # while (ectool powerinfo | grep Device | grep -v -q 0x0) ; do
  #   sleep 0.5;
  # done
  # while (ectool powerinfo | grep Device | grep -v -q 0x20010) ; do
  #   sleep 0.5;
  # done
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

  config_tty

  log "Starting Factory Installer."
  lightup_screen

  log "Initializing TPS65090."
  init_tps65090

  load_modules

  colorize "green"

  test_ec_flash_presence
  test_devsw_presence

  reset_chromeos_device

  local install_from_omaha="1"
  if [ "$(findLSBValue FACTORY_INSTALL_FROM_USB)" = "1" ]; then
    install_from_omaha=""
  fi

  if [ "$(findLSBValue NETBOOT_RAMFS)" = "1" ]; then
    NETBOOT_RAMFS="1"
  fi

  # Check for any configuration overrides.
  overrides

  if [ -n "$install_from_omaha" ]; then

    colorize "yellow"
    log "Waiting for ethernet connectivity to install"
    if [ $DEVSW_PRESENT -eq 1 ]; then
      log "Or disable developer mode to factory reset."
    elif [ $DEVSW_PRESENT -eq 0 ]; then
      log "Or press Ctrl-R to factory reset."
    fi

    while true; do
      if [ -n "$NETBOOT_RAMFS" ]; then
        # For initramfs network boot, there is no upstart job. We have to
        # bring up network interface and get IP address from DHCP on our own.
        # The network interface may not be ready, so let's ignore any
        # error here.
        bringup_network || true
      fi
      if check_ethernet_status; then
        break
      elif [ $DEVSW_PRESENT -eq 1 -a "$(crossystem devsw_cur)" = "0" ]; then
        factory_reset
      elif [ $DEVSW_PRESENT -eq 0 ]; then
        local input=$(match_char_timeout 1 12 < "$TTY")
        if [ "$input" = "12" ]; then
          factory_reset
        fi
      else
        sleep 1
      fi
    done

    # TODO(hungte) how to set time in RMA?
    set_time || die "Please check if the server is configured correctly."
  fi

  colorize "green"
  get_dst_drive
  prepare_disk
  select_board

  if [ -n "$install_from_omaha" ]; then
    factory_install_omaha
  else
    factory_install_usb
  fi

  log "Factory Installer Complete."
  factory_on_complete

  # Default action after installation: reboot.
  trap - EXIT
  sleep 3
  if [ -n "$NETBOOT_RAMFS" ]; then
    # There is no 'shutdown' and 'init' in initramfs.
    busybox reboot -f
  else
    shutdown -r now
  fi

  # sleep indefinitely to avoid re-spawning rather than shutting down
  sleep 1d
}

main "$@"
