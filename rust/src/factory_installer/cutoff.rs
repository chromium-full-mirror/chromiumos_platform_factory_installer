// Copyright 2024 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

// This file controls battery power level and performs required battery
// cutoff protection by sending commands to EC with ectool.

use anyhow::{self, Result};

use crate::system::context::Context;
use crate::utils::process_utils::StringOutput;

const CUTOFF_DIR: &str = "/usr/share/cutoff";

pub fn do_cutoff(context: &mut dyn Context) -> Result<()> {
    // TODO(jasonchuang): Replace cutoff.sh to rust implementation.
    let path = context.root_dir().join(CUTOFF_DIR).join("cutoff.sh");
    let child = context.command(path.to_str().unwrap()).spawn()?;
    let output = child.wait_with_output()?;
    if !output.status.success() {
        let stdout = output.stdout();
        let stderr = output.stderr();
        anyhow::bail!("cutoff fail, stdout: {}, stderr: {}", stdout, stderr)
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::factory_installer::cutoff;
    use crate::system::context::ContextImpl;

    #[test]
    fn test_cutoff_success() {
        let mut context = ContextImpl::new();
        context.set_command_stdout("/usr/share/cutoff/cutoff.sh", "".to_string());
        let result = cutoff::do_cutoff(&mut context);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cutoff_error() {
        let mut context = ContextImpl::new();
        context.set_command_stderr("/usr/share/cutoff/cutoff.sh", "stderr".to_string());
        let result = cutoff::do_cutoff(&mut context);
        let err = result.unwrap_err();
        let message = err.root_cause();
        assert_eq!(
            format!("{}", message),
            "cutoff fail, stdout: , stderr: stderr"
        );
    }
}
