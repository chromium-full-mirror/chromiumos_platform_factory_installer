// Copyright 2022 The ChromiumOS Authors.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::os::unix::io::IntoRawFd;
use std::path::Path;

use anyhow::Result;

use crate::utils::file_utils;

mod ioctl {
    use std::mem::size_of;

    use nix;
    // The nix::ioctl_xxx macro generates a wrapper function for the ioctl.
    // This macro generates function:
    //   pub unsafe fn blkdiscard(fd: &libc::c_int, data: *mut [u64; 2])
    //     -> Result<libc::c_int>
    nix::ioctl_write_ptr_bad!(
        blkdiscard,
        // Below statement is equivalent to:
        //   #define BLKDISCARD _IO(0x12, 119);
        // Ref: include/uapi/linux/fs.h
        nix::request_code_none!(0x12, 119),
        [u64; 2]
    );

    // This macro generates function:
    //   pub unsafe fn blkgetsize64(fd: &libc::c_int, blksize: *mut u64)
    //     -> Result<libc::c_int>
    nix::ioctl_read_bad!(
        blkgetsize64,
        // Below statement is equivalent to:
        //   #define BLKGETSIZE64 _IOR(0x12, 114, size_t);
        // Ref: include/uapi/linux/fs.h
        nix::request_code_read!(0x12, 114, size_of::<usize>()),
        u64
    );
}

/// Discards a block device using `BLKDISCARD`.
///
/// # Arguments
/// * `block_dev` - Path to the block device.
/// * `offset` - The starting position to be discarded.
/// * `size` - The length to be discarded.
pub fn discard_block_dev<P>(block_dev: P, offset: u64, size: u64) -> Result<()>
where
    P: AsRef<Path>,
{
    let block_dev = block_dev.as_ref();
    let fd = file_utils::open_file(block_dev, true, true)?.into_raw_fd();
    unsafe {
        ioctl::blkdiscard(fd, &[offset, size])?;
    }

    Ok(())
}

/// Reads the size of a block device using `BLKGETSIZE64`.
///
/// # Arguments
/// * `block_dev` - Path to the block device.
///
/// # Return
/// The size of the given block device.
pub fn get_block_dev_size<P>(block_dev: P) -> Result<u64>
where
    P: AsRef<Path>,
{
    let block_dev = block_dev.as_ref();
    let fd = file_utils::open_file(block_dev, true, true)?.into_raw_fd();
    let mut size = 0;
    unsafe {
        ioctl::blkgetsize64(fd, &mut size)?;
    }
    Ok(size)
}
