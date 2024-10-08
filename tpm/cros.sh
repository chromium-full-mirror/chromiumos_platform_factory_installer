#!/bin/bash
# Copyright 2021 The ChromiumOS Authors
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.

# Because tpm/*.sh is installed in a different directory, we should use
# absolute path.
. "/usr/sbin/factory_common.sh"
. "/usr/share/cros/gsc-constants.sh"

GSCTOOL="gsctool_cmd"
TPM_NAME="$(gsc_name)"


_get_cr50_output_value() {
  local output="$1"
  local key="$2"
  echo "${output}" | grep "^${key}" | sed "s/${key}=//g"
}

_get_cr50_rw_version() {
  _get_cr50_output_value "$(${GSCTOOL} -a -f -M)" 'RW_FW_VER'
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
  /usr/sbin/gsc_reset
}

tpm_check_rsu_support() {
  if ! command -v /usr/sbin/gsc_reset >/dev/null; then
    # gsc_reset is not available, we cannot perform RSU.
    echo "unsupported"
    return 0
  fi

  echo "supported"
  return 0;
}

tpm_enable_factory_mode() {
  if _is_cr50_factory_mode_enabled; then
    log "Factory mode was already enabled."
    return 0
  fi

  if check_hwwp && [ "$(tpm_get_chassis_open)" != "true" ]; then
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
}

tpm_get_info() {
  echo -n "Cr50 version: $(_get_cr50_rw_version); "
  echo -n "Board ID flags: 0x$(tpm_get_board_id_flags)"
}

tpm_get_board_id_flags() {
  _get_cr50_output_value "$(${GSCTOOL} -a -i -M)" 'BID_FLAGS'
}

# Outputs chassis open status.
# Possible outputs are:
#   "unsupported"
#   "true"
#   "false"
tpm_get_chassis_open() {
  # The command is only supported on Ti50 devices.
  if [ "${TPM_NAME}" != "ti50" ]; then
    echo "unsupported"
    return 0
  fi;
  "${GSCTOOL}" -a -K chassis_open | sed 's/^Chassis Open: //g'
}
