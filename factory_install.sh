#!/bin/bash

# Copyright 2015 The ChromiumOS Authors
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.

set -e
umask 022

# Dump bash trace to default log file.
: "${LOG_FILE:=/var/log/factory_install.log}"
: "${LOG_TTY:=}"
if [ -n "${LOG_FILE}" ]; then
  mkdir -p "$(dirname "${LOG_FILE}")"
  # shellcheck disable=SC2093 disable=SC1083
  exec {LOG_FD}>>"${LOG_FILE}"
  export BASH_XTRACEFD="${LOG_FD}"
fi

. "/usr/share/misc/storage-info-common.sh"
# Include after other common include, side effect on GPT variable.
. "/usr/sbin/write_gpt.sh"
. "$(dirname "$0")/factory_common.sh"
. "$(dirname "$0")/factory_cros_payload.sh"
. "$(dirname "$0")/factory_tpm.sh"

normalize_server_url() {
  local removed_protocol="${1#http://}"
  local removed_path="${removed_protocol%%/*}"
  echo "http://${removed_path}"
}

# Variables from dev_image/etc/lsb-factory that developers may want to override.
#
# - Override this if we want to install with a board different from installer
BOARD="$(findLSBValue CHROMEOS_RELEASE_BOARD)"
# - Override this if we want to install with a different factory server.
OMAHA="$(normalize_server_url "$(findLSBValue CHROMEOS_AUSERVER)")"
# - Override this for the default action if no keys are pressed before timeout.
DEFAULT_ACTION="$(findLSBValue FACTORY_INSTALL_DEFAULT_ACTION)"
# - Override this to enable/disable the countdown before doing default action.
ACTION_COUNTDOWN="$(findLSBValue FACTORY_INSTALL_ACTION_COUNTDOWN)"
# - Override this to 'false' to prevent waiting for prompt after installation.
# shellcheck disable=SC2019 disable=SC2018
COMPLETE_PROMPT="$(findLSBValue FACTORY_INSTALL_COMPLETE_PROMPT |
                   tr 'A-Z' 'a-z')"

# Variables prepared by image_tool or netboot initramfs code.
NETBOOT_RAMFS="$(findLSBValue NETBOOT_RAMFS)"
FACTORY_INSTALL_FROM_USB="$(findLSBValue FACTORY_INSTALL_FROM_USB)"
RMA_AUTORUN="$(findLSBValue RMA_AUTORUN)"

# Global variables
DST_DRIVE=""
COMPLETE_SCRIPT=""

# The ethernet interface to be used. It will be determined in
# check_ethernet_status and be passed to cros_payload_get_server_json_path as an
# environment variable.
ETH_INTERFACE=""

# Definition of ChromeOS partition layout
DST_FACTORY_KERNEL_PART=2
DST_FACTORY_ROOTFS_PART=3
DST_RELEASE_KERNEL_PART=4
DST_RELEASE_ROOTFS_PART=5
DST_STATE_PART=1
DST_POWERWASH_PART=11

# Supported actions (a set of lowercase characters)
# Each action x is implemented in an action_$x handler (e.g.,
# action_i); see the handlers for more information about what
# each option is.
SUPPORTED_ACTIONS=bcdefimprstvyz
SUPPORTED_ACTIONS_BOARD=

# Supported actions when hardware writeprotect is enabled.
# Add limitation of actions for security issue.
SUPPORTED_ACTIONS_WP_ENABLED=bdefmprtv

# Supported actions when RSU is required.
# We only support [e] RMA unlock to do RSU.
SUPPORTED_ACTIONS_RSU_REQUIRED=e

# These two arrays are shared b/w get_dlc_image_from_dlc_factory_cach and
# verify_dlc_hash.
CURRENT_DLC_IMG_PATHS=()
CURRENT_DLC_IDS=()
EXPECTED_DLC_IDS=()

TOTAL_DLC_PREALLOCATE_SIZE=0

FAI_VPD_CONFIG='{
  "ro_vpd": {
    "DataFiles": {
      "root": "/sys/firmware/vpd/ro",
      "read_content": true
    }
  },
  "rw_vpd": {
    "DataFiles": {
      "root": "/sys/firmware/vpd/rw",
      "read_content": true
    }
  }
}'

# Define our own logging function.
log() {
  echo "$*"
}

# Change color by ANSI escape sequence code
colorize() {
  set +x
  local code="$1"
  case "${code}" in
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
  printf "\033[%sm" "${code}"
  set -x
}

# Checks if 'cros_debug' is enabled.
is_allow_debug() {
  grep -qw "cros_debug" /proc/cmdline
}

# Checks if complete prompt is disabled.
is_complete_prompt_disabled() {
  grep -qw "nocompleteprompt" /proc/cmdline
}

# Checks if the system has been boot by Ctrl-U.
is_dev_firmware() {
  crossystem "mainfw_type?developer" 2>/dev/null
}

is_netboot() {
  grep -qw cros_netboot /proc/cmdline
}

explain_cros_debug() {
  log "To debug with a shell, boot factory shim in developer firmware (Ctrl-U)
   or add 'cros_debug' to your kernel command line:
    - Factory shim: Add cros_debug into --boot_args or make_dev_ssd
    - Netboot: Change kernel command line config file on TFTP."
}

# Error message for any unexpected error.
on_die() {
  set +x
  kill_bg_jobs
  colorize red
  echo
  log "ERROR: Factory installation has been stopped."
  if [ -n "${LOG_TTY}" ]; then
    local tty_num="${LOG_TTY##*[^0-9]}"
    log "See ${LOG_TTY} (Ctrl-Alt-F${tty_num}) for detailed information."
    log "(The F${tty_num} key is ${tty_num} keys to the right of 'esc' key.)"
  else
    log "See ${LOG_FILE} for detailed information."
  fi

  # Open a terminal if the kernel command line allows debugging.
  if is_allow_debug; then
    while true; do sh; done
  else
    explain_cros_debug
  fi

  colorize white
  read -N 1 -p "Press any key to reboot... "
  reboot

  exit 1
}

kill_bg_jobs() {
  local pids=$(jobs -p)
  # Disown all background jobs to avoid terminated messages
  disown -a
  # Kill the jobs
  echo "${pids}" | xargs -r kill -9 2>/dev/null || true
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

is_rsu_required() {
  # TPM should support RSU.
  [ "$(tpm_check_rsu_support)" != "unsupported" ] || return 1

  gdbus wait --system org.chromium.UserDataAuth

  local flags
  local FWMP_DEV_DISABLE_CCD_UNLOCK="0x40"

  flags="$(device_management_client \
           --action=get_firmware_management_parameters |
           grep -o "flags=0x[0-9a-f]\+" |
           cut -d = -f 2)"

  # FWMP_DEV_DISABLE_CCD_UNLOCK (0x40) is set during enterprise enrollment if
  # dev mode is blocked by the policy. We can use this flag to determine whether
  # RSU required.
  #
  # According to platform/cr50/board/cr50/factory_mode.c, Cr50 allows factory
  # mode when the following conditions are met:
  #   - HWWP signal is deasserted (battery disconnected, or pins shorted)
  #   - FWMP allows CCD unlock. FWMP_DEV_DISABLE_CCD_UNLOCK (0x40) is not set
  #   - CCD password is not set
  #
  # Assuming that CCD password is not set, if FWMP_DEV_DISABLE_CCD_UNLOCK is not
  # set, RSU is not required and we can enter factory mode by disconnecting the
  # battery or shorting pins.
  if [ -z "${flags}" ] ||
     [ $(( flags & FWMP_DEV_DISABLE_CCD_UNLOCK )) -eq 0 ]; then
    return 1
  fi
  return 0
}

# Checks if hardware write protection is enabled.
check_hwwp() {
  crossystem 'wpsw_cur?1' 2>/dev/null
}

# Get the supported actions under the current device status.
get_supported_actions() {
  if is_rsu_required; then
    echo "${SUPPORTED_ACTIONS_RSU_REQUIRED}"
  elif check_hwwp; then
    echo "${SUPPORTED_ACTIONS_WP_ENABLED}"
  else
    echo "${SUPPORTED_ACTIONS}${SUPPORTED_ACTIONS_BOARD}"
  fi
}

# Print helper messages when the supported actions are restricted.
print_restricted_supported_actions_message() {
  colorize yellow
  if is_rsu_required; then
    echo
    echo "This device has FWMP and blocks developer mode."
    echo "It is possibly a managed device."
    echo "Please ask the admin to deprovision the device or perform"
    echo "RSU (RMA Server Unlock)."
    echo
    echo "Some actions are unsupported and marked as red."
    echo
  elif check_hwwp; then
    echo
    echo "This device has hardware write protection enabled."
    echo
    echo "Some actions are unsupported and marked as red."
    echo
  fi
  colorize white
}
# Checks if host firmware software write protection is enabled.
check_host_swwp() {
  futility flash --wp-status --ignore-hw |
    grep -q "WP status: enabled"
}

# Checks if ec firmware software write protection is enabled.
check_ec_swwp() {
  ectool flashprotect | grep "Flash protect flags" | grep "ro_at_boot"
}

clear_fwwp() {
  log "Firmware Write Protect disabled, clearing status registers."
  ectool flashprotect disable
  futility flash --wp-disable
  log "WP registers should be cleared now"
}

ensure_fwwp_consistency() {
  local ec_wp host_wp

  ec_wp=$(command_to_yes_or_no check_ec_swwp)
  host_wp=$(command_to_yes_or_no check_host_swwp)
  if [ "${ec_wp}" != "${host_wp}" ]; then
    die "Inconsistent firmware write protection status: " \
        "host=${host_wp}, ec=${ec_wp}." \
        "Please disable Hardware Write Protection and restart again."
  fi
}

is_pvt_phase() {
  # If there is any error then pvt phase is returned as a safer default
  # option.
  local bid_flags="0x$(tpm_get_board_id_flags)"

  # If board phase is not 0x0 then this is a development board.
  local pre_pvt=$(( bid_flags & 0x7F ))
  if (( pre_pvt > 0 )); then
    return ${pre_pvt}
  fi

  return 0
}

config_tty() {
  stty opost

  # Turn off VESA blanking
  setterm -blank 0 -powersave off -powerdown 0
}

echo_huge_ok() {
  cat <<HERE

               ######                     ######         ######
         ##################               ######        ######
        ####################              ######       ######
      #########      #########            ######      ######
    ########            ########          ######     ######
   #######                #######         ######    ######
   ######                  ######         ######   ######
   ######                  ######         ######  ######
   ######                  ######         ###### ######
   ######                  ######         ############
   ######                  ######         ############
   ######                  ######         ############
   ######                  ######         ###### ######
   ######                  ######         ######  ######
   ######                  ######         ######   ######
   #######                #######         ######    ######
    ########            ########          ######     ######
      #########      #########            ######      ######
        ####################              ######       ######
         ##################               ######        ######
               ######                     ######         ######

HERE
}

set_time() {
  log "Setting time from:"
  # Extract only the server and port.
  local time_server_port="${OMAHA#http://}"

  log " Server ${time_server_port}."
  local result="$(htpdate -s -t "${time_server_port}" 2>&1)"
  if ! echo "${result}" | grep -Eq "(failed|unavailable)"; then
    log "Success, time set to $(date)"
    hwclock -w 2>/dev/null
    return 0
  fi

  log "Failed to set time: $(echo "${result}" | grep -E "(failed|unavailable)")"
  return 1
}

check_ethernet_status() {
  local link i
  local result=1
  link=$(ip -f link addr | sed 'N;s/\n/ /' | grep -w 'ether' |
    cut -d ' ' -f 2 | sed 's/://')
  for i in ${link}; do
    if ip -f inet addr show "${i}" | grep -q inet; then
      if ! iw "${i}" info >/dev/null 2>&1; then
        log "$(ip -f inet addr show "${i}" | grep inet)"
        ETH_INTERFACE=${i}
        result=0
      fi
    fi
  done
  return ${result}
}

clear_block_devmode() {
  # Try our best to clear block_devmode.
  crossystem block_devmode=0 || true
  vpd -i RW_VPD -d block_devmode -d check_enrollment || true
}

reset_chromeos_device() {
  if [[ -e /dev/nvram ]]; then
    log "Clearing NVData."
    dd if=/dev/zero of=/dev/nvram bs=1 count="$(wc -c </dev/nvram)" \
      status=none conv=nocreat || log "Failed to clear NVData (non-critical)."
  fi

  clear_block_devmode

  if is_netboot; then
    log "Device is network booted."
    return
  fi

  if crossystem "mainfw_type?nonchrome"; then
    # Non-ChromeOS firmware devices can stop now.
    log "Device running Non-ChromeOS firmware."
    return
  fi

  log "Request to clear TPM owner at next boot."
  # No matter if whole TPM (see below) is cleared or not, we always
  # want to clear TPM ownership (safe and easy) so factory test program and
  # release image won't start with unknown ownership.
  crossystem clear_tpm_owner_request=1 || true

  log "Checking if TPM should be recovered (for version and owner)"
  # To clear TPM, we need it unlocked (only in recovery boot).
  # Booting with USB in developer mode (Ctrl-U) does not work.

  if crossystem "mainfw_type?recovery"; then
    if ! chromeos-tpm-recovery; then
      colorize yellow
      log " - TPM recovery failed.

      This is usually not a problem for devices on manufacturing line,
      but if you are using factory shim to reset TPM (for antirollback issue),
      there's something wrong.
      "
      sleep 3
    else
      log "TPM recovered."
    fi
  else
    mainfw_type="$(crossystem mainfw_type)"
    colorize yellow
    log " - System was not booted in recovery mode (current: ${mainfw_type}).

    WARNING: TPM won't be cleared. To enforce clearing TPM, make sure you are
    using correct image signed with same key (MP, Pre-MP, or DEV), turn on
    developer switch if you haven't, then hold recovery button and reboot the
    system again.  Ctrl-U won't clear TPM.
    "
    # Alert for a while
    sleep 3
  fi
}

get_dst_drive() {
  load_base_vars
  DST_DRIVE="$(get_fixed_dst_drive)"
  if [ -z "${DST_DRIVE}" ]; then
    die "Cannot find fixed drive."
  fi
}

# In some envs (such as netboot's initramfs), backlight_tool is not available,
# so if that fails we fallback to controlling backlight brightness via
# entries under /sys/class/backlight/.
set_brightness_percentage() {
  local backlight_device
  local max_brightness
  local desired_brightness
  local percentage="$1"

  if backlight_tool --set_brightness_percent="${percentage}"; then
    return
  fi

  # We set all backlight devices to the desired brightness percentage.
  for backlight_device in /sys/class/backlight/*; do
    # In order to set brightness to specific percentage, we do the following:
    # - Turn the backlight device on (using bl_power).
    # - Read max_brightness and scale that into the desired brightness.
    # - Write the desired brightness to the brightness entry.
    echo 0 > "${backlight_device}/bl_power" # 0 = FB_BLANK_UNBLANK (i.e.: on)
    max_brightness="$(cat "${backlight_device}/max_brightness")"
    desired_brightness=$((max_brightness * percentage / 100))
    echo "${desired_brightness}" > "${backlight_device}/brightness"
  done
}

lightup_screen() {
  # Always backlight on factory install shim.
  ectool forcelidopen 1 || true
  # Before we try to light up the screen, we set the brightness to another one
  # in case we can't set the brightness. Since we set the brightness via the
  # /sys/class/../brightness, the entry is only used to set the brightness and
  # does not reflect the actual brightness change (brightness may be different
  # from actual_brightness). Therefore, if the entry value is already the value
  # we want to set, we should first set the value to another value, otherwise
  # the value will not be set eventually.
  # Please refer to b/203831384 for more information.
  set_brightness_percentage 80 || true
  # Light up screen in case you can't see our splash image.
  set_brightness_percentage 100 || true
}

load_modules() {
  # Required kernel modules might not be loaded. Load them now.
  modprobe i2c-dev || true
}

prepare_disk() {
  log "Factory Install: Setting partition table"

  local pmbr_code="/root/.pmbr_code"
  [ -r ${pmbr_code} ] || die "Missing ${pmbr_code}; please rebuild image."

  write_base_table "${DST_DRIVE}" "${pmbr_code}" 2>&1
  reload_partitions "${DST_DRIVE}"

  log "Done preparing disk"
}

ufs_init() {
  local ufs_init_bin="/usr/sbin/factory_ufs"
  if [ -x "${ufs_init_bin}" ]; then
    ${ufs_init_bin} provision
  fi
}

find_var() {
  # Check kernel commandline for a specific key value pair.
  # Usage: omaha=$(find_var omahaserver)
  # Assume values are space separated, keys are unique within the commandline,
  # and that keys and values do not contain spaces.
  local key="$1"

  # shellcheck disable=SC2013
  for item in $(cat /proc/cmdline); do
    if echo "${item}" | grep -q "${key}"; then
      echo "${item}" | cut -d'=' -f2
      return 0
    fi
  done
  return 1
}

override_from_firmware() {
  # Check for Omaha URL or Board type from kernel commandline.
  local omaha=""
  if omaha="$(find_var omahaserver)"; then
    OMAHA="$(normalize_server_url "${omaha}")"
    log " Kernel cmdline OMAHA override to ${OMAHA}"
  fi

  local board=""
  if board="$(find_var cros_board)"; then
    log " Kernel cmdline BOARD override to ${board}"
    BOARD="${board}"
  fi
}

override_from_board() {
  # Call into any board specific configuration settings we may need.
  # The file should be installed in factory-board/files/installer/usr/sbin/.
  local lastboard="${BOARD}"
  local board_customize_file="/usr/sbin/factory_install_board.sh"
  if [ -f "${board_customize_file}" ]; then
    . "${board_customize_file}"
  fi

  # Let's notice if BOARD has changed and print a message.
  if [ "${lastboard}" != "${BOARD}" ]; then
    colorize red
    log " Private overlay customization BOARD override to ${BOARD}"
    sleep 1
  fi
}

override_from_tftp() {
  # Check for Omaha URL from tftp server.
  local tftp=""
  local omahaserver_config="omahaserver.conf"
  local tftp_output=""
  # Use board specific config if ${BOARD} is not null.
  [ -z "${BOARD}" ] || omahaserver_config="omahaserver_${BOARD}.conf"
  tftp_output="/tmp/${omahaserver_config}"

  if tftp="$(find_var tftpserverip)"; then
    log "override_from_tftp: kernel cmdline tftpserverip ${tftp}"
    # Get omahaserver_config from tftp server.
    # Use busybox tftp command with options: "-g: Get file",
    # "-r FILE: Remote FILE" and "-l FILE: local FILE".
    rm -rf "${tftp_output}"
    tftp -g -r "${omahaserver_config}" -l "${tftp_output}" "${tftp}" || true
    if [ -f "${tftp_output}" ]; then
      OMAHA="$(normalize_server_url "$(cat "${tftp_output}")")"
      log "override_from_tftp: OMAHA override to ${OMAHA}"
    fi
  fi
}

overrides() {
  override_from_firmware
  override_from_board
}

disable_release_partition() {
  # Release image is not allowed to boot unless the factory test is passed
  # otherwise the wipe and final verification can be skipped.
  if ! cgpt add -i "${DST_RELEASE_KERNEL_PART}" -P 0 -T 0 -S 0 "${DST_DRIVE}"
  then
    # Destroy kernels otherwise the system is still bootable.
    dst="$(make_partition_dev "${DST_DRIVE}" "${DST_RELEASE_KERNEL_PART}")"
    dd if=/dev/zero of="${dst}" bs=1M count=1
    dst="$(make_partition_dev "${DST_DRIVE}" "${DST_FACTORY_KERNEL_PART}")"
    dd if=/dev/zero of="${dst}" bs=1M count=1
    die "Failed to lock release image. Destroy all kernels."
  fi
  # cgpt changed partition table, so we have to make sure it's notified.
  reload_partitions "${DST_DRIVE}"
}

run_postinst() {
  local install_dev="$1"
  local mount_point="$(mktemp -d)"
  local result=0

  mount -t ext2 -o ro "${install_dev}" "${mount_point}"
  IS_FACTORY_INSTALL=1 "${mount_point}"/postinst \
    "${install_dev}" 2>&1 || result="$?"

  umount "${install_dev}" || true
  rmdir "${mount_point}" || true
  return ${result}
}

stateful_postinst() {
  local stateful_dev="$1"
  local mount_point="$(mktemp -d)"
  local dev_image_mount_point

  mount "${stateful_dev}" "${mount_point}"

  local lsb_factory="${mount_point}/dev_image/etc/lsb-factory"

  # After M135, dev_image is now a sparse file system rather than a directory
  # on some board, as a result we need to mount it if it exists, before
  # installing stub files.
  dev_image_mount_point="$(mktemp -d)"
  local block_file="${mount_point}/unencrypted/dev_image.block"
  if [ -f "${block_file}" ]; then
    mount "${block_file}" "${dev_image_mount_point}"
    lsb_factory="${dev_image_mount_point}/dev_image/etc/lsb-factory"
  fi

  # Update lsb-factory on stateful partition.
  if [ -z "${FACTORY_INSTALL_FROM_USB}" ]; then
    log "Save active factory server URL to stateful partition lsb-factory."
    echo "FACTORY_OMAHA_URL=${OMAHA}" >>"${lsb_factory}"
  else
    log "Clone lsb-factory to stateful partition."
    cat "${LSB_FACTORY_FILE}" >>"${lsb_factory}"
  fi

  if [ -f "${block_file}" ]; then
    umount "${dev_image_mount_point}" || true
    rmdir "${dev_image_mount_point}" || true
  fi
  umount "${mount_point}" || true
  rmdir "${mount_point}" || true
}

omaha_greetings() {
  if [ -n "${FACTORY_INSTALL_FROM_USB}" ]; then
    return
  fi

  local message="$1"
  local uuid="$2"
  curl "${OMAHA}/greetings/${message}/${uuid}" >/dev/null 2>&1 || true
}

factory_on_complete() {
  if [ ! -s "${COMPLETE_SCRIPT}" ]; then
    return 0
  fi

  log "Executing completion script... (${COMPLETE_SCRIPT})"
  if ! sh "${COMPLETE_SCRIPT}" "${DST_DRIVE}" 2>&1; then
    die "Failed running completion script ${COMPLETE_SCRIPT}."
  fi
  log "Completion script executed successfully."
}

# Call reset code on the fixed driver.
#
# Assume the global variable DST_DRIVE contains the drive to operate on.
#
# Args:
#   action: describe how to erase the drive.
#     Allowed actions:
#     - wipe: action z
#     - secure: action c
#     - verify: action y
factory_disk_action() {
  local action="$1"
  log "Performing factory disk ${action}"
  if ! /usr/sbin/factory_installer factory-reset "${action}"; then
    die "Factory disk ${action} failed."
  fi
  log "Done."
  exit_success
}

enlarge_partition() {
  local dev="$1"
  local block_size="$(dumpe2fs -h "${dev}" | sed -n 's/Block size: *//p')"
  local minimal="$(resize2fs -P "${dev}" | sed -n 's/Estimated .*: //p')"

  local test_rootfs_mount_point test_rootfs_dev
  test_rootfs_mount_point="$(mktemp -d)"
  test_rootfs_dev="$(make_partition_dev "${DST_DRIVE}" \
                  "${DST_FACTORY_ROOTFS_PART}")"
  mount -o ro "${test_rootfs_dev}" "${test_rootfs_mount_point}"
  calculate_dlc_preallocate_size "${dev}" "${test_rootfs_mount_point}"
  umount "${test_rootfs_mount_point}"
  rmdir "${test_rootfs_mount_point}"

  if [ "${minimal}" -gt 0 ] && [ "${block_size}" -gt 0 ]; then
    local e2fsck_ret=0
    e2fsck -f -y "${dev}" || e2fsck_ret=$?
    if [ "$((e2fsck_ret & ~1))" -ne 0 ]; then
      die "e2fsck return: ${e2fsck_ret}"
    fi
    # DLC images will be installed to stateful partition in run time.
    # We need to preserve space for them.
    log "Allocate ${TOTAL_DLC_PREALLOCATE_SIZE} bytes for DLCs."
    local dlc_size=$((TOTAL_DLC_PREALLOCATE_SIZE / block_size))
    # Try to allocate 2G if possible for factory toolkit + run time overhead.
    local reserve=$((1024 * 2097152 / block_size))
    resize2fs "${dev}" "$((minimal + reserve + dlc_size))" || true
  fi
}

reload_partitions() {
  # Some devices, for example NVMe, may need extra time to update block device
  # files via udev. We should do sync, partprobe, and then wait until partition
  # device files appear again.
  local drive="$1"
  log "Reloading partition table changes..."
  sync

  # Reference: src/platform2/installer/chromeos-install#reload_partitions
  udevadm settle || true  # Netboot environment may not have udev.
  for delay in 0 1 2 4 8; do
    sleep "${delay}"
    blockdev --rereadpt "${drive}" && return ||
      log "Failed to reload partitions on ${drive}"
  done
  die "Continually failed to reload partitions on ${drive}"
}

cros_payload_get_server_json_path() {
  local server_url="$1"
  local eth_interface="$2"

  # Try to get resource map from Umpire.
  local sn="$(vpd -i RO_VPD -g serial_number)" || sn=""
  local mlb_sn="$(vpd -i RO_VPD -g mlb_serial_number)" || mlb_sn=""
  local mac_addr="$(ip link show ${eth_interface} | grep link/ether |
    tr -s ' '| cut -d ' ' -f 3)"
  local resourcemap=""
  local mac="mac.${eth_interface}=${mac_addr};"
  local values="sn=${sn}; mlb_sn=${mlb_sn}; board=${BOARD}; ${mac}"
  local empty_values="firmware=; ec=; stage=;"
  local header="X-Umpire-DUT: ${values} ${empty_values}"
  local target="${server_url}/resourcemap"
  # This is following Factory Server/Umpire protocol.
  echo "Header: ${header}" >&2

  resourcemap="$(curl -f --header "${header}" "${target}")"
  if [ -z "${resourcemap}" ]; then
    echo "Missing /resourcemap - please upgrade Factory Server." >&2
    return 1
  fi
  echo "resourcemap: ${resourcemap}" >&2
  # Check if multicast config exists
  local json_name="$(echo "${resourcemap}" | grep "^multicast: " |
    cut -d ' ' -f 2)"
  if [ -z "${json_name}" ]; then
    # Multicast config not found. Fallback to normal payload config.
    json_name="$(echo "${resourcemap}" | grep "^payloads: " | cut -d ' ' -f 2)"
  fi
  if [ -n "${json_name}" ]; then
    echo "res/${json_name}"
  else
    echo "'payloads' not in resourcemap, please upgrade Factory Server." >&2
    return 1
  fi
}

is_intel_platform() {
  local tmp_dir contain_desc
  tmp_dir="$(mktemp -d)"
  contain_desc=0
  futility read --region FMAP --split-output "${tmp_dir}"/bios
  fmap_decode "${tmp_dir}"/bios_FMAP | grep -qw SI_DESC || contain_desc="$?"
  rm "${tmp_dir}"/bios_FMAP
  rmdir "${tmp_dir}"
  return "${contain_desc}"
}

is_ME_locked() {
  local tmp_dir cur_desc new_desc is_locked
  tmp_dir="$(mktemp -d)"
  is_locked=0
  cur_desc="${tmp_dir}"/bios_SI_DESC
  new_desc="$(mktemp)"
  futility read --region SI_DESC --split-output "${tmp_dir}"/bios
  ifdtool -p ifd2 -lr -O "${new_desc}" "${cur_desc}"
  diff "${new_desc}" "${cur_desc}" || is_locked="$?"
  rm "${cur_desc}" "${new_desc}"
  rmdir "${tmp_dir}"
  return "${is_locked}"
}

is_flexible_eom_supported() {
  local unsupported_boards="dedede"

  for board in $unsupported_boards; do
    if [ "$BOARD" = "$board" ]; then
      return 1
    fi
  done

  return 0
}

# Gets the size in byte of the release_image.dlc_factory_cache file
# Usage: get_dlc_gz_size JSON_PATH
get_dlc_gz_size() {
  local json_url dlc_gz_file dlc_gz_size
  json_url="$1"

  if [ -n "${FACTORY_INSTALL_FROM_USB}" ]; then
    dlc_gz_file="$(jq -r \
                  '."release_image"."dlc_factory_cache"' "${json_url}")"
    dlc_gz_file="$(dirname "${json_url}")/${dlc_gz_file}"
    dlc_gz_size="$(du -B1 "${dlc_gz_file}" | \
                  awk '{ size=$1 } END { print size }')"
  else
    dlc_gz_file="$(curl "${json_url}" | \
                  jq -r '."release_image"."dlc_factory_cache"')"
    dlc_gz_file="$(dirname "${json_url}")/${dlc_gz_file}"
    # curl -sI returns the header information of the DLC file.
    # The header looks like this:
    #   Location: <url_to_dlc_file>
    #   Date: <date>
    #   ...
    #   Content-Length: <size_of_content>
    #   ...
    # We use `awk` to get the second column of the Content-Length, which is
    # the size of the DLC file, and trim off the white spaces using `tr`.
    dlc_gz_size="$(curl -sI "${dlc_gz_file}" | grep -i Content-Length | \
                  awk '{ print $2 }' | tr -d '[:space:]')"
  fi

  echo "${dlc_gz_size}"
}

# Enlarges the dev_image.block if exists, with specified size_in_byte
# Usage: enlarge_dev_image_if_needed state_dev size_in_byte
enlarge_dev_image_if_needed() {
  local state_dev mount_point image block_size minimal reserve size_in_byte

  state_dev="$1"
  size_in_byte="$2"

  if [ "${size_in_byte}" -gt 0 ]; then
    mount_point="$(mktemp -d)"
    mount "${state_dev}" "${mount_point}"
    image="${mount_point}/unencrypted/dev_image.block"

    if [ -f "${image}" ]; then
      block_size="$(dumpe2fs -h "${image}" | sed -n 's/Block size: *//p')"
      minimal="$(resize2fs -P "${image}" | sed -n 's/Estimated .*: //p')"

      e2fsck -f -y "${image}"
      reserve=$((size_in_byte / block_size))
      resize2fs "${image}" "$((minimal + reserve))" || true
    fi

    umount "${mount_point}"
    rmdir "${mount_point}"
  fi
}

factory_install_cros_payload() {
  local src_media="$1"
  local json_path="$2"
  local dlc_path="$3"
  local src_mount=""
  local tmp_dir="$(mktemp -d)"
  local json_url="${src_media}/${json_path}"
  local dlc_gz_size
  local state_dev
  local cur_tmpfs_size

  if [ -b "${src_media}" ]; then
    src_mount="$(mktemp -d)"
    colorize yellow
    mount -o ro "${src_media}" "${src_mount}"
    json_url="${src_mount}/${json_path}"
  fi

  # Generate the uuid for current install session
  local uuid="$(uuidgen 2>/dev/null)" || uuid="Not_Applicable"

  # Say hello to server if available.
  omaha_greetings "hello" "${uuid}"

  # Get DLC factory cache size
  dlc_gz_size="$(get_dlc_gz_size "${json_url}")"

  if [ -n "${dlc_path}" ]; then
    log "Installation will fail if factory server version is less than" \
        "20211102181209!"
    # Estimate the size of the DLC and resize the tmpfs accordingly.
    # This is a short term solution since the max size of the tmpfs is
    # limited by the size of the memory on DUT.

    # tmpfs mounts on /
    cur_tmpfs_size="$(df -B1 / | awk '(NR>1) { size=$2 } END { print size }')"
    # The size of the compressed and decompressed DLC images are similar.
    mount -o remount,size="$((dlc_gz_size * 2 + cur_tmpfs_size))" /

    cros_payload install "${json_url}" "${dlc_path}" \
      release_image.dlc_factory_cache
  else
    cros_payload install "${json_url}" "${DST_DRIVE}" test_image release_image

    state_dev="$(make_partition_dev "${DST_DRIVE}" "${DST_STATE_PART}")"

    # Test image stateful partition may be pretty full and we may want more
    # space, before installing toolkit (which may be huge).
    enlarge_partition "${state_dev}"

    # For boards having dev_image.block sparse filesystem, in order to install
    # DLC factory cache, after partition enlargement, we need to also enlarge
    # dev_image filesystem as well.
    enlarge_dev_image_if_needed "${state_dev}" "${dlc_gz_size}"

    cros_payload install "${json_url}" "${DST_DRIVE}" toolkit

    # Install optional components.
    cros_payload install_optional "${json_url}" "${DST_DRIVE}" \
      release_image.crx_cache release_image.dlc_factory_cache hwid \
      toolkit_config project_config
    cros_payload install_optional "${json_url}" "${tmp_dir}" firmware complete
  fi

  if [ -n "${src_mount}" ]; then
    umount "${src_mount}"
  fi
  colorize green

  # Notify server that all downloads are completed.
  omaha_greetings "download_complete" "${uuid}"

  if [ -n "${dlc_path}" ]; then
    return
  fi

  # Disable release partition and activate factory partition
  disable_release_partition
  run_postinst "$(make_partition_dev "${DST_DRIVE}" \
    "${DST_FACTORY_ROOTFS_PART}")" || die "Failed running postinst script."
  stateful_postinst "$(make_partition_dev "${DST_DRIVE}" "${DST_STATE_PART}")"

  if [ -s "${tmp_dir}/firmware" ]; then
    log "Found firmware updater."
    local unlock_flag
    if is_intel_platform; then
      log "DUT is using Intel platform."
      if is_flexible_eom_supported; then
        if is_ME_locked; then
          log "ME has already been locked!"
        else
          log "ME is unlocked. Flash unlocked firmware..."
          unlock_flag="--quirks=unlock_csme"
        fi
      else
        log "${BOARD} does not support flexible EOM..."
      fi
    fi
    # TODO(hungte) Check if we need to run --mode=recovery if WP is enabled.
    sh "${tmp_dir}/firmware" --force --mode=factory_install \
      ${unlock_flag:+"${unlock_flag}"} || die "Firmware updating failed."
    rm "${tmp_dir}/firmware"
  fi
  if [ -s "${tmp_dir}/complete" ]; then
    log "Found completion script."
    COMPLETE_SCRIPT="${tmp_dir}/complete"
  fi

  # After post processing, notify server a installation session has been
  # successfully completed.
  omaha_greetings "goodbye" "${uuid}"
}

get_usb_dev_stateful() {
  local src_dev
  src_dev="$(findLSBValue REAL_USB_DEV)"
  [ -n "${src_dev}" ] || src_dev="$(rootdev -s 2>/dev/null)"
  [ -n "${src_dev}" ] ||
    die "Unknown media source. Please define REAL_USB_DEV."

  # shellcheck disable=SC2001
  echo "${src_dev}" | sed 's/[0-9]\+$/1/'
}

factory_install_usb() {
  local dlc_path stateful_dev mount_point json_path
  dlc_path="$1"
  stateful_dev="$(get_usb_dev_stateful)"
  # Switch to stateful partition.
  mount_point="$(mktemp -d)"
  mount -o ro "${stateful_dev}" "${mount_point}"
  json_path="$(cros_payload_metadata "${mount_point}")"
  umount "${stateful_dev}"
  rmdir "${mount_point}"

  if [ -n "${json_path}" ]; then
    factory_install_cros_payload "${stateful_dev}" "${json_path}" \
      "${dlc_path}"
  else
    die "Cannot find cros_payload metadata."
  fi
}

connect_to_ethernet() {
  log "Waiting for ethernet connectivity to install"

  while true; do
    if [ -n "${NETBOOT_RAMFS}" ]; then
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
}

factory_install_network() {
  local dlc_path="$1"
  # Register to Overlord if haven't.
  if [ -z "${OVERLORD_READY}" ]; then
    register_to_overlord "${OMAHA}" "${TTY_FILE}" "${LOG_FILE}"
  fi
  # Get path of cros_payload json file from server (Umpire or Mini-Omaha).
  local json_path
  json_path="$(cros_payload_get_server_json_path \
    "${OMAHA}" "${ETH_INTERFACE}" 2>/dev/null)"
  [ -n "${json_path}" ] || die "Failed to get payload json path from server."
  factory_install_cros_payload "${OMAHA}" "${json_path}" "${dlc_path}"
}

# Retrieves the IDs of factory-installed DLC images from the rootfs
# partition's `/opt/google/dlc` directory.
get_expected_factory_installed_dlc() {
  local rootfs_mount_point="$1"
  local metadata_cmd variant dlc_list_str dlc_list
  metadata_cmd="chroot ${rootfs_mount_point} dlc_metadata_util \
    --metadata_dir=/opt/google/dlc/"

  variant="$(cros_config /modem firmware-variant)" || true

  if [[ -z "${variant}" ]]; then
    log "Warning: NO LTE on this device. Skipping DLC check."
    return
  fi

  # Check if variant is in the expected format. E.g. rynax_rw101
  if [[ "${variant}" =~ ^[[:alnum:]]+_[[:alnum:]]+$ ]]; then
    log "modem firmware variant: ${variant}"
  else
    die "Error: Invalid variant format: ${variant}"
  fi

  dlc_list_str=$(
    ${metadata_cmd} --list --attribute="${variant}"
  )
  mapfile -t dlc_list < <(echo "${dlc_list_str}" | jq -r '.[]')

  if [[ ${#dlc_list[@]} -eq 0 ]]; then
    log "No DLCs found for attribute: ${variant}"
    if is_pvt_phase; then
      die "ERROR: DLCs are mandatory in the current (PVT) stage. Aborting."
    else
      log "Currently in an early development phase, so missing DLCs " \
          "are permitted. Proceeding with caution."
    fi
  fi

  for dlc_id in "${dlc_list[@]}"; do
    EXPECTED_DLC_IDS+=("${dlc_id}")
  done
}

# Calculates the total pre-allocated size required for factory-installed DLC
# images. This size is used for partition enlargement.
calculate_dlc_preallocate_size() {
  local stateful_mnt_pnt="$1"
  local rootfs_mount_point="$2"
  # Get the CURRENT_DLC_IDS from the existing DLC directory
  get_dlc_image_from_dlc_factory_cache \
    "${stateful_mnt_pnt}"/unencrypted/dlc-factory-images

  # Calculate the pre-allocate size
  local metadata_cmd="dlc_metadata_util \
    --metadata_dir=${rootfs_mount_point}/opt/google/dlc/"
  for dlc_id in "${CURRENT_DLC_IDS[@]}"; do
    local metadata
    if metadata=$(${metadata_cmd} --get --id="${dlc_id}"); then
      local pre_alloc_size
      pre_alloc_size=$(echo "${metadata}" | \
        jq -r '."manifest"."pre-allocated-size"')
      # The run time overhead of factory installed DLC images is
      # `2 * pre-allocated-size`. We need to allocate additional space for
      # this in `enlarge_partition`.
      ((TOTAL_DLC_PREALLOCATE_SIZE += pre_alloc_size * 2))
    fi
  done
}

get_dlc_image_from_dlc_factory_cache() {
  local dlc_factory_cache_dir="$1"
  for d in "${dlc_factory_cache_dir}"/* ; do
    local image_path="${d}"/package/dlc.img

    if [ -f "${image_path}" ]; then
      CURRENT_DLC_IMG_PATHS+=( "${image_path}" )
      CURRENT_DLC_IDS+=( "$(basename "${d}")" )
    fi
  done
}

# Verify the DLC hash by calling `dlcverify`.
verify_dlc_hash() {
  local release_rootfs_mount_point="$1"
  log "DLCs to be verified: ${CURRENT_DLC_IDS[*]}"
  # `dlcverify` locates at stateful partition of the shim.
  local stateful_dev mount_point
  stateful_dev="$(get_usb_dev_stateful)"

  mount_point="$(mktemp -d)"
  mount -o ro "${stateful_dev}" "${mount_point}"

  local dlcverify_bin="${mount_point}"/dev_image/bin/dlcverify
  for index in ${!CURRENT_DLC_IMG_PATHS[*]}; do
    local verify_ret
    verify_ret="$("${dlcverify_bin}" --id="${CURRENT_DLC_IDS[${index}]}" \
                  --image="${CURRENT_DLC_IMG_PATHS[${index}]}" \
                  --rootfs_mount="${release_rootfs_mount_point}")"
    log "${verify_ret}"
    if echo "${verify_ret}" | grep -q ERROR; then
      die "DLC verify failed. The factory installed DLC images should be" \
          "extracted from the same release image on the DUT."
    fi
  done

  log "Verification succeeded!"

  umount "${stateful_dev}"
  rmdir "${mount_point}"
}

gbb_force_dev_mode() {
  # Set factory-friendly gbb flags 0x39, which contains
  # VB2_GBB_FLAG_DEV_SCREEN_SHORT_DELAY            0x00000001
  # VB2_GBB_FLAG_FORCE_DEV_SWITCH_ON               0x00000008
  # VB2_GBB_FLAG_FORCE_DEV_BOOT_USB                0x00000010
  # VB2_GBB_FLAG_DISABLE_FW_ROLLBACK_CHECK         0x00000020
  /usr/bin/futility gbb --flash --set --flags="+0x39"
}

# Echoes "on" or "off" based on the value of a crossystem Boolean flag.
crossystem_on_or_off() {
  local value
  if value="$(crossystem "$1" 2>/dev/null)"; then
    case "${value}" in
    "0")
      echo off
      ;;
    "1")
      echo on
      ;;
    *)
      echo "${value}"
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

command_to_yes_or_no() {
  "$@" >/dev/null 2>&1 && echo yes || echo no
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
      echo -n "EC ${type} version: "
      ectool version | grep "^${type} version" | sed -e 's/[^:]*: *//'
    done
    echo
    echo System time: "$(date)"
    local hwid="$(crossystem hwid 2>/dev/null)"
    echo "HWID: ${hwid:-(not set)}"
    echo -n "Dev mode: $(crossystem_on_or_off devsw_boot); "
    echo -n "Recovery mode: $(crossystem_on_or_off recoverysw_boot); "
    echo -n "HW write protect: $(crossystem_on_or_off wpsw_cur); "
    echo "SW write protect: $(command_to_yes_or_no check_host_swwp)"
    echo -n "EC SW write protect: $(command_to_yes_or_no check_ec_swwp); "
    echo -n "$(tpm_get_info)"
    echo
    local description=""
    if [ -s "${DESCRIPTION_FILE}" ]; then
      description="$(cat "${DESCRIPTION_FILE}" 2>/dev/null)"
      echo "${description}"
      echo
    fi
}

print_rma_info() {
  local stateful_dev mount_point cros_payload_path rma_metadata_path
  stateful_dev="$(get_usb_dev_stateful)"
  mount_point="$(mktemp -d)"
  mount -o ro "${stateful_dev}" "${mount_point}"

  cros_payload_path="${mount_point}"/cros_payloads
  rma_metadata_path="${cros_payload_path}"/rma_metadata.json
  if [ -f "${rma_metadata_path}" ]; then
    echo "RMA shim contains payloads from board:"
    jq -r '.[].board' "${rma_metadata_path}"
    local model
    model="$(cros_config / name)" || true
    echo "Board name: ${BOARD}"
    echo "Model Name: ${model}"
    if [ -f "${cros_payload_path}/${model}".json ]; then
      echo "Find corresponding RMA payloads by model name: ${model}"
    elif [ -f "${cros_payload_path}/${BOARD}".json ]; then
      echo "Find corresponding RMA payloads by board name: ${BOARD}"
    else
      echo "No payloads found for ${model} or ${BOARD}."
      echo "Did you set the wrong model/board name?"
    fi
  else
    echo "Skip checking RMA payloads since it is not a RMA shim."
  fi

  umount "${mount_point}"
  rmdir "${mount_point}"
}

# Checks if the given action is supported.
#
# Args:
#   $1: Single-character option name ("i" for install)
#   $2: String containing all supported options
is_supported_action() {
  echo "$1" | grep -q "^[$2]$"
}

# Displays a line in the menu. Used in the menu function.
#
# Args:
#   $1: Single-character option name ("i" for install)
#   $2: String containing all supported options
#   $3: Brief description
#   $4: Further explanation
menu_line() {
  echo -n "  "
  if is_supported_action "$1" "$2"; then
    colorize boldwhite
  else
    colorize red
  fi
  echo -n "$1  "
  colorize white
  printf "%-22s%s\n" "$3" "$4"
}

# Virtual function to show menu of board-specific actions.
menu_board() {
  return 0
}

# Displays a menu, saving the action (one of $1, always lowercase) in the
# "ACTION" variable. If no supported action is chosen, ACTION will be empty.
#
# Args:
#   $1: Supported actions
menu() {
  local supported_actions="$1"

  # Clear up terminal
  stty sane echo
  # Enable cursor (if tput is available)
  tput cnorm 2>/dev/null || true

  colorize white
  echo
  echo
  echo Please select an action and press Enter.
  echo

  menu_line i "${supported_actions}" \
              "Install" "Performs a network or USB install"
  menu_line r "${supported_actions}" \
              "Reset" "Performs a factory reset; finalized devices only"
  menu_line p "${supported_actions}" \
              "Custom reset process" "Performs a customized factory reset;" \
              "finalized devices only"
  menu_line b "${supported_actions}" \
              "Battery cutoff" "Performs a battery cutoff"
  menu_line s "${supported_actions}" \
              "Shell" "Opens bash; available only with developer firmware"
  menu_line v "${supported_actions}" \
              "View configuration" "Shows crossystem, VPD, etc."
  menu_line d "${supported_actions}" \
              "Debug info and logs" \
              "Shows useful debugging information and kernel/firmware logs"
  menu_line z "${supported_actions}" \
              "Zero (wipe) storage" "Makes device completely unusable"
  menu_line c "${supported_actions}" \
              "SeCure erase" \
              "Performs full storage erase, write a verification pattern"
  menu_line y "${supported_actions}" \
              "VerifY erase" \
              "Verifies the storage has been erased with option C"
  menu_line t "${supported_actions}" \
              "Reset TPM" "Call chromeos-tpm-recovery"
  menu_line e "${supported_actions}" \
              "Perform RSU" "Perform RSU (RMA Server Unlock)"
  menu_line m "${supported_actions}" \
              "Enable factory mode" "Enable TPM factory mode"
  menu_line f "${supported_actions}" \
              "Perform factory FAI" \
              "Perform Factory FAI (First Article Inspection)"

  menu_board

  colorize white
  echo
  read -p 'action> ' ACTION
  echo

  if ! is_supported_action "${ACTION}" "${supported_actions}"; then
    echo "Action [${ACTION}] is not supported."
    echo "Please select a supported action from the menu."
    ACTION=
  fi
}

#
# Action handlers
#

# i = Install.
action_i() {
  if is_netboot; then
    # Only set idle mode when both AC and battery are present.
    if ectool battery | grep -q "Flags.*AC_PRESENT.*BATT_PRESENT"; then
      ectool chargecontrol idle
    fi
  fi

  # `print_rma_info` prints the information about the usb shim.
  # Skip printing when using netboot.
  if ! is_netboot; then
    print_rma_info || true
  fi

  reset_chromeos_device

  clear_fwwp
  ensure_fwwp_consistency

  if [ -z "${FACTORY_INSTALL_FROM_USB}" ]; then

    colorize yellow
    connect_to_ethernet

    # Check for factory server override from tftp server.
    override_from_tftp

    # TODO(hungte) how to set time in RMA?
    set_time || die "Please check if the server is configured correctly."
  fi

  colorize green
  ufs_init
  get_dst_drive
  prepare_disk

  if [ -n "${FACTORY_INSTALL_FROM_USB}" ]; then
    factory_install_usb
  else
    factory_install_network
  fi

  sync
  # Some installation procedure may clear or reset NVdata, so we want to ensure
  # TPM will be cleared again.
  crossystem clear_tpm_owner_request=1 || true

  # The gbb flag which forces the dev switch on (0x8) is set when
  # (1) installing the firmware-updater and (2) doing RSU using gsc_reset.
  # However, if the factory bundle does not contain firmware-updater and at the
  # same time the hwwp is disabled via removing battery + action_m, then the
  # gbb flag will not be set. Therefore, after installation, the DUT will try
  # to boot into test image under normal mode. This results in 0x43 (see
  # b/199803466 for more info.) Though user can enable developer mode and boot
  # into test image, we decide to make it more user-friendly by setting the
  # gbb flag here.
  log "Setting user-friendly gbb flags 0x39..."
  gbb_force_dev_mode

  colorize green
  echo_huge_ok
  log "Factory Installer Complete."
  sync

  # Make sure the next boot will boot to the factory toolkit.
  crossystem dev_default_boot=disk
  factory_on_complete
  # Both kernel command line and lsb-factory can disable complete prompt.
  if is_complete_prompt_disabled || [ "${COMPLETE_PROMPT}" = "false" ] \
                                 || [ "${TTY}" = /dev/null ]; then
    sleep 3
  else
    printf "Press Enter to restart... "
    head -c 1 >/dev/null
  fi

  # Default action after installation: reboot.
  trap - EXIT

  # TPM factory mode can only be enabled when hardware write protection is
  # disabled. Assume we only do netboot in factory, so that in netboot
  # environment we don't need to enable factory mode because the device should
  # already be in factory mode.
  # TODO(chenghan) Figure out the use case of netboot besides factory process.
  if [ -z "${NETBOOT_RAMFS}" ] && ! check_hwwp; then
    # Enable factory mode if it's supported.
    # Enabling factory mode would trigger a reboot automatically and be halt
    # inside this function until reboots.
    enable_factory_mode_and_reboot
  fi

  # Try to do EC reboot. If it fails, do normal reboot.
  if [ -n "${NETBOOT_RAMFS}" ]; then
    # There is no 'shutdown' and 'init' in initramfs.
    ectool reboot_ec cold at-shutdown && busybox poweroff -f ||
      busybox reboot -f
  else
    ectool reboot_ec cold at-shutdown && shutdown -h now || shutdown -r now
  fi

  # sleep indefinitely to avoid re-spawning rather than shutting down
  sleep 1d
}

prepare_reset() {
  if [ -n "${NETBOOT_RAMFS}" ]; then
    # factory reset is not available in netboot mode.
    colorize red
    log "Not available in netboot."
    return
  fi

  # First check to make sure that the factory software has been wiped.
  local state_mount_point state_dev
  state_mount_point=/tmp/stateful
  mkdir -p /tmp/stateful
  get_dst_drive

  state_dev="$(make_partition_dev "${DST_DRIVE}" "${DST_STATE_PART}")"
  state_dev="$(get_path_to_lvm_stateful "${state_dev}")"
  log "Device path to stateful partition: ${state_dev}"

  # If powerwash (partnum=11) partition formatted as ext4, this device should
  # have been created as dm-default-key stateful. For this kind of devices,
  # as the stateful is encrypted, it is impossible to mount it like LVM/non-LVM
  # case to try to put DLC back. Re-format it into pure ext4 first and create
  # unencrypted/dlc-factory-images directory again so that below logic stays
  # the same.
  powerwash_dev="$(make_partition_dev "${DST_DRIVE}" "${DST_POWERWASH_PART}")"
  if blkid -o value -s TYPE "${powerwash_dev}" | grep -q "ext4"; then
    # Zero out the first block to prevent from mkfs confusion.
    dd if=/dev/zero of="${state_dev}" bs=1M count=1 > /dev/null 2>&1
    mkfs.ext4 -L "H-STATE" "${state_dev}"

    # Also zero out & mkfs on powerwash partition to discard previous session
    # crypto information.
    dd if=/dev/zero of="${powerwash_dev}" bs=1M count=1 > /dev/null 2>&1
    mkfs.ext4 "${powerwash_dev}"
  fi

  # Mount as rw since we might need to copy factory installed DLC images
  # to the stateful partition.
  mount -o rw "${state_dev}" "${state_mount_point}"

  # In case we need to re-create unencrypted directory
  mkdir -p "${state_mount_point}"/unencrypted

  local factory_exists=false
  [ -e "${state_mount_point}"/dev_image/factory ] && factory_exists=true

  if ${factory_exists}; then
    colorize red
    log "Factory software is still installed (device has not been finalized)."
    log "Unable to perform factory reset."
    return
  fi

  check_host_swwp && check_ec_swwp || ! is_pvt_phase || \
    die "SW write protect is not enabled in the device with PVT phase."

  # Then check if the release rootfs has factory installed DLC metadata.
  # If so, we copy the DLC images from network or RMA shim.
  local release_rootfs_mount_point release_dev
  release_rootfs_mount_point=/tmp/release_rootfs
  mkdir -p "${release_rootfs_mount_point}"
  release_dev="$(make_partition_dev "${DST_DRIVE}" \
                "${DST_RELEASE_ROOTFS_PART}")"
  mount -o ro "${release_dev}" "${release_rootfs_mount_point}"
  get_expected_factory_installed_dlc "${release_rootfs_mount_point}"
  log "Number of factory installed DLC metadata: ${#EXPECTED_DLC_IDS[@]}"
  local dlc_stateful="${state_mount_point}"/unencrypted/dlc-factory-images
  if [ -d "${dlc_stateful}" ]; then
    log "Remove existing factory installed DLC images on stateful partition..."
    rm -rf "${dlc_stateful}"
  fi
  if [ ${#EXPECTED_DLC_IDS[@]} -gt 0 ]; then
    # Install the factory installed DLC images to dlc_path.
    local dlc_path=/tmp/dlc
    mkdir -p "${dlc_path}"
    log "Get factory installed DLC images from usb/network..."
    if [ -n "${FACTORY_INSTALL_FROM_USB}" ]; then
      factory_install_usb "${dlc_path}"
    else
      connect_to_ethernet
      factory_install_network "${dlc_path}"
    fi
    # Unzip the DLC and verify their hashes.
    log "Unpack factory installed DLC images to ${dlc_path}"
    tar -xpvf "${dlc_path}"/release_image.dlc_factory_cache -C "${dlc_path}"
    get_dlc_image_from_dlc_factory_cache \
      "${dlc_path}"/unencrypted/dlc-factory-images
    # Check if all elements in EXPECTED_DLC_IDS are present in CURRENT_DLC_IDS
    all_found=true
    for expected_id in "${EXPECTED_DLC_IDS[@]}"; do
      found=false
      for current_id in "${CURRENT_DLC_IDS[@]}"; do
        if [ "${expected_id}" = "${current_id}" ]; then
          found=true
          break
        fi
      done
      if ! ${found}; then
        all_found=false
        break
      fi
    done
    if ! ${all_found}; then
      die "Not all expected DLCs found. Expected: ${EXPECTED_DLC_IDS[*]}, " \
          "Current: ${CURRENT_DLC_IDS[*]}."
    fi

    verify_dlc_hash "${release_rootfs_mount_point}"
    log "Install the DLCs to stateful partition..."
    mv "${dlc_path}"/unencrypted/dlc-factory-images \
       "${state_mount_point}"/unencrypted
  else
    log "No factory installed DLC metadata found. Skip copying..."
  fi

  umount "${state_mount_point}"
  umount "${release_rootfs_mount_point}"

  reset_chromeos_device
}
# r = Factory reset.
action_r() {
  prepare_reset
  crossystem disable_dev_request=1

  log "Performing factory reset"
  if ! /usr/sbin/factory_installer factory-reset reset; then
    die "Factory reset failed."
  fi

  log "Done."
  # TODO(hungte) shutdown or reboot once we decide the default behavior.
  exit_success
}

# p = Custom reset process
action_p() {
  prepare_reset
  crossystem disable_dev_request=1

  log "Performing custom factory reset"
  if ! /usr/sbin/factory_installer custom-reset-process; then
    die "Custom factory reset failed."
  fi

  log "Done."
  # TODO(hungte) shutdown or reboot once we decide the default behavior.
  exit_success
}

# b = Battery cutoff.
action_b() {
  crossystem disable_dev_request=1

  # Apply the cutoff settings in toolkit config.
  /usr/sbin/factory_installer battery-cutoff
}

# s = Shell.
action_s() {
  if ! is_allow_debug && ! is_dev_firmware; then
    colorize red
    echo "Cannot open a shell (need devfw [Ctrl-U] or cros_debug build)."
    explain_cros_debug
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
  bash || true
}

# v = View configuration.
action_v() {
  (
    print_device_info

    local temp_config
    temp_config="$(mktemp)"
    echo "${FAI_VPD_CONFIG}" > "${temp_config}"
    echo
    echo "VPD:"
    /usr/sbin/factory_installer fai --config-path "${temp_config}" || true
    rm "${temp_config}"

    echo
    echo "crossystem:"
    crossystem || true

    echo
    echo "lsb-factory:"
    cat /mnt/stateful_partition/dev_image/etc/lsb-factory || true
  ) 2>&1 | secure_less.sh
}

# d = Debug info and logs.
action_d() {
  /usr/sbin/factory_debug.sh
}

# Confirm and erase the fixed drive.
#
# Identify the fixed drive, ask confirmation and call
# factory_disk_action function.
#
# Args:
#   action: describe how to erase the drive.
erase_drive() {
  local action="$1"
  if [ -n "${NETBOOT_RAMFS}" ]; then
    # factory reset is not available in netboot mode.
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
  if [ "${yes_or_no}" = yes ]; then
    factory_disk_action "${action}"
  else
    echo "You did not type 'yes'. Cancelled."
  fi
}

# z = Zero
action_z() {
  erase_drive wipe
}

# c = SeCure
action_c() {
  erase_drive secure
}

# y = VerifY
action_y() {
  if [ -n "${NETBOOT_RAMFS}" ]; then
    # factory reset is not available in netboot mode.
    colorize red
    log "Not available in netboot."
    return
  fi
  get_dst_drive
  factory_disk_action verify
}

# t = Reset TPM
action_t() {
  chromeos-tpm-recovery
}

# e = Perform RSU
action_e() {
  echo
  log "Follow OEM-provided instructions to temporarily disable " \
      "write-protection and restart to proceed."
  log "If you are a large-scale repair center, press any key to continue RSU."
  read -N 1
  if [ "${RMA_AUTORUN}" = "true" ]; then
    # This will make DUT boot to the shim directly after RSU succeds, which will
    # enable VB2_GBB_FLAG_FORCE_DEV_BOOT_USB.
    crossystem dev_default_boot=usb
  fi
  tpm_perform_rsu
}

# Enable TPM factory mode, force dev mode, and reboot at the end.
# Args:
#   default_boot_usb: set to "true" to enable boot from USB by default.
enable_factory_mode_and_reboot() {
  local default_boot_usb="$1"

  tpm_enable_factory_mode

  # Force the next boot to be in developer mode so that we can boot to RMA shim
  # again.
  gbb_force_dev_mode

  if [ "${default_boot_usb}" = "true" ]; then
    # This will make DUT boot to the shim directly since we've already enabled
    # VB2_GBB_FLAG_FORCE_DEV_BOOT_USB.
    crossystem dev_default_boot=usb
  fi

  reboot

  # sleep indefinitely to avoid re-spawning rather than reboot.
  sleep 1d
}

# m = Enable TPM factory mode
action_m() {
  # If RMA_AUTORUN is true, assume the user is using an RMA shim and would like
  # to automatically boot into the shim again after reboot. Hence we also
  # configure the device to boot from USB by default.
  local default_boot_usb="${RMA_AUTORUN}"
  enable_factory_mode_and_reboot "${default_boot_usb}"
}

# f = Perform Factory FAI process
action_f() {
  /usr/sbin/factory_installer fai --save-to-usb
  colorize yellow
  log "Please upload the FAI data to issue tracker for Google review."
  colorize white
}

try_set_default_action_rsu_required() {
  if is_rsu_required; then
    log "RSU is required."
    log "Set default action to (e) Perform RSU."
    DEFAULT_ACTION=e
    return 0
  fi
  return 1
}

try_set_default_action_wp_enabled() {
  if check_hwwp; then
    if [ "$(tpm_get_chassis_open)" = "true" ]; then
      log "Chassis is opened."
      log "Set default action to (m) Enable TPM factory mode."
      DEFAULT_ACTION=m
      return 0
    else
      log "Hardware write protection on."
      log "Set default action to (e) Perform RSU."
      DEFAULT_ACTION=e
      return 0
    fi
  fi
  return 1
}

set_default_action_install() {
  log "Hardware write protection off. "
  log "Set default action to (i) Install."
  DEFAULT_ACTION=i
  return 0
}

read_default_rma_autorun_tpm_support_option() {
  try_set_default_action_rsu_required || \
    try_set_default_action_wp_enabled || \
    set_default_action_install
}

read_default_option() {
  # Read default options
  if [ "${NETBOOT_RAMFS}" = 1 ]; then
    log "Netbooting."
    log "Set default action to (i) Install."
    DEFAULT_ACTION=i
  elif [ "${RMA_AUTORUN}" = "true" ]; then
    case "$(tpm_check_rsu_support)" in
      unsupported)
        if check_hwwp; then
          log "Hardware write protection on."
          log "RSU is not supported, please disable hardware write protect."
          DEFAULT_ACTION=""
        else
          log "Hardware write protection off."
          log "Set default action to (i) Install."
          DEFAULT_ACTION=i
        fi
        ;;
      supported)
        read_default_rma_autorun_tpm_support_option
        ;;
    esac
  fi
}

main() {
  if [ "$(id -u)" -ne 0 ]; then
    echo "You must run this as root."
    exit 1
  fi
  config_tty || true  # Never abort if TTY has problems.

  log "Starting Factory Installer."
  # TODO: do we still need this call now that the kernel was tweaked to
  # provide a good light level by default?
  lightup_screen

  load_modules

  colorize white
  clear

  # Check for any configuration overrides.
  overrides

  local supported_actions
  supported_actions="$(get_supported_actions)"
  read_default_option

  # Validate the default action.
  if [ -n "${DEFAULT_ACTION}" ]; then
    log "Default action: [${DEFAULT_ACTION}]."
    if ! is_supported_action "${DEFAULT_ACTION}" "${supported_actions}"; then
      log "Action [${DEFAULT_ACTION}] is not supported."
      log "Only support ${supported_actions}."
      log "Will fallback to normal menu..."
      DEFAULT_ACTION=""
      sleep 3
    fi
  fi

  while true; do
    clear
    print_header

    # If default action is set and ACTION_COUNTDOWN=true, give the user some
    # time to abort the default action.
    local do_default_action=false
    if [ -n "${DEFAULT_ACTION}" ]; then
      do_default_action=true
      log "Will automatically perform action [${DEFAULT_ACTION}]."
      if [ "${ACTION_COUNTDOWN}" = "true" ]; then
        # Give the user the chance to press any key to abort the default action
        # and display the menu.
        log "Press any key to show menu instead..."
        local timeout_secs=3
        for i in $(seq "${timeout_secs}" -1 1); do
          # Read with timeout doesn't reliably work multiple times without
          # a sub shell.
          if ( read -N 1 -p "Press any key within ${i} sec> " -t 1 ); then
            echo
            do_default_action=false
            break
          fi
          echo
        done
      fi
    fi

    if ${do_default_action}; then
      # Default action is set and not aborted: perform the default action.
      "action_${DEFAULT_ACTION}"
    else
      # Print messages when the actions are restricted.
      print_restricted_supported_actions_message

      # Display the menu for the user to select an option.
      menu "${supported_actions}"
      if [ -n "${ACTION}" ]; then
        # Perform the selected action.
        "action_${ACTION}"
      fi
    fi

    colorize white
    read -N 1 -p "Press any key to continue> "
  done
}
main "$@"
