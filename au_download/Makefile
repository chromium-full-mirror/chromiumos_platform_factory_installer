# Copyright (c) 2013 The Chromium OS Authors. All rights reserved.
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.

OUT ?= .
SRC ?= .

CXXFLAGS ?= -O2
CXXFLAGS += -Wall


all: $(OUT)/recover_from_server

$(OUT)/recover_from_server: $(SRC)/recover_from_server.cc
	$(CXX) $(CXXFLAGS) $(LDFLAGS) $< -o $@

clean:
	rm -f recover_from_server *~
