#!/bin/bash

# Copyright (c) 2014 The Chromium OS Authors. All rights reserved.
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.

# Opens "less" securely as the "nobody" user.  Only piping from stdin
# is supported (there may be no command line arguments).

set -ex

if [ $# -ne 0 ]; then
  echo "Usage: secure_less.sh" >& 2
  echo "(no command-line arguments are allowed)" >& 2
  exit 1
fi

# Disable EDITOR and SHELL, just in case.  Always use busybox less,
# since it has no fancy features that could enable exploits.
exec sudo -u nobody -s /bin/sh \
  -c "EDITOR=/bin/false SHELL=/bin/false busybox less"
