// Copyright 2022 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use anyhow::{self, Context, Result};
use lazy_static;
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

lazy_static::lazy_static! {
    static ref VPD_BLOCKLIST: HashMap<&'static str, Vec<&'static str>> = {
        let mut blocklist = HashMap::new();
        blocklist.insert("ro_vpd", vec![
            "stable_device_secret_DO_NOT_SHARE",
        ]);
        blocklist.insert("rw_vpd", vec![
            "ubind_attribute",
            "gbind_attribute",
        ]);
        blocklist
    };
}

fn read_files<P: AsRef<Path>>(root: P, glob_string: P) -> Result<Value> {
    let root = root.as_ref();
    let glob_string = glob_string.as_ref();
    let mut data = Map::new();

    for path in file_utils::list_files(root, glob_string, true)? {
        let filename = path.display().to_string();
        let path = Path::new(root).join(&filename);
        let content = String::from_utf8_lossy(&fs::read(path.as_path())?)
            .trim()
            .to_string();
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

/// Redacts VPDs that might cause privacy issues.
pub fn redact_vpd_data(fai_data: &mut Map<String, Value>) -> Result<()> {
    for (partition, blocked_vals) in VPD_BLOCKLIST.iter() {
        match fai_data.get_mut::<str>(partition) {
            Some(vpd) => {
                for blocked_val in blocked_vals {
                    match vpd.get_mut(blocked_val) {
                        Some(vpd) => {
                            *vpd = serde_json::json!("<redacted>");
                        }
                        None => continue,
                    }
                }
            }
            None => continue,
        }
    }

    Ok(())
}

pub fn collect_fai_data(fai_config: FAIConfig) -> Result<String> {
    let mut fai_data = Map::new();
    for (name, config) in fai_config.iter() {
        let data = match config {
            DataCollector::DataCommand(data_cmd) => {
                match Command::new(&data_cmd.cmd).args(&data_cmd.args).output() {
                    Ok(output) => data_cmd.parser.parse(output.stdout()),
                    Err(e) => Err(e.into()),
                }
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
        let data = match data {
            Ok(data) => data,
            Err(e) => {
                eprintln!("Error: Failed to collect \"{}\".", name);
                Value::String(e.to_string())
            }
        };

        fai_data.insert(name.to_string(), data);
    }
    redact_vpd_data(&mut fai_data)?;
    Ok(serde_json::to_string_pretty(&fai_data)?)
}

#[cfg(test)]
mod tests {
    use lazy_static;
    use serde_json::{Map, Value};

    use crate::factory_fai;

    lazy_static::lazy_static! {
        #[derive(Debug)]
        static ref REDACTED_VPD: Map<String, Value> = {
            let mut redacted_vpd = Map::new();
            redacted_vpd.insert("ro_vpd".to_string(), serde_json::json!({
                "oem_name": "Google",
                "stable_device_secret_DO_NOT_SHARE": "<redacted>"
            }));
            redacted_vpd.insert("rw_vpd".to_string(), serde_json::json!({
                    "gbind_attribute": "<redacted>",
                    "should_send_rlz_ping": "1",
                    "ubind_attribute": "<redacted>"
            }));
            redacted_vpd
        };
    }

    #[test]
    fn test_redact_vpd_data() {
        let mut vpd = Map::new();
        vpd.insert(
            "ro_vpd".to_string(),
            serde_json::json!({
                "oem_name": "Google",
                "stable_device_secret_DO_NOT_SHARE": "stable_device_secret"
            }),
        );
        vpd.insert(
            "rw_vpd".to_string(),
            serde_json::json!({
                "gbind_attribute": "gbind_val",
                "should_send_rlz_ping": "1",
                "ubind_attribute": "ubind_val"
            }),
        );
        factory_fai::redact_vpd_data(&mut vpd).unwrap();
        assert_eq!(vpd, *REDACTED_VPD);
    }
}

pub fn perform_fai(
    output_path: Option<String>,
    config_path: Option<String>,
    dump_config: bool,
    need_save_to_usb: bool,
) -> Result<()> {
    if dump_config {
        println!("{}", config::DEFAULT_CONFIG);
        return Ok(());
    }

    let fai_config = config::load_config(config_path).context("Failed to load config.")?;

    let data = collect_fai_data(fai_config)?;

    println!("{}", &data);
    if need_save_to_usb {
        let (dev, filename) = save_to_usb(&data)?;
        eprintln!("Succeed to save {} in {}.", filename, dev);
    }

    if let Some(path) = output_path {
        eprintln!("Writing result to {}.", path);
        fs::write(path, &data).context("Failed to write result to output file.")?;
    }
    Ok(())
}
