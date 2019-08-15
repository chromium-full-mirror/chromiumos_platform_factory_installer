#!/bin/sh

# Copyright 2019 The Chromium OS Authors. All rights reserved.
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.

# Try to clear Cr50 FWMP at the beginning of factory shim if it exists, and the
# shim is booted in recovery mode.
# The command only succeeds if it is run after clearing TPM ownership and before
# tpm_managerd starts.

has_fwmp() {
  tpmc getp 0x100a >/dev/null 2>&1
}

tpm_not_owned() {
  tpmc geto | grep -q 'Owned: no'
}

if crossystem "mainfw_type?recovery" && has_fwmp && tpm_not_owned; then
  # Same as `tpmc undef 0x100a`, but older versions of tpmc don't support
  # `undef` command.
  tpmc sendraw \
    0x80 0x02           `# TPM_ST_SESSIONS` \
    0x00 0x00 0x00 0x1f `# commandSize` \
    0x00 0x00 0x01 0x22 `# TPM_CC_NV_UndefineSpace` \
    0x40 0x00 0x00 0x01 `# TPM_RH_OWNER` \
    0x01 0x00 0x10 0x0a `# nvIndex` \
    0x00 0x00 0x00 0x09 `# sessionSize` \
    0x40 0x00 0x00 0x09 `# sessionHandle = TPM_RS_PW` \
    0x00 0x00           `# nonceSize` \
    0x00                `# sessionAttributes` \
    0x00 0x00           `# password length`
fi
