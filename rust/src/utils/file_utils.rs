// Copyright 2022 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::error::Error;
use std::fmt::{Debug, Display, Formatter, Result as FMTResult};
use std::fs::{self, File};
use std::path::{Path, PathBuf};

use anyhow::Result;
use glob;

use crate::utils::string_utils;

#[derive(Debug)]
pub enum PathError {
    NoParentDirError,
    GetFilenameError,
}

impl Error for PathError {}

impl Display for PathError {
    fn fmt(&self, f: &mut Formatter) -> FMTResult {
        match self {
            PathError::NoParentDirError => write!(f, "No parent directory can be found."),
            PathError::GetFilenameError => write!(f, "Cannot get the file name."),
        }
    }
}

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

/// Create a file and its parent directories if not exists.
///
/// # Arguments
/// * `path` - The absolute path where the file should be created.
/// * `content` - The content of the file.
pub fn create_file<P, C>(path: P, content: C) -> Result<()>
where
    P: AsRef<Path>,
    C: AsRef<str>,
{
    let path = path.as_ref();
    let content = content.as_ref();
    let parent_path = path.parent().ok_or(PathError::NoParentDirError)?;
    fs::create_dir_all(parent_path)?;
    Ok(fs::write(path, content)?)
}

/// Reads a file which contains a hex string from a given path.
///
/// # Arguments
/// * `path` - The path to the file.
///
/// # Return
/// An u32 integer representation of the hex string in the file.
pub fn read_hex_file<P>(path: P) -> Result<u32>
where
    P: AsRef<Path>,
{
    let path = path.as_ref();
    let raw_string = fs::read_to_string(path)?;
    Ok(string_utils::hex_to_u32(raw_string)?)
}

/// Opens a file with given permission.
///
/// # Arguments
/// * `path` - The path to the file.
/// * `read` - Opens with read permission or not.
/// * `write` - Opens with write permission or not.
///
/// # Return
/// The opened file.
pub fn open_file<P>(path: P, read: bool, write: bool) -> Result<File>
where
    P: AsRef<Path>,
{
    let path = path.as_ref();
    Ok(File::options().read(read).write(write).open(path)?)
}

/// Gets the file name from a given path.
///
/// # Arguments
/// * `path` - The path to the file.
///
/// # Return
/// The file name. Will be None if path ends with `..`.
pub fn get_file_name<P>(path: P) -> Option<String>
where
    P: AsRef<Path>,
{
    path.as_ref()
        .file_name()
        .map(|osstr| osstr.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use std::fs::{self, File};
    use std::path::PathBuf;

    use crate::system::context::{self, Context};
    use crate::utils::file_utils;

    #[test]
    fn test_list_files() {
        let context = context::global_context();
        let mut file_path = context.root_dir().to_path_buf();
        file_path.push("a_file");
        File::create(file_path.as_path()).unwrap();
        let files = file_utils::list_files(context.root_dir(), "**/*", true).unwrap();
        assert_eq!(files, vec![PathBuf::from("a_file")]);
    }

    #[test]
    fn test_create_file() {
        let context = context::global_context();
        let test_file = context
            .tempdir(None)
            .unwrap()
            .join("dir1")
            .join("dir2")
            .join("test_file");
        let write_content = "1234";
        file_utils::create_file(&test_file, write_content).unwrap();
        let read_string = fs::read_to_string(&test_file).unwrap();
        assert_eq!(read_string, write_content);
    }
}
