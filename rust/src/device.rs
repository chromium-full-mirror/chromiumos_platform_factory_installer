// Copyright 2022 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::collections::HashMap;
use std::fs;

use anyhow::{Context as _, Result};
use regex::Regex;

use crate::system::context::Context;
use crate::utils::process_utils::StringOutput;
use crate::utils::string_utils;

const GET_FIXED_DST_DRIVE_CMD: &str =
    ". /usr/sbin/write_gpt.sh; load_base_vars; get_fixed_dst_drive;";

/// Gets the path of fixed device storage.
///
/// # Return
/// The return string will be a path of device like `/dev/sda` or `/dev/mmcnlk0`.
pub fn get_fixed_device_storage(context: &mut dyn Context) -> Result<String> {
    Ok(context
        .command("sh")
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
pub fn get_release_rootfs_partition(context: &mut dyn Context) -> Result<String> {
    let mut fixed_device_storage = get_fixed_device_storage(context)?;
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

/// Gets the stateful partition of release image.
/// There're two cases of storage device path:
///   /dev/sda -> release stateful at /dev/sda1
///   /dev/mmcblk0 -> release stateful at /dev/mmcblk0p1
///
/// If the device enables LVM stateful partition, get the partition path from `pvdisplay`.
pub fn get_release_stateful_partition(context: &mut dyn Context) -> Result<String> {
    let mut fixed_device_storage = get_fixed_device_storage(context)?;
    // If the last character is digit, it is the case of `/dev/mmcblk0`, Otherwise is the case of
    // `/dev/sda`.
    if fixed_device_storage
        .chars()
        .last()
        .unwrap_or(' ')
        .is_digit(10)
    {
        fixed_device_storage.push_str("p1");
    } else {
        fixed_device_storage.push('1');
    }

    // Get the device path to LVM stateful partition.
    let mut vg_name = context
        .command("pvdisplay")
        .args([
            "-C",
            "--quiet",
            "--noheadings",
            "--separator",
            "'|'",
            "-o",
            "vg_name",
            &fixed_device_storage,
        ])
        .output()?
        .stdout();
    vg_name.retain(|c| !c.is_whitespace());

    if vg_name.len() > 0 {
        context
            .command("vgchange")
            .args(["-ay", &vg_name])
            .output()?
            .exit_ok()?;
        return Ok(format!("/dev/{}/unencrypted", vg_name));
    }

    Ok(fixed_device_storage)
}

/// Gets the config in lsb-factory in stateful partition.
pub fn get_lsb_factory(context: &dyn Context) -> Result<HashMap<String, String>> {
    // The format of `lsb-factory` is "shell-executable" which may contain "#" comments and empty
    // lines.
    Ok(string_utils::parse_dict_ignore_error(
        fs::read_to_string(
            context
                .root_dir()
                .join("mnt/stateful_partition/dev_image/etc/lsb-factory"),
        )?
        .trim()
        .split("\n")
        .map(|line| {
            line.split("#")
                .nth(0)
                .unwrap()
                // Shell-executable allows value quoted by "".
                .trim_matches('"')
        })
        .collect::<Vec<&str>>(),
        "=",
    ))
}

/// Gets the stateful partition of dev image.
pub fn get_dev_stateful_partition(context: &dyn Context) -> Result<String> {
    let lsb_factory = get_lsb_factory(context)?;

    // `REAL_USB_DEV` presents the rootfs partition (usually the 3rd partition).
    // Replace with the 1st partition which is the stateful partition.
    // E.g. /dev/sda3 -> /dev/sda1
    Ok(String::from(
        Regex::new("[0-9]")?.replace(
            lsb_factory
                .get("REAL_USB_DEV")
                .context("Unable to get the path of dev image.")?,
            "1",
        ),
    ))
}

/// Gets the model name of device.
pub fn get_model_name(context: &mut dyn Context) -> Result<String> {
    Ok(context
        .command("cros_config")
        .args(["/", "name"])
        .output()?
        .stdout())
}

/// Gets the firmware manifest key of device.
pub fn get_firmware_manifest_key(context: &mut dyn Context) -> Result<String> {
    let crosid = string_utils::parse_dict_ignore_error(
        context
            .command("crosid")
            .output()?
            .stdout()
            .trim()
            .split("\n")
            .collect::<Vec<&str>>(),
        "=",
    );
    Ok(crosid
        .get("FIRMWARE_MANIFEST_KEY")
        .context("Unable to get firmware manifest key from crosid.")?
        .trim_matches('\'')
        .to_string())
}

/// Returns True if the device is using Ti50.
pub fn is_ti50(context: &mut dyn Context) -> Result<bool> {
    let gsc_name = context
        .command("sh")
        .args(["-c", ". /usr/share/cros/gsc-constants.sh && gsc_name"])
        .output()?
        .stdout();
    Ok(gsc_name == "ti50")
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::process::Output;

    use crate::device;
    use crate::system::context::{Context, ContextImpl};
    use crate::utils::process_utils::StringOutput;

    #[test]
    fn test_get_fix_device_storage_success() {
        let mut context = ContextImpl::new();
        context.set_command_stdout("sh", "/dev/sda".to_string());
        assert_eq!(
            device::get_fixed_device_storage(&mut context).unwrap(),
            "/dev/sda".to_string()
        );
    }

    #[test]
    fn test_get_release_rootfs_partition_success_dev_sda() {
        let mut context = ContextImpl::new();
        context.set_command_stdout("sh", "/dev/sda".to_string());
        assert_eq!(
            device::get_release_rootfs_partition(&mut context).unwrap(),
            "/dev/sda5".to_string()
        );
    }

    #[test]
    fn test_get_release_rootfs_partition_success_dev_mmcblk0() {
        let mut context = ContextImpl::new();
        context.set_command_stdout("sh", "/dev/mmcblk0".to_string());
        assert_eq!(
            device::get_release_rootfs_partition(&mut context).unwrap(),
            "/dev/mmcblk0p5".to_string()
        );
    }

    #[test]
    fn test_get_release_stateful_partition_success_dev_sda() {
        let mut context = ContextImpl::new();
        context.set_command_stdout("sh", "/dev/sda".to_string());
        context.set_command_stdout("pvdisplay", "".to_string());
        assert_eq!(
            device::get_release_stateful_partition(&mut context).unwrap(),
            "/dev/sda1".to_string()
        );
    }

    #[test]
    fn test_get_release_stateful_partition_success_dev_mmcblk0() {
        let mut context = ContextImpl::new();
        context.set_command_stdout("sh", "/dev/mmcblk0".to_string());
        context.set_command_stdout("pvdisplay", "".to_string());
        assert_eq!(
            device::get_release_stateful_partition(&mut context).unwrap(),
            "/dev/mmcblk0p1".to_string()
        );
    }

    #[test]
    fn test_get_release_stateful_partition_success_lvm() {
        let mut context = ContextImpl::new();
        context.set_command_stdout("sh", "/dev/sda".to_string());
        context.set_command_stdout("pvdisplay", "vg_name".to_string());
        context.set_command_output("vgchange", Output::new(0, None, None));
        assert_eq!(
            device::get_release_stateful_partition(&mut context).unwrap(),
            "/dev/vg_name/unencrypted".to_string()
        );
    }

    #[test]
    fn test_get_dev_stateful_partition_success() {
        let context = ContextImpl::new();
        let dir = context
            .root_dir()
            .join("mnt/stateful_partition/dev_image/etc");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("lsb-factory"), "REAL_USB_DEV=/dev/sda3").unwrap();
        assert_eq!(
            device::get_dev_stateful_partition(&context).unwrap(),
            "/dev/sda1".to_string()
        );
    }

    #[test]
    fn test_get_model_name_success() {
        let mut context = ContextImpl::new();
        context.set_command_stdout("cros_config", "model".to_string());
        assert_eq!(
            device::get_model_name(&mut context).unwrap(),
            "model".to_string()
        );
    }

    #[test]
    fn test_get_firmware_manifest_key_success() {
        let mut context = ContextImpl::new();
        context.set_command_stdout("crosid", "FIRMWARE_MANIFEST_KEY=key".to_string());
        assert_eq!(
            device::get_firmware_manifest_key(&mut context).unwrap(),
            "key".to_string()
        );
    }

    #[test]
    fn test_is_ti50_true() {
        let mut context = ContextImpl::new();
        context.set_command_stdout("sh", "ti50".to_string());
        assert!(device::is_ti50(&mut context).unwrap());
    }

    #[test]
    fn test_is_ti50_false() {
        let mut context = ContextImpl::new();
        context.set_command_stdout("sh", "cr50".to_string());
        assert!(!device::is_ti50(&mut context).unwrap());
    }
}
