// Copyright 2024 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use anyhow::Result;

use crate::system::context::Context;
use crate::utils::process_utils::StringOutput;

/// Gets the value by parameter.
pub fn get_value(param: &str, context: &mut dyn Context) -> Result<String> {
    let output = context.command("crossystem").arg(param).output()?;
    if output.status.success() {
        Ok(output.stdout())
    } else {
        anyhow::bail!("Get {} fail.", param)
    }
}

/// Cutoff the battery after reboot
pub fn set_battery_cutoff_request(value: i32, context: &mut dyn Context) -> Result<()> {
    let output = context
        .command("crossystem")
        .arg(format!("battery_cutoff_request={}", value))
        .output()?;
    if !output.status.success() {
        anyhow::bail!("Call crossystem fail.")
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::system::context::ContextImpl;
    use crate::utils::crossystem_utils;

    #[test]
    fn test_get_value_success() {
        let mut context = ContextImpl::new();
        context.set_command_stdout("crossystem", "res".to_string());

        let result = crossystem_utils::get_value("", &mut context);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "res");
    }

    #[test]
    fn test_get_value_failed() {
        let mut context = ContextImpl::new();
        context.set_command_stderr("crossystem", "error".to_string());

        let result = crossystem_utils::get_value("key", &mut context);
        let err = result.unwrap_err();
        let message = err.root_cause();
        assert_eq!(format!("{}", message), "Get key fail.");
    }

    #[test]
    fn test_cutoff_battery_success() {
        let mut context = ContextImpl::new();
        context.set_command_stdout("crossystem", "".to_string());

        let result = crossystem_utils::set_battery_cutoff_request(1, &mut context);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cutoff_battery_failed() {
        let mut context = ContextImpl::new();
        context.set_command_stderr("crossystem", "error".to_string());

        let result = crossystem_utils::set_battery_cutoff_request(1, &mut context);
        let err = result.unwrap_err();
        let message = err.root_cause();
        assert_eq!(format!("{}", message), "Call crossystem fail.");
    }
}
