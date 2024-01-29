// Copyright 2024 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

// This runs from the factory install/reset shim. This MUST be run
// from USB, in developer mode. This script will wipe OQC activity and
// put the system back into factory fresh/shippable state.
// Preserve files in the CRX cache.  Largely copied from clobber-state.

// TODO(dgarrett,jsalz): Consolidate.

use anyhow::{self, Result};

use crate::device;
use crate::factory_installer::args::ResetAction;
use crate::factory_installer::script_wrapper;
use crate::system::context::Context;
use crate::utils::process_utils::StringOutput;
#[cfg(not(test))]
use crate::utils::sys_utils;
#[cfg(test)]
use crate::utils::sys_utils::mock as sys_utils;

pub fn factory_reset(action: ResetAction, context: &mut dyn Context) -> Result<()> {
    let disk_path = device::get_fixed_device_storage(context)?;

    // TODO(jasonchuang): Check if we can add io-block. https://docs.rs/io-block/latest
    let mut dev_size = context
        .command("blockdev")
        .args(["--getsize64", &disk_path])
        .output()?
        .stdout();
    dev_size = dev_size.trim().to_string();

    // Tcsd will bring up the tpm and de-own it,
    // as we are in developer/recovery mode.
    context.command("start").arg("tcsd").output()?;

    match action {
        ResetAction::Reset => do_reset(&disk_path, context),
        ResetAction::Wipe => do_wipe(&disk_path, &dev_size, context),
        ResetAction::Secure => do_secure(&disk_path, &dev_size, context),
        ResetAction::Verify => do_verify(&disk_path, &dev_size, context),
    }?;

    // TODO(jasonchuang): Replace factory_reset.sh to rust implementation.
    let mut child = context.command("/usr/sbin/factory_reset.sh").spawn()?;
    let status = child.wait()?;
    if !status.success() {
        anyhow::bail!("factory_reset fail")
    }
    Ok(())
}

fn do_reset(disk_path: &str, context: &mut dyn Context) -> Result<()> {
    let state_dev = device::get_release_stateful_partition(context)?;
    let root_dev = device::get_release_rootfs_partition(context)?;
    eprintln!(
        "Running clobber-state: ROOT_DEV={}, ROOT_DISK: {}",
        root_dev, disk_path
    );

    // clobber-state preserves the crx and factory installed dlc files under
    // `/mnt/stateful_partition`. We need to mount `${STATE_DEV}` to that
    // directory, so that the files can be correctly preserved.
    let state_mount_point = "/mnt/stateful_partition";
    sys_utils::mount(state_dev, state_mount_point, true)?;
    let mut child = context
        .command("clobber-state")
        .args(["factory", "fast"])
        .env("ROOT_DEV", &root_dev)
        .env("ROOT_DISK", disk_path)
        .spawn()?;
    let status = child.wait()?;
    if !status.success() {
        anyhow::bail!("reset fail")
    }
    sys_utils::umount(state_mount_point)?;
    Ok(())
}

fn do_wipe(disk_path: &str, dev_size: &str, context: &mut dyn Context) -> Result<()> {
    // TODO(jasonchuang): Find an alternative for the progress bar.
    // Nuke the disk.
    let mut child = context
        .command("bash")
        .args([
            "-c",
            &format!(
                "pv -etpr -s {} -B 8M /dev/zero | dd bs=8M of={} oflag=dsync iflag=fullblock",
                dev_size, disk_path
            ),
        ])
        .spawn()?;
    child.wait()?;
    Ok(())
}

fn do_secure(disk_path: &str, dev_size: &str, context: &mut dyn Context) -> Result<()> {
    // Erase using firmware feature first.
    script_wrapper::call_script_wrapper(vec!["secure_erase", disk_path], context)?;
    script_wrapper::call_script_wrapper(
        vec!["perform_fio_op", disk_path, dev_size, "write"],
        context,
    )?;
    Ok(())
}

fn do_verify(disk_path: &str, dev_size: &str, context: &mut dyn Context) -> Result<()> {
    script_wrapper::call_script_wrapper(
        vec!["perform_fio_op", disk_path, dev_size, "verify"],
        context,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {

    use crate::factory_installer::factory_reset;
    use crate::factory_installer::factory_reset::ResetAction;
    use crate::system::context::ContextImpl;

    #[test]
    fn test_factory_reset_reset_success() {
        let mut context = ContextImpl::new();
        setup_disk("/dev/sda".to_string(), "size".to_string(), &mut context);
        context.set_command_stdout("start", "".to_string());
        context.set_command_stdout("clobber-state", "".to_string());
        context.set_command_stdout("/usr/sbin/factory_reset.sh", "".to_string());
        let result = factory_reset::factory_reset(ResetAction::Reset, &mut context);
        assert!(result.is_ok());
    }

    #[test]
    fn test_factory_reset_reset_error() {
        let mut context = ContextImpl::new();
        setup_disk("/dev/sda".to_string(), "size".to_string(), &mut context);
        context.set_command_stdout("start", "".to_string());
        context.set_command_stderr("clobber-state", "stderr".to_string());
        let result = factory_reset::factory_reset(ResetAction::Reset, &mut context);
        let err = result.unwrap_err();
        let message = err.root_cause();
        assert_eq!(format!("{}", message), "reset fail");
    }

    #[test]
    fn test_factory_reset_wipe_success() {
        let mut context = ContextImpl::new();
        setup_disk("disk_path".to_string(), "size".to_string(), &mut context);
        context.set_command_stdout("start", "".to_string());
        context.set_command_stdout("bash", "".to_string());
        context.set_command_stdout("/usr/sbin/factory_reset.sh", "".to_string());
        let result = factory_reset::factory_reset(ResetAction::Wipe, &mut context);
        assert!(result.is_ok());
    }

    #[test]
    fn test_factory_reset_wipe_error_ignore() {
        let mut context = ContextImpl::new();
        setup_disk("disk_path".to_string(), "size".to_string(), &mut context);
        context.set_command_stdout("start", "".to_string());
        context.set_command_stderr("bash", "stderr".to_string());
        context.set_command_stdout("/usr/sbin/factory_reset.sh", "".to_string());
        let result = factory_reset::factory_reset(ResetAction::Wipe, &mut context);
        assert!(result.is_ok());
    }

    #[test]
    fn test_factory_reset_secure_success() {
        let mut context = ContextImpl::new();
        setup_disk("disk_path".to_string(), "size".to_string(), &mut context);
        context.set_command_stdout("start", "".to_string());
        context.set_command_stdout("/usr/sbin/script_wrapper.sh", "".to_string());
        context.set_command_stdout("/usr/sbin/script_wrapper.sh", "".to_string());
        context.set_command_stdout("/usr/sbin/factory_reset.sh", "".to_string());
        let result = factory_reset::factory_reset(ResetAction::Secure, &mut context);
        assert!(result.is_ok());
    }

    #[test]
    fn test_factory_reset_secure_erase_error() {
        let mut context = ContextImpl::new();
        setup_disk("disk_path".to_string(), "size".to_string(), &mut context);
        context.set_command_stdout("start", "".to_string());
        context.set_command_stderr("/usr/sbin/script_wrapper.sh", "".to_string());
        let result = factory_reset::factory_reset(ResetAction::Secure, &mut context);
        let err = result.unwrap_err();
        let message = err.root_cause();
        assert_eq!(
            format!("{}", message),
            "call wrapper fail, cmd: secure_erase disk_path"
        );
    }

    #[test]
    fn test_factory_reset_secure_perform_fio_op_error() {
        let mut context = ContextImpl::new();
        setup_disk("disk_path".to_string(), "size".to_string(), &mut context);
        context.set_command_stdout("start", "".to_string());
        context.set_command_stdout("/usr/sbin/script_wrapper.sh", "".to_string());
        context.set_command_stderr("/usr/sbin/script_wrapper.sh", "".to_string());
        let result = factory_reset::factory_reset(ResetAction::Secure, &mut context);
        let err = result.unwrap_err();
        let message = err.root_cause();
        assert_eq!(
            format!("{}", message),
            "call wrapper fail, cmd: perform_fio_op disk_path size write"
        );
    }

    #[test]
    fn test_factory_reset_verify_success() {
        let mut context = ContextImpl::new();
        setup_disk("disk_path".to_string(), "size".to_string(), &mut context);
        context.set_command_stdout("start", "".to_string());
        context.set_command_stdout("/usr/sbin/script_wrapper.sh", "".to_string());
        context.set_command_stdout("/usr/sbin/factory_reset.sh", "".to_string());
        let result = factory_reset::factory_reset(ResetAction::Verify, &mut context);
        assert!(result.is_ok());
    }

    #[test]
    fn test_factory_reset_verify_error() {
        let mut context = ContextImpl::new();
        setup_disk("disk_path".to_string(), "size".to_string(), &mut context);
        context.set_command_stdout("start", "".to_string());
        context.set_command_stderr("/usr/sbin/script_wrapper.sh", "".to_string());
        let result = factory_reset::factory_reset(ResetAction::Verify, &mut context);
        let err = result.unwrap_err();
        let message = err.root_cause();
        assert_eq!(
            format!("{}", message),
            "call wrapper fail, cmd: perform_fio_op disk_path size verify"
        );
    }

    fn setup_disk(disk_path: String, size: String, context: &mut ContextImpl) {
        context.set_command_stdout("sh", disk_path.clone());
        context.set_command_stdout("sh", disk_path.clone());
        context.set_command_stdout("sh", disk_path.clone());
        context.set_command_stdout("pvdisplay", "".to_string());
        context.set_command_stdout("blockdev", size);
    }
}
