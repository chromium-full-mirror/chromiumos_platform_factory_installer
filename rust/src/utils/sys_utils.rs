// Copyright 2022 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::ffi::{CString, OsStr};
use std::io::Error;
use std::os::unix::ffi::OsStrExt;
use std::{ptr, thread, time};

use anyhow::{self, Result};
use nix::sys::reboot::{self, RebootMode};

// TODO(wyuang): add more possible fstype.
const FSTYPES: &[&str] = &["ext4", "exfat", "vfat"];

/// Mounts a node to certain path.
/// # Arguments
/// * `src` - source device to be mounted.
/// * `dst` - mount point.
/// * `ro` - whether mount the device as readonly.
pub fn mount<S: AsRef<OsStr>, D: AsRef<OsStr>>(src: S, dst: D, ro: bool) -> Result<()> {
    // TODO(wyuang): support more options and flags in the future.
    let src = CString::new(src.as_ref().as_bytes())?;
    let dst = CString::new(dst.as_ref().as_bytes())?;
    let flags = if ro { libc::MS_RDONLY } else { 0 };

    // Auto detect possible fstypes like `mount -t auto`.
    for fstype in FSTYPES {
        let fstype = CString::new(*fstype)?;
        let retval = unsafe {
            libc::mount(
                src.as_ptr(),
                dst.as_ptr(),
                fstype.as_ptr(),
                flags,
                ptr::null(),
            )
        };
        if retval == 0 {
            return Ok(());
        }
    }
    anyhow::bail!("Failed to mount: {}", Error::last_os_error());
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

pub fn shutdown() -> Result<()> {
    reboot::reboot(RebootMode::RB_POWER_OFF)?;
    Ok(())
}

pub fn reboot() -> Result<()> {
    reboot::reboot(RebootMode::RB_AUTOBOOT)?;
    Ok(())
}

pub fn sleep(sec: f32) -> Result<()> {
    let millisec = (sec * 1000.0) as u64;
    Ok(thread::sleep(time::Duration::from_millis(millisec)))
}

/// Mocked mount/umount for unit test.
///
/// the syscalls are unmockable so just do nothing in the unit tests.
#[cfg(test)]
pub mod mock {
    use std::ffi::OsStr;

    use anyhow::Result;

    pub fn mount<S: AsRef<OsStr>, D: AsRef<OsStr>>(_src: S, _dst: D, _ro: bool) -> Result<()> {
        Ok(())
    }
    pub fn umount<D: AsRef<OsStr>>(_dst: D) -> Result<()> {
        Ok(())
    }
    pub fn shutdown() -> Result<()> {
        Ok(())
    }
    pub fn reboot() -> Result<()> {
        Ok(())
    }
    pub fn sleep(_sec: f32) -> Result<()> {
        Ok(())
    }
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
