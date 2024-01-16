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
const SYS_DEV_PATTERN: &str = "/sys/devices/*/*/host*/ufs-bsg*";

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

/// Returns the path to pattern [SYS_DEV_PATTERN].
pub fn get_ufs_bsg_sys() -> Result<PathBuf> {
    match_path(SYS_DEV_PATTERN)
}

/// Returns the path to power control node.
pub fn get_power_control_node() -> Result<PathBuf> {
    let fixed_device_storage = device::get_fixed_device_storage(&mut *context::global_context())?;
    let rootdev = file_utils::get_file_name(Path::new(&fixed_device_storage))
        .ok_or(PathError::GetFilenameError)?;
    let path = fs::canonicalize(format!("/sys/block/{}/device/power/control", rootdev))?;

    return Ok(path);
}
