#!/bin/bash

# Copyright 2014 The ChromiumOS Authors
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.

# Opens "less" securely as the "nobody" user.  Only piping from stdin
# is supported (there may be no command line arguments).

set -e

if [ $# -ne 0 ]; then
  echo "Usage: secure_less.sh" >& 2
  echo "(no command-line arguments are allowed)" >& 2
  exit 1
fi


. "$(dirname "$0")/factory_common.sh"

PAGER="busybox less"
BOARD="$(findLSBValue CHROMEOS_RELEASE_BOARD)"

# On some boards, `busybox less` will suffer from segmentation
# fault so we take `busybox more` as a WA here. Even though it cannot scroll
# back, at least it could display logs. As we're seeing less board-specific
# issues lately, in the common scene, we oftentimes can reproduce bugs on other
# boards with `busybox less` working so it's an acceptable WA on dedede & rauru.
if [[ "${BOARD}" == "dedede" ||
      "${BOARD}" == "rauru" || "${BOARD}" == "skywalker" ]]; then
  PAGER="busybox more"
fi

# Disable EDITOR and SHELL, just in case.  Always use busybox less,
# since it has no fancy features that could enable exploits.

# We can switch back to only su if either of these bugs get fixed:
# https://bugs.debian.org/663200
# https://bugs.busybox.net/9231
if sudo -h >/dev/null 2>&1; then
  set -x
  exec sudo -u nobody -s /bin/sh \
    -c "EDITOR=/bin/false SHELL=/bin/false ${PAGER}"
else
  set -x
  exec su -s /bin/sh \
    -c "EDITOR=/bin/false SHELL=/bin/false ${PAGER}" - nobody
fi
