// Copyright 2024 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

// This runs from the factory install/reset shim. This MUST be run
// from USB, in developer mode. This script will wipe OQC activity and
// put the system back into factory fresh/shippable state.
// Preserve files in the CRX cache.  Largely copied from clobber-state.

// TODO(dgarrett,jsalz): Consolidate.

use anyhow::{self, Result};

use crate::factory_installer::args::ResetAction;
use crate::factory_installer::script_wrapper;
use crate::system::context::Context;
use crate::utils::process_utils::StringOutput;
#[cfg(not(test))]
use crate::utils::sys_utils;
#[cfg(test)]
use crate::utils::sys_utils::mock as sys_utils;
use crate::{cutoff, device};

// CUTOFF_DIR is provided from platform/factory/sh/cutoff, repacked by ebuild.
const CUTOFF_DIR: &str = "/usr/share/cutoff";

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

    display_qrcode(context)?;
    inform_shopfloor(context)?;
    Ok(())
}

pub fn do_reset(disk_path: &str, context: &mut dyn Context) -> Result<()> {
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

pub fn display_qrcode(context: &mut dyn Context) -> Result<()> {
    let lsb_factory = device::get_lsb_factory(context)?;
    if lsb_factory
        .get("DISPLAY_QRCODE")
        .is_some_and(|value| value == "true")
    {
        if let Some(display_info) = lsb_factory.get("DISPLAY_INFO") {
            cutoff::display_qrcode(display_info, context)?;
        }
    }
    Ok(())
}

pub fn inform_shopfloor(context: &mut dyn Context) -> Result<()> {
    // inform_shopfloor will load factory server URL from lsb-factory, and
    // ignore the request if FACTORY_SEVER_URL is not set. Note that
    // notification to shopfloor is proxied by the factory server, so we need a
    // factory server URL here.
    let path = context
        .root_dir()
        .join(CUTOFF_DIR)
        .join("inform_shopfloor.sh");
    let mut child = context
        .command(path.to_str().unwrap())
        .args(["", "factory_reset"])
        .spawn()?;
    let status = child.wait()?;
    if !status.success() {
        anyhow::bail!("inform shopfloor fail")
    }
    Ok(())
}

#[cfg(test)]
mod tests {

    use std::fs::{self, File};

    use crate::factory_installer::factory_reset::{self, ResetAction};
    use crate::system::context::{Context, ContextImpl};

    #[test]
    fn test_factory_reset_reset_success() {
        let mut context = ContextImpl::new();
        prepare_files(&mut context, "");
        setup_disk("/dev/sda".to_string(), "size".to_string(), &mut context);
        context.set_command_stdout("start", "".to_string());
        context.set_command_stdout("clobber-state", "".to_string());
        context.set_command_stdout("/usr/share/cutoff/inform_shopfloor.sh", "".to_string());
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
        prepare_files(&mut context, "");
        setup_disk("disk_path".to_string(), "size".to_string(), &mut context);
        context.set_command_stdout("start", "".to_string());
        context.set_command_stdout("bash", "".to_string());
        context.set_command_stdout("/usr/share/cutoff/inform_shopfloor.sh", "".to_string());
        let result = factory_reset::factory_reset(ResetAction::Wipe, &mut context);
        assert!(result.is_ok());
    }

    #[test]
    fn test_factory_reset_wipe_error_ignore() {
        let mut context = ContextImpl::new();
        prepare_files(&mut context, "");
        setup_disk("disk_path".to_string(), "size".to_string(), &mut context);
        context.set_command_stdout("start", "".to_string());
        context.set_command_stderr("bash", "stderr".to_string());
        context.set_command_stdout("/usr/share/cutoff/inform_shopfloor.sh", "".to_string());
        let result = factory_reset::factory_reset(ResetAction::Wipe, &mut context);
        assert!(result.is_ok());
    }

    #[test]
    fn test_factory_reset_secure_success() {
        let mut context = ContextImpl::new();
        prepare_files(&mut context, "");
        setup_disk("disk_path".to_string(), "size".to_string(), &mut context);
        context.set_command_stdout("start", "".to_string());
        context.set_command_stdout("/usr/sbin/script_wrapper.sh", "".to_string());
        context.set_command_stdout("/usr/sbin/script_wrapper.sh", "".to_string());
        context.set_command_stdout("/usr/share/cutoff/inform_shopfloor.sh", "".to_string());
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
        prepare_files(&mut context, "");
        setup_disk("disk_path".to_string(), "size".to_string(), &mut context);
        context.set_command_stdout("start", "".to_string());
        context.set_command_stdout("/usr/sbin/script_wrapper.sh", "".to_string());
        context.set_command_stdout("/usr/share/cutoff/inform_shopfloor.sh", "".to_string());
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

    #[test]
    fn test_display_qrcode() {
        let mut context = ContextImpl::new();
        prepare_files(&mut context, "frecon_id");
        let dir = context
            .root_dir()
            .join("mnt/stateful_partition/dev_image/etc");
        fs::write(
            dir.join("lsb-factory"),
            "DISPLAY_QRCODE=true\nDISPLAY_INFO=hwid serial_number wifi_mac0 \
             mlb_serial_number service_tag",
        )
        .unwrap();

        context.set_command_stdout("crossystem", "HWID".to_string());
        context.set_command_stdout("vpd", "SN".to_string());
        context.set_command_stdout("vpd", "WIFI".to_string());
        context.set_command_stdout("vpd", "MLBSN".to_string());
        context.set_command_stdout("vpd", "TAG".to_string());
        context.set_command_stdout("bash", "".to_string());
        context.set_tempfile("qrcode.png".to_string());

        let result = factory_reset::display_qrcode(&mut context);
        assert!(result.is_ok());
        assert_eq!(
            fs::read(
                context
                    .root_dir()
                    .join("proc/frecon_id/root/qrcode.png")
                    .to_str()
                    .unwrap()
            )
            .unwrap(),
            fs::read("tests/qrcode/ALL.png").unwrap()
        );
    }

    #[test]
    fn test_display_qrcode_multi() {
        let mut context = ContextImpl::new();
        prepare_files(&mut context, "frecon_id");
        let dir = context
            .root_dir()
            .join("mnt/stateful_partition/dev_image/etc");
        fs::write(
            dir.join("lsb-factory"),
            "DISPLAY_QRCODE=true\nDISPLAY_INFO=serial_number,hwid",
        )
        .unwrap();
        context.set_command_stdout("vpd", "SN".to_string());
        context.set_command_stdout("crossystem", "HWID".to_string());
        context.set_command_stdout("bash", "".to_string());
        context.set_command_stdout("bash", "".to_string());
        context.set_tempfile("qrcode.png".to_string());
        context.set_tempfile("qrcode1.png".to_string());

        let result = factory_reset::display_qrcode(&mut context);
        assert!(result.is_ok());
        assert_eq!(
            fs::read(
                context
                    .root_dir()
                    .join("proc/frecon_id/root/qrcode.png")
                    .to_str()
                    .unwrap()
            )
            .unwrap(),
            fs::read("tests/qrcode/SN.png").unwrap()
        );
        assert_eq!(
            fs::read(
                context
                    .root_dir()
                    .join("proc/frecon_id/root/qrcode1.png")
                    .to_str()
                    .unwrap()
            )
            .unwrap(),
            fs::read("tests/qrcode/HWID.png").unwrap()
        );
    }

    #[test]
    fn test_display_qrcode_unsupported_key() {
        let mut context = ContextImpl::new();
        prepare_files(&mut context, "frecon_id");
        let dir = context
            .root_dir()
            .join("mnt/stateful_partition/dev_image/etc");
        fs::write(
            dir.join("lsb-factory"),
            "DISPLAY_QRCODE=true\nDISPLAY_INFO=abc",
        )
        .unwrap();

        let result = factory_reset::display_qrcode(&mut context);
        let err = result.unwrap_err();
        let message = err.root_cause();
        assert_eq!(format!("{}", message), "Unknown key: abc");
    }

    fn setup_disk(disk_path: String, size: String, context: &mut ContextImpl) {
        context.set_command_stdout("sh", disk_path.clone());
        context.set_command_stdout("sh", disk_path.clone());
        context.set_command_stdout("sh", disk_path.clone());
        context.set_command_stdout("pvdisplay", "".to_string());
        context.set_command_stdout("blockdev", size);
    }

    fn prepare_files(context: &mut ContextImpl, frecon_id: &str) {
        let dir = context
            .root_dir()
            .join("mnt/stateful_partition/dev_image/etc");
        fs::create_dir_all(&dir).unwrap();
        File::create(dir.join("lsb-factory")).unwrap();
        fs::create_dir_all(context.root_dir().join("proc").join(frecon_id).join("root")).unwrap();
        let dir = context.root_dir().join("run/frecon");
        fs::create_dir_all(dir.clone()).unwrap();
        File::create(dir.join("vt0")).unwrap();
        fs::write(dir.join("pid"), frecon_id).unwrap();
        let dir = context.root_dir().join("usr/share/cutoff");
        fs::create_dir_all(&dir).unwrap();
        File::create(dir.join("cutoff.json")).unwrap();
        File::create(dir.join("display_wipe_message.sh")).unwrap();
    }
}
