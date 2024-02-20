// Copyright 2024 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use anyhow::Result;

use crate::system::context::Context;
use crate::utils::process_utils::StringOutput;

/// Gets the value of hardware id.
pub fn get_hwid(context: &mut dyn Context) -> Result<String> {
    let output = context.command("crossystem").arg("hwid").output()?;
    if output.status.success() {
        Ok(output.stdout())
    } else {
        anyhow::bail!("Get HWID fail.")
    }
}

#[cfg(test)]
mod tests {
    use crate::system::context::ContextImpl;
    use crate::utils::crossystem_utils;

    #[test]
    fn test_get_hwid_success() {
        let mut context = ContextImpl::new();
        context.set_command_stdout("crossystem", "hwid".to_string());

        let result = crossystem_utils::get_hwid(&mut context);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "hwid");
    }

    #[test]
    fn test_get_hwid_failed() {
        let mut context = ContextImpl::new();
        context.set_command_stderr("crossystem", "error".to_string());

        let result = crossystem_utils::get_hwid(&mut context);
        let err = result.unwrap_err();
        let message = err.root_cause();
        assert_eq!(format!("{}", message), "Get HWID fail.");
    }
}
