// Copyright 2023 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::fmt::{Display, Formatter, Result as FMTResult};
use std::fs;

use anyhow::Result;

use crate::factory_ufs::utils::path_utils;

pub enum PowerControl {
    On,
    Auto,
}

impl Display for PowerControl {
    fn fmt(&self, f: &mut Formatter) -> FMTResult {
        match self {
            PowerControl::On => write!(f, "on"),
            PowerControl::Auto => write!(f, "auto"),
        }
    }
}

/// Sets the power control mode to `on` or `auto`.
pub fn set_power_control(mode: PowerControl) -> Result<()> {
    let node = path_utils::get_power_control_node()?;
    println!(
        "Set power control mode of node {} to: {}",
        node.display(),
        mode
    );
    fs::write(&node, mode.to_string())?;
    Ok(())
}
