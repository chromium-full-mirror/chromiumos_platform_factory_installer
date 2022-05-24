// Copyright 2022 The ChromiumOS Authors.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::ffi::{CString, OsStr};
use std::io::Error;
use std::os::unix::ffi::OsStrExt;
use std::ptr;

use anyhow::{self, Result};

/// Mounts a node to certain path.
/// # Arguments
/// * `src` - source device to be mounted.
/// * `dst` - mount point.
/// * `ro` - whether mount the device as readonly.
pub fn mount<S: AsRef<OsStr>, D: AsRef<OsStr>>(src: S, dst: D, ro: bool) -> Result<()> {
    // TODO(wyuang): Currently RO and ext4 is enough, but may want to support more options and
    // flags in the future.
    let src = CString::new(src.as_ref().as_bytes())?;
    let dst = CString::new(dst.as_ref().as_bytes())?;
    let fstype = CString::new("ext4")?;
    let flags = if ro { libc::MS_RDONLY } else { 0 };
    let retval = unsafe {
        libc::mount(
            src.as_ptr(),
            dst.as_ptr(),
            fstype.as_ptr(),
            flags,
            ptr::null(),
        )
    };
    if retval != 0 {
        anyhow::bail!("Failed to mount: {}", Error::last_os_error());
    }
    Ok(())
}

/// Unmounts a node.
/// # Arguments
/// * `dst` - mount point.
pub fn umount<D: AsRef<OsStr>>(dst: D) -> Result<()> {
    let dst = CString::new(dst.as_ref().as_bytes())?;
    let retval = unsafe { libc::umount(dst.as_ptr()) };
    if retval < 0 {
        anyhow::bail!("Failed to umount: {}", Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::utils::sys_utils;

    #[test]
    fn test_mount_failed() {
        let output = sys_utils::mount("src_not_exist", "dst_not_exist", true);
        assert!(output.is_err());
    }

    #[test]
    fn test_umount_failed() {
        let output = sys_utils::umount("dst_not_exist");
        assert!(output.is_err());
    }
}
