// Copyright 2022 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::fmt::Debug;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use glob;
use thiserror::Error;

use crate::device;
use crate::system::context;
use crate::utils::file_utils::{self, PathError};

const UFS_NODE_PATTERN: &str = "/dev/bsg/ufs-bsg*";
const SYSFS_DEV_PCI: &str = "/sys/devices/pci*/*/host*/ufs-bsg*";
const SYSFS_DEV_PLAT: &str = "/sys/devices/platform/soc/*/host*/ufs-bsg*";

pub struct UFSPath {
    pub bsg_node: PathBuf,
    pub sys_dev: PathBuf,
}

#[derive(Error, Debug)]
pub enum PathMatchError {
    #[error("Path not found.")]
    PathNotFound,
    #[error("Multiple paths found.")]
    MultiplePathFound,
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
pub fn match_path(path_regex: &str) -> Result<PathBuf> {
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

/// Returns the path to pattern [UFS_NODE_PATTERN].
pub fn get_ufs_bsg_node() -> Result<PathBuf> {
    match_path(UFS_NODE_PATTERN)
}

/// Returns the path to pattern [SYSFS_DEV_{PCI, PLAT}].
pub fn get_ufs_bsg_sys() -> Result<PathBuf> {
    let sysfs_path = match_path(SYSFS_DEV_PCI);
    let sysfs_path = match sysfs_path {
        Ok(path) => path,
        Err(err) => {
            if let Some(err) = err.downcast_ref::<PathMatchError>() {
                if matches!(err, PathMatchError::PathNotFound) {
                    return match_path(SYSFS_DEV_PLAT);
                }
            }
            anyhow::bail!(PathMatchError::MultiplePathFound);
        }
    };

    Ok(sysfs_path)
}

/// Returns the path to power control node.
pub fn get_power_control_node() -> Result<PathBuf> {
    let fixed_device_storage = device::get_fixed_device_storage(&mut *context::global_context())?;
    let rootdev = file_utils::get_file_name(Path::new(&fixed_device_storage))
        .ok_or(PathError::GetFilenameError)?;
    let path = fs::canonicalize(format!("/sys/block/{}/device/power/control", rootdev))?;

    return Ok(path);
}

pub fn is_pci_path(sys_dev_path: &Path) -> bool {
    sys_dev_path
        .display()
        .to_string()
        .starts_with("/sys/devices/pci")
}

pub fn is_platform_path(sys_dev_path: &Path) -> bool {
    sys_dev_path
        .display()
        .to_string()
        .starts_with("/sys/devices/platform/soc")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::factory_ufs::utils::path_utils;

    #[test]
    fn test_is_pci_path() {
        let pci_path = Path::new("/sys/devices/pci0000:00/0000:00:12.7");
        assert!(path_utils::is_pci_path(pci_path));
        let non_pci_path = Path::new("/sys/devices/platform/soc");
        assert!(!path_utils::is_pci_path(non_pci_path));
    }

    #[test]
    fn test_is_platform_path() {
        let platform_path = Path::new("/sys/devices/platform/soc/16810000.ufshci/");
        assert!(path_utils::is_platform_path(platform_path));
        let non_platform_path = Path::new("/sys/devices/pci0000:00/");
        assert!(!path_utils::is_platform_path(non_platform_path));
    }
}
