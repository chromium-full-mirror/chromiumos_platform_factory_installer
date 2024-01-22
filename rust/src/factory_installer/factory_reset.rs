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
use crate::system::context::Context;
use crate::utils::process_utils::StringOutput;

pub fn factory_reset(action: ResetAction, context: &mut dyn Context) -> Result<()> {
    let disk_path = device::get_fixed_device_storage(context)?;

    match action {
        ResetAction::Reset => do_reset(&disk_path, context),
        ResetAction::Wipe => do_wipe(&disk_path, context),
        ResetAction::Secure => do_secure(&disk_path, context),
        ResetAction::Verify => do_verify(&disk_path, context),
    }
}

fn do_reset(disk_path: &str, context: &mut dyn Context) -> Result<()> {
    // TODO(jasonchuang): Replace cutoff.sh to rust implementation.
    let child = context
        .command("/usr/sbin/factory_reset.sh")
        .arg(disk_path)
        .spawn()?;
    let output = child.wait_with_output()?;
    if !output.status.success() {
        let stdout = output.stdout();
        let stderr = output.stderr();
        anyhow::bail!("factory_reset fail, stdout: {}, stderr: {}", stdout, stderr)
    }
    Ok(())
}

fn do_wipe(disk_path: &str, context: &mut dyn Context) -> Result<()> {
    // TODO(jasonchuang): Replace cutoff.sh to rust implementation.
    let child = context
        .command("/usr/sbin/factory_reset.sh")
        .arg(disk_path)
        .arg("wipe")
        .spawn()?;
    let output = child.wait_with_output()?;
    if !output.status.success() {
        let stdout = output.stdout();
        let stderr = output.stderr();
        anyhow::bail!("factory_reset fail, stdout: {}, stderr: {}", stdout, stderr)
    }
    Ok(())
}

fn do_secure(disk_path: &str, context: &mut dyn Context) -> Result<()> {
    // TODO(jasonchuang): Replace cutoff.sh to rust implementation.
    // Erase using firmware feature first.
    let child = context
        .command("/usr/sbin/factory_reset.sh")
        .arg(disk_path)
        .arg("secure")
        .spawn()
        .unwrap();

    let output = child.wait_with_output()?;
    if !output.status.success() {
        let stdout = output.stdout();
        let stderr = output.stderr();
        anyhow::bail!("factory_reset fail, stdout: {}, stderr: {}", stdout, stderr)
    }
    Ok(())
}

fn do_verify(disk_path: &str, context: &mut dyn Context) -> Result<()> {
    // TODO(jasonchuang): Replace cutoff.sh to rust implementation.
    let child = context
        .command("/usr/sbin/factory_reset.sh")
        .arg(disk_path)
        .arg("verify")
        .spawn()
        .unwrap();
    let output = child.wait_with_output()?;
    if !output.status.success() {
        let stdout = output.stdout();
        let stderr = output.stderr();
        anyhow::bail!("factory_reset fail, stdout: {}, stderr: {}", stdout, stderr)
    }
    Ok(())
}

#[cfg(test)]
mod tests {

    use crate::factory_installer::factory_reset;
    use crate::factory_installer::factory_reset::ResetAction;
    use crate::system::context::ContextImpl;

    #[test]
    fn test_factory_reset_success() {
        let mut context = ContextImpl::new();
        for action in [
            ResetAction::Reset,
            ResetAction::Wipe,
            ResetAction::Secure,
            ResetAction::Verify,
        ] {
            context.set_command_stdout("sh", "/dev/sda".to_string());
            context.set_command_stdout("/usr/sbin/factory_reset.sh", "".to_string());
            let result = factory_reset::factory_reset(action, &mut context);
            assert!(result.is_ok());
        }
    }

    #[test]
    fn test_factory_reset_error() {
        let mut context = ContextImpl::new();
        for action in [
            ResetAction::Reset,
            ResetAction::Wipe,
            ResetAction::Secure,
            ResetAction::Verify,
        ] {
            context.set_command_stdout("sh", "/dev/sda".to_string());
            context.set_command_stderr("/usr/sbin/factory_reset.sh", "stderr".to_string());
            let result = factory_reset::factory_reset(action, &mut context);
            let err = result.unwrap_err();
            let message = err.root_cause();
            assert_eq!(
                format!("{}", message),
                "factory_reset fail, stdout: , stderr: stderr"
            );
        }
    }
}
