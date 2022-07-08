// Copyright 2022 The ChromiumOS Authors.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::env;
use std::error::Error;
use std::fmt::{Debug, Display, Formatter, Result as fmt_Result};
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use anyhow::{self, Result};
use bincode;
use byteorder::ReadBytesExt;
use glob;
use num_traits::FromPrimitive;
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

use crate::device;
use crate::factory_ufs::ufs_spec::ufs_common::{
    self, CommonDescriptorTrait, PurgeStatus, SupportedWBBufferTypes,
    SupportedWBBufferUserSpaceReductionTypes,
};
use crate::factory_ufs::ufs_spec::ufs_config_descriptor::{
    ProvisionLun0, UFS2ConfigDescriptor, UFS3ConfigDescriptor, WriteBoosterConfig,
};
use crate::factory_ufs::ufs_spec::{ufs_2_1, ufs_3_1};
use crate::utils::process_utils::{Command, StringOutput};

#[derive(Debug)]
pub enum PathMatchError {
    PathNotFound,
    MultiplePathFound,
}

impl Error for PathMatchError {}

impl Display for PathMatchError {
    fn fmt(&self, f: &mut Formatter) -> fmt_Result {
        match self {
            PathMatchError::PathNotFound => write!(f, "Path not found."),
            PathMatchError::MultiplePathFound => write!(f, "Multiple paths found."),
        }
    }
}

#[derive(Debug)]
pub struct NoParentDirError;

impl Error for NoParentDirError {}

impl Display for NoParentDirError {
    fn fmt(&self, f: &mut Formatter) -> fmt_Result {
        write!(f, "No parent directory can be found.")
    }
}

pub struct UFSPath {
    pub bsg_node: PathBuf,
    pub sys_dev: PathBuf,
}

/// Gets one match path given a path regex.
///
/// # Arguments
///
/// * `path_regex` - A string which defines the pattern to match.
///
/// # Return value
///
/// Returns a result type. Raise error if path is not found or multiple paths
/// are found.
fn match_path(path_regex: &str) -> Result<PathBuf> {
    let mut paths = Vec::<PathBuf>::new();
    for path in glob::glob(path_regex)? {
        paths.push(path?);
    }
    if paths.is_empty() {
        anyhow::bail!(PathMatchError::PathNotFound);
    } else if paths.len() != 1 {
        anyhow::bail!(PathMatchError::MultiplePathFound);
    }

    Ok(paths[0].clone())
}

/// Returns the path to pattern [ufs_common::UFS_NODE_PATTERN].
pub fn get_ufs_bsg_node() -> Result<PathBuf> {
    match_path(ufs_common::UFS_NODE_PATTERN)
}

/// Returns the path to pattern [ufs_common::SYS_DEV_PATTERN].
pub fn get_ufs_bsg_sys() -> Result<PathBuf> {
    match_path(ufs_common::SYS_DEV_PATTERN)
}

/// Rescans to bring the device up.
fn rescan_dev_and_sync(sys_dev_path: &Path, rescan_timeout: u32) -> Result<()> {
    let pci_remove_path = Path::new(&sys_dev_path).join("remove");

    println!("Perform pci remove: {}", pci_remove_path.display());
    fs::write(&pci_remove_path, "1")?;
    println!("Perform pci rescan: {}", ufs_common::PCI_RESCAN_PATH);
    fs::write(ufs_common::PCI_RESCAN_PATH, "1")?;

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
        match match_path(&rootdev_pattern) {
            Ok(path) => {
                println!("Root device found after pci rescan: {}", path.display());
                return Ok(());
            }
            Err(err) => {
                if let Some(err) = err.downcast_ref::<PathMatchError>() {
                    if matches!(err, PathMatchError::PathNotFound) {
                        println!("Root device not found. Keep waiting...");
                        continue;
                    } else {
                        anyhow::bail!("Multiple root device found after pci rescan!");
                    }
                } else {
                    anyhow::bail!("Error occurs after pci rescan: {}", err);
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

/// Writes new UFS config using ufs-utils.
fn write_ufs_config<T: Serialize>(bsg_node_path: &Path, ufs_config: &T) -> Result<()> {
    let config_path = NamedTempFile::new()?.path().display().to_string();
    let config_bin_vec = serialize_config::<T>(&ufs_config)?;
    fs::write(&config_path, config_bin_vec)?;

    println!("Write config to {}...", bsg_node_path.display());
    let output = Command::new("ufs-utils")
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

/// Reads the sysfs file from a given path.
fn read_sysfs(sys_dev_path: &Path, dir_name: &str, f_name: &str) -> Result<u32> {
    // The content of the descriptor is a hex string.
    // We need to transform it into u32.
    let path = Path::new(sys_dev_path).join(dir_name).join(f_name);
    let raw_string = fs::read_to_string(path)?
        .trim()
        .trim_start_matches("0x")
        .to_string();

    Ok(u32::from_str_radix(&raw_string, 16)?)
}

fn read_geometry_descriptor(sys_dev_path: &Path, f_name: &str) -> Result<u32> {
    Ok(read_sysfs(&sys_dev_path, "geometry_descriptor", &f_name)?)
}

fn read_attributes(sys_dev_path: &Path, f_name: &str) -> Result<u32> {
    Ok(read_sysfs(&sys_dev_path, "attributes", &f_name)?)
}

fn read_device_descriptor(sys_dev_path: &Path, f_name: &str) -> Result<u32> {
    Ok(read_sysfs(&sys_dev_path, "device_descriptor", &f_name)?)
}

fn set_purge_flag(bsg_node_path: &Path) -> Result<()> {
    // -t 6 is flag type `fPurgeEnable`
    Command::new("ufs-utils")
        .args([
            "fl",
            "-t",
            "6",
            "-e",
            "-p",
            &bsg_node_path.display().to_string(),
        ])
        .output()?
        .exit_ok()?;
    Ok(())
}

fn discard_stateful() -> Result<()> {
    let stateful_dev = device::get_release_stateful_partition()?;
    println!(
        "Discard stateful partition {} using blkdiscard...",
        stateful_dev
    );
    Command::new("blkdiscard")
        .args([stateful_dev])
        .output()?
        .exit_ok()?;
    Ok(())
}

/// Gets the total number of allocation units from the geometry_descriptor.
fn get_alloc_units(sys_dev_path: &Path) -> Result<u32> {
    let total_capacity = read_geometry_descriptor(sys_dev_path, "raw_device_capacity")?;
    let segment_size = read_geometry_descriptor(sys_dev_path, "segment_size")?;
    let segments_per_allocation_unit =
        read_geometry_descriptor(sys_dev_path, "allocation_unit_size")?;

    println!("UFS capacity: {} GB", total_capacity / 0x200000);
    let units = total_capacity / segment_size / segments_per_allocation_unit;

    Ok(units)
}

/// Gets the current UFS config using ufs-utils.
pub fn get_current_config_binary(bsg_node_path: &Path) -> Result<Vec<u8>> {
    // Needs to switch to /tmp since ufs-utils dumps the config to cwd
    // with name `ufs_common::UFS_CONFIG_NAME`.
    Command::new("ufs-utils")
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
    let cur_config_path = env::temp_dir().join(ufs_common::UFS_CONFIG_NAME);

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
fn config_write_booster_if_supported<T: WriteBoosterConfig>(
    sys_dev_path: &Path,
    ufs_config: &mut T,
) -> Result<()> {
    if !ufs_config.support_write_booster() {
        println!("UFS Spec does not support write booster. Skipping...");
        return Ok(());
    }

    // Reads the DEVICE_DESC.dExtendedUFSFeaturesSupport bits.
    // The bits[8] defines if a device supports the WriteBooster feature or not.
    let ext_feature_sup = read_device_descriptor(&sys_dev_path, "ext_feature_sup")?;
    let support_write_booster =
        (ext_feature_sup & ufs_common::SUPPORT_WB_MASK) == ufs_common::SUPPORT_WB_MASK;

    // Reads the GEOM_DESC.bSupportedWriteBoosterBufferUserSpaceReductionTypes bits.
    let wb_sup_red_type = read_geometry_descriptor(&sys_dev_path, "wb_sup_red_type")?;
    let support_preserver_user_space = match FromPrimitive::from_u32(wb_sup_red_type) {
        Some(SupportedWBBufferUserSpaceReductionTypes::PreserveUserSpaceType)
        | Some(SupportedWBBufferUserSpaceReductionTypes::EitherType) => true,
        _ => false,
    };

    // Reads the GEOM_DESC.bSupportedWriteBoosterBufferTypes.
    let wb_sup_wb_type = read_geometry_descriptor(&sys_dev_path, "wb_sup_wb_type")?;
    let support_single_shared_buffer = match FromPrimitive::from_u32(wb_sup_red_type) {
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
        let wb_max_alloc_units = read_geometry_descriptor(&sys_dev_path, "wb_max_alloc_units")?;
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

    config_write_booster_if_supported(&ufs_path.sys_dev, &mut new_config)?;

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
    let cur_config_binary = get_current_config_binary(&ufs_path.bsg_node)?;
    let header_value = read_header(&cur_config_binary)?;

    match header_value {
        ufs_2_1::HEADER_LENGTH => {
            println!("Provision UFS 2.1...");
            provision_ufs_if_needed::<UFS2ConfigDescriptor>(
                &ufs_path,
                &cur_config_binary,
                rescan_timeout,
            )
        }
        ufs_3_1::HEADER_LENGTH => {
            println!("Provision UFS 3.1...");
            provision_ufs_if_needed::<UFS3ConfigDescriptor>(
                &ufs_path,
                &cur_config_binary,
                rescan_timeout,
            )
        }
        value => anyhow::bail!(
            "Cannot parse current UFS config! Unknown header value: {}",
            value
        ),
    }
}

/// Shows the current UFS config on DUT.
pub fn action_show(_ufs_path: &UFSPath) -> Result<()> {
    println!("Action show is not supported yet.");
    Ok(())
}

/// Performs clobber to clean up the stateful partition.
///
/// This function first discards the stateful partition by calling `blkdiscard`.
/// It then enables purge operation and waits until the operation is finished.
pub fn action_clobber(ufs_path: &UFSPath, purge_timeout: u32) -> Result<()> {
    discard_stateful()?;
    if purge_timeout == 0 {
        println!("Skip purging since it is disabled.");
        println!("Clobber completed successfully.");
        return Ok(());
    }

    set_purge_flag(&ufs_path.bsg_node)?;

    println!("Running purge...");
    for sec in 0..purge_timeout {
        let message: &str;
        // Ref: UFS2.1 12.2.3.3 Purge operation
        let purge_status = read_attributes(&ufs_path.sys_dev, "purge_status")?;
        match FromPrimitive::from_u32(purge_status) {
            Some(PurgeStatus::Idle) => {
                message = "Purge hasn't started yet.";
            }
            Some(PurgeStatus::InProgress) => {
                message = "Purge operation in progress.";
            }
            Some(PurgeStatus::StoppedPrematurely) => {
                anyhow::bail!("Purge operation stopped prematurely by the host.");
            }
            Some(PurgeStatus::Complete) => {
                println!("Purge completed successfully.");
                return Ok(());
            }
            Some(PurgeStatus::QueueNotEmpty) => {
                anyhow::bail!("Purge operation failed due to logical unit queue not empty.");
            }
            None => {
                anyhow::bail!("Purge operation general failure.");
            }
        }

        if sec % 30 == 0 {
            println!(
                "{}\nKeep waiting... (Time remained: {} seconds)",
                message,
                purge_timeout - sec
            );
        }
        thread::sleep(Duration::from_secs(1));
    }

    anyhow::bail!("Purge operation timeout after {} seconds!", purge_timeout);
}

#[cfg(test)]
mod tests {
    use std::fs::{self, File};
    use std::path::{Path, PathBuf};

    use tempdir::TempDir;

    use crate::factory_ufs::ufs_provision;
    use crate::factory_ufs::ufs_spec::ufs_config_descriptor::{
        UFS2ConfigDescriptor, UFS3ConfigDescriptor,
    };
    use crate::factory_ufs::ufs_spec::{ufs_2_1, ufs_3_1, ufs_common};

    #[test]
    fn test_read_header() {
        let config_binary: Vec<u8> = vec![0x90, 0x01, 0x00, 0x00];
        let header_value = ufs_provision::read_header(&config_binary).unwrap();

        assert_eq!(header_value, config_binary[0]);
    }

    fn generate_ufs2_config_binary() -> Vec<u8> {
        vec![
            144, 1, 0, 1, 0, 1, 127, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 119, 50, 0, 12,
            3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 12, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 12, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 12, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 12, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 12, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 12, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 12, 0,
            0, 0, 0, 0, 0,
        ]
    }

    #[test]
    fn test_deserialize_config() {
        // TODO: (phoebewang) add ufs3
        let ufs2_config_binary = generate_ufs2_config_binary();
        let ufs2_config_descriptor =
            ufs_provision::deserialize_config::<UFS2ConfigDescriptor>(&ufs2_config_binary).unwrap();

        assert_eq!(
            ufs2_config_descriptor.device_config.b_length,
            ufs_2_1::HEADER_LENGTH
        );
        assert_eq!(ufs2_config_descriptor.units_config[0].b_lu_enable, 0x1);
        // 119 = 0x77, 50 = 0x32
        assert_eq!(
            ufs2_config_descriptor.units_config[0].d_num_alloc_units,
            0x7732
        );
        assert_eq!(
            ufs2_config_descriptor.units_config[0].b_provisioning_type,
            ufs_common::PROVISIONING_TYPE
        );
    }

    #[test]
    fn test_serialize_config() {
        let mut ufs2_config = UFS2ConfigDescriptor::default();
        ufs2_config.units_config[0].b_lu_enable = 0x1;
        ufs2_config.units_config[0].d_num_alloc_units = 0x7732;
        ufs2_config.units_config[0].b_provisioning_type = ufs_common::PROVISIONING_TYPE;

        let ufs2_config_binary =
            ufs_provision::serialize_config::<UFS2ConfigDescriptor>(&ufs2_config).unwrap();

        assert_eq!(ufs2_config_binary, generate_ufs2_config_binary());
    }

    #[test]
    fn test_generate_new_config() {
        let alloc_units: u32 = 10;
        let ufs2_config =
            ufs_provision::generate_new_config::<UFS2ConfigDescriptor>(alloc_units).unwrap();
        let ufs3_config =
            ufs_provision::generate_new_config::<UFS3ConfigDescriptor>(alloc_units).unwrap();

        assert_eq!(ufs2_config.device_config.b_length, ufs_2_1::HEADER_LENGTH);
        assert_eq!(ufs2_config.units_config[0].d_num_alloc_units, alloc_units);

        assert_eq!(ufs3_config.device_config.b_length, ufs_3_1::HEADER_LENGTH);
        assert_eq!(ufs3_config.units_config[0].d_num_alloc_units, alloc_units);
    }

    fn create_geometry_descriptor_dir(sys_dev_path: &Path) -> PathBuf {
        let geometry_dir = sys_dev_path.join("geometry_descriptor");
        fs::create_dir(&geometry_dir).unwrap();

        geometry_dir.to_path_buf()
    }

    fn create_geometry_descriptor_files(geometry_dir: &Path, fname: &str, content: &str) {
        let geometry_file_path = geometry_dir.join(fname);
        File::create(&geometry_file_path).unwrap();
        fs::write(geometry_file_path, content).unwrap();
    }

    #[test]
    fn test_read_geometry_descriptor() {
        let geometry_test_file = "geometry_test";
        let sys_dev_path = TempDir::new("test_sys_dev").unwrap();
        let geometry_dir = create_geometry_descriptor_dir(&sys_dev_path.path());
        create_geometry_descriptor_files(&geometry_dir, &geometry_test_file, "0x1234");

        let val =
            ufs_provision::read_geometry_descriptor(&sys_dev_path.path(), &geometry_test_file)
                .unwrap();
        assert_eq!(val, 0x1234);
    }

    #[test]
    fn test_get_alloc_units() {
        let sys_dev_path = TempDir::new("test_sys_dev").unwrap();
        let geometry_dir = create_geometry_descriptor_dir(&sys_dev_path.path());
        create_geometry_descriptor_files(&geometry_dir, "raw_device_capacity", "0xee64000");
        create_geometry_descriptor_files(&geometry_dir, "segment_size", "0x2000");
        create_geometry_descriptor_files(&geometry_dir, "allocation_unit_size", "0x1");

        let val = ufs_provision::get_alloc_units(&sys_dev_path.path()).unwrap();
        assert_eq!(val, 30514);
    }
}
