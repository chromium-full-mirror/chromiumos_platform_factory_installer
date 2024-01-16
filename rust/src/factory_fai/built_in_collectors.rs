// Copyright 2022 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};

use anyhow::{self, Context as _, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::device;
use crate::factory_fai::parsers::{DictParser, Parser, RegexParser};
use crate::system::context::Context;
use crate::tools::ap_ro_hash;
use crate::utils::error::RegexMatchError;
use crate::utils::file_utils;
use crate::utils::futility_utils;
use crate::utils::process_utils::StringOutput;
#[cfg(not(test))]
use crate::utils::sys_utils;
#[cfg(test)]
use crate::utils::sys_utils::mock as sys_utils;

const GSCVD_MAGIC: &[u8] = b"5afe";
const GSCVD_RLZ_OFFSET: u64 = 12;

const INTEL_PSRTOOL_PATH: &str = "intel-psrtool";

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
}

impl BuiltInCollector {
    pub fn collect(&self, context: &mut dyn Context) -> Result<Value> {
        match self {
            BuiltInCollector::PartitionTable => partition_table(context),
            BuiltInCollector::ReleaseImageInfo => release_image_info(context),
            BuiltInCollector::SigningKeys => signing_keys(context),
            BuiltInCollector::CbiData => cbi_data(context),
            BuiltInCollector::DeviceApRoHash => device_ap_ro_hash(context),
            BuiltInCollector::ReleaseImageStatefulPartition => {
                release_image_stateful_partition(context)
            }
            BuiltInCollector::RoGscvdBoardId => ro_gscvd_board_id(context),
            BuiltInCollector::IntelPsr => intel_psr(context),
        }
    }
}

fn partition_table(context: &mut dyn Context) -> Result<Value> {
    let fixed_device_storage = device::get_fixed_device_storage(context)?;
    let parser = RegexParser::new(
        r"(?P<start>\d+)\s+(?P<size>\d+)\s+(?P<partition>\d+)\s+(?P<type>.+)".to_string(),
    );
    let output = context
        .command("cgpt")
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

fn release_image_info(context: &mut dyn Context) -> Result<Value> {
    let rootfs_partition = device::get_release_rootfs_partition(context)?;
    let mount_point = context.tempdir(None)?;

    sys_utils::mount(rootfs_partition, mount_point.as_os_str(), true)?;
    let output = fs::read_to_string(mount_point.join("etc/lsb-release"))?.to_string();
    sys_utils::umount(mount_point)?;

    Ok(DictParser::new("=".to_string()).parse(output)?)
}

fn signing_keys(context: &mut dyn Context) -> Result<Value> {
    let rootfs_partition = device::get_release_rootfs_partition(context)?;
    let mount_point = context.tempdir(None)?;

    sys_utils::mount(rootfs_partition, mount_point.as_os_str(), true)?;
    let output: Map<String, Value> = serde_json::from_str(
        &context
            .command(
                &mount_point
                    .join("usr/sbin/chromeos-firmwareupdate")
                    .to_string_lossy(),
            )
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
    let firmware_manifest_key = device::get_firmware_manifest_key(context)?;
    Ok(output
        .get(&firmware_manifest_key)
        .context("Unable to get firmware image from manifest.")?
        .get("host")
        .context("Unable to get firmware info from manifest.")?
        .get("keys")
        .cloned()
        .context("Unabel to get keys from manifest.")?)
}

fn cbi_data(context: &mut dyn Context) -> Result<Value> {
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
    let cbi_usage = context
        .command("ectool")
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
                let output = context
                    .command("ectool")
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

fn device_ap_ro_hash(context: &mut dyn Context) -> Result<Value> {
    Ok(Value::String(ap_ro_hash::ap_ro_hash(context, None)?))
}

fn release_image_stateful_partition(context: &mut dyn Context) -> Result<Value> {
    let stateful_partition = device::get_release_stateful_partition(context)?;
    let mount_point = context.tempdir(None)?;

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

fn ro_gscvd_board_id(context: &mut dyn Context) -> Result<Value> {
    let ro_gscvd_file = futility_utils::read(context, Some("RO_GSCVD"))?;

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

fn intel_psr(context: &mut dyn Context) -> Result<Value> {
    let output = context
        .command(INTEL_PSRTOOL_PATH)
        .args(["-d"])
        .output()?
        .stdout();
    if Regex::new(PSR_UNSUPPORTED)?.is_match(&output) {
        return Ok(Value::String(
            "PSR is either unsupported or has not been provisioned.".to_string(),
        ));
    }

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
    for captures in Regex::new(psr_supported_regex.as_str())?.captures_iter(&output) {
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
    Ok(Value::Object(psr_data))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::process::Output;

    use serde_json;

    use crate::factory_fai::built_in_collectors::{self, BuiltInCollector};
    use crate::system::context::{Context, ContextImpl};
    use crate::utils::file_utils;
    use crate::utils::process_utils::StringOutput;

    #[test]
    fn test_partition_table_success() {
        let mut context = ContextImpl::new();
        context.set_command_stdout("sh", "/dev/sda".to_string());
        let cgpt_output: &str = "
            17321984    43487184       1  Linux data
            405504         65536       2  ChromeOS kernel";
        context.set_command_stdout("cgpt", cgpt_output.to_string());
        let output = BuiltInCollector::PartitionTable
            .collect(&mut context)
            .unwrap();
        let expected = serde_json::json!([
            {"start": "17321984", "size": "43487184", "partition": "1", "type": "Linux data"},
            {"start": "405504", "size": "65536", "partition": "2", "type": "ChromeOS kernel"}
        ]);
        assert_eq!(output, expected);
    }

    #[test]
    fn test_release_image_info_success() {
        let mut context = ContextImpl::new();
        context.set_command_stdout("sh", "/dev/sda".to_string());
        let lsb_release: &str = "
            CHROMEOS_RELEASE_VERSION=15702.0.0
            CHROMEOS_RELEASE_KEYSET=devkeys";
        file_utils::create_file(
            context.tempdir(None).unwrap().join("etc/lsb-release"),
            lsb_release,
        )
        .unwrap();
        let expected = serde_json::json!({
            "CHROMEOS_RELEASE_VERSION": "15702.0.0",
            "CHROMEOS_RELEASE_KEYSET": "devkeys"
        });
        let output = BuiltInCollector::ReleaseImageInfo
            .collect(&mut context)
            .unwrap();
        assert_eq!(output, expected);
    }

    #[test]
    fn test_signing_keys_success() {
        let mut context = ContextImpl::new();
        let fw_updater = context
            .tempdir(None)
            .unwrap()
            .join("usr/sbin/chromeos-firmwareupdate");
        let fw_manifest = serde_json::json!({
            "foo": {
                "host": {
                    "keys": {
                        "root": "ROOTKEY",
                        "recovery": "RECOVERY_KEY"
                    }
                }
            }
        });
        context.set_command_stdout("sh", "/dev/sda".to_string());
        context.set_command_stdout("crosid", "FIRMWARE_MANIFEST_KEY=foo".to_string());
        context.set_command_stdout(
            fw_updater.to_string_lossy().into_owned(),
            serde_json::to_string(&fw_manifest).unwrap(),
        );
        let output = BuiltInCollector::SigningKeys.collect(&mut context).unwrap();
        let expected = serde_json::json!({
            "root": "ROOTKEY",
            "recovery": "RECOVERY_KEY"
        });
        assert_eq!(output, expected);
    }

    #[test]
    fn test_cbi_data_success() {
        let mut context = ContextImpl::new();
        let ectool_usage: &str = "\
            Usage: ectool cbi get <tag> [get_flag]
            Usage: ectool cbi set <tag> <value/string> <size> [set_flag]
              <tag> is one of:
                0: BOARD_VERSION
                1: OEM_ID";
        context.set_command_output(
            "ectool",
            Output::new(0, None, Some(ectool_usage.to_string())),
        );
        context.set_command_stdout("ectool", "As uint: 3 (0x3)".to_string());
        context.set_command_stdout("ectool", "some value".to_string());
        let output = BuiltInCollector::CbiData.collect(&mut context).unwrap();
        let expected = serde_json::json!({
            "BOARD_VERSION": 3,
            "OEM_ID": "some value"
        });
        assert_eq!(output, expected);
    }

    #[test]
    fn test_release_image_stateful_partition_success() {
        let mut context = ContextImpl::new();
        context.set_command_stdout("sh", "/dev/sda".to_string());
        context.set_command_stdout("pvdisplay", "".to_string());
        let stateful_dir = context.tempdir(None).unwrap();
        file_utils::create_file(stateful_dir.join("top_level_file"), "").unwrap();
        file_utils::create_file(stateful_dir.join("dev_image/dev_image_file"), "").unwrap();
        file_utils::create_file(
            stateful_dir.join("unencrypted/import_extensions/extensions/preserved.crx"),
            "",
        )
        .unwrap();
        file_utils::create_file(
            stateful_dir.join("unencrypted/dlc-factory-images/dlc/package/dlc.img"),
            "",
        )
        .unwrap();
        file_utils::create_file(stateful_dir.join("unencrypted/this/should/be/excluded"), "")
            .unwrap();
        let output = BuiltInCollector::ReleaseImageStatefulPartition
            .collect(&mut context)
            .unwrap();
        let expected = serde_json::json!([
            "dev_image",
            "top_level_file",
            "unencrypted",
            "dev_image/dev_image_file",
            "unencrypted/import_extensions/extensions/preserved.crx",
            "unencrypted/dlc-factory-images/dlc/package/dlc.img",
        ]);
        assert_eq!(output, expected);
    }

    #[test]
    fn test_ro_gscvd_board_id_success() {
        let mut context = ContextImpl::new();
        context.set_command_output("futility", Output::new(0, None, None));
        let ro_gscvd = context.tempdir(None).unwrap().join("bios_RO_GSCVD");
        fs::write(ro_gscvd, "5afe00000000RCZZ".as_bytes()).unwrap();
        let output = BuiltInCollector::RoGscvdBoardId
            .collect(&mut context)
            .unwrap();
        assert_eq!(output, "ZZCR".to_string());
    }

    #[test]
    fn test_ro_gscvd_board_id_magic_not_found() {
        let mut context = ContextImpl::new();
        context.set_command_output("futility", Output::new(0, None, None));
        let ro_gscvd = context.tempdir(None).unwrap().join("bios_RO_GSCVD");
        fs::write(ro_gscvd, "000000000000RCZZ".as_bytes()).unwrap();
        let err = BuiltInCollector::RoGscvdBoardId
            .collect(&mut context)
            .err()
            .unwrap();
        assert_eq!(
            err.root_cause().to_string(),
            "Failed to find magic number in RO_GSCVD! \
            Expected: [53, 97, 102, 101], \
            Real: [48, 48, 48, 48]"
                .to_string()
        );
    }

    #[test]
    fn test_ro_gscvd_board_id_magic_rlz_not_char() {
        let mut context = ContextImpl::new();
        context.set_command_output("futility", Output::new(0, None, None));
        let ro_gscvd = context.tempdir(None).unwrap().join("bios_RO_GSCVD");
        fs::write(ro_gscvd, "5afe000000000000".as_bytes()).unwrap();
        let err = BuiltInCollector::RoGscvdBoardId
            .collect(&mut context)
            .err()
            .unwrap();
        assert_eq!(
            err.root_cause().to_string(),
            "Each char in the RLZ code should be any char between A~Z. \
            Found: [48, 48, 48, 48]"
                .to_string()
        );
    }

    #[test]
    fn test_intel_psr_provisioned_successfully() {
        let output: &str = "
            Genesis Information:
                Log Start Date: 2023-05-25 02:39:59 (UTC)
                OEM Name: Google
                OEM Make: ChromeOS
                OEM Model: MTL vPro
                Country of Manufacturer: CN
                OEM Data:";
        let mut context = ContextImpl::new();
        context.set_command_stdout("intel-psrtool", output.to_string());
        let actual = BuiltInCollector::IntelPsr.collect(&mut context).unwrap();
        let expected = serde_json::json!({
          "OEM Name": "Google",
          "OEM Make": "ChromeOS",
          "OEM Model": "MTL vPro",
          "Country of Manufacturer": "CN",
        });

        assert_eq!(actual, expected);
    }

    #[test]
    fn test_intel_psr_unsupported() {
        let output = "PSR is not supported on this device.";
        let mut context = ContextImpl::new();
        context.set_command_stdout("intel-psrtool", output.to_string());
        let actual = BuiltInCollector::IntelPsr.collect(&mut context).unwrap();
        let expected = "PSR is either unsupported or has not been provisioned.".to_string();
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_check_psr_missing_data() {
        let output: &str = "
            Genesis Information:
                Log Start Date: 2023-05-25 02:39:59 (UTC)
                OEM Data:";
        let mut context = ContextImpl::new();
        context.set_command_stdout("intel-psrtool", output.to_string());
        let actual_error = BuiltInCollector::IntelPsr
            .collect(&mut context)
            .unwrap_err()
            .root_cause()
            .to_string();
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
