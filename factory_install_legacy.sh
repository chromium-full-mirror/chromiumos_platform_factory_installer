#!/bin/bash

# Copyright 2017 The Chromium OS Authors. All rights reserved.
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.

. "/usr/sbin/write_gpt.sh"
. "/opt/google/memento_updater/memento_updater_logging.sh"
. "/opt/google/memento_updater/find_omaha.sh"

# Tail memento output to both our stdout and the log file, since
# there's no better way to display of download/install for individual
# partitions.
tail -F "${MEMENTO_AU_LOG}" 2>/dev/null | tee -a "${LOG_FILE}" &

# Definition of ChromeOS partition layout
DST_FACTORY_KERNEL_PART=2
DST_FACTORY_PART=3
DST_RELEASE_KERNEL_PART=4
DST_RELEASE_PART=5
DST_OEM_PART=8
DST_EFI_PART=12
DST_STATE_PART=1

# Mount point for installer resources.
RESOURCES_MOUNT_POINT="$(mktemp -d)"
USB_INSTALL_OFFSET="$(findLSBValue FACTORY_INSTALL_USB_OFFSET)"

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
  mount -o ro -t ext2 "${INSTALLED_ROOT}" "${CHROOT_PATH}"
  mount "${INSTALLED_STATEFUL}" "${CHROOT_PATH}"/mnt/stateful_partition
  if [ -s "${HWID_SCRIPT}" ]; then
    cp "${HWID_SCRIPT}" "${CHROOT_PATH}${HWID_COPIED_PATH}"
  fi
  if [ -s "${FIRMWARE_SCRIPT}" ]; then
    cp "${FIRMWARE_SCRIPT}" "${CHROOT_PATH}${FIRMWARE_COPIED_PATH}"
  fi
  cp -p /bin/netboot_postinst.sh "${CHROOT_PATH}${POST_PROCESS_SCRIPT}"
  chroot "${CHROOT_PATH}" "${POST_PROCESS_SCRIPT}"
  umount "${CHROOT_PATH}/mnt/stateful_partition" || true
  umount "${CHROOT_PATH}" || true
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

  # If there is nothing assigned, we should load firmware from release rootfs;
  # otherwise follow the assigned location (currently only stateful partition is
  # supported).
  if [ -z "${updater}" ]; then
    updater="${mount_point}/usr/sbin/chromeos-firmwareupdate"
    install_dev="$(make_partition_dev "${install_drive}" "${DST_RELEASE_PART}")"
    mount_opt="-t ext2 -o ro"
  elif [ "${updater}" != "${stateful_updater}" ]; then
    updater="${mount_point}/${stateful_updater}"
    install_dev="$(make_partition_dev "${install_drive}" "${DST_STATE_PART}")"
    # Stateful partition may be ext4 and turned off ext2 compatibility mode.
    mount_opt="-o ro"
  else
    die "Unknown firmware updater location: ${updater}"
  fi

  log "Running firmware updater from ${install_dev} (${updater})..."
  mount ${mount_opt} "${install_dev}" "${mount_point}"
  # If write protection is disabled, perform factory (RO+RW) firmware setup;
  # otherwise run updater in recovery (RW only) mode.
  if ! check_fwwp; then
    "${updater}" --force --mode=factory_install 2>&1 ||
      result="$?"
  else
    # We need to recover the firmware and then enable developer firmware.
    "${updater}" --force --mode=recovery 2>&1 || result="$?"
    # For two-stop firmware, todev is a simple crossystem call; but for other
    # old firmware (alex/zgb), todev will perform flashrom and then reboot.
    # So this must be the very end command.
    "${updater}" --force --mode=todev 2>&1 || result="$?"
  fi

  umount "${mount_point}" || true
  rmdir "${mount_point}" || true
  return ${result}
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

save_omaha_url() {
  log "Save active Omaha server URL to stateful partition"
  local stateful_dev="$1"
  local mount_point="$(mktemp -d)"
  local output_file="${mount_point}/dev_image/etc/lsb-factory"

  mount "${stateful_dev}" "${mount_point}"
  mkdir -p "$(dirname "${output_file}")"
  echo "FACTORY_OMAHA_URL=${OMAHA}" >> "${output_file}"

  umount "${mount_point}" || true
  rmdir "${mount_point}" || true
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

factory_install_usb_old() {
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

factory_install_omaha_old() {
  local i=""
  local result=""
  local return_code=""
  local dst=""
  local dst_arg=""
  local src_url=""
  local src_checksum=""
  local ping_result=""
  local var

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
  ping_result=$(ETH_INTERFACE=${ETH_INTERFACE} ping_shopfloor 2>/dev/null) ||
      return_code="$?"
  if [ "${return_code}" = "" ]; then
    eval "${ping_result}"
  fi

  # Generate the uuid for current install session
  uuid="$(uuidgen 2>/dev/null)" || uuid="Not_Applicable"

  # Register to Overlord if haven't.
  [ -n "${OVERLORD_READY}" ] ||
    register_to_overlord "${OMAHA}" "${TTY_FILE}" "${LOG_FILE}"

  # Say hello to miniomaha server
  omaha_greetings "hello" "${uuid}"

  # Install the partitions
  for i in EFI OEM STATE RELEASE FACTORY FIRMWARE HWID COMPLETE; do
    # DST_*_PART can be a numeric partition number or plain file.
    var="DST_${i}_PART"
    local part="${!var}"

    if [ -z "${part}" ]; then
      die "INVALID CHANNEL: ${i}."
    elif echo "${part}" | grep -qs "^[0-9]*$"; then
      dst="$(make_partition_dev ${DST_DRIVE} ${part})"
      dst_arg=""
    else
      dst="${part}"
      dst_arg="--nocheck_block_device"
    fi

    log "Factory Install: Installing ${i} image to ${dst}"

    var="${i}_CHANNEL_ARG"
    local channel_arg="${!var}"
    local kpart="none"
    if [ "${i}" = "FACTORY" -o "${i}" = "RELEASE" ]; then
      # Set up kernel partition
      kpart=""
    fi

    local extra_arg="--skip_postinst"
    if [ "${i}" = "FACTORY" ]; then
      # Do postinst after update
      extra_arg=""
    fi
    if [ -n "${BOARD}" ]; then
      extra_arg="${extra_arg} --board=${BOARD}"
    fi

    if [ -n "${OMAHA}" ]; then
      extra_arg="${extra_arg} --omaha_url=${OMAHA}"
    fi

    if [ "${SHOPFLOOR_INSTALL}" = "1" ]; then
      var="SRC_${i}_URL"
      src_url="${!var}"
      var="SRC_${i}_CHECKSUM"
      src_checksum="${!var}"

      if [ -z "$src_url" ]; then
        continue;
      fi

      extra_arg="${extra_arg} --install_url=${src_url}"
      extra_arg="${extra_arg} --install_url_checksum=${src_checksum}"
    fi

    return_code=0
    result="$(IS_FACTORY_INSTALL=1 \
      /opt/google/memento_updater/memento_updater.sh \
      --dst_partition "${dst}" --kernel_partition "${kpart}" \
      --allow_removable_boot ${channel_arg} ${dst_arg} ${extra_arg})" ||
      return_code=$?

    if [ "${i}" = "RELEASE" ]; then
      # Disable release partition.
      disable_release_partition
    fi

    # Check the result
    if [ ${return_code} -ne 0 ]; then
      # memento update has encountered a fatal error.
      die "Factory install of target ${dst} has failed with error ${return_code}."
    elif [ "${result}" != "UPDATED" ]; then
      if [ "${i}" = "FACTORY" ]; then
        # Factory image is required.
        die "AU failed."
      fi
    fi
  done

  # Notify miniomaha server all downloads (including postinst) are completed.
  omaha_greetings "download_complete" "${uuid}"

  # Post-processing channels in self-executable file.
  if [ -n "${NETBOOT_RAMFS}" ]; then
    run_netboot_postinst \
      "$(make_partition_dev ${DST_DRIVE} 3)" \
      "$(make_partition_dev ${DST_DRIVE} 1)" \
      "${DST_HWID_PART}" \
      "${DST_FIRMWARE_PART}"
  else
    if [ -s "${DST_FIRMWARE_PART}" ]; then
      log "Execute firmware-install script"
      dst="${DST_FIRMWARE_PART}"
      dst_arg="--force --mode=factory_install"
      if ! sh "${dst}" ${dst_arg} 2>&1; then
        die "Firmware updating failed."
      fi
    fi
    if [ -s "${DST_HWID_PART}" ]; then
      log "Execute HWID component list updater script"
      dst="${DST_HWID_PART}"
      dst_arg="$(make_partition_dev ${DST_DRIVE} ${DST_STATE_PART})"
      if ! sh "${dst}" "${dst_arg}" 2>&1; then
        die "HWID component list updating failed."
      fi
    fi
  fi

  # Update omaha information into stateful partition.
  save_omaha_url "$(make_partition_dev ${DST_DRIVE} ${DST_STATE_PART})" \
    2>&1 || die "Failed to save OMAHA server information."

  # After post processing, notify miniomaha server a installation session
  # has been successfully completed.
  omaha_greetings "goodbye" "${uuid}"

  if [ -s "${DST_COMPLETE_PART}" ]; then
    log "Found completion script."
    COMPLETE_SCRIPT="${DST_COMPLETE_PART}"
  fi
}
