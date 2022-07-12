// Copyright 2022 The ChromiumOS Authors.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::fs;
use std::path::Path;

use anyhow::{self, Context, Result};
use serde_json;
use serde_json::{Map, Value};
use tempfile;

use crate::device;
use crate::factory_fai::config::{DataCollector, FAIConfig};
use crate::factory_fai::parsers::Parser;
use crate::utils::file_utils;
use crate::utils::process_utils::{Command, StringOutput};
use crate::utils::sys_utils;

pub mod args;
pub mod built_in_collectors;
pub mod config;
pub mod parsers;

fn read_files<P: AsRef<Path>>(root: P, glob_string: P) -> Result<Value> {
    let root = root.as_ref();
    let glob_string = glob_string.as_ref();
    let mut data = Map::new();

    for path in file_utils::list_files(root, glob_string, true)? {
        let filename = path.display().to_string();
        let path = Path::new(root).join(&filename);
        let content = fs::read_to_string(path.as_path())?.trim().to_string();
        data.insert(filename, Value::String(content));
    }
    Ok(Value::Object(data))
}

fn list_files<P: AsRef<Path>>(root: P, glob_string: P) -> Result<Value> {
    Ok(file_utils::list_files(root, glob_string, false)?
        .iter()
        .map(|path| Value::String(path.display().to_string()))
        .collect())
}

/// Saves the data to the stateful partition of factory shim.
/// # Returns
/// Returns the tuple of information of saved `(device_partition, filename)`.
pub fn save_to_usb(data: &str) -> Result<(String, String)> {
    let usb = device::get_dev_stateful_partition()?;
    let mount_point = tempfile::tempdir()?.into_path();
    let filename = format!(
        "factory_fai_{}_{}.json",
        device::get_model_name()?,
        device::get_serial_number()?
    );

    sys_utils::mount(&usb, mount_point.as_os_str(), false).context(
        "No mountable USB device is available. \
                 Do not remove factory shim before this operation.",
    )?;
    fs::write(mount_point.join(&filename), data)?;
    sys_utils::umount(mount_point)?;
    Ok((usb, filename))
}

pub fn collect_fai_data(fai_config: FAIConfig) -> Result<String> {
    let mut fai_data = Map::new();
    for (name, config) in fai_config.iter() {
        let data = match config {
            DataCollector::DataCommand(data_cmd) => {
                let output = Command::new(&data_cmd.cmd).args(&data_cmd.args).output()?;
                data_cmd.parser.parse(output.stdout())
            }
            DataCollector::DataFiles(data_files) => {
                if data_files.read_content {
                    read_files(&data_files.root, &data_files.glob)
                } else {
                    list_files(&data_files.root, &data_files.glob)
                }
            }
            DataCollector::BuiltInCollector(collector) => collector.collect(),
        };
        let data = data.with_context(|| format!("Failed to collect \"{}\".", name))?;

        fai_data.insert(name.to_string(), data);
    }
    Ok(serde_json::to_string_pretty(&fai_data)?)
}
