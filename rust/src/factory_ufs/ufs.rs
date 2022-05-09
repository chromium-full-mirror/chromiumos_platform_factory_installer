// Copyright 2022 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::factory_ufs::config;
use crate::utils::process_utils::Command;

use anyhow::{bail, Result};
use bincode::config as bincode_config;
use glob::glob;
use std::env::temp_dir;
use std::error::Error;
use std::fmt::{Display, Formatter, Result as fmt_Result};
use std::fs::{read, read_to_string, write};
use tempfile::NamedTempFile;

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
fn match_path(path_regex: &str) -> Result<String> {
    let mut paths = Vec::<String>::new();
    for path in glob(path_regex)? {
        paths.push(path?.display().to_string());
    }
    if paths.is_empty() {
        bail!(PathMatchError::PathNotFound);
    } else if paths.len() != 1 {
        bail!(PathMatchError::MultiplePathFound);
    }

    Ok(paths[0].to_string())
}

/// Returns the path to pattern [config::UFS_NODE_PATTERN].
pub fn get_ufs_bsg_node() -> Result<String> {
    match_path(config::UFS_NODE_PATTERN)
}

/// Returns the path to pattern [config::SYS_DEV_PATTERN].
pub fn get_ufs_bsg_sys() -> Result<String> {
    match_path(config::SYS_DEV_PATTERN)
}

/// Rescans to bring the device up.
fn rescan_dev_and_sync(ufs_dev_path: &str) -> Result<()> {
    let pattern = format!("{}/host*/scsi_host/host*/scan", ufs_dev_path);
    let scsi_path = match_path(&pattern)?;
    Command::new("echo")
        .args(["- - -", ">", &scsi_path])
        .output()?;

    Command::new("sync").output()?;
    Ok(())
}

/// Writes new UFS config using ufs-utils.
fn write_ufs_config(ufs_bsg_node_path: &str, ufs_config: &config::ConfigDescriptor) -> Result<()> {
    let config_path = NamedTempFile::new()?.path().display().to_string();
    let config_bin_vec = bincode_config().big_endian().serialize(&ufs_config)?;
    write(&config_path, config_bin_vec)?;

    Command::new("ufs-utils")
        .args([
            "desc",
            "-t",
            "1",
            "-p",
            &ufs_bsg_node_path,
            "-w",
            &config_path,
        ])
        .current_dir(temp_dir())
        .output()?;
    Ok(())
}

/// Gets the current UFS config using ufs-utils.
fn get_current_config(ufs_bsg_node_path: &str) -> Result<config::ConfigDescriptor> {
    // Needs to switch to /tmp since ufs-utils dumps the config to cwd
    // with name `config::UFS_CONFIG_NAME`.
    Command::new("ufs-utils")
        .args(["desc", "-t", "1", "-p", ufs_bsg_node_path])
        .current_dir(temp_dir())
        .output()
        .ok();

    let cur_config_path = format!(
        "{}/{}",
        temp_dir().display().to_string(),
        config::UFS_CONFIG_NAME
    );

    let config_binary = read(cur_config_path)?;

    Ok(bincode_config().big_endian().deserialize(&config_binary)?)
}

/// Generates a new UFS config given the allocation units.
fn generate_new_config(alloc_units: u32) -> Result<config::ConfigDescriptor> {
    let mut unit0 = config::UnitConfigDescriptor::default();
    unit0.b_lu_enable = 0x01;
    unit0.d_num_alloc_units = alloc_units;
    unit0.b_provisioning_type = config::PROVISIONING_TYPE;

    let mut ufs_config = config::ConfigDescriptor::default();
    ufs_config.units_config[0] = unit0;

    Ok(ufs_config)
}

/// Reads the geometry_descriptor from a given path.
fn read_geometry_descriptor(ufs_dev_path: &str, f_name: &str) -> Result<u32> {
    // The content of the geometry_descriptor is a hex string.
    // We need to transform it into u32.
    let path_name = format!("{}/geometry_descriptor/{}", ufs_dev_path, f_name);
    let raw_string = read_to_string(path_name)?
        .trim()
        .trim_start_matches("0x")
        .to_string();

    Ok(u32::from_str_radix(&raw_string, 16)?)
}

/// Gets the total number of allocation units from the geometry_descriptor.
fn get_alloc_units(ufs_dev_path: &str) -> Result<u32> {
    let total_capacity = read_geometry_descriptor(ufs_dev_path, "raw_device_capacity")?;
    let segment_size = read_geometry_descriptor(ufs_dev_path, "segment_size")?;
    let segments_per_allocation_unit =
        read_geometry_descriptor(ufs_dev_path, "allocation_unit_size")?;

    println!("UFS capacity: {} GB", total_capacity / 0x200000);
    let units = total_capacity / segment_size / segments_per_allocation_unit;

    Ok(units)
}

/// Do UFS provisioning if needed.
///
/// This function compares the current UFS config and the newly generated
/// config, and skips provisioning if these two configs are the same.
pub fn do_provisioning_if_needed(ufs_bsg_node_path: &str, ufs_bsg_sys_path: &str) -> Result<()> {
    // `ufs_bsg_sys_path` looks like: /sys/devices/pcixxx/xxx/hostx/ufs-bsgx,
    // but we actually want: /sys/devices/pcixxx/xxx
    let mut string_vec: Vec<&str> = ufs_bsg_sys_path.split("/").collect();
    string_vec.truncate(string_vec.len() - 2);
    let ufs_dev_path = string_vec.join("/");
    let alloc_units = get_alloc_units(&ufs_dev_path)?;
    let cur_config = get_current_config(&ufs_bsg_node_path)?;
    let new_config = generate_new_config(alloc_units)?;

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
