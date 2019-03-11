#!/bin/sh
# Copyright 2017 The Chromium OS Authors. All rights reserved.
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.

# A common library for factory shim.

LSB_RELEASE_FILE=/etc/lsb-release
LSB_FACTORY_FILE=/mnt/stateful_partition/dev_image/etc/lsb-factory

# Returns the key for "key=value" format string.
getLSBKey() {
  local key=""
  if [[ "$1" == ?*'='* ]]; then
    key="$(echo "$1" | cut -d = -f 1)"
  fi
  echo "${key}"
}

# Returns the value for "key=value" format string.
getLSBValue() {
  local value=""
  if [[ "$1" == ?*'='* ]]; then
    value="$(echo "$1" | cut -d = -f 2-)"
  fi
  echo "${value}"
}

# Return the value for a given key in the override lsb-release file.
# If no value is found, checks in the standard lsb-release file.
findLSBValue()
{
  # Check factory LSB file.
  local key="$1"
  local value="$(getLSBValue "$(grep "^${key}" "${LSB_FACTORY_FILE}")")"

  # TODO(hungte) Support dev_image/etc/lsb-release if needed.

  if [ -z "$value" ]; then
    value="$(getLSBValue "$(grep "^${key}" "${LSB_RELEASE_FILE}")")"
  fi

  echo "${value}"
}
