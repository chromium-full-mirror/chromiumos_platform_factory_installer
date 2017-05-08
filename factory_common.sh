#!/bin/sh
# Copyright 2017 The Chromium OS Authors. All rights reserved.
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.

# A common library for factory shim.

FACTORY_LSB_FILE=/mnt/stateful_partition/dev_image/etc/lsb-factory

# Return the value for a given key in the override lsb-release file.
# If no value is found, checks in the standard lsb-release file.
findLSBValue()
{
  # Check factory LSB file.
  local value="$(grep "^$1" "${FACTORY_LSB_FILE}" | cut -d = -f 2-)"

  # TODO(hungte) Support dev_image/etc/lsb-release if needed.

  if [ -z "$value" ]; then
    value="$(grep "^$1" /etc/lsb-release | cut -d = -f 2-)"
  fi

  echo "${value}"
}
