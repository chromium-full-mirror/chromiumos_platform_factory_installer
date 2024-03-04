// Copyright 2024 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use anyhow::Result;

use crate::system::context::Context;
use crate::utils::process_utils::StringOutput;

/// Gets the value in RO_VPD section with given key.
pub fn get_ro_value(key: &str, context: &mut dyn Context) -> Result<String> {
    let output = context.command("vpd").args(["-g", key]).output()?;
    if output.status.success() {
        Ok(output.stdout())
    } else {
        anyhow::bail!("Get RO VPD fail. key: {}", key)
    }
}

/// Gets the value in RW_VPD section with given key.
pub fn get_rw_value(key: &str, context: &mut dyn Context) -> Result<String> {
    let output = context
        .command("vpd")
        .args(["-i", "RW_VPD", "-g", key])
        .output()?;
    if output.status.success() {
        Ok(output.stdout())
    } else {
        anyhow::bail!("Get RW VPD fail. key: {}", key)
    }
}

/// Deletes the value in RW_VPD section with given key.
pub fn delete_rw_value(key: &str, context: &mut dyn Context) -> Result<()> {
    eprintln!("Checking {} in RW VPD...", key);
    match get_rw_value(key, context) {
        Ok(_) => {
            eprintln!("Deleting {} from VPD.", key);
            let output = context
                .command("vpd")
                .args(["-i", "RW_VPD", "-d", key])
                .output()?;
            if output.status.success() {
                Ok(())
            } else {
                anyhow::bail!("Delete RW VPD fail. key: {}", key)
            }
        }
        Err(_) => {
            eprintln!("OK: no {} found.", key);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::system::context::ContextImpl;
    use crate::utils::vpd_utils;

    #[test]
    fn test_get_ro_value_success() {
        let mut context = ContextImpl::new();
        context.set_command_stdout("vpd", "value".to_string());

        let result = vpd_utils::get_ro_value("key", &mut context);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "value");
    }

    #[test]
    fn test_get_ro_value_failed() {
        let mut context = ContextImpl::new();
        context.set_command_stderr("vpd", "error".to_string());

        let result = vpd_utils::get_ro_value("k", &mut context);
        let err = result.unwrap_err();
        let message = err.root_cause();
        assert_eq!(format!("{}", message), "Get RO VPD fail. key: k");
    }

    #[test]
    fn test_get_rw_value_success() {
        let mut context = ContextImpl::new();
        context.set_command_stdout("vpd", "value".to_string());

        let result = vpd_utils::get_rw_value("key", &mut context);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "value");
    }

    #[test]
    fn test_get_rw_value_failed() {
        let mut context = ContextImpl::new();
        context.set_command_stderr("vpd", "error".to_string());

        let result = vpd_utils::get_rw_value("k", &mut context);
        let err = result.unwrap_err();
        let message = err.root_cause();
        assert_eq!(format!("{}", message), "Get RW VPD fail. key: k");
    }

    #[test]
    fn test_delete_rw_value_success() {
        let mut context = ContextImpl::new();
        context.set_command_stdout("vpd", "value".to_string());
        context.set_command_stdout("vpd", "".to_string());

        let result = vpd_utils::delete_rw_value("key", &mut context);
        assert!(result.is_ok());
    }

    #[test]
    fn test_delete_rw_value_already_removed() {
        let mut context = ContextImpl::new();
        context.set_command_stderr("vpd", "error".to_string());

        let result = vpd_utils::delete_rw_value("key", &mut context);
        assert!(result.is_ok());
    }

    #[test]
    fn test_delete_rw_value_remove_failed() {
        let mut context = ContextImpl::new();
        context.set_command_stdout("vpd", "value".to_string());
        context.set_command_stderr("vpd", "error".to_string());

        let result = vpd_utils::delete_rw_value("k", &mut context);
        let err = result.unwrap_err();
        let message = err.root_cause();
        assert_eq!(format!("{}", message), "Delete RW VPD fail. key: k");
    }
}
