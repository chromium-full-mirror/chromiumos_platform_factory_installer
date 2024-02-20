// Copyright 2022 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{self, Context as _, Result};
use lazy_static;
use serde_json;
use serde_json::{Map, Value};

use crate::device;
use crate::factory_fai::config::{ConfigOptions, DataCollector, FAIConfig};
use crate::factory_fai::parsers::Parser;
use crate::system::context::Context;
use crate::utils::process_utils::{Command, StringOutput};
#[cfg(not(test))]
use crate::utils::sys_utils;
#[cfg(test)]
use crate::utils::sys_utils::mock as sys_utils;
use crate::utils::{file_utils, vpd_utils};

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
fn save_to_usb(context: &mut dyn Context, data: &str) -> Result<(String, String)> {
    let usb = device::get_dev_stateful_partition(context)?;
    let mount_point = context.tempdir(None)?;
    let filename = format!(
        "factory_fai_{}_{}.json",
        device::get_model_name(context)?,
        vpd_utils::get_ro_value("serial_number", context)?
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
fn redact_vpd_data(fai_data: &mut Map<String, Value>) -> Result<()> {
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

fn collect_fai_data(context: &mut dyn Context, fai_config: FAIConfig) -> Result<String> {
    let mut fai_data = Map::new();
    for (name, config) in fai_config.iter() {
        eprintln!("Collecting \"{}\"...", name);
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
            DataCollector::BuiltInCollector(collector) => collector.collect(context),
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

pub fn perform_fai(
    context: &mut dyn Context,
    output_path: Option<String>,
    config_path: Option<String>,
    dump_config: bool,
    need_save_to_usb: bool,
) -> Result<()> {
    let config_options = ConfigOptions {
        config_path: match config_path {
            Some(path_str) => Some(PathBuf::from(path_str)),
            None => None,
        },
        is_ti50: device::is_ti50(context)?,
    };

    let fai_config = config::load_config(&config_options).context("Failed to load config.")?;

    if dump_config {
        println!("{}", serde_json::to_string(&fai_config)?);
        return Ok(());
    }

    let data = collect_fai_data(context, fai_config)?;

    println!("{}", &data);
    if need_save_to_usb {
        let (dev, filename) = save_to_usb(context, &data)?;
        eprintln!("Succeed to save {} in {}.", filename, dev);
    }

    if let Some(path) = output_path {
        eprintln!("Writing result to {}.", path);
        fs::write(path, &data).context("Failed to write result to output file.")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::Value;

    use crate::factory_fai;
    use crate::system::context::{Context, ContextImpl};
    use crate::utils::file_utils;

    #[test]
    fn test_perform_fai_success_save_to_usb() {
        let mut context = ContextImpl::new();
        context.set_command_stdout("sh", "ti50".to_string());
        file_utils::create_file(
            context
                .root_dir()
                .join("mnt/stateful_partition/dev_image/etc/lsb-factory"),
            "REAL_USB_DEV=/dev/sda3",
        )
        .unwrap();
        // The output JSON filename should be "factory_fai_<model>_<serial_number>.json".
        context.set_command_stdout("vpd", "sn12345".to_string());
        context.set_command_stdout("cros_config", "model".to_string());
        let config_path = context.tempdir(None).unwrap().join("config.json");
        file_utils::create_file(&config_path, "{}").unwrap();
        factory_fai::perform_fai(
            &mut context,
            None,
            Some(config_path.to_string_lossy().into_owned()),
            false,
            true,
        )
        .unwrap();
        let output_path = context
            .tempdir(None)
            .unwrap()
            .join("factory_fai_model_sn12345.json");
        assert!(output_path.as_path().exists());
    }

    #[test]
    fn test_perform_fai_success_redact_vpd_data() {
        let mut context = ContextImpl::new();
        context.set_command_stdout("sh", "ti50".to_string());
        let output_path = context.tempdir(None).unwrap().join("output.json");
        let config_path = context.tempdir(None).unwrap().join("config.json");
        let ro_vpd = context.root_dir().join("sys/firmware/vpd/ro");
        let rw_vpd = context.root_dir().join("sys/firmware/vpd/rw");
        let config = serde_json::json!({
          "ro_vpd": {
              "DataFiles": {
                  "root": ro_vpd,
                  "read_content": true
              }
          },
          "rw_vpd": {
              "DataFiles": {
                  "root": rw_vpd,
                  "read_content": true
              }
          }
        });
        file_utils::create_file(&config_path, serde_json::to_string(&config).unwrap()).unwrap();
        file_utils::create_file(ro_vpd.join("oem_name"), "Google").unwrap();
        file_utils::create_file(
            ro_vpd.join("stable_device_secret_DO_NOT_SHARE"),
            "stable_device_secret",
        )
        .unwrap();
        file_utils::create_file(rw_vpd.join("gbind_attribute"), "gbind_val").unwrap();
        file_utils::create_file(rw_vpd.join("should_send_rlz_ping"), "1").unwrap();
        file_utils::create_file(rw_vpd.join("ubind_attribute"), "ubind_val").unwrap();

        factory_fai::perform_fai(
            &mut context,
            Some(output_path.to_string_lossy().into_owned()),
            Some(config_path.to_string_lossy().into_owned()),
            false,
            false,
        )
        .unwrap();
        let output: Value =
            serde_json::from_str(&fs::read_to_string(output_path).unwrap()).unwrap();

        let expected = serde_json::json!({
            "ro_vpd": {
                "oem_name": "Google",
                "stable_device_secret_DO_NOT_SHARE": "<redacted>"
            },
            "rw_vpd": {
                "gbind_attribute": "<redacted>",
                "should_send_rlz_ping": "1",
                "ubind_attribute": "<redacted>"
            }
        });
        assert_eq!(expected, output);
    }
}
