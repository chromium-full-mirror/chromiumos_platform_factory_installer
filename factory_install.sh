#!/bin/bash

# Copyright (c) 2014 The Chromium OS Authors. All rights reserved.
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.

set -e

# TTY which will get a copy of bash traces, and at the end a pager to
# look through the trace if things fail.
TRACE_TTY=4

# Dump bash trace to /var/log/factory_install.log.
LOG_FILE=/var/log/factory_install.log
mkdir -p $(dirname "$LOG_FILE")
exec {LOG_FD}>"$LOG_FILE"
tail -f "$LOG_FILE" > /dev/tty"$TRACE_TTY" &
export BASH_XTRACEFD="$LOG_FD"

. "$(dirname "$0")/chromeos-common.sh"
. "$(dirname "$0")/ping_shopfloor.sh"
. "/opt/google/memento_updater/memento_updater_logging.sh"
. "/opt/google/memento_updater/find_omaha.sh"

# Tail memento output to both our stdout and the log file, since
# there's no better way to display of download/install for individual
# partitions.
tail -F "$MEMENTO_AU_LOG" 2>/dev/null | tee -a "$LOG_FILE" &

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
RELEASE_ONLY="$(findLSBValue RELEASE_ONLY)"

# Global variables
DST_DRIVE=""
EC_PRESENT=0
DEVSW_PRESENT=1
NETBOOT_RAMFS=""

# Supported actions (a set of lowercase characters)
SUPPORTED_ACTIONS=irsvz

# Each action x is implemented in an action_$x handler (e.g.,
# action_i); see the handlers for more information about what
# each option is.

# Mount point for installer resources.
RESOURCES_MOUNT_POINT="$(mktemp -d)"
USB_INSTALL_OFFSET="$(findLSBValue FACTORY_INSTALL_USB_OFFSET)"

# Define our own logging function.  The one from memento_updater writes to
# a file, which may cause a race condition if we tail it.
log() {
  echo "$*"
}

# Change color in tty1 by ANSI escape sequence code
colorize() {
  set +x
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
    "white" )
      code="0;37"
      ;;
    "boldwhite" )
      code="1;37"
      ;;
  esac
  printf "\033[%sm" "$code"
  set -x
}

# Error message for any unexpected error.
on_die() {
  set +x
  kill_bg_jobs
  colorize red
  echo
  log "ERROR: Factory installation has been stopped."
  log "See tty$TRACE_TTY (Ctrl-Alt-F$TRACE_TTY) for detailed information."
  log "(The F$TRACE_TTY key is $TRACE_TTY keys to the right of the 'esc' key.)"
  busybox openvt -c "$TRACE_TTY" sh -c "clear; exec secure_less.sh < $LOG_FILE"
}

kill_bg_jobs() {
  pids=$(jobs -p)
  # Disown all background jobs to avoid terminated messages
  disown -a
  # Kill the jobs
  echo "$pids" | xargs -r kill -9 2>/dev/null || true
}

exit_success() {
  trap - EXIT
  kill_bg_jobs
  exit 0
}

trap on_die EXIT

die() {
  set +x
  colorize red
  set +x
  log "ERROR: $*"
  kill_bg_jobs
  exit 1
}

config_tty() {
  stty opost
}

clear_fwwp() {
  log "Firmware Write Protect disabled, clearing status registers."
  if [ $EC_PRESENT -eq 1 ]; then
    flashrom -p ec --wp-disable
  fi
  flashrom -p host --wp-disable
  log "WP registers should be cleared now"
}

ensure_fwwp_consistency() {
  local ec_wp main_wp

  if [ $EC_PRESENT -eq 0 ]; then
    return
  fi

  ec_wp="$(flashrom -p ec --wp-status 2>/dev/null)" || return
  main_wp="$(flashrom -p host --wp-status 2>/dev/null)"
  ec_wp="$(echo "$ec_wp" | sed -nr 's/WP.*(disabled|enabled).$/\1/pg')"
  main_wp="$(echo "$main_wp" | sed -nr 's/WP.*(disabled|enabled).$/\1/pg')"
  if [ "$ec_wp" != "$main_wp" ]; then
    die "Inconsist firmware write protection status: main=$main_wp, ec=$ec_wp."\
        "Please disable Hardware Write Protection and restart again."
  fi
}

check_fwwp() {
  flashrom -p host --wp-status 2>/dev/null |
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

check_ethernet_status() {
  local link i
  link=$(ip -f link addr | sed 'N;s/\n/ /' | grep -w 'ether' |
    cut -d ' ' -f 2 | sed 's/://')
  for i in $link; do
    if ip -f inet addr show $i | grep -q inet; then
      if ! iw $i info >/dev/null 2>&1; then
        log "$(ip -f inet addr show $i | grep inet)"
        return 0
      fi
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
    colorize yellow
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

# Find a non-removable /dev/sd* or /dev/mmcblk* device to install to.
# Largely copied from recovery_init.sh. See http://crbug.com/336654.
get_dst_drive() {
  local num_dev=0
  local dev

  DST_DRIVE=""
  for dev in /sys/block/sd* /sys/block/mmcblk*; do
    if [ ! -d "${dev}" ]; then
      continue
    fi
    # Do not install to removable devices.
    if [ "$(cat "${dev}/removable")" = 1 ]; then
      continue
    fi
    # Do not install to devices of size < 1 GiB (2097152 blocks of size 512
    # bytes). Devices of such size are too small for Chromium OS, and may in
    # fact be special partitions of large drives.
    if [ "$(cat "${dev}/size")" -lt 2097152 ]; then
      continue
    fi
    # The mmcblk driver may identify media cards as non-removable, so exclude
    # SD cards, which are inherently removable. BUG: We may still identify
    # external MMC cards as non-removable.
    if [ -f "${dev}/device/type" ]; then
      case "$(cat "${dev}/device/type")" in
      SD*)
        continue;
        ;;
      esac
    fi

    DST_DRIVE="/dev/$(basename "${dev}")"
    : $(( num_dev += 1 ))
  done

  # Check that there is only 1 internal device. This will fail on non-ChromeOS
  # devices with more than 1 internal drive, but the user is very unlikely
  # to want to take that approach since it is uncertain which drive this
  # will install to.
  if [ ${num_dev} -ne 1 ]; then
    die "There are more than one ($num_dev) internal devices to install to."
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
  write_base_table "${DST_DRIVE}" "${pmbr_code}" 2>&1

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
    echo -n "Enter the board you want to install (ex: x86-mario): "
    read BOARD
  fi
}

define_board() {
  # Define board if BOARD_SELECT is true.
  local board_select="$(findLSBValue BOARD_SELECT | tr '[:upper:]' '[:lower:]')"
  local board_name="$(findLSBValue BOARD_NAME | tr '[:upper:]' '[:lower:]')"
  if [ "$board_select" = "true" ]; then
    BOARD="$board_name"
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
    colorize red
    log " Private overlay customization BOARD override to $BOARD"
    sleep 1
  fi
}

override_from_tftp() {
  # Check for Omaha URL from tftp server.
  # OMAHA is override env variables used when calling memento_updater.
  local omaha=""
  local tftp=""
  local omahaserver_config="omahaserver.conf"
  local tftp_output=""
  # Use board specific config if $BOARD is not null.
  [ -z $BOARD ] || omahaserver_config="omahaserver_$BOARD.conf"
  tftp_output="/tmp/${omahaserver_config}"

  if tftp="$(find_var tftpserverip)"; then
    log "override_from_tftp: kernel cmdline tftpserverip $tftp"
    # Get omahaserver_config from tftp server.
    # Use busybox tftp command with options: "-g: Get file",
    # "-r FILE: Remote FILE" and "-l FILE: local FILE".
    rm -rf "$tftp_output"
    tftp -g -r $omahaserver_config -l $tftp_output $tftp || true
    if [ -f $tftp_output ]; then
      OMAHA="$(cat $tftp_output)"
      log "override_from_tftp: OMAHA override to $OMAHA"
    fi
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
    "$install_dev" 2>&1 || result="$?"

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
    "$updater" --force --mode=factory_install 2>&1 ||
      result="$?"
  else
    # We need to recover the firmware and then enable developer firmware.
    "$updater" --force --mode=recovery 2>&1 || result="$?"
    # For two-stop firmware, todev is a simple crossystem call; but for other
    # old firmware (alex/zgb), todev will perform flashrom and then reboot.
    # So this must be the very end command.
    "$updater" --force --mode=todev 2>&1 || result="$?"
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

omaha_greetings() {
  local message="$1"
  local uuid="$2"
  if [ -n "$OMAHA" ]; then
    wget -q "${OMAHA%/update}/greetings/${message}/${uuid}" \
         -O /dev/null 2>/dev/null || true
  fi

}

factory_on_complete() {
  if [ ! -s "$COMPLETE_SCRIPT" ]; then
    return 0
  fi

  log "Executing completion script... ($COMPLETE_SCRIPT)"
  if ! sh "$COMPLETE_SCRIPT" "$DST_DRIVE" 2>&1; then
    die "Failed running completion script $COMPLETE_SCRIPT."
  fi
  log "Completion script executed successfully."
}

disable_dev_switch() {
  # Turn off developer mode on devices without physical developer switch.
  if [ $DEVSW_PRESENT -eq 0 ]; then
    crossystem disable_dev_request=1
  # When physical switch exists, force user to turn it off.
  elif [ $DEVSW_PRESENT -eq 1 -a "$(crossystem devsw_cur)" = "1" ]; then
    while [ "$(crossystem devsw_cur)" = "1" ]; do
      log "Please turn off developer switch to continue"
      sleep 5
    done
  fi
}

factory_reset() {
  # Turn off developer mode on devices without physical developer switch.
  if [ $DEVSW_PRESENT -eq 0 ]; then
    crossystem disable_dev_request=1
  fi

  log "Performing factory reset"
  if ! /usr/sbin/factory_reset.sh "$DST_DRIVE"; then
    die "Factory reset failed."
  fi

  log "Done."
  # TODO(hungte) shutdown or reboot once we decide the default behavior.
  exit_success
}

factory_disk_wipe() {
  log "Performing factory disk wipe"
  if ! /usr/sbin/factory_reset.sh "$DST_DRIVE" wipe; then
    die "Factory disk wipe failed."
  fi
  log "Done."
  exit_success
}

# When install from USB drive, the partition mapping from source USB drive
# to the destination internal storage is as below.
# src_partition: dst_partition
# ----------------------------
#  1:     1 [stateful]
#  2:  None [install-kernel] for bootstrap, not installed to destination
#  3:  None [install-rootfs] for bootstrap, not installed to destination
#  4:     2 [factory-kernel] src_offset=2
#  5:     3 [factory-rootfs] src_offset=2
#  6:     4 [release-kernel] src_offset=2
#  7:     5 [release-rootfs] src_offset=2
#  8:     8 [oem]
# 12:    12 [efi]
# ----------------------------
process_usb_install_mapping() {
  local callback="$1"
  shift
  local param="$@" i

  for i in EFI OEM STATE FACTORY FACTORY_KERNEL RELEASE RELEASE_KERNEL; do
    local var="DST_${i}_PART"
    local part="${!var}"
    local src_offset=""
    case "${i}" in
      FACTORY* | RELEASE* )
        src_offset="${USB_INSTALL_OFFSET}"
        ;;
    esac
    # Early return when callback function fails.
    "${callback}" "${part}" "${src_offset}" ${param} || return 1
  done
}

# Callback of process_usb_install_mapping.
# Copy a partition to DST_DRIVE by given partition number, src_offset
# and src_drive.
dst_drive_partition_copy() {
  local part="$1"
  local src_offset="$2"
  local src_drive="$3"

  local src="$(make_partition_dev "${src_drive}" "$((part + src_offset))")"
  local dst="$(make_partition_dev "${DST_DRIVE}" "${part}")"

  # Detect file system size
  local dd_param="bs=1M"
  local count="$(dumpe2fs -h "${src}" 2>/dev/null |
                 grep "^Block count" |
                 sed 's/.*: *//')"

  if [ -n "${count}" ]; then
    local bs="$(dumpe2fs -h "${src}" 2>/dev/null |
                grep "^Block size" |
                sed 's/.*: *//')"

    # Optimize copy speed, with restriction: bs<10M
    while [ "$(( (count > 0) &&
                 (count % 2 == 0) &&
                 (bs / 1048576 < 10) ))" = "1" ]; do
      count="$((count / 2))"
      bs="$((bs * 2))"
    done
    dd_param="bs=${bs} count=${count}"
  fi

  log "Copying: [${i}] ${src} -> ${dst} (${dd_param})"
  # TODO(hungte) Detect copy failure
  pv -B 1M "${src}" |
    dd ${dd_param} of="${dst}" iflag=fullblock oflag=dsync
}

# Callback of process_usb_install_mapping.
# Verify a image resource by given partition number, src_offset and
# resources_dir.
verify_resources_checksum() {
  local part="$1"
  local src_offset="$2"
  local resources_dir="$3"
  local resources_file=""

  part=$(( part + src_offset ))
  resources_file="${resources_dir}/${part}.gz"
  [ -f "${resources_file}" ] || return 1

  # The config file contains the checksum of each image file.
  # For example:
  #   1_checksum = HSQdstR/U3hoBpDtnHVEfdNgm/Y=
  #   4_checksum = 3pt+EALoyH6hUmDbk735NY/MfAQ=
  #   5_checksum = ZyDaPXjzJJK8i0hkIcsD4D+o2DE=
  #   6_checksum = U/uLMDAkR3aP9oY/eD44K4ZReSg=
  #   7_checksum = twHsO9+o8m0mn9KL3pKhzJATsHQ=
  #   8_checksum = twHsO9+o8m0mn9KL3pKhzJATsHQ=
  #  12_checksum = twHsO9+o8m0mn9KL3pKhzJATsHQ=
  local config="${resources_dir}/config"
  local expected_checksum="$(grep "${part}_checksum" "${config}" |
                             cut -d" " -f3)"
  local actual_checksum="$(openssl sha1 -binary "${resources_file}" |
                           openssl base64)"
  if [ "${actual_checksum}" = "${expected_checksum}" ]; then
    return 0
  else
    return 1
  fi
}

# Callback of process_usb_install_mapping.
# Install a image resource to a partition on DST_DRIVE by given
# partition number, src_offset and resources_dir.
dst_drive_resources_install() {
  local part="$1"
  local src_offset="$2"
  local resources_dir="$3"

  local resource_file="${resources_dir}/$(( part + src_offset )).gz"
  local dst="$(make_partition_dev "${DST_DRIVE}" "${part}")"

  log "Installing: ${resource_file} -> ${dst}"
  pv -B 1M "${resource_file}" | gunzip -c |
    dd bs=1M of="${dst}" iflag=fullblock oflag=dsync
}

mount_installer_resources() {
  local resources_drive="$1"
  # Resources are stored in partition 1.
  local resources_dev="$(make_partition_dev "${resources_drive}" 1)"
  local resources_dir="${RESOURCES_MOUNT_POINT}/installer_resources"

  mount -t ext4 -o ro "${resources_dev}" "${RESOURCES_MOUNT_POINT}"
  if [ -d "${resources_dir}" ]; then
    echo "${resources_dir}"
  else
    echo ""
  fi
}

append_lsb_from_dst_drive() {
  local lsb_file="dev_image/etc/lsb-factory"
  local dst_path="/mnt/stateful_partition/${lsb_file}"
  local state_dev="$(make_partition_dev "${DST_DRIVE}" 1)"
  local state_mount="$(mktemp -d)"
  local src_path="${state_mount}/${lsb_file}"

  if ! mount "${state_dev}" "${state_mount}"; then
    die "Failed to mount ${state_dev}!!"
  fi

  if [ -f "${src_path}" ]; then
    cat "${src_path}" >> "${dst_path}"
  else
    die "Failed to find ${src_path}!!"
  fi

  umount "${state_mount}" || true
  rmdir "${state_mount}" || true
}

factory_install_usb_resources() {
  local installer_resources_dir="$1"
  local kern_guid="$(findLSBValue KERN_ARG_KERN_GUID)"
  local board_resources_dir="${installer_resources_dir}/${kern_guid}"

  [ -n "${kern_guid}" ] || die "Kernel GUID not found in kernel cmdline."
  [ -d "${board_resources_dir}" ] || die "Board resources not found."

  # Verify resources checksum before installation.
  log "Verifying resources checksum..."
  process_usb_install_mapping \
      verify_resources_checksum "${board_resources_dir}" ||
    die "Resources checksum verification failed."
  log "Done verifying checksum, start installation..."

  # Install from board_resources_dir.
  process_usb_install_mapping \
      dst_drive_resources_install "${board_resources_dir}"

  # Load board specific lsb-factory in DST_DRIVE for post installations.
  # For example, FACTORY_INSTALL_FIRMWARE in lsb-factory.
  append_lsb_from_dst_drive
}

factory_install_usb() {
  local i="" installer_resources_dir=""
  local src_drive="$(findLSBValue REAL_USB_DEV)"
  # REAL_USB_DEV is optional on systems without initramfs (e.g., ARM).
  [ -n "${src_drive}" ] || src_drive="$(rootdev -s 2>/dev/null)"
  [ -n "${src_drive}" ] ||
    die "Unknown media source. Please define REAL_USB_DEV."

  # Finds the real drive by removing the partition numbers at the end.
  # For example: sd[a-z][0-9]* -> sd[a-z]
  #              mmcblk[0-9]*p[0-9]* -> mmcblk[0-9]*
  src_drive="$(echo "${src_drive}" | sed 's/[0-9]\+$//')"
  src_drive="$(echo "${src_drive}" | sed 's/\(mmcblk[0-9]*\)p/\1/')"

  colorize yellow
  installer_resources_dir="$(mount_installer_resources "${src_drive}")"
  if [ -z "${installer_resources_dir}" ]; then
    # Install from src_drive if installer resources are not found.
    process_usb_install_mapping dst_drive_partition_copy "${src_drive}"
  else
    log "Installer resources found on USB stick."
    # Install from installer resources.
    factory_install_usb_resources "${installer_resources_dir}"
  fi
  umount "${RESOURCES_MOUNT_POINT}" || true

  # Run postinst on release partition to validate rootfs and create
  # verity table.
  run_postinst "$(make_partition_dev "${DST_DRIVE}" "${DST_RELEASE_PART}")"
  # Disable release partition and activate factory partition
  disable_release_partition
  run_postinst "$(make_partition_dev "${DST_DRIVE}" "${DST_FACTORY_PART}")"
  run_firmware_update "${DST_DRIVE}"
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

  # Generate the uuid for current install session
  uuid="$(uuidgen 2>/dev/null)" || uuid="Not_Applicable"

  # Say hello to miniomaha server
  omaha_greetings "hello" "${uuid}"

  # Install the partitions
  for i in EFI OEM STATE RELEASE FACTORY FIRMWARE HWID COMPLETE; do
    if [ "$i" = "FACTORY" -a -n "$RELEASE_ONLY" ]; then
        # Ignore factory image in release only mode
        continue
    fi

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
    if [ "$i" = "RELEASE" -a -n "$RELEASE_ONLY" ]; then
      # Do postinst after update in release only mode
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

    if [ "$i" = "RELEASE" -a -z "$RELEASE_ONLY" ]; then
      # Disable release partition when we are not in release only mode.
      disable_release_partition
    fi

    # Check the result
    if [ "$return_code" != "0" ]; then
      # memento update has encountered a fatal error.
      die "Factory install of target $dst has failed with error $return_code."
    elif [ "$result" != "UPDATED" ]; then
      if [ "$i" = "RELEASE" -a -n "$RELEASE_ONLY" ]; then
        # Only updating the primary root/kernel partition is strictly required.
        # If the omahaserver is configured to not update others that's fine.
        # In release only mode, release image is required.
        die "AU failed."
      elif [ "$i" = "FACTORY" -a -z "$RELEASE_ONLY" ]; then
        # Otherwise, factory image is required.
        die "AU failed."
      fi
    fi
  done

  # Notify miniomaha server all downloads (including postinst) are completed.
  omaha_greetings "download_complete" "${uuid}"

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
      # In release only mode, we want to run firmware updater in recovery mode.
      if [ -n "$RELEASE_ONLY" ]; then
        dst_arg="--force --mode=recovery"
      fi
      if ! sh "$dst" $dst_arg 2>&1; then
        die "Firmware updating failed."
      fi
    fi
    if [ -s "$DST_HWID_PART" ]; then
      log "Execute HWID component list updater script"
      dst="$DST_HWID_PART"
      dst_arg="$(make_partition_dev $DST_DRIVE $DST_STATE_PART)"
      if ! sh "$dst" "$dst_arg" 2>&1; then
        die "HWID component list updating failed."
      fi
    fi
  fi

  # Update omaha information into stateful partition.
  save_omaha_url "$(make_partition_dev $DST_DRIVE $DST_STATE_PART)" \
    2>&1 || die "Failed to save OMAHA server information."

  # After post processing, notify miniomaha server a installation session
  # has been successfully completed.
  omaha_greetings "goodbye" "${uuid}"

  if [ -s "$DST_COMPLETE_PART" ]; then
    log "Found completion script."
    COMPLETE_SCRIPT="$DST_COMPLETE_PART"
  fi

  # In release only mode, disable developer switch in the end to avoid
  # entering developer mode after restart.
  if [ -n "$RELEASE_ONLY" ]; then
    disable_dev_switch
  fi
}

test_ec_flash_presence() {
  # If "flashrom -p ec --get-size" command succeeds (returns 0),
  # then EC flash chip is present in system. Otherwise, assume EC flash is not
  # present or supported.
  if flashrom -p ec --get-size >/dev/null 2>&1; then
    EC_PRESENT=1
  else
    EC_PRESENT=0
  fi
}

test_devsw_presence() {
  local VBSD_HONOR_VIRT_DEV_SWITCH="0x400"
  local vdat_flags="$(crossystem vdat_flags || echo 0)"

  if [ "$((vdat_flags & VBSD_HONOR_VIRT_DEV_SWITCH))" = "0" ]; then
    DEVSW_PRESENT=1
  else
    DEVSW_PRESENT=0
  fi
}

board_pre_install() {
  BOARD_PRE_INSTALL=/usr/sbin/board_factory_pre_install.sh
  if [ -x "${BOARD_PRE_INSTALL}" ]; then
    echo "Running board specific pre-install: ${BOARD_PRE_INSTALL}"
    ${BOARD_PRE_INSTALL} || exit 1
  fi
}

board_post_install() {
  BOARD_POST_INSTALL=/usr/sbin/board_factory_post_install.sh
  if [ -x "${BOARD_POST_INSTALL}" ]; then
    echo "Running board specific post-install: ${BOARD_POST_INSTALL}"
    ${BOARD_POST_INSTALL} || exit 1
  fi
}

# Echoes "on" or "off" based on the value of a crossystem Boolean flag.
crossystem_on_or_off() {
  local value
  if value="$(crossystem "$1" 2>/dev/null)"; then
    case "$value" in
    "0")
      echo off
      ;;
    "1")
      echo on
      ;;
    *)
      echo "$value"
      ;;
    esac
  else
    echo "(unknown)"
  fi
}

# Echoes "yes" or "no" based on a Boolean argument (0 or 1).
bool_to_yes_or_no() {
  [ "$1" = 1 ] && echo yes || echo no
}

# Prints a header (a title, plus all the info in print_device_info)
print_header() {
    colorize boldwhite
    echo CrOS Factory Shim
    colorize white
    echo -----------------
    print_device_info
}

# Prints various information about the device.
print_device_info() {
    echo "Factory shim version: $(findLSBValue CHROMEOS_RELEASE_DESCRIPTION)"
    local bios_version="$(crossystem ro_fwid 2>/dev/null)"
    echo "BIOS version: ${bios_version:-(unknown)}"
    for type in RO RW; do
      echo -n "EC $type version: "
      ectool version | grep "^$type version" | sed -e 's/[^:]*: *//'
    done
    echo
    echo System time: "$(date)"
    local hwid="$(crossystem hwid 2>/dev/null)"
    echo "HWID: ${hwid:-(not set)}"
    echo -n "Dev mode: $(crossystem_on_or_off devsw_boot); "
    echo -n "Recovery mode: $(crossystem_on_or_off recoverysw_boot); "
    echo -n "HW write protect: $(crossystem_on_or_off wpsw_boot); "
    echo "SW write protect: $(crossystem_on_or_off sw_wpsw_boot)"
    echo -n "EC present: $(bool_to_yes_or_no "$EC_PRESENT"); "
    echo "Dev switch present: $(bool_to_yes_or_no "$DEVSW_PRESENT")"
    echo
}

# Displays a line in the menu.  Used in the menu function.
#
# Args:
#   $1: Single-character option name ("I" for install)
#   $2: Brief description
#   $3: Further explanation
menu_line() {
  echo -n "  "
  colorize boldwhite
  echo -n "$1  "
  colorize white
  printf "%-22s%s\n" "$2" "$3"
}

# Displays a menu, saving the action (one of $SUPPORTED_ACTIONS, always
# lowercase) in the "ACTION" variable.  If no valid action is chosen,
# ACTION will be empty.
menu() {
  # Clear up terminal
  stty sane echo
  # Enable cursor (if tput is available)
  tput cnorm 2>/dev/null || true

  echo
  echo
  echo Please select an action and press Enter.
  echo

  menu_line I "Install" "Performs a network or USB install"
  menu_line R "Reset" "Performs a factory reset; finalized devices only"
  menu_line S "Shell" "Opens bash; available only with developer firmware"
  menu_line V "View configuration" "Shows crossystem, VPD, etc."
  menu_line Z "Zero (wipe) storage" "Makes device completely unusable"

  echo
  read -p 'action> ' ACTION
  echo
  ACTION="$(echo "$ACTION" | tr A-Z a-z)"

  if echo "$ACTION" | grep -q "^[$SUPPORTED_ACTIONS]$"; then
      return
  fi
  echo "Invalid action; please select an action from the menu."
  ACTION=
}

main() {
  if [ "$(id -u)" -ne 0 ]; then
    echo "You must run this as root."
    exit 1
  fi
  if [ -n "$RELEASE_ONLY" ]; then
    # Change the partition in release only mode
    DST_RELEASE_KERNEL_PART=2
    DST_RELEASE_PART=3
  fi
  config_tty

  log "Starting Factory Installer."
  lightup_screen

  load_modules

  colorize white
  clear

  board_pre_install

  test_ec_flash_presence
  test_devsw_presence

  local install_from_omaha="1"
  if [ "$(findLSBValue FACTORY_INSTALL_FROM_USB)" = "1" ]; then
    install_from_omaha=""
  fi

  if [ "$(findLSBValue NETBOOT_RAMFS)" = "1" ]; then
    NETBOOT_RAMFS="1"
  fi

  # Check for any configuration overrides.
  overrides

  # In netboot, automatically perform the I action; but first give the
  # user the chance to press any key to display the menu.
  if [ "$NETBOOT_RAMFS" = 1 ]; then
    clear
    print_header
    log "Netbooting.  Will automatically perform 'install' action."
    log "Or press any key to show menu instead..."
    local prevent_install=false
    local timeout_secs=3
    for i in $(seq $timeout_secs -1 1); do
      # Read with timeout doesn't reliably work multiple times without
      # a subshell.
      if ( read -N 1 -p "Press any key within $i sec> " -t 1 ); then
        echo
        prevent_install=true
        break
      fi
      echo
    done
    if ! $prevent_install; then
      # No key pressed: perform the install action (which should
      # never return)
      action_i
    fi
  fi

  while true; do
    # Display the header and UI.
    clear
    print_header
    menu

    if [ -n "$ACTION" ]; then
      # Perform the selected action.
      action_${ACTION}
    fi

    colorize white
    read -N 1 -p "Press any key to continue> "
  done
}

#
# Action handlers
#

# I = Install.
action_i() {
  reset_chromeos_device

  if [ -n "$install_from_omaha" ]; then

    colorize yellow
    log "Waiting for ethernet connectivity to install"

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
      else
        sleep 1
      fi
    done

    # Check for OMAHA override from tftp server.
    override_from_tftp

    # TODO(hungte) how to set time in RMA?
    set_time || die "Please check if the server is configured correctly."
  fi

  colorize green
  get_dst_drive
  prepare_disk
  select_board
  define_board

  if [ -n "$install_from_omaha" ]; then
    factory_install_omaha
  else
    factory_install_usb
  fi

  log "Factory Installer Complete."
  sync
  sleep 3
  factory_on_complete

  board_post_install

  # Default action after installation: reboot.
  trap - EXIT
  sync
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

# R = Factory reset.
action_r() {
  if [ -n "$NETBOOT_RAMFS" ]; then
    # factory_reset.sh script is not available in netboot mode.
    colorize red
    log "Not available in netboot."
    return
  fi

  # First check to make sure that the factory software has been wiped.
  MOUNT_POINT=/tmp/stateful
  mkdir -p /tmp/stateful
  get_dst_drive
  mount -o ro "$(make_partition_dev $DST_DRIVE $DST_STATE_PART)" "$MOUNT_POINT"

  local factory_exists=false
  [ -e $MOUNT_POINT/dev_image/factory ] && factory_exists=true
  umount "$MOUNT_POINT"

  if $factory_exists; then
    colorize red
    log "Factory software is still installed (device has not been finalized)."
    log "Unable to perform factory reset."
    return
  fi

  reset_chromeos_device
  factory_reset
}

# S = Shell.
action_s() {
  if ! crossystem "mainfw_type?developer" 2>/dev/null; then
    colorize red
    echo "Developer firmware is not enabled; unable to open a shell."
    return
  fi

  log "Trying to bring up network..."
  if bringup_network 2>/dev/null; then
    colorize green
    log "Network enabled."
    colorize white
  else
    colorize yellow
    log "Unable to bring up network (or it's already up).  Proceeding anyway."
    colorize white
  fi

  echo Entering shell.
  bash
}

# V = View configuration.
action_v() {
  (
    print_device_info

    for partition in RO_VPD RW_VPD; do
      echo
      echo "$partition contents:"
      vpd -i "$partition" -l || true
    done

    echo
    echo "crossystem:"
    crossystem || true

    echo
    echo "lsb-factory:"
    cat /mnt/stateful_partition/dev_image/etc/lsb-factory || true
  ) 2>&1 | secure_less.sh
}

# Z = Zero
action_z() {
  if [ -n "$NETBOOT_RAMFS" ]; then
    # factory_reset.sh script is not available in netboot mode.
    colorize red
    log "Not available in netboot."
    return
  fi

  colorize red
  get_dst_drive
  echo "!!"
  echo "!! You are about to wipe the entire internal disk."
  echo "!! After this, the device will not boot anymore, and you"
  echo "!! need a recovery USB disk to bring it back to life."
  echo "!!"
  echo "!! Type 'yes' to do this, or anything else to cancel."
  echo "!!"
  colorize white
  local yes_or_no
  read -p "Wipe the internal disk? (yes/no)> " yes_or_no
  if [ "$yes_or_no" = yes ]; then
    factory_disk_wipe
  else
    echo "You did not type 'yes'. Cancelled."
  fi
}

main "$@"
