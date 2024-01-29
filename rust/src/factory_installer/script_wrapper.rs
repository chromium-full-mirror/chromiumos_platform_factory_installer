// Copyright 2024 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use anyhow::Result;

use crate::system::context::Context;

// When migrating factory installer to rust, there might be some functions
// defined in scripts that can't be called directly. This file will call
// platform/factory_installer/script_wrapper.sh as a proxy. After we implement
// all functions by rust, we can remove this file.

pub fn call_script_wrapper(command: Vec<&str>, context: &mut dyn Context) -> Result<()> {
    let mut child = context
        .command("/usr/sbin/script_wrapper.sh")
        .args(command.clone())
        .spawn()?;
    let status = child.wait()?;
    if !status.success() {
        anyhow::bail!("call wrapper fail, cmd: {}", command.join(" "))
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::factory_installer::script_wrapper;
    use crate::system::context::ContextImpl;

    #[test]
    fn test_call_script_wrapper_success() {
        let mut context = ContextImpl::new();
        context.set_command_stdout("/usr/sbin/script_wrapper.sh", "".to_string());
        let result = script_wrapper::call_script_wrapper(vec!["cmd"], &mut context);
        assert!(result.is_ok());
    }

    #[test]
    fn test_call_script_wrapper_failed() {
        let mut context = ContextImpl::new();
        context.set_command_stderr("/usr/sbin/script_wrapper.sh", "".to_string());
        let result = script_wrapper::call_script_wrapper(vec!["cmd"], &mut context);
        let err = result.unwrap_err();
        let message = err.root_cause();
        assert_eq!(format!("{}", message), "call wrapper fail, cmd: cmd");
    }
}
