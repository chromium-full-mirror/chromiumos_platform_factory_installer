// Copyright 2022 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::factory_fai::config::{DataCollector, FAIConfig};
use crate::factory_fai::parsers::Parser;
use crate::utils::process_utils::{Command, StringOutput};

use anyhow::{Context, Result};
use serde_json;
use serde_json::Map;

pub mod args;
pub mod config;
pub mod parsers;

pub fn collect_fai_data(fai_config: FAIConfig) -> Result<String> {
    let mut fai_data = Map::new();
    for (name, config) in fai_config.iter() {
        let data = match config {
            DataCollector::DataCommand(data_cmd) => {
                let output = Command::new(&data_cmd.cmd)
                    .args(&data_cmd.args)
                    .output()
                    .with_context(|| format!("Failed to collect \"{}\".", name))?;
                data_cmd.parser.parse(output.stdout())?
            }
        };

        fai_data.insert(name.to_string(), data);
    }
    Ok(serde_json::to_string_pretty(&fai_data)?)
}
