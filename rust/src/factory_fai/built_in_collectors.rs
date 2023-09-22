// Copyright 2022 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};

use anyhow::{self, Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tempfile;

use crate::device;
use crate::factory_fai::parsers::{DictParser, Parser, RegexParser};
use crate::tools::ap_ro_hash;
use crate::utils::file_utils;
use crate::utils::process_utils::{Command, StringOutput};
use crate::utils::sys_utils;

const GSCVD_RLZ_OFFSET: u64 = 12;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuiltInCollector {
    PartitionTable,
    ReleaseImageInfo,
    SigningKeys,
    CbiData,
    DeviceApRoHash,
    ReleaseImageStatefulPartition,
    RoGscvdBoardId,
    #[cfg(test)]
    MockForTest, // Variant for unit test.
}

impl BuiltInCollector {
    pub fn collect(&self) -> Result<Value> {
        match self {
            BuiltInCollector::PartitionTable => partition_table(),
            BuiltInCollector::ReleaseImageInfo => release_image_info(),
            BuiltInCollector::SigningKeys => signing_keys(),
            BuiltInCollector::CbiData => cbi_data(),
            BuiltInCollector::DeviceApRoHash => device_ap_ro_hash(),
            BuiltInCollector::ReleaseImageStatefulPartition => release_image_stateful_partition(),
            BuiltInCollector::RoGscvdBoardId => ro_gscvd_board_id(),
            #[cfg(test)]
            BuiltInCollector::MockForTest => mock_for_test(),
        }
    }
}

fn partition_table() -> Result<Value> {
    let fixed_device_storage = device::get_fixed_device_storage()?;
    let parser = RegexParser::new(
        r"(?P<start>\d+)\s+(?P<size>\d+)\s+(?P<partition>\d+)\s+(?P<type>.+)".to_string(),
    );
    let output = Command::new("cgpt")
        .args(["show", "-q", &fixed_device_storage])
        .output()?
        .stdout()
        .split('\n')
        .filter_map(|line| {
            if let Ok(matches) = parser.parse(line.to_string()) {
                if matches.as_object().unwrap().keys().len() > 0 {
                    return Some(matches);
                }
            }
            None
        })
        .collect();
    Ok(Value::Array(output))
}

fn release_image_info() -> Result<Value> {
    let rootfs_partition = device::get_release_rootfs_partition()?;
    let mount_point = tempfile::tempdir()?.into_path();

    sys_utils::mount(rootfs_partition, mount_point.as_os_str(), true)?;
    let output = fs::read_to_string(mount_point.join("etc/lsb-release"))?.to_string();
    sys_utils::umount(mount_point)?;

    Ok(DictParser::new("=".to_string()).parse(output)?)
}

fn signing_keys() -> Result<Value> {
    let rootfs_partition = device::get_release_rootfs_partition()?;
    let mount_point = tempfile::tempdir()?.into_path();

    sys_utils::mount(rootfs_partition, mount_point.as_os_str(), true)?;
    let output: Map<String, Value> = serde_json::from_str(
        &Command::new(mount_point.join("usr/sbin/chromeos-firmwareupdate"))
            .arg("--manifest")
            .output()?
            .stdout(),
    )?;
    sys_utils::umount(mount_point)?;

    // Format of firmware manifest:
    //   {
    //     "${FIRMWARE_MANIFEST_KEY}": {
    //       "host": {
    //         "keys": {
    //           "root": ROOTKEY,
    //           "recovery": RECOVERY_KEY
    //         }
    //       },
    //       ...
    //     },
    //     ...
    //   }
    let firmware_manifest_key = device::get_firmware_manifest_key()?;
    Ok(output
        .get(&firmware_manifest_key)
        .context("Unable to get firmware image from manifest.")?
        .get("host")
        .context("Unable to get firmware info from manifest.")?
        .get("keys")
        .cloned()
        .context("Unabel to get keys from manifest.")?)
}

fn cbi_data() -> Result<Value> {
    let mut data = Map::new();

    // Parse the CBI usage:
    //   Usage: ectool cbi get <tag> [get_flag]
    //   Usage: ectool cbi set <tag> <value/string> <size> [set_flag]
    //     <tag> is one of:
    //       0: BOARD_VERSION
    //       1: OEM_ID
    //       ...
    //     <other usage>
    //     ...
    let cbi_usage = Command::new("ectool")
        .args(["cbi", "get"])
        .output()?
        .stderr();
    let mut tag_section = false;
    for line in cbi_usage.split('\n') {
        if tag_section {
            // Parse "ID: TAG" pairs
            if let Some(matches) = Regex::new(r"(\d+):\s*([A-Z_]+)")?.captures(line) {
                let id = matches.get(1).unwrap().as_str().to_string();
                let tag = matches.get(2).unwrap().as_str().to_string();

                // Execute `ectool cbi get <tag>` to obtain CBI data.
                let output = Command::new("ectool")
                    .args(["cbi", "get", &id])
                    .output()?
                    .stdout();
                if output.len() == 0 {
                    continue;
                }

                // Parse different type of CBI:
                //   int: prefix "As unint: VALUE"
                //   string: raw string
                let value = match Regex::new(r"As uint:\s*(\d+)")?.captures(&output) {
                    Some(caps) => Value::Number(caps.get(1).unwrap().as_str().parse().unwrap()),
                    None => Value::String(output.trim().to_string()),
                };
                data.insert(tag, value);
            } else {
                break;
            }
        } else if line.trim().starts_with("<tag> is one of") {
            tag_section = true;
        }
    }
    Ok(Value::Object(data))
}

fn device_ap_ro_hash() -> Result<Value> {
    Ok(Value::String(ap_ro_hash::ap_ro_hash(None)?))
}

fn release_image_stateful_partition() -> Result<Value> {
    let stateful_partition = device::get_release_stateful_partition()?;
    let mount_point = tempfile::tempdir()?.into_path();

    sys_utils::mount(stateful_partition, mount_point.as_os_str(), true)?;
    let mut file_list = Vec::new();
    // There are hundreds of files in release image stateful partition, so we only collect some
    // desired files:
    // 1. Top level folders and files.
    file_list.append(&mut file_utils::list_files(&mount_point, "*", false)?);
    // 2. Contents of dev_image which should be empty.
    file_list.append(&mut file_utils::list_files(
        &mount_point,
        "dev_image/*",
        false,
    )?);
    // 3. `GetPreservedFilesList` list in `src/platform2/init/clobber_state.cc`.
    file_list.append(&mut file_utils::list_files(
        &mount_point,
        "unencrypted/import_extensions/extensions/*.crx",
        false,
    )?);
    file_list.append(&mut file_utils::list_files(
        &mount_point,
        "unencrypted/dlc-factory-images/*/package/dlc.img",
        false,
    )?);
    sys_utils::umount(mount_point)?;

    Ok(file_list
        .iter()
        .map(|path| Value::String(path.display().to_string()))
        .collect())
}

fn ro_gscvd_board_id() -> Result<Value> {
    let ro_gscvd_file = tempfile::Builder::new().prefix("ro_gscvd").tempfile()?;
    let mut futility = Command::new("futility");
    let cmd = futility.args([
        "read",
        "--region",
        "RO_GSCVD",
        &ro_gscvd_file.path().display().to_string(),
    ]);

    if !cmd.status()?.success() {
        println!("{}", cmd.output()?.stderr());
        return Ok(Value::String(String::new()));
    }

    // Offset 12~15 contains the RLZ code.
    let mut ro_gscvd_reader = File::open(ro_gscvd_file)?;
    let mut rlz_bytes = vec![0; 4];
    ro_gscvd_reader.seek(SeekFrom::Start(GSCVD_RLZ_OFFSET))?;
    ro_gscvd_reader.read_exact(&mut rlz_bytes)?;

    // Convert the byte string into little endian order.
    rlz_bytes.reverse();
    Ok(Value::String(String::from_utf8(rlz_bytes.to_vec())?))
}

#[cfg(test)]
fn mock_for_test() -> Result<Value> {
    Ok(Value::String("text for test".to_string()))
}

#[cfg(test)]
mod tests {
    use crate::factory_fai::built_in_collectors::BuiltInCollector;

    #[test]
    fn test_collect() {
        let output = BuiltInCollector::MockForTest.collect().unwrap();

        assert_eq!(output, "text for test");
    }
}
