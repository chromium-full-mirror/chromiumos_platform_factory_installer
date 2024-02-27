// Copyright 2024 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

/// Ref: https://chromium.googlesource.com/chromiumos/platform/factory/+/HEAD/sh/cutoff/README.md
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum CutoffMethod {
    Shutdown,
    Reboot,
    EctoolCutoff,
    BatteryCutoff,
    EcHibernate,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum CutoffAcState {
    ConnectAc,
    RemoveAc,
}
