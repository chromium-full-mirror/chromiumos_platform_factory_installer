// Copyright 2022 The ChromiumOS Authors.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::path::{Path, PathBuf};

use anyhow::Result;
use glob;

/// List files for the given glob string.
/// # Arguments
/// * `root` - The root directory to be listed.
/// * `glob_string` - Glob string to be matched.
/// * `exclude_dirs` - Whether to exclude directories.
///
/// # Return
/// A `Vec` of `PathBuf` which contains the file paths matching the glob string.
pub fn list_files<R, G>(root: R, glob_string: G, exclude_dirs: bool) -> Result<Vec<PathBuf>>
where
    R: AsRef<Path>,
    G: AsRef<Path>,
{
    let root = root.as_ref();
    let glob_string = root.join(glob_string.as_ref()).display().to_string();

    if glob_string.find("../").is_some() {
        anyhow::bail!("Do not use `../` in glob string.");
    }

    Ok(glob::glob(&glob_string)?
        .filter_map(Result::ok)
        // Exclude dirs.
        .filter(|path| !(exclude_dirs && path.is_dir()))
        // Strip the root prefix.
        .map(|path| path.strip_prefix(root).unwrap().to_path_buf())
        .collect())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::utils::file_utils;

    const TEST_FS: &str = "tests/test_fs";

    #[test]
    fn test_list_files() {
        let files = file_utils::list_files(TEST_FS, "**/*", true).unwrap();
        assert_eq!(files, vec![PathBuf::from("a_file")]);
    }
}
