// Copyright 2022 The ChromiumOS Authors.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::env;
use std::error::Error;
use std::fmt::{Debug, Display, Formatter, Result as fmt_Result};
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use anyhow::{self, Result};
use bincode;
use byteorder::ReadBytesExt;
use glob;
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

use crate::factory_ufs::ufs_spec::ufs_common::{self, CommonDescriptorTrait};
use crate::factory_ufs::ufs_spec::ufs_config_descriptor::ProvisionLun0;
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
struct NoParentDir;

impl Error for NoParentDir {}

impl Display for NoParentDir {
    fn fmt(&self, f: &mut Formatter) -> fmt_Result {
        write!(f, "No parent directory can be found.")
    }
}

/// Gets one match path given a path regex.
///
/// # Arguments
///
/// * `path_regex` - A string which defines the pattern to match.
///
/// # Return value
///
/// Return a result type. Raise error if path is not found or multiple paths
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
fn rescan_dev_and_sync(ufs_dev_path: &Path) -> Result<()> {
    let pattern = Path::new(ufs_dev_path)
        .join("host*")
        .join("scsi_host")
        .join("host*")
        .join("scan")
        .display()
        .to_string();
    let scsi_path = match_path(&pattern)?;

    println!("Perform scsi rescan: {}", scsi_path.display());
    fs::write(&scsi_path, "- - -")?;

    println!("Sync write...");
    Command::new("sync").output()?;

    Ok(())
}

/// Writes new UFS config using ufs-utils.
fn write_ufs_config<T: Serialize>(ufs_bsg_node_path: &Path, ufs_config: &T) -> Result<()> {
    let config_path = NamedTempFile::new()?.path().display().to_string();
    let config_bin_vec = serialize_config::<T>(&ufs_config)?;
    fs::write(&config_path, config_bin_vec)?;

    println!("Write config to {}...", ufs_bsg_node_path.display());
    let output = Command::new("ufs-utils")
        .args([
            "desc",
            "-t",
            "1",
            "-p",
            &ufs_bsg_node_path.display().to_string(),
            "-w",
            &config_path,
        ])
        .current_dir(env::temp_dir())
        .output()?;
    println!("{}", output.stdout());
    Ok(())
}

/// Serialize an UFS config to a u8 vector.
fn serialize_config<T: Serialize>(ufs_config: &T) -> Result<Vec<u8>> {
    Ok(bincode::config().big_endian().serialize::<T>(&ufs_config)?)
}

/// Deserialize a binary config to a given type.
fn deserialize_config<'de, T: Deserialize<'de>>(config_binary: &'de Vec<u8>) -> Result<T> {
    Ok(bincode::config().big_endian().deserialize(&config_binary)?)
}

/// Generates a new UFS config given the allocation units.
fn generate_new_config<T: Default + ProvisionLun0>(alloc_units: u32) -> Result<T> {
    let mut ufs_config = T::default();
    ufs_config.provision_lun_0(alloc_units);

    Ok(ufs_config)
}

/// Reads the geometry_descriptor from a given path.
fn read_geometry_descriptor(ufs_dev_path: &Path, f_name: &str) -> Result<u32> {
    // The content of the geometry_descriptor is a hex string.
    // We need to transform it into u32.
    let path = Path::new(ufs_dev_path)
        .join("geometry_descriptor")
        .join(f_name);
    let raw_string = fs::read_to_string(path)?
        .trim()
        .trim_start_matches("0x")
        .to_string();

    Ok(u32::from_str_radix(&raw_string, 16)?)
}

/// Gets the total number of allocation units from the geometry_descriptor.
fn get_alloc_units(ufs_dev_path: &Path) -> Result<u32> {
    let total_capacity = read_geometry_descriptor(ufs_dev_path, "raw_device_capacity")?;
    let segment_size = read_geometry_descriptor(ufs_dev_path, "segment_size")?;
    let segments_per_allocation_unit =
        read_geometry_descriptor(ufs_dev_path, "allocation_unit_size")?;

    println!("UFS capacity: {} GB", total_capacity / 0x200000);
    let units = total_capacity / segment_size / segments_per_allocation_unit;

    Ok(units)
}

/// Gets the current UFS config using ufs-utils.
pub fn get_current_config_binary(ufs_bsg_node_path: &Path) -> Result<Vec<u8>> {
    // Needs to switch to /tmp since ufs-utils dumps the config to cwd
    // with name `ufs_common::UFS_CONFIG_NAME`.
    Command::new("ufs-utils")
        .args([
            "desc",
            "-t",
            "1",
            "-p",
            &ufs_bsg_node_path.display().to_string(),
        ])
        .current_dir(env::temp_dir())
        .output()
        .ok();
    let cur_config_path = env::temp_dir().join(ufs_common::UFS_CONFIG_NAME);

    Ok(fs::read(cur_config_path)?)
}

/// Read the first byte from a config binary.
pub fn read_header(ufs_config_binary: &Vec<u8>) -> Result<u8> {
    Ok(Cursor::new(ufs_config_binary).read_u8()?)
}

/// Do UFS provisioning if needed.
///
/// This function compares the current UFS config and the newly generated
/// config, and skips provisioning if these two configs are the same.
pub fn do_provisioning_if_needed<'de, T: CommonDescriptorTrait<'de> + ProvisionLun0>(
    ufs_bsg_node_path: &Path,
    ufs_bsg_sys_path: &Path,
    cur_config_binary: &'de Vec<u8>,
) -> Result<()> {
    // `ufs_bsg_sys_path` looks like: /sys/devices/pcixxx/xxx/hostx/ufs-bsgx,
    // but we actually want: /sys/devices/pcixxx/xxx.
    let ufs_dev_path = Path::new(ufs_bsg_sys_path)
        .parent()
        .ok_or(NoParentDir)?
        .parent()
        .ok_or(NoParentDir)?;
    let alloc_units = get_alloc_units(&ufs_dev_path)?;

    let cur_config = deserialize_config::<'de, T>(cur_config_binary)?;
    let new_config = generate_new_config::<T>(alloc_units)?;

    println!("Current UFS config on DUT: {:#?}", cur_config);
    println!("New UFS config: {:#?}", new_config);

    if cur_config == new_config {
        println!("Current UFS config is the same as the new config. Skip UFS provisioning.");
        return Ok(());
    }

    write_ufs_config(&ufs_bsg_node_path, &new_config)?;
    rescan_dev_and_sync(&ufs_dev_path)?;
    println!("Finish UFS provisioning.");

    Ok(())
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

        assert_eq!(ufs2_config_descriptor.device_config.b_length, 0x90);
        assert_eq!(ufs2_config_descriptor.units_config[0].b_lu_enable, 0x1);
        // 119 = 0x77, 50 = 0x32
        assert_eq!(
            ufs2_config_descriptor.units_config[0].d_num_alloc_units,
            0x7732
        );
        assert_eq!(
            ufs2_config_descriptor.units_config[0].b_provisioning_type,
            0x3
        );
    }

    #[test]
    fn test_serialize_config() {
        let mut ufs2_config = UFS2ConfigDescriptor::default();
        ufs2_config.units_config[0].b_lu_enable = 0x1;
        ufs2_config.units_config[0].d_num_alloc_units = 0x7732;
        ufs2_config.units_config[0].b_provisioning_type = 0x3;

        let ufs2_config_binary =
            ufs_provision::serialize_config::<UFS2ConfigDescriptor>(&ufs2_config).unwrap();

        assert_eq!(ufs2_config_binary, generate_ufs2_config_binary())
    }

    #[test]
    fn test_generate_new_config() {
        let alloc_units: u32 = 10;
        let ufs2_config =
            ufs_provision::generate_new_config::<UFS2ConfigDescriptor>(alloc_units).unwrap();
        let ufs3_config =
            ufs_provision::generate_new_config::<UFS3ConfigDescriptor>(alloc_units).unwrap();

        assert_eq!(ufs2_config.device_config.b_length, 0x90);
        assert_eq!(ufs2_config.units_config[0].d_num_alloc_units, alloc_units);

        assert_eq!(ufs3_config.device_config.b_length, 0xe6);
        assert_eq!(ufs3_config.units_config[0].d_num_alloc_units, alloc_units);
    }

    fn create_geometry_descriptor_dir(ufs_dev_path: &Path) -> PathBuf {
        let geometry_dir = ufs_dev_path.join("geometry_descriptor");
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
        let ufs_dev_path = TempDir::new("test_ufs_dev").unwrap();
        let geometry_dir = create_geometry_descriptor_dir(&ufs_dev_path.path());
        create_geometry_descriptor_files(&geometry_dir, &geometry_test_file, "0x1234");

        let val =
            ufs_provision::read_geometry_descriptor(&ufs_dev_path.path(), &geometry_test_file)
                .unwrap();
        assert_eq!(val, 0x1234);
    }

    #[test]
    fn test_get_alloc_units() {
        let ufs_dev_path = TempDir::new("test_ufs_dev").unwrap();
        let geometry_dir = create_geometry_descriptor_dir(&ufs_dev_path.path());
        create_geometry_descriptor_files(&geometry_dir, "raw_device_capacity", "0xee64000");
        create_geometry_descriptor_files(&geometry_dir, "segment_size", "0x2000");
        create_geometry_descriptor_files(&geometry_dir, "allocation_unit_size", "0x1");

        let val = ufs_provision::get_alloc_units(&ufs_dev_path.path()).unwrap();
        assert_eq!(val, 30514);
    }
}
