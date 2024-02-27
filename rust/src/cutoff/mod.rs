// Copyright 2024 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

// This file controls battery power level and performs required battery
// cutoff protection by sending commands to EC with ectool.

use std::fs;

use anyhow::{self, Result};
use serde::Deserialize;

use crate::cutoff::config::{CutoffAcState, CutoffMethod};
use crate::system::context::Context;
use crate::utils::crossystem_utils;
use crate::utils::ectool_utils::{self, ChargeControl};
#[cfg(not(test))]
use crate::utils::sys_utils;
#[cfg(test)]
use crate::utils::sys_utils::mock as sys_utils;

pub mod config;

const CUTOFF_DIR: &str = "usr/share/cutoff";

#[derive(Deserialize, Debug)]
#[serde(rename_all(deserialize = "SCREAMING_SNAKE_CASE"))]
pub struct CutoffConfig {
    pub cutoff_method: CutoffMethod,
    pub cutoff_ac_state: CutoffAcState,
    #[serde(default = "i32_min")]
    pub cutoff_battery_min_percentage: i32,
    #[serde(default = "i32_max")]
    pub cutoff_battery_max_percentage: i32,
    #[serde(default = "i32_min")]
    pub cutoff_battery_min_voltage: i32,
    #[serde(default = "i32_max")]
    pub cutoff_battery_max_voltage: i32,
    #[serde(default)]
    pub shopfloor_url: String,
    #[serde(default)]
    pub tty: String,
    #[serde(default)]
    pub continue_key: String,
    #[serde(default)]
    pub qrcode_info: String,
}

fn i32_min() -> i32 {
    i32::MIN
}

fn i32_max() -> i32 {
    i32::MAX
}

pub fn do_cutoff(context: &mut dyn Context) -> Result<()> {
    // TODO(jasonchuang): Replace cutoff.sh to rust implementation.
    let path = context.root_dir().join(CUTOFF_DIR).join("cutoff.sh");
    let mut child = context.command(path.to_str().unwrap()).spawn()?;
    let status = child.wait()?;
    if !status.success() {
        anyhow::bail!("cutoff fail")
    }
    Ok(())
}

fn get_cutoff_config(context: &mut dyn Context) -> Result<CutoffConfig> {
    let path = context.root_dir().join(CUTOFF_DIR).join("cutoff.json");
    // TODO(jasonchuang): Also need to access the path of cutoff.json for toolkit.

    let config: CutoffConfig = serde_json::from_str(&fs::read_to_string(path)?)?;
    Ok(config)
}

pub fn cutoff_battery(context: &mut dyn Context) -> Result<()> {
    ectool_utils::charge_control(ChargeControl::Normal, context)?;
    display_message("cutting_off", context)?;
    let config = get_cutoff_config(context)?;
    /*
    TODO (shunhsingou): In bug
    https://bugs.chromium.org/p/chromium/issues/detail?id=589677, small amount
    of devices fail to do cut off in factory. This may be caused by unstable
    ectool, or shutdown fail in the tmpfs. Here we add more retries for
    solving this problem. Remove the retry when finding the root cause.
    */
    for _ in 1..5 {
        let result = match config.cutoff_method {
            CutoffMethod::Reboot => sys_utils::reboot(),
            // If virtual dev mode was enabled, ectool cutoff will leave the device
            // in developer mode. Unfortunately we can't check that because TPM
            // service was not running, and tpm_nvread won't work.
            CutoffMethod::EctoolCutoff => {
                ectool_utils::cutoff(context)?;
                sys_utils::shutdown()
            }
            CutoffMethod::BatteryCutoff => {
                crossystem_utils::set_battery_cutoff_request(1, context)?;
                sys_utils::sleep(3.0)?;
                sys_utils::reboot()
            }
            CutoffMethod::EcHibernate => {
                ectool_utils::hibernate(context)?;
                sys_utils::shutdown()
            }
            // By default we shutdown the device without doing anything.
            CutoffMethod::Shutdown => sys_utils::shutdown(),
        };
        if result.is_ok() {
            return Ok(());
        }
    }
    anyhow::bail!("Cutoff fail")
}

fn display_message(message: &str, context: &mut dyn Context) -> Result<()> {
    let mut child = context
        .command(
            &context
                .root_dir()
                .join(CUTOFF_DIR)
                .join("display_wipe_message.sh")
                .to_str()
                .unwrap(),
        )
        .arg(message)
        .spawn()?;
    child.wait()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::json;

    use crate::cutoff;
    use crate::system::context::{Context, ContextImpl};

    #[test]
    fn test_cutoff_success() {
        let mut context = ContextImpl::new();
        context.set_command_stdout(
            context.root_dir().join("usr/share/cutoff/cutoff.sh"),
            "".to_string(),
        );
        let result = cutoff::do_cutoff(&mut context);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cutoff_error() {
        let mut context = ContextImpl::new();
        context.set_command_stderr(
            context.root_dir().join("usr/share/cutoff/cutoff.sh"),
            "error".to_string(),
        );
        let result = cutoff::do_cutoff(&mut context);
        let err = result.unwrap_err();
        let message = err.root_cause();
        assert_eq!(format!("{}", message), "cutoff fail");
    }

    #[test]
    fn test_cutoff_battery_reboot() {
        let mut context = ContextImpl::new();
        context.set_command_stdout("ectool", "".to_string());
        context.set_command_stdout(
            context
                .root_dir()
                .join("usr/share/cutoff/display_wipe_message.sh"),
            "".to_string(),
        );
        let dir = context.root_dir().join("usr/share/cutoff");
        fs::create_dir_all(dir.clone()).unwrap();
        let config = json!({
            "CUTOFF_METHOD": "reboot",
            "CUTOFF_AC_STATE": "connect_ac"
        });
        fs::write(dir.join("cutoff.json"), config.to_string()).unwrap();

        let result = cutoff::cutoff_battery(&mut context);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cutoff_battery_ectool_cutoff() {
        let mut context = ContextImpl::new();
        context.set_command_stdout("ectool", "".to_string());
        context.set_command_stdout("ectool", "".to_string());
        context.set_command_stdout(
            context
                .root_dir()
                .join("usr/share/cutoff/display_wipe_message.sh"),
            "".to_string(),
        );
        let dir = context.root_dir().join("usr/share/cutoff");
        fs::create_dir_all(dir.clone()).unwrap();
        let config = json!({
            "CUTOFF_METHOD": "ectool_cutoff",
            "CUTOFF_AC_STATE": "connect_ac"
        });
        fs::write(dir.join("cutoff.json"), config.to_string()).unwrap();

        let result = cutoff::cutoff_battery(&mut context);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cutoff_battery_battery_cutoff() {
        let mut context = ContextImpl::new();
        context.set_command_stdout("ectool", "".to_string());
        context.set_command_stdout("crossystem", "".to_string());
        context.set_command_stdout(
            context
                .root_dir()
                .join("usr/share/cutoff/display_wipe_message.sh"),
            "".to_string(),
        );
        let dir = context.root_dir().join("usr/share/cutoff");
        fs::create_dir_all(dir.clone()).unwrap();
        let config = json!({
            "CUTOFF_METHOD": "battery_cutoff",
            "CUTOFF_AC_STATE": "connect_ac"
        });
        fs::write(dir.join("cutoff.json"), config.to_string()).unwrap();

        let result = cutoff::cutoff_battery(&mut context);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cutoff_battery_ec_hibernate() {
        let mut context = ContextImpl::new();
        context.set_command_stdout("ectool", "".to_string());
        context.set_command_stdout("ectool", "".to_string());
        context.set_command_stdout(
            context
                .root_dir()
                .join("usr/share/cutoff/display_wipe_message.sh"),
            "".to_string(),
        );
        let dir = context.root_dir().join("usr/share/cutoff");
        fs::create_dir_all(dir.clone()).unwrap();
        let config = json!({
            "CUTOFF_METHOD": "ec_hibernate",
            "CUTOFF_AC_STATE": "connect_ac"
        });
        fs::write(dir.join("cutoff.json"), config.to_string()).unwrap();

        let result = cutoff::cutoff_battery(&mut context);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cutoff_battery_shutdown() {
        let mut context = ContextImpl::new();
        context.set_command_stdout("ectool", "".to_string());
        context.set_command_stdout(
            context
                .root_dir()
                .join("usr/share/cutoff/display_wipe_message.sh"),
            "".to_string(),
        );
        let dir = context.root_dir().join("usr/share/cutoff");
        fs::create_dir_all(dir.clone()).unwrap();
        let config = json!({
            "CUTOFF_METHOD": "shutdown",
            "CUTOFF_AC_STATE": "connect_ac"
        });
        fs::write(dir.join("cutoff.json"), config.to_string()).unwrap();

        let result = cutoff::cutoff_battery(&mut context);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cutoff_battery_unknown_method() {
        let mut context = ContextImpl::new();
        context.set_command_stdout("ectool", "".to_string());
        context.set_command_stdout(
            context
                .root_dir()
                .join("usr/share/cutoff/display_wipe_message.sh"),
            "".to_string(),
        );
        let dir = context.root_dir().join("usr/share/cutoff");
        fs::create_dir_all(dir.clone()).unwrap();
        let config = json!({
            "CUTOFF_METHOD": "unknown",
            "CUTOFF_AC_STATE": "connect_ac"
        });
        fs::write(dir.join("cutoff.json"), config.to_string()).unwrap();

        let result = cutoff::cutoff_battery(&mut context);
        let err = result.unwrap_err();
        let message = err.root_cause();
        assert!(message.to_string().contains("unknown variant `unknown`"));
    }

    #[test]
    fn test_cutoff_battery_config_missing_state() {
        let mut context = ContextImpl::new();
        context.set_command_stdout("ectool", "".to_string());
        context.set_command_stdout(
            context
                .root_dir()
                .join("usr/share/cutoff/display_wipe_message.sh"),
            "".to_string(),
        );
        let dir = context.root_dir().join("usr/share/cutoff");
        fs::create_dir_all(dir.clone()).unwrap();
        let config = json!({
            "CUTOFF_METHOD": "shutdown",
        });
        fs::write(dir.join("cutoff.json"), config.to_string()).unwrap();

        let result = cutoff::cutoff_battery(&mut context);
        let err = result.unwrap_err();
        let message = err.root_cause();
        assert!(message
            .to_string()
            .contains("missing field `CUTOFF_AC_STATE`"));
    }

    #[test]
    fn test_cutoff_battery_config_missing_method() {
        let mut context = ContextImpl::new();
        context.set_command_stdout("ectool", "".to_string());
        context.set_command_stdout(
            context
                .root_dir()
                .join("usr/share/cutoff/display_wipe_message.sh"),
            "".to_string(),
        );
        let dir = context.root_dir().join("usr/share/cutoff");
        fs::create_dir_all(dir.clone()).unwrap();
        let config = json!({
            "CUTOFF_AC_STATE": "connect_ac"
        });
        fs::write(dir.join("cutoff.json"), config.to_string()).unwrap();

        let result = cutoff::cutoff_battery(&mut context);
        let err = result.unwrap_err();
        let message = err.root_cause();
        assert!(message
            .to_string()
            .contains("missing field `CUTOFF_METHOD`"));
    }
}
