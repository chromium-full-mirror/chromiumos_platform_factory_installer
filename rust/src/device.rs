// Copyright 2022 The ChromiumOS Authors.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::fs;

use anyhow::{Context, Result};

use crate::utils::process_utils::{Command, StringOutput};
use crate::utils::string_utils;

use regex::Regex;

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

/// Gets the stateful partition of dev image.
pub fn get_dev_stateful_partition() -> Result<String> {
    // The format of `lsb-factory` is "shell-executable" which may contain "#" comments and empty
    // lines.
    let lsb_factory = string_utils::parse_dict_ignore_error(
        fs::read_to_string("/mnt/stateful_partition/dev_image/etc/lsb-factory")?
            .trim()
            .split("\n")
            .map(|line| line.split("#").nth(0).unwrap())
            .collect::<Vec<&str>>(),
        "=",
    );

    // `REAL_USB_DEV` presents the rootfs partition (usually the 3rd partition).
    // Replace with the 1st partition which is the stateful partition.
    // E.g. /dev/sda3 -> /dev/sda1
    let dev_stateful_partition = String::from(
        Regex::new("[0-9]")?.replace(
            lsb_factory
                .get("REAL_USB_DEV")
                .context("Unable to get the path of dev image.")?,
            "1",
        ),
    )
    // Shell-executable allows value quoted by "".
    .trim_matches('"')
    .to_string();
    Ok(dev_stateful_partition)
}

/// Gets the model name of device.
pub fn get_model_name() -> Result<String> {
    Ok(Command::new("cros_config")
        .args(["/", "name"])
        .output()?
        .stdout())
}

/// Get the serial number of the device.
pub fn get_serial_number() -> Result<String> {
    Ok(fs::read_to_string("/sys/firmware/vpd/ro/serial_number")?)
}
