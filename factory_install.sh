#!/bin/bash

# Copyright 2015 The Chromium OS Authors. All rights reserved.
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

. "/usr/sbin/write_gpt.sh"
. "$(dirname "$0")/factory_common.sh"
. "$(dirname "$0")/ping_shopfloor.sh"

# Variables from dev_image/etc/lsb-factory that developers may want to override.
#
# - Override this if we want to install with a board different from installer
BOARD="$(findLSBValue CHROMEOS_RELEASE_BOARD)"
# - Override this if we want to install with a different omaha server.
OMAHA="$(findLSBValue CHROMEOS_AUSERVER)"
# - Override this to switch between legacy mode and new cros_payload mode.
USE_CROS_PAYLOAD="$(findLSBValue USE_CROS_PAYLOAD)"
# - Override this for The default action if no keys were pressed before timeout.
DEFAULT_ACTION="$(findLSBValue FACTORY_INSTALL_DEFAULT_ACTION)"

# Variables prepared by make_factory_package or netboot initramfs code.
NETBOOT_RAMFS="$(findLSBValue NETBOOT_RAMFS)"
FACTORY_INSTALL_FROM_USB="$(findLSBValue FACTORY_INSTALL_FROM_USB)"

# Global variables
DST_DRIVE=""
EC_PRESENT=0
DEVSW_PRESENT=1
COMPLETE_SCRIPT=""

# The ethernet interface to be used. It will be determined in
# check_ethernet_status and be passed to ping_shopfloor as an
# environment variable.
ETH_INTERFACE=""

# Definition of ChromeOS partition layout
DST_FACTORY_KERNEL_PART=2
DST_FACTORY_PART=3
DST_RELEASE_KERNEL_PART=4
DST_STATE_PART=1

# Supported actions (a set of lowercase characters)
# Each action x is implemented in an action_$x handler (e.g.,
# action_i); see the handlers for more information about what
# each option is.
SUPPORTED_ACTIONS=cdirsvyz

if [ -z "${USE_CROS_PAYLOAD}" ] || [ -z "${FACTORY_INSTALL_FROM_USB}" ] ; then
  . "$(dirname "$0")/factory_install_legacy.sh"
fi

# Define our own logging function.  The one from memento_updater writes to
# a file, which may cause a race condition if we tail it.
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
  if grep -qw "cros_debug" /proc/cmdline 2>/dev/null; then
    while true; do sh; done
  else
    log "To debug with a shell, add 'cros_debug' to your kernel command line."
    log " - Factory netboot: Change kernel command line config file on TFTP."
    log " - Factory install shim: Add cros_debug into --boot_args."
  fi

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

config_tty() {
  stty opost

  # Turn off VESA blanking
  setterm -blank 0 -powersave off -powerdown 0
}

clear_fwwp() {
  log "Firmware Write Protect disabled, clearing status registers."
  if [ ${EC_PRESENT} -eq 1 ]; then
    flashrom -p ec --wp-disable
  fi
  flashrom -p host --wp-disable
  log "WP registers should be cleared now"
}

ensure_fwwp_consistency() {
  local ec_wp main_wp

  if [ ${EC_PRESENT} -eq 0 ]; then
    return
  fi

  ec_wp="$(flashrom -p ec --wp-status 2>/dev/null)" || return
  main_wp="$(flashrom -p host --wp-status 2>/dev/null)"
  ec_wp="$(echo "${ec_wp}" | sed -nr 's/WP.*(disabled|enabled).$/\1/pg')"
  main_wp="$(echo "${main_wp}" | sed -nr 's/WP.*(disabled|enabled).$/\1/pg')"
  if [ "${ec_wp}" != "${main_wp}" ]; then
    die "Inconsistent firmware write protection status: " \
        "main=${main_wp}, ec=${ec_wp}." \
        "Please disable Hardware Write Protection and restart again."
  fi
}

# Checks if firmware software write protection is enabled.
# Args
#   target: a "flashrom -p" descriptor. Defaults to "host".
check_fwwp() {
  local target="${1:-host}"
  # Note "crossystem sw_wpsw_boot" only works on Baytrail systems, so we have to
  # use flashrom for better compatibility.
  flashrom -p "${target}" --wp-status 2>/dev/null |
    grep -q "write protect is enabled"
}

set_time() {
  log "Setting time from:"
  # Extract only the server and port.
  local time_server_url="${OMAHA%/update}"
  local time_server_port="${time_server_url#http://}"

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
  log "Clearing NVData."
  if ! mosys nvram clear; then
    # Not every platforms really need this - OK if nvram is not cleared.
    log "Warning: NVData not cleared."
  fi

  clear_block_devmode

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
  ensure_fwwp_consistency
}

get_dst_drive() {
  load_base_vars
  DST_DRIVE="$(get_fixed_dst_drive)"
  if [ -z "${DST_DRIVE}" ]; then
    die "Cannot find fixed drive."
  fi
}

lightup_screen() {
  # Light up screen in case you can't see our splash image.
  backlight_tool --set_brightness_percent=100 || true
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

  # Informs OS of partition table changes. The autoupdater has trouble with loop
  # devices.
  log "Reloading partition table changes..."
  sync
  sleep 3  # Wait for sync to take place.
  partprobe "${DST_DRIVE}"

  log "Done preparing disk"
}

select_board() {
  # Prompt the user if USER_SELECT is true.
  local user_select="$(findLSBValue USER_SELECT | tr '[:upper:]' '[:lower:]')"
  if [ "${user_select}" = "true" ]; then
    echo -n "Enter the board you want to install (ex: x86-mario): "
    read BOARD
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
  # OMAHA and BOARD are override env variables used when calling
  # memento_updater.
  local omaha=""
  if omaha="$(find_var omahaserver)"; then
    log " Kernel cmdline OMAHA override to ${omaha}"
    OMAHA="${omaha}"
  fi

  local board=""
  if board="$(find_var cros_board)"; then
    log " Kernel cmdline BOARD override to ${board}"
    BOARD="${board}"
  fi
}

override_from_board() {
  # Call into any board specific configuration settings we may need.
  # These will be provided by chromeos-bsp-[board] build in the private overlay.
  local lastboard="${BOARD}"
  if [ -f "/usr/sbin/board_customize_install.sh" ]; then
    . /usr/sbin/board_customize_install.sh
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
  # OMAHA is override env variables used when calling memento_updater.
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
      OMAHA="$(cat "${tftp_output}")"
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
  # TODO(hungte) do this in memento_updater or postinst may be better
  if ! cgpt add -i "${DST_RELEASE_KERNEL_PART}" -P 0 -T 0 -S 0 "${DST_DRIVE}"
  then
    # Destroy kernels otherwise the system is still bootable.
    dst="$(make_partition_dev "${DST_DRIVE}" "${DST_RELEASE_KERNEL_PART}")"
    dd if=/dev/zero of="${dst}" bs=1M count=1
    dst="$(make_partition_dev "${DST_DRIVE}" "${DST_FACTORY_KERNEL_PART}")"
    dd if=/dev/zero of="${dst}" bs=1M count=1
    die "Failed to lock release image. Destroy all kernels."
  fi
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

  mount "${stateful_dev}" "${mount_point}"
  mkdir -p "$(dirname "${output_file}")"

  # Update lsb-factory on stateful partition.
  local lsb_factory="${mount_point}/dev_image/etc/lsb-factory"
  if [ -z "${FACTORY_INSTALL_FROM_USB}" ]; then
    log "Save active Omaha server URL to stateful partition lsb-factory."
    echo "FACTORY_OMAHA_URL=${OMAHA}" >> "${lsb_factory}"
  else
    log "Clone lsb-factory to stateful partition."
    cat "${FACTORY_LSB_FILE}" >>"${lsb_factory}"
  fi

  # Gain some space.
  local portage_db="mount_point}/var_overlay/db/pkg"
  if [ -d "${portage_db}" ]; then
    rm -rf "${portage_db}"
  fi

  # TODO(hungte) This is a dirty hack. This should be done only in factory
  # wiping process in future.
  # Extract release_image.part1, which may be gz or bz2.
  local release_part1_src="${mount_point}/cros_payloads/release_image.part1"
  local command=""
  if [ -f "${release_part1_src}.gz" ]; then
    release_part1_src="${release_part1_src}.gz"
    command="gzip"
  elif [ -f "${release_part1_src}.bz2" ]; then
    release_part1_src="${release_part1_src}.bz2"
    command="bzip2"
  fi

  if [ -n "${command}" ]; then
    local release_part1="$(mktemp)"
    "${command}" -cd "${release_part1_src}" >"${release_part1}"

    local release_stateful="$(mktemp -d)"
    mount -o ro "${release_part1}" "${release_stateful}"
    local extensions_path="unencrypted/import_extensions"
    rm -rf "${mount_point}/${extensions_path}"
    mkdir -p "${mount_point}/unencrypted"
    cp -prf "${release_stateful}/${extensions_path}" \
      "${mount_point}/${extensions_path}"

    rm "${release_part1}"
    umount "${release_stateful}" || true
    rmdir "${release_stateful}" || true
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
  curl "${OMAHA%/update}/greetings/${message}/${uuid}" >/dev/null 2>&1 || true
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

disable_dev_switch() {
  # Turn off developer mode on devices without physical developer switch.
  if [ ${DEVSW_PRESENT} -eq 0 ]; then
    crossystem disable_dev_request=1
  # When physical switch exists, force user to turn it off.
  elif [ ${DEVSW_PRESENT} -eq 1 -a "$(crossystem devsw_cur)" = "1" ]; then
    while [ "$(crossystem devsw_cur)" = "1" ]; do
      log "Please turn off developer switch to continue"
      sleep 5
    done
  fi
}

factory_reset() {
  # Turn off developer mode on devices without physical developer switch.
  if [ ${DEVSW_PRESENT} -eq 0 ]; then
    crossystem disable_dev_request=1
  fi

  log "Performing factory reset"
  if ! /usr/sbin/factory_reset.sh "${DST_DRIVE}"; then
    die "Factory reset failed."
  fi

  log "Done."
  # TODO(hungte) shutdown or reboot once we decide the default behavior.
  exit_success
}

# Call reset code on the fixed driver.
#
# Assume the global variable DST_DRIVE contains the drive to operate on.
#
# Args:
#   action: describe how to erase the drive.
#     Allowed actions:
#     - wipe: action Z
#     - secure: action C
#     - verify: action Y
factory_disk_action() {
  local action="$1"
  log "Performing factory disk ${action}"
  if ! /usr/sbin/factory_reset.sh "${DST_DRIVE}" "${action}"; then
    die "Factory disk ${action} failed."
  fi
  log "Done."
  exit_success
}

factory_install_cros_paylod() {
  local src_media="$1"
  local src_mount=""
  local tmp_dir="$(mktemp -d)"
  local json_url="${src_media}/${BOARD}.json"

  if [ -b "${src_media}" ]; then
    src_mount="$(mktemp -d)"
    colorize yellow
    mount -o ro "${src_media}" "${src_mount}"
    json_url="${src_mount}/cros_payloads/${BOARD}.json"
  fi

  # Generate the uuid for current install session
  local uuid="$(uuidgen 2>/dev/null)" || uuid="Not_Applicable"

  # Say hello to server if available.
  omaha_greetings "hello" "${uuid}"

  cros_payload install "${json_url}" "${DST_DRIVE}" \
    test_image release_image toolkit hwid
  cros_payload install "${json_url}" "${tmp_dir}" \
    firmware complete
  # The release_image.part1 contains pre-downloaded extensions to preserve.
  cros_payload download "${json_url}" "${DST_DRIVE}" \
    release_image.part1

  if [ -n "${src_mount}" ]; then
    umount "${src_mount}"
  fi
  colorize green

  # Notify server that all downloads are completed.
  omaha_greetings "download_complete" "${uuid}"

  # Disable release partition and activate factory partition
  disable_release_partition
  run_postinst "$(make_partition_dev "${DST_DRIVE}" "${DST_FACTORY_PART}")"
  stateful_postinst "$(make_partition_dev "${DST_DRIVE}" "${DST_STATE_PART}")"

  if [ -s "${tmp_dir}/firmware" ]; then
    log "Found firmware updater."
    # TODO(hungte) Check if we need to run --mode=recovery if WP is enabled.
    sh "${tmp_dir}/firmware" --force --mode=factory_install ||
      die "Firmware updating failed."
  fi
  if [ -s "${tmp_dir}/complete" ]; then
    log "Found completion script."
    COMPLETE_SCRIPT="${tmp_dir}/complete"
  fi

  # After post processing, notify server a installation session has been
  # successfully completed.
  omaha_greetings "goodbye" "${uuid}"
}

factory_install_usb() {
  local src_dev="$(findLSBValue REAL_USB_DEV)"
  [ -n "${src_dev}" ] || src_dev="$(rootdev -s 2>/dev/null)"
  [ -n "${src_dev}" ] ||
    die "Unknown media source. Please define REAL_USB_DEV."

  # Switch to stateful partition.
  # shellcheck disable=SC2001
  src_dev="$(echo "${src_dev}" | sed 's/[0-9]\+$/1/')"

  factory_install_cros_paylod "${src_dev}"
}

factory_install_omaha() {
  local ping_result return_code=0

  # Ping shopfloor server
  ping_result=$(ETH_INTERFACE=${ETH_INTERFACE} ping_shopfloor 2>/dev/null) ||
      return_code="$?"
  if [ "${return_code}" = "" ]; then
    eval "${ping_result}"
  fi

  # Register to Overlord if haven't.
  if [ -z "${OVERLORD_READY}" ]; then
    register_to_overlord "${OMAHA}" "${TTY_FILE}" "${LOG_FILE}"
  fi

  # Check if remote server supports cros_payload.
  local cros_payload_base="${OMAHA%/update}/static"
  if curl -f "${cros_payload_base}/${BOARD}.json" >/dev/null 2>&1; then
    factory_install_cros_paylod "${cros_payload_base}"
  else
    factory_install_omaha_old
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
    echo -n "HW write protect: $(crossystem_on_or_off wpsw_boot); "
    echo "SW write protect: $(command_to_yes_or_no check_fwwp host)"
    echo -n "EC present: $(bool_to_yes_or_no "${EC_PRESENT}"); "
    if [ "${EC_PRESENT}" = "1" ]; then
      echo -n "EC SW write protect: $(command_to_yes_or_no check_fwwp ec); "
    fi
    echo "Dev switch present: $(bool_to_yes_or_no "${DEVSW_PRESENT}")"
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

# Checks if the given action is valid and supported.
is_valid_action() {
  echo "$1" | grep -q "^[${SUPPORTED_ACTIONS}]$"
}

# Displays a menu, saving the action (one of ${SUPPORTED_ACTIONS}, always
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
  menu_line D "Debug info and logs" \
              "Shows useful debugging information and kernel/firmware logs"
  menu_line Z "Zero (wipe) storage" "Makes device completely unusable"
  menu_line C "SeCure erase" \
              "Performs full storage erase, write a verification pattern"
  menu_line Y "VerifY erase" \
              "Verifies the storage has been erased with option C"

  echo
  read -p 'action> ' ACTION
  echo
  # busybox tr may not have '[:upper:]'.
  # shellcheck disable=SC2019 disable=SC2018
  ACTION="$(echo "${ACTION}" | tr 'A-Z' 'a-z')"

  if is_valid_action "${ACTION}"; then
      return
  fi
  echo "Invalid action; please select an action from the menu."
  ACTION=
}

#
# Action handlers
#

# I = Install.
action_i() {
  reset_chromeos_device

  if [ -z "${FACTORY_INSTALL_FROM_USB}" ]; then

    colorize yellow
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

    # Check for OMAHA override from tftp server.
    override_from_tftp

    # TODO(hungte) how to set time in RMA?
    set_time || die "Please check if the server is configured correctly."
  fi

  colorize green
  get_dst_drive
  prepare_disk
  select_board

  if [ -z "${FACTORY_INSTALL_FROM_USB}" ]; then
    factory_install_omaha
  elif [ -n "${USE_CROS_PAYLOAD}" ]; then
    factory_install_usb
  else
    factory_install_usb_old
  fi

  log "Factory Installer Complete."
  sync
  sleep 3
  factory_on_complete

  # Some installation procedure may clear or reset NVdata, so we want to ensure
  # TPM will be cleared again.
  crossystem clear_tpm_owner_request=1 || true

  # Default action after installation: reboot.
  trap - EXIT
  sync
  sleep 3
  if [ -n "${NETBOOT_RAMFS}" ]; then
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
  if [ -n "${NETBOOT_RAMFS}" ]; then
    # factory_reset.sh script is not available in netboot mode.
    colorize red
    log "Not available in netboot."
    return
  fi

  # First check to make sure that the factory software has been wiped.
  MOUNT_POINT=/tmp/stateful
  mkdir -p /tmp/stateful
  get_dst_drive
  mount -o ro "$(make_partition_dev "${DST_DRIVE}" "${DST_STATE_PART}")" \
    "${MOUNT_POINT}"

  local factory_exists=false
  [ -e ${MOUNT_POINT}/dev_image/factory ] && factory_exists=true
  umount "${MOUNT_POINT}"

  if ${factory_exists}; then
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
  bash || true
}

# V = View configuration.
action_v() {
  (
    print_device_info

    for partition in RO_VPD RW_VPD; do
      echo
      echo "${partition} contents:"
      vpd -i "${partition}" -l || true
    done

    echo
    echo "crossystem:"
    crossystem || true

    echo
    echo "lsb-factory:"
    cat /mnt/stateful_partition/dev_image/etc/lsb-factory || true
  ) 2>&1 | secure_less.sh
}

# D = Debug info and logs.
action_d() {
  (
    echo "## Information in this commend:"
    echo "##   dev_debug_vboot"
    echo "##     date"
    echo "##     crossystem --all"
    echo "##     rootdev -s"
    echo "##     la -aCF /root"
    echo "##     ls -aCF /mnt/stateful_partition"
    echo "##     cgpt show /dev/mmcblk0"
    echo "##     cgpt show /dev/sda"
    echo "##     etc..."
    echo "##   firmware event log"
    echo "##   /sys/firmware/log"
    echo "##   kernel log"
    echo "##   SMART log"
    echo "##   checksums"

    echo
    echo "## dev_debug_vboot:"
    dev_debug_vboot || true

    echo
    echo "## debug_vboot_noisy.log:"
    cat /var/log/debug_vboot_noisy.log || true

    echo
    echo "## firmware event log:"
    mosys eventlog list || true

    echo
    echo "## /sys/firmware/log:"
    cat /sys/firmware/log || true

    echo
    echo "## kernel log:"
    dmesg || true

    echo
    echo "## SMART log:"
    smartctl -x /dev/sda || true

    echo
    echo "## checksums:"
    md5sum /dev/mmcblk0p2 /dev/mmcblk0p4 || true
    md5sum /dev/sda2 /dev/sda4 || true
  ) 2>&1 | secure_less.sh
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
  if [ "${yes_or_no}" = yes ]; then
    factory_disk_action "${action}"
  else
    echo "You did not type 'yes'. Cancelled."
  fi
}

# Z = Zero
action_z() {
  erase_drive wipe
}

# C =SeCure
action_c() {
  erase_drive secure
}

# Y = VerifY
action_y() {
  if [ -n "${NETBOOT_RAMFS}" ]; then
    # factory_reset.sh script is not available in netboot mode.
    colorize red
    log "Not available in netboot."
    return
  fi
  get_dst_drive
  factory_disk_action verify
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

  test_ec_flash_presence
  test_devsw_presence

  # Check for any configuration overrides.
  overrides

  # Read default options
  if [ "${NETBOOT_RAMFS}" = 1 ]; then
    log "Netbooting. Set default action to (I)nstall."
    DEFAULT_ACTION=i
  fi

  # Sanity check default action
  if [ -n "${DEFAULT_ACTION}" ]; then
    clear
    print_header
    log "Default action: [${DEFAULT_ACTION}]."
    if ! is_valid_action "${DEFAULT_ACTION}"; then
      log "Action [${DEFAULT_ACTION}] is invalid."
      log "Only support ${SUPPORTED_ACTIONS}. Will fallback to normal menu..."
      DEFAULT_ACTION=""
      sleep 3
    fi
  fi

  # Automatically perform the default action; but first give the user the chance
  # to press any key to display the menu.
  if [ -n "${DEFAULT_ACTION}" ]; then
    log "Will automatically perform action [${DEFAULT_ACTION}]."
    log "Or press any key to show menu instead..."
    local prevent_default_action=false
    local timeout_secs=3
    for i in $(seq ${timeout_secs} -1 1); do
      # Read with timeout doesn't reliably work multiple times without
      # a sub shell.
      if ( read -N 1 -p "Press any key within ${i} sec> " -t 1 ); then
        echo
        prevent_default_action=true
        break
      fi
      echo
    done
    if ! ${prevent_default_action}; then
      # No key pressed: perform the default action (which should never return)
      action_${DEFAULT_ACTION}
    fi
  fi

  while true; do
    # Display the header and UI.
    clear
    print_header
    menu

    if [ -n "${ACTION}" ]; then
      # Perform the selected action.
      "action_${ACTION}"
    fi

    colorize white
    read -N 1 -p "Press any key to continue> "
  done
}
main "$@"
