// Copyright 2022 The ChromiumOS Authors.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use anyhow::Result;

use crate::utils::process_utils::{Command, StringOutput};

const GET_FIXED_DST_DRIVE_CMD: &str =
    ". /usr/sbin/write_gpt.sh; load_base_vars; get_fixed_dst_drive;";

/// Gets the path of fixed device storage.
///
/// # Return
/// The return string will be a path of device like `/dev/sda` or `/dev/mmcnlk0`.
pub fn get_fixed_device_storage() -> Result<String> {
    Ok(Command::new("sh")
        .args(["-c", GET_FIXED_DST_DRIVE_CMD])
        .output()?
        .stdout()
        .trim()
        .to_string())
}

/// Gets the rootfs partition of release image.
/// There're two cases of storage device path:
///   /dev/sda -> release rootfs at /dev/sda5
///   /dev/mmcblk0 -> release rootfs at /dev/mmcblk0p5
pub fn get_release_rootfs_partition() -> Result<String> {
    let mut fixed_device_storage = get_fixed_device_storage()?;
    // If the last character is digit, it is the case of `/dev/mmcblk0`, Otherwise is the case of
    // `/dev/sda`.
    if fixed_device_storage
        .chars()
        .last()
        .unwrap_or(' ')
        .is_digit(10)
    {
        fixed_device_storage.push_str("p5");
    } else {
        fixed_device_storage.push('5');
    }
    Ok(fixed_device_storage)
}

/// Gets the model name of device.
pub fn get_model_name() -> Result<String> {
    Ok(Command::new("cros_config")
        .args(["/", "name"])
        .output()?
        .stdout())
}
