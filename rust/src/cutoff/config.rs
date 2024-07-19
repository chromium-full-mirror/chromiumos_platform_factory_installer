// Copyright 2024 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

/// Ref: https://chromium.googlesource.com/chromiumos/platform/factory/+/HEAD/sh/cutoff/README.md
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Debug)]
#[serde(rename_all(deserialize = "SCREAMING_SNAKE_CASE"))]
pub struct CutoffConfig {
    pub cutoff_method: CutoffMethod,
    pub cutoff_ac_state: CutoffAcState,
    #[serde(default = "i32_min")]
    pub cutoff_battery_min_percentage: i32,
    #[serde(default = "i32_max")]
    pub cutoff_battery_max_percentage: i32,
    #[serde(default = "i32_min")]
    pub cutoff_battery_min_voltage: i32,
    #[serde(default = "i32_max")]
    pub cutoff_battery_max_voltage: i32,
    #[serde(default)]
    pub factory_server_url: String,
    #[serde(default)]
    pub tty: String,
    #[serde(default)]
    pub continue_key: String,
    #[serde(default)]
    pub qrcode_info: String,
}

fn i32_min() -> i32 {
    i32::MIN
}

fn i32_max() -> i32 {
    i32::MAX
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum CutoffMethod {
    Shutdown,
    Reboot,
    BatteryCutoff,
    EcHibernate,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum CutoffAcState {
    ConnectAc,
    RemoveAc,
}
