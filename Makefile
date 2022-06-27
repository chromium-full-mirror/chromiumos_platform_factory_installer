# Copyright 2013 The Chromium OS Authors. All rights reserved.
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.

SUBDIRS =

.PHONY: subdirs $(SUBDIRS)

subdirs: $(SUBDIRS)

cargo_fmt:
	cd rust; cargo fmt --check -q || \
	  (echo " Please run \`cargo fmt\` to format your code"; exit 1)
