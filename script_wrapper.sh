#!/bin/bash
# Copyright 2024 The ChromiumOS Authors
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.

. "/usr/share/misc/chromeos-common.sh"
. "$(dirname "$0")/factory_common.sh"
. "$(dirname "$0")/secure-wipe.sh"
. "$(dirname "$0")/factory_cros_payload.sh"

"$@"
