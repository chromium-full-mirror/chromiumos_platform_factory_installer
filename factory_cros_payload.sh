#!/bin/sh

# Copyright 2019 The Chromium OS Authors. All rights reserved.
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.

# cros_payload related functions.

. "$(dirname "$0")/factory_common.sh"

# Patch dev_image/etc/lsb-factory using lsb_factory payload.
cros_payload_patch_lsb_factory() {
  local real_usb_dev="$(findLSBValue REAL_USB_DEV)"
  [ -n "${real_usb_dev}" ] || return 0
  local stateful_dev="${real_usb_dev%[0-9]*}1"
  local mount_point="$(mktemp -d)"
  local board="$(findLSBValue CHROMEOS_RELEASE_BOARD)"
  local json_path="${mount_point}/cros_payloads/${board}.json"
  local temp_lsb_factory="$(mktemp)"

  echo 'Patching lsb-factory...'

  mount -o ro "${stateful_dev}" "${mount_point}"
  # If the RMA shim doesn't have lsb_factory payload, this command will fail,
  # leaving temp_lsb_factory empty.
  cros_payload install "${json_path}" "${temp_lsb_factory}" lsb_factory || true
  umount "${stateful_dev}"
  rmdir "${mount_point}"

  # Append to lsb-factory file.
  cat "${temp_lsb_factory}" >>"${LSB_FACTORY_FILE}"
  rm "${temp_lsb_factory}"
}

# Patch cutoff config in reset shim using toolkit_config payload.
cros_payload_patch_cutoff_config() {
  local real_usb_dev="$(findLSBValue REAL_USB_DEV)"
  [ -n "${real_usb_dev}" ] || return 0
  local stateful_dev="${real_usb_dev%[0-9]*}1"
  local mount_point="$(mktemp -d)"
  local board="$(findLSBValue CHROMEOS_RELEASE_BOARD)"
  local json_path="${mount_point}/cros_payloads/${board}.json"
  local temp_config_path="$(mktemp)"

  echo 'Patching cutoff config...'

  # Get cutoff config in cros_payload.
  mount -o ro "${stateful_dev}" "${mount_point}"
  cros_payload install "${json_path}" "${temp_config_path}" toolkit_config ||
    true
  umount "${stateful_dev}"
  rmdir "${mount_point}"

  local cutoff_config=""
  if [ -s "${temp_config_path}" ]; then
    cutoff_config="$(
      jq -s '.[].cutoff | select(. != null)' "${temp_config_path}")"
  fi
  rm "${temp_config_path}"

  # Overwrite board-specific cutoff config (see factory/sh/cutoff/options.sh).
  if [ -n "${cutoff_config}" ]; then
    local config_path="/usr/share/cutoff/cutoff.json"
    echo "${cutoff_config}" >"${config_path}"
  fi
}
