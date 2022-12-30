// Copyright 2022 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::error::Error;
use std::fmt::{Debug, Display, Formatter, Result as FMTResult};
use std::path::PathBuf;

use anyhow::Result;
use glob;

const UFS_NODE_PATTERN: &str = "/dev/bsg/ufs-bsg*";
const SYS_DEV_PATTERN: &str = "/sys/devices/*/*/host*/ufs-bsg*";

pub struct UFSPath {
    pub bsg_node: PathBuf,
    pub sys_dev: PathBuf,
}

#[derive(Debug)]
pub enum PathMatchError {
    PathNotFound,
    MultiplePathFound,
}

impl Error for PathMatchError {}

impl Display for PathMatchError {
    fn fmt(&self, f: &mut Formatter) -> FMTResult {
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
