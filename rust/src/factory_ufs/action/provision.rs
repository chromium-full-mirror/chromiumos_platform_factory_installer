// Copyright 2022 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::env;
use std::fs;
use std::io::Cursor;
use std::path::Path;
use std::thread;
use std::time::Duration;

use anyhow::{self, Result};
use bincode;
use byteorder::ReadBytesExt;
use num_derive::FromPrimitive;
use num_traits::FromPrimitive;
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

use crate::factory_ufs::action::constants;
use crate::factory_ufs::ufs::descriptor::common::CommonDescriptorTrait;
use crate::factory_ufs::ufs::descriptor::config::{ProvisionLun0, WriteBoosterConfig};
use crate::factory_ufs::ufs::spec::ufs_2_1::{self, UFS2ConfigDescriptor};
use crate::factory_ufs::ufs::spec::ufs_3_1::{self, UFS3ConfigDescriptor};
use crate::factory_ufs::utils::file_utils;
use crate::factory_ufs::utils::path_utils::{self, PathMatchError, UFSPath};
use crate::factory_ufs::utils::sys_utils::{self, PowerControl};
use crate::utils::file_utils::PathError;
use crate::utils::process_utils::{Command, StringOutput};

#[derive(FromPrimitive)]
enum SupportedWBBufferUserSpaceReductionTypes {
    UserSpaceReductionType = 0x0,
    PreserveUserSpaceType = 0x1,
    EitherType = 0x2,
}

#[derive(FromPrimitive)]
enum SupportedWBBufferTypes {
    LUBased = 0x0,
    SingleShared = 0x1,
    SupportBoth = 0x2,
}

const UFS_CONFIG_NAME: &str = "config_desc_data_ind_0";
const PCI_RESCAN_PATH: &str = "/sys/bus/pci/rescan";
const MTK_UFSHCD_PATH: &str = "/sys/bus/platform/drivers/ufshcd-mtk";
const SUPPORT_WB_MASK: u32 = 1 << 8;

/// Rescans to bring the device up.
fn rescan_dev_and_sync(sys_dev_path: &Path, rescan_timeout: u32) -> Result<()> {
    if path_utils::is_pci_path(sys_dev_path) {
        let pci_remove_path = Path::new(&sys_dev_path).join("remove");

        println!("Perform pci remove: {}", pci_remove_path.display());
        fs::write(&pci_remove_path, "1")?;
        println!("Perform pci rescan: {}", PCI_RESCAN_PATH);
        fs::write(PCI_RESCAN_PATH, "1")?;
    } else if path_utils::is_platform_path(sys_dev_path) {
        let mtk_ufsci_path = Path::new(MTK_UFSHCD_PATH)
            .join("*.ufshci")
            .display()
            .to_string();
        let device_path = path_utils::match_path(&mtk_ufsci_path)?;
        let device_name = device_path
            .file_name()
            .ok_or(PathError::GetFilenameError)?
            .to_str()
            .ok_or(anyhow::anyhow!("Fail to convert basename into string."))?;
        let unbind_path = Path::new(MTK_UFSHCD_PATH).join("unbind");
        let bind_path = Path::new(MTK_UFSHCD_PATH).join("bind");

        println!(
            "Unbind device {} via {}",
            device_name,
            unbind_path.display()
        );
        fs::write(&unbind_path, device_name)?;
        println!("Bind device {} via {}", device_name, bind_path.display());
        fs::write(&bind_path, device_name)?;
    } else {
        anyhow::bail!("Failed to rebind UFS storage");
    }

    println!("Wait for root device to be available...");
    let rootdev_pattern = Path::new(&sys_dev_path)
        .join("host*")
        .join("target*")
        .join("*")
        .join("block")
        .join("sd*")
        .display()
        .to_string();
    for _ in 0..rescan_timeout {
        thread::sleep(Duration::from_secs(1));
        match path_utils::match_path(&rootdev_pattern) {
            Ok(path) => {
                println!("Root device found: {}", path.display());
                return Ok(());
            }
            Err(err) => {
                if let Some(err) = err.downcast_ref::<PathMatchError>() {
                    if matches!(err, PathMatchError::PathNotFound) {
                        println!("Root device not found. Keep waiting...");
                        continue;
                    } else {
                        anyhow::bail!("Multiple root device found!");
                    }
                } else {
                    anyhow::bail!("Error occurs: {}", err);
                }
            }
        };
    }
    anyhow::bail!(
        "Root device pattern {} not found after {} seconds. Aborting...",
        rootdev_pattern,
        rescan_timeout
    );
}

/// Writes new UFS config using [constants::UFS_UTILS].
fn write_ufs_config<T: Serialize>(bsg_node_path: &Path, ufs_config: &T) -> Result<()> {
    let config_path = NamedTempFile::new()?.path().display().to_string();
    let config_bin_vec = serialize_config::<T>(&ufs_config)?;
    fs::write(&config_path, config_bin_vec)?;

    println!("Write config to {}...", bsg_node_path.display());
    let output = Command::new(constants::UFS_UTILS)
        .args([
            "desc",
            "-t",
            "1",
            "-p",
            &bsg_node_path.display().to_string(),
            "-w",
            &config_path,
        ])
        .current_dir(env::temp_dir())
        .output()?;
    println!("{}", output.stdout());
    Ok(())
}

/// Serializes an UFS config to a u8 vector.
fn serialize_config<T: Serialize>(ufs_config: &T) -> Result<Vec<u8>> {
    Ok(bincode::config().big_endian().serialize::<T>(&ufs_config)?)
}

/// Deserializes a binary config to a given type.
fn deserialize_config<'de, T: Deserialize<'de>>(config_binary: &'de Vec<u8>) -> Result<T> {
    Ok(bincode::config().big_endian().deserialize(&config_binary)?)
}

/// Generates a new UFS config given the allocation units.
///
/// Here we provision the LUN 0 with `thin` provisioning type with TPRZ.
/// In this mode, when we try to securely remove the data from the device,
/// UNMAP triggers `Erase operation`, and renders the value of the block to
/// be zero on read. Additional purge operation can be used to ensure physical
/// erasure of the unmapped blocks.
fn generate_new_config<T: Default + ProvisionLun0>(alloc_units: u32) -> Result<T> {
    let mut ufs_config = T::default();
    ufs_config.provision_lun_0(alloc_units);

    Ok(ufs_config)
}

/// Gets the total number of allocation units from the geometry_descriptor.
fn get_alloc_units(sys_dev_path: &Path) -> Result<u32> {
    let total_capacity = file_utils::read_geometry_descriptor(sys_dev_path, "raw_device_capacity")?;
    let segment_size = file_utils::read_geometry_descriptor(sys_dev_path, "segment_size")?;
    let segments_per_allocation_unit =
        file_utils::read_geometry_descriptor(sys_dev_path, "allocation_unit_size")?;

    println!("UFS capacity: {} GB", total_capacity / 0x200000);
    let units = total_capacity / segment_size / segments_per_allocation_unit;

    Ok(units)
}

/// Gets the current UFS config using [constants::UFS_UTILS].
pub fn get_current_config_binary(bsg_node_path: &Path) -> Result<Vec<u8>> {
    // Needs to switch to /tmp since ufs-utils dumps the config to cwd
    // with name `UFS_CONFIG_NAME`.
    let cur_config_path = env::temp_dir().join(UFS_CONFIG_NAME);
    println!("Dumping current config to {}", cur_config_path.display());
    Command::new(constants::UFS_UTILS)
        .args([
            "desc",
            "-t",
            "1",
            "-p",
            &bsg_node_path.display().to_string(),
        ])
        .current_dir(env::temp_dir())
        .output()
        .ok();

    println!("Reading current config...");
    Ok(fs::read(cur_config_path)?)
}

/// Reads the first byte from a config binary.
pub fn read_header(ufs_config_binary: &Vec<u8>) -> Result<u8> {
    Ok(Cursor::new(ufs_config_binary).read_u8()?)
}

/// Configures the write booster buffer if the operation is supported.
///
/// The write performance of TLC (triple level cell) NAND is slower than
/// SLC (single level cell) NAND. To improve the write performance, we can
/// configure part of the TLC NAND (normal storage) to SLC NAND (write buffer).
/// Ref: 13.4.17.2 WriteBooster configuration from `UFS 3.1`
fn config_write_booster<T: WriteBoosterConfig>(
    sys_dev_path: &Path,
    ufs_config: &mut T,
) -> Result<()> {
    if !ufs_config.support_write_booster() {
        println!("UFS Spec does not support write booster. Skipping...");
        return Ok(());
    }

    // Reads the DEVICE_DESC.dExtendedUFSFeaturesSupport bits.
    // The bits[8] defines if a device supports the WriteBooster feature or not.
    let ext_feature_sup = file_utils::read_device_descriptor(&sys_dev_path, "ext_feature_sup")?;
    let support_write_booster = (ext_feature_sup & SUPPORT_WB_MASK) == SUPPORT_WB_MASK;

    // Reads the GEOM_DESC.bSupportedWriteBoosterBufferUserSpaceReductionTypes bits.
    let wb_sup_red_type = file_utils::read_geometry_descriptor(&sys_dev_path, "wb_sup_red_type")?;
    let support_preserver_user_space = match FromPrimitive::from_u32(wb_sup_red_type) {
        Some(SupportedWBBufferUserSpaceReductionTypes::PreserveUserSpaceType)
        | Some(SupportedWBBufferUserSpaceReductionTypes::EitherType) => true,
        _ => false,
    };

    // Reads the GEOM_DESC.bSupportedWriteBoosterBufferTypes.
    let wb_sup_wb_type = file_utils::read_geometry_descriptor(&sys_dev_path, "wb_sup_wb_type")?;
    let support_single_shared_buffer = match FromPrimitive::from_u32(wb_sup_wb_type) {
        Some(SupportedWBBufferTypes::SingleShared) | Some(SupportedWBBufferTypes::SupportBoth) => {
            true
        }
        _ => false,
    };

    println!(
        "DEVICE_DESC.dExtendedUFSFeaturesSupport: {}",
        ext_feature_sup
    );
    println!(
        "GEOM_DESC.bSupportedWriteBoosterBufferUserSpaceReductionTypes: {}",
        wb_sup_red_type
    );
    println!(
        "GEOM_DESC.bSupportedWriteBoosterBufferTypes: {}",
        wb_sup_wb_type
    );

    // Configures WB as single shared buffer that preserves user space capacity.
    if support_write_booster && support_preserver_user_space && support_single_shared_buffer {
        // Reads the GEOM_DESC.dWriteBoosterBufferMaxNAllocUnits bits.
        // This is the maximum total write booster buffer size which is
        // supported by the entire device.
        let wb_max_alloc_units =
            file_utils::read_geometry_descriptor(&sys_dev_path, "wb_max_alloc_units")?;
        println!(
            "GEOM_DESC.dWriteBoosterBufferMaxNAllocUnits: {}",
            wb_max_alloc_units
        );
        println!("Enable write booster...");
        let wb_alloc_units = ufs_config.enable_write_booster(wb_max_alloc_units)?;
        println!(
            "{} units is allocated for write booster buffer.",
            wb_alloc_units
        );
    } else {
        println!("Disable write booster...");
        ufs_config.disable_write_booster()?;
    }

    Ok(())
}

/// Provisions the UFS if needed.
///
/// This function compares the current UFS config and the newly generated
/// config, and skips provisioning if these two configs are the same.
fn provision_ufs_if_needed<
    'de,
    T: CommonDescriptorTrait<'de> + ProvisionLun0 + WriteBoosterConfig,
>(
    ufs_path: &UFSPath,
    cur_config_binary: &'de Vec<u8>,
    rescan_timeout: u32,
) -> Result<()> {
    let cur_config = deserialize_config::<'de, T>(cur_config_binary)?;

    let alloc_units = get_alloc_units(&ufs_path.sys_dev)?;
    let mut new_config = generate_new_config::<T>(alloc_units)?;

    config_write_booster(&ufs_path.sys_dev, &mut new_config)?;

    println!("Current UFS config on DUT: {:#?}", cur_config);

    if cur_config == new_config {
        println!("Current UFS config is the same as the new config. Skip UFS provisioning.");
        return Ok(());
    }

    println!("New UFS config: {:#?}", new_config);

    write_ufs_config(&ufs_path.bsg_node, &new_config)?;
    rescan_dev_and_sync(&ufs_path.sys_dev, rescan_timeout)?;
    println!("Finish UFS provisioning.");

    Ok(())
}

/// Provisions UFS storage.
pub fn action_provision(ufs_path: &UFSPath, rescan_timeout: u32) -> Result<()> {
    // We need to parse the header of current UFS config to decide which UFS
    // spec to load.
    println!("Running action_provision...");
    let cur_config_binary = get_current_config_binary(&ufs_path.bsg_node)?;
    let header_value = read_header(&cur_config_binary)?;

    // Disable auto-suspend so that the descriptors can be queried. (b/263069232)
    if sys_utils::set_power_control(PowerControl::On).is_err() {
        println!("Failed to set power control. Skip setting power control.");
    }
    match header_value {
        ufs_2_1::HEADER_LENGTH => {
            println!("Provision UFS 2.1...");
            provision_ufs_if_needed::<UFS2ConfigDescriptor>(
                &ufs_path,
                &cur_config_binary,
                rescan_timeout,
            )?;
        }
        ufs_3_1::HEADER_LENGTH => {
            println!("Provision UFS 3.1...");
            provision_ufs_if_needed::<UFS3ConfigDescriptor>(
                &ufs_path,
                &cur_config_binary,
                rescan_timeout,
            )?;
        }
        value => anyhow::bail!(
            "Cannot parse current UFS config! Unknown header value: {}",
            value
        ),
    }
    sys_utils::set_power_control(PowerControl::Auto)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::cmp::min;
    use std::path::Path;

    use crate::factory_ufs::action::provision::{
        self, SupportedWBBufferTypes, SupportedWBBufferUserSpaceReductionTypes,
    };
    use crate::factory_ufs::ufs::descriptor::config::ProvisionLun0;
    use crate::factory_ufs::ufs::descriptor::device::GetDeviceField;
    use crate::factory_ufs::ufs::descriptor::unit::GetUnitField;
    use crate::factory_ufs::ufs::spec::constants;
    use crate::factory_ufs::ufs::spec::ufs_2_1::{self, UFS2ConfigDescriptor};
    use crate::factory_ufs::ufs::spec::ufs_3_1::{self, UFS3ConfigDescriptor};
    use crate::utils::file_utils;
    use crate::utils::string_utils;

    #[test]
    fn test_read_header() {
        let config_binary: Vec<u8> = vec![0x90, 0x01, 0x00, 0x00];
        let header_value = provision::read_header(&config_binary).unwrap();

        assert_eq!(header_value, config_binary[0]);
    }

    // The allocation units in FAKE_UFSX_CONFIG_BINARY.
    const ALLOC_UNITS: u32 = 0x7732;

    // FAKE_UFS2_CONFIG_BINARY and FAKE_UFS3_CONFIG_BINARY share similar
    // values, except that the header length are different.
    const FAKE_UFS2_CONFIG_BINARY: [u8; 144] = [
        0x90, 0x1, 0x0, 0x1, 0x0, 0x1, 0x7f, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x1, 0x0,
        0x0, 0x0, 0x0, 0x0, 0x77, 0x32, 0x0, 0xc, 0x3, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0,
        0x0, 0x0, 0x0, 0x0, 0x0, 0xc, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0,
        0x0, 0x0, 0x0, 0xc, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0,
        0x0, 0xc, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0xc,
        0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0xc, 0x0, 0x0,
        0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0xc, 0x0, 0x0, 0x0, 0x0,
        0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0xc, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0,
    ];

    const FAKE_UFS3_CONFIG_BINARY: [u8; 230] = [
        0xe6, 0x1, 0x0, 0x1, 0x0, 0x1, 0x7f, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0,
        0x0, 0x0, 0x0, 0x0, 0x1, 0x0, 0x0, 0x0, 0x0, 0x0, 0x77, 0x32, 0x0, 0xc, 0x3, 0x0, 0x0, 0x0,
        0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0,
        0x0, 0x0, 0x0, 0xc, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0,
        0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0xc, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0,
        0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0,
        0x0, 0xc, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0,
        0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0xc, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0,
        0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0xc,
        0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0,
        0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0xc, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0,
        0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0xc, 0x0, 0x0,
        0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0,
    ];

    fn compare_unit0_descriptor<T: GetUnitField>(unit_descriptor: &T) {
        // The input unit descriptor should have the same field values in the
        // FAKE_UFSX_CONFIG_BINARY.
        assert_eq!(unit_descriptor.get_lu_enabled(), 0x1);
        assert_eq!(unit_descriptor.get_alloc_units(), ALLOC_UNITS);
        assert_eq!(
            unit_descriptor.get_provisioning_type(),
            constants::PROVISIONING_TYPE
        );
    }

    #[test]
    fn test_deserialize_config() {
        let ufs2_config_descriptor = provision::deserialize_config::<UFS2ConfigDescriptor>(
            &FAKE_UFS2_CONFIG_BINARY.to_vec(),
        )
        .unwrap();

        assert_eq!(
            ufs2_config_descriptor.device_config.get_header_length(),
            ufs_2_1::HEADER_LENGTH
        );
        compare_unit0_descriptor(&ufs2_config_descriptor.units_config[0]);

        let ufs3_config_descriptor = provision::deserialize_config::<UFS3ConfigDescriptor>(
            &FAKE_UFS3_CONFIG_BINARY.to_vec(),
        )
        .unwrap();

        assert_eq!(
            ufs3_config_descriptor.device_config.get_header_length(),
            ufs_3_1::HEADER_LENGTH
        );
        compare_unit0_descriptor(&ufs3_config_descriptor.units_config[0]);
    }

    #[test]
    fn test_serialize_config() {
        let mut ufs2_config = UFS2ConfigDescriptor::default();
        ufs2_config.provision_lun_0(ALLOC_UNITS);
        let ufs2_config_binary =
            provision::serialize_config::<UFS2ConfigDescriptor>(&ufs2_config).unwrap();

        assert_eq!(ufs2_config_binary, FAKE_UFS2_CONFIG_BINARY);

        let mut ufs3_config = UFS3ConfigDescriptor::default();
        ufs3_config.provision_lun_0(ALLOC_UNITS);
        let ufs3_config_binary =
            provision::serialize_config::<UFS3ConfigDescriptor>(&ufs3_config).unwrap();

        assert_eq!(ufs3_config_binary, FAKE_UFS3_CONFIG_BINARY);
    }

    #[test]
    fn test_generate_new_config() {
        let alloc_units: u32 = 10;
        let ufs2_config =
            provision::generate_new_config::<UFS2ConfigDescriptor>(alloc_units).unwrap();
        let ufs3_config =
            provision::generate_new_config::<UFS3ConfigDescriptor>(alloc_units).unwrap();

        assert_eq!(
            ufs2_config.device_config.get_header_length(),
            ufs_2_1::HEADER_LENGTH
        );
        assert_eq!(ufs2_config.units_config[0].get_alloc_units(), alloc_units);

        assert_eq!(
            ufs3_config.device_config.get_header_length(),
            ufs_3_1::HEADER_LENGTH
        );
        assert_eq!(ufs3_config.units_config[0].get_alloc_units(), alloc_units);
    }

    #[test]
    fn test_get_alloc_units() {
        let sys_dev_path = tempfile::Builder::new()
            .prefix("test_sys_dev")
            .tempdir()
            .unwrap();
        let geometry_descriptor_path = sys_dev_path.path().join("geometry_descriptor");
        file_utils::create_file(
            geometry_descriptor_path.join("raw_device_capacity"),
            "0xee64000",
        )
        .unwrap();
        file_utils::create_file(geometry_descriptor_path.join("segment_size"), "0x2000").unwrap();
        file_utils::create_file(geometry_descriptor_path.join("allocation_unit_size"), "0x2")
            .unwrap();

        let val = provision::get_alloc_units(&sys_dev_path.path()).unwrap();
        assert_eq!(val, 0x3b99);
    }

    #[test]
    fn test_config_write_booster_unsupported() {
        // UFS2_1 do not support write booster, so the config will not be modified.
        let mut ufs2_config =
            provision::generate_new_config::<UFS2ConfigDescriptor>(ALLOC_UNITS).unwrap();
        provision::config_write_booster(Path::new("unused_path"), &mut ufs2_config).unwrap();
        assert_eq!(
            ufs2_config,
            provision::generate_new_config::<UFS2ConfigDescriptor>(ALLOC_UNITS).unwrap()
        );
    }

    fn setup_write_booster_test(
        sys_dev_path: &Path,
        ext_feature_sup: u32,
        wb_sup_red_type: u32,
        wb_sup_wb_type: u32,
        wb_max_alloc_units: u32,
    ) {
        file_utils::create_file(
            sys_dev_path
                .join("device_descriptor")
                .join("ext_feature_sup"),
            string_utils::u32_to_hex(ext_feature_sup),
        )
        .unwrap();
        file_utils::create_file(
            sys_dev_path
                .join("geometry_descriptor")
                .join("wb_sup_red_type"),
            string_utils::u32_to_hex(wb_sup_red_type),
        )
        .unwrap();
        file_utils::create_file(
            sys_dev_path
                .join("geometry_descriptor")
                .join("wb_sup_wb_type"),
            string_utils::u32_to_hex(wb_sup_wb_type),
        )
        .unwrap();
        file_utils::create_file(
            sys_dev_path
                .join("geometry_descriptor")
                .join("wb_max_alloc_units"),
            string_utils::u32_to_hex(wb_max_alloc_units),
        )
        .unwrap();
    }

    #[test]
    fn test_config_write_booster_enable() {
        let sys_dev_path = tempfile::Builder::new()
            .prefix("test_sys_dev")
            .tempdir()
            .unwrap();
        let wb_max_alloc_units: u32 = 0x1dcb;
        let mut ufs3_config =
            provision::generate_new_config::<UFS3ConfigDescriptor>(ALLOC_UNITS).unwrap();
        setup_write_booster_test(
            &sys_dev_path.path(),
            provision::SUPPORT_WB_MASK,
            SupportedWBBufferUserSpaceReductionTypes::EitherType as u32,
            SupportedWBBufferTypes::SupportBoth as u32,
            wb_max_alloc_units,
        );
        provision::config_write_booster(&sys_dev_path.path(), &mut ufs3_config).unwrap();
        let device_config = ufs3_config.device_config;

        assert_eq!(
            device_config.b_write_booster_buffer_preserve_user_space_en,
            0x1
        );
        assert_eq!(device_config.b_write_booster_buffer_type, 0x1);
        assert_eq!(
            device_config.d_num_shared_write_booster_buffer_alloc_units,
            min((ALLOC_UNITS as f32 * 0.1) as u32, wb_max_alloc_units)
        );
    }

    #[test]
    fn test_config_write_booster_disable() {
        let sys_dev_path = tempfile::Builder::new()
            .prefix("test_sys_dev")
            .tempdir()
            .unwrap();
        let mut ufs3_config =
            provision::generate_new_config::<UFS3ConfigDescriptor>(ALLOC_UNITS).unwrap();

        setup_write_booster_test(&sys_dev_path.path(), 0, 0, 0, 0);
        provision::config_write_booster(&sys_dev_path.path(), &mut ufs3_config).unwrap();
        let device_config = ufs3_config.device_config;
        assert_eq!(
            device_config.b_write_booster_buffer_preserve_user_space_en,
            0x0
        );
        assert_eq!(device_config.b_write_booster_buffer_type, 0x0);
        assert_eq!(
            device_config.d_num_shared_write_booster_buffer_alloc_units,
            0x0
        );
    }
}
