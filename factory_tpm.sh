#!/bin/bash
# Copyright 2021 The ChromiumOS Authors
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.

tpm_init_factory_tpm() {
  # shellcheck source=/dev/null
  # tpm_utils.sh is installed in build time by factory_installer ebuild.
  source "/usr/share/factory_installer/tpm/tpm_utils.sh"
}

tpm_init_factory_tpm
