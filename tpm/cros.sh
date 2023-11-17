#!/bin/bash
# Copyright 2021 The ChromiumOS Authors
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.

# Because tpm/*.sh is installed in a different directory, we should use
# absolute path.
. "/usr/sbin/factory_common.sh"
. "/usr/share/cros/gsc-constants.sh"

GSCTOOL="gsctool_cmd"
PROD_CR50_PATH="$(gsc_image_base_name | sed -E 's/^\///g').prod"
TPM_NAME="$(gsc_name)"

# Since ti50 fw version < 0.23.0 has boot issues on the second boot, we need
# to update it on the first boot.
need_to_update_ti50() {
  if [ "${TPM_NAME}" != "ti50" ]; then
    return 1;
  fi;
  local device_version device_version_major
  device_version="$(_get_cr50_rw_version)"
  device_version_major=$(_get_cr50_major_version "${device_version}")
  [ "${device_version_major}" -lt 23 ]
}

_copy_prod_cr50_firmware() {
  # Create a copy of prod cr50 firmware in rootfs to a temporary file.
  # The caller is responsible for deleting the temp file.
  local rootfs_dev="$1"

  local temp_firmware
  temp_firmware="$(mktemp)"

  if [ -z "${rootfs_dev}" ]; then
    # rootfs_dev is not given, let's use the prod firmware from the rootfs of
    # the factory shim. We don't need to find and mount the rootfs again.
    # Because the content is already copied to the current root by
    # `bootstrap.sh`.
    cp "/${PROD_CR50_PATH}" "${temp_firmware}"
  else
    local mount_point firmware_path
    mount_point="$(mktemp -d)"
    firmware_path="${mount_point}/${PROD_CR50_PATH}"

    mount -o ro "${rootfs_dev}" "${mount_point}"
    cp "${firmware_path}" "${temp_firmware}"
    umount "${rootfs_dev}"
    rmdir "${mount_point}"
  fi

  echo "${temp_firmware}"
}

_get_cr50_output_value() {
  local output="$1"
  local key="$2"
  echo "${output}" | grep "^${key}" | sed "s/${key}=//g"
}

_get_cr50_rw_version() {
  _get_cr50_output_value "$(${GSCTOOL} -a -f -M)" 'RW_FW_VER'
}

_get_cr50_image_rw_version() {
  _get_cr50_output_value "$(${GSCTOOL} -b -M "$1")" 'IMAGE_RW_FW_VER'
}

_get_cr50_major_version() {
  echo "$1" | cut -d '.' -f 2
}

_get_cr50_minor_version() {
  echo "$1" | cut -d '.' -f 3
}

_check_need_update_cr50() {
  local temp_firmware image_version image_version_major image_version_minor
  temp_firmware="$(_copy_prod_cr50_firmware)"
  image_version="$(_get_cr50_image_rw_version "${temp_firmware}")"
  image_version_major=$(_get_cr50_major_version "${image_version}")
  image_version_minor=$(_get_cr50_minor_version "${image_version}")
  rm "${temp_firmware}"

  local device_version device_version_major device_version_minor
  device_version="$(_get_cr50_rw_version)"
  device_version_major=$(_get_cr50_major_version "${device_version}")
  device_version_minor=$(_get_cr50_minor_version "${device_version}")

  # Update only if cr50 version on the device is smaller than the cr50 version
  # in install shim. Some older devices may have cr50 version 0.0.*, 0.1.* or
  # 0.2.*, and we also update these devices to 0.3.*.
  # See go/cr50-release-notes for more information.
  if [ "${device_version_major}" -ne "${image_version_major}" ]; then
    [ "${device_version_major}" -lt "${image_version_major}" ]
  else
    [ "${device_version_minor}" -lt "${image_version_minor}" ]
  fi
}

_is_cr50_factory_mode_enabled() {
  # If the cr50 RW version is 0.0.*, the device is booted to install shim
  # straight from factory. The cr50 firmware does not support '-I' option and
  # factory mode, so we treat it as factory mode enabled to avoid turning on
  # factory mode.
  local rw_version="$(_get_cr50_rw_version)"
  if [[ "${rw_version}" = '0.0.'* ]]; then
    echo "Cr50 version is ${rw_version}. Assume factory mode enabled."
    return 0
  fi
  # The pattern of output is as below in case of factory mode enabled:
  # State: Locked
  # Password: None
  # Flags: 000000
  # Capabilities, current and default:
  #   ...
  # Capabilities are modified.
  #
  # If factory mode is disabed then the last line would be
  # Capabilities are default.
  ${GSCTOOL} -a -I 2>/dev/null | \
    grep '^Capabilities are modified.$' >/dev/null
  return $?
}

tpm_perform_rsu() {
  /usr/share/cros/hwsec-utils/gsc_reset
}

tpm_check_rsu_support() {
  if ! command -v /usr/share/cros/hwsec-utils/gsc_reset >/dev/null; then
    # gsc_reset is not available, we cannot perform RSU.
    echo "unsupported"
    return 0
  fi

  if _check_need_update_cr50; then
    echo "need_update"
    return 0
  else
    echo "supported"
    return 0;
  fi
}

tpm_update_firmware() {
  local rootfs_dev="$1"
  local temp_firmware image_version
  temp_firmware="$(_copy_prod_cr50_firmware "${rootfs_dev}")"
  image_version="$(_get_cr50_image_rw_version "${temp_firmware}")"
  echo "Attempt to update with the TPM prod fw (version: ${image_version})"

  local result=0
  "${GSCTOOL}" -a -u "${temp_firmware}" || result="$?"

  rm "${temp_firmware}"

  # Allow 0(no-op), 1(all_updated), 2(rw_updated), other return values are
  # considered fail.
  # See trunk/src/platform/ec/extra/usb_updater/gsctool.h for more detail.
  return "${result}"
}

tpm_enable_factory_mode() {
  if _is_cr50_factory_mode_enabled; then
    log "Factory mode was already enabled."
    return 0
  fi

  if check_hwwp; then
    die "The hardware write protection should be disabled first."
  fi

  log "Starting to enable factory mode and will reboot automatically."
  local ret=0
  ${GSCTOOL} -a -F enable 2>&1 || ret=$?

  if [ ${ret} != 0 ]; then
    local ver="$(_get_cr50_rw_version)"
    log "Failed to enable factory mode; cr50 version: ${ver}"
    log "Try RSU..."
    action_e || die "Failed to perform RSU..."
    # action_e should reboot if it succeeds.
    return 0
  fi

  # Once enabling factory mode, system should reboot automatically.
  # cr50 version >= 0.5.110/0.6.110 won't auto reboot after enabling factory
  # mode. Manually reboot after successfully enable factory mode.

  # Enabling factory model takes a while after the process termintaed. Sleep 5s
  # here to make sure factory mode enabled before reboot.
  log "Successfully to enable factory mode and should reboot in 5 seconds."
  sleep 5s
  reboot

  # sleep indefinitely to avoid re-spawning rather than reboot.
  sleep 1d
}

tpm_get_info() {
  echo -n "Cr50 version: $(_get_cr50_rw_version); "
  echo -n "Board ID flags: 0x$(tpm_get_board_id_flags)"
}

tpm_get_board_id_flags() {
  _get_cr50_output_value "$(${GSCTOOL} -a -i -M)" 'BID_FLAGS'
}
