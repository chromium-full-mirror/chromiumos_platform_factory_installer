// Copyright 2022 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::process::Command;

use anyhow::{self, Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tempfile;

use crate::device;
use crate::factory_fai::parsers::{DictParser, Parser, RegexParser};
use crate::tools::ap_ro_hash;
use crate::utils::error::RegexMatchError;
use crate::utils::file_utils;
use crate::utils::futility_utils;
use crate::utils::process_utils::StringOutput;
use crate::utils::sys_utils;

const GSCVD_MAGIC: &[u8] = b"5afe";
const GSCVD_RLZ_OFFSET: u64 = 12;

#[cfg(not(test))]
const INTEL_PSRTOOL_PATH: &str = "intel-psrtool";
#[cfg(test)]
const INTEL_PSRTOOL_PATH: &str = "tests/bin/intel-psrtool";

const PSR_UNSUPPORTED: &str = "PSR is not supported on this device";
const PSR_KEYS: [&'static str; 4] = [
    "OEM Name",
    "OEM Make",
    "OEM Model",
    "Country of Manufacturer",
];

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
    IntelPsr,
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
            BuiltInCollector::IntelPsr => intel_psr(),
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
    let ro_gscvd_file = futility_utils::read_region("RO_GSCVD")?;

    let mut ro_gscvd_reader = File::open(ro_gscvd_file)?;
    let mut magic = vec![0; 4];
    ro_gscvd_reader.read_exact(&mut magic)?;
    anyhow::ensure!(
        magic == GSCVD_MAGIC,
        format!(
            "Failed to find magic number in RO_GSCVD! Expected: {:?}, Real: {:?}",
            GSCVD_MAGIC, magic
        )
    );

    // Offset 12~15 contains the RLZ code.
    let mut rlz_bytes = vec![0; 4];
    ro_gscvd_reader.seek(SeekFrom::Start(GSCVD_RLZ_OFFSET))?;
    ro_gscvd_reader.read_exact(&mut rlz_bytes)?;

    // Convert the byte string into little endian order.
    rlz_bytes.reverse();

    anyhow::ensure!(
        std::str::from_utf8(&rlz_bytes)?
            .chars()
            .any(|b| b.is_ascii_uppercase()),
        format!(
            "Each char in the RLZ code should be any char between A~Z. Found: {:?}",
            rlz_bytes
        )
    );

    Ok(Value::String(String::from_utf8(rlz_bytes.to_vec())?))
}

fn is_intel_psr_unsupported(psrtool_output: &str) -> Result<bool> {
    Ok(Regex::new(PSR_UNSUPPORTED)?.is_match(&psrtool_output))
}

fn parse_psr_provisioned_data(psrtool_output: &str) -> Result<Map<String, Value>> {
    // Example output if PSR is supported and has been provisioned.
    // ...
    // Genesis Information:
    //    Log Start Date: 2023-05-25 02:39:59 (UTC)
    //    OEM Name: Google
    //    OEM Make: ChromeOS
    //    OEM Model: MTL vPro
    //    Country of Manufacturer: CN
    //    OEM Data:
    // ...
    let joined_keys = PSR_KEYS.join("|");
    let psr_supported_regex = format!(r"(?P<key>({joined_keys})): (?P<value>.+)");

    let mut psr_data = Map::new();
    for captures in Regex::new(psr_supported_regex.as_str())?.captures_iter(&psrtool_output) {
        let key = captures
            .name("key")
            .ok_or(RegexMatchError::NamedGroupNotFoundError)?
            .as_str();
        let value = captures
            .name("value")
            .ok_or(RegexMatchError::NamedGroupNotFoundError)?
            .as_str();
        psr_data.insert(key.to_string(), value.into());
    }
    Ok(psr_data)
}

fn check_psr_missing_keys(psr_data: &Map<String, Value>) -> Result<()> {
    let mut missing_keys = Vec::new();
    for key in PSR_KEYS {
        if !psr_data.contains_key(key) {
            missing_keys.push(key);
        }
    }
    if !missing_keys.is_empty() {
        missing_keys.sort();
        anyhow::bail!(
            "Missing keys: {:?} from `{} -d`.",
            missing_keys,
            INTEL_PSRTOOL_PATH
        );
    }
    Ok(())
}

fn intel_psr() -> Result<Value> {
    let output = Command::new(INTEL_PSRTOOL_PATH)
        .args(["-d"])
        .output()?
        .stdout();
    if is_intel_psr_unsupported(&output)? {
        return Ok(Value::String(
            "PSR is either unsupported or has not been provisioned.".to_string(),
        ));
    }
    let psr_data = parse_psr_provisioned_data(&output)?;
    check_psr_missing_keys(&psr_data)?;
    Ok(Value::Object(psr_data))
}

#[cfg(test)]
fn mock_for_test() -> Result<Value> {
    Ok(Value::String("text for test".to_string()))
}

#[cfg(test)]
mod tests {
    use serde_json::{self, Map};

    use crate::factory_fai::built_in_collectors::{self, BuiltInCollector};

    #[test]
    fn test_collect() {
        let output = BuiltInCollector::MockForTest.collect().unwrap();

        assert_eq!(output, "text for test");
    }

    #[test]
    fn test_intel_psr_provisioned_successfully() {
        let actual = BuiltInCollector::IntelPsr.collect().unwrap();
        let expected = serde_json::json!({
          "OEM Name": "Google",
          "OEM Make": "ChromeOS",
          "OEM Model": "MTL vPro",
          "Country of Manufacturer": "CN",
        });

        assert_eq!(actual, expected);
    }

    #[test]
    fn is_intel_psr_unsupported() {
        let mock_psrtool_output = "PSR is not supported on this device.";
        assert_eq!(
            true,
            built_in_collectors::is_intel_psr_unsupported(&mock_psrtool_output).unwrap()
        );
    }

    #[test]
    fn test_check_psr_missing_data() {
        let mock_psr_data = Map::new();
        let actual_error = format!(
            "{}",
            built_in_collectors::check_psr_missing_keys(&mock_psr_data)
                .unwrap_err()
                .root_cause()
        );
        let expected_error = format!(
            "Missing keys: {:?} from `{} -d`.",
            vec![
                "Country of Manufacturer",
                "OEM Make",
                "OEM Model",
                "OEM Name",
            ],
            built_in_collectors::INTEL_PSRTOOL_PATH
        );
        assert_eq!(actual_error, expected_error)
    }
}
