// Copyright 2024 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

// This file controls battery power level and performs required battery
// cutoff protection by sending commands to EC with ectool.

use std::fs;
use std::process::Stdio;

use anyhow::{self, Result};

use crate::cutoff::config::{CutoffConfig, CutoffMethod};
use crate::system::context::Context;
use crate::utils::crossystem_utils;
use crate::utils::ectool_utils::{self, BatteryInfo, ChargeControl};
#[cfg(not(test))]
use crate::utils::sys_utils;
#[cfg(test)]
use crate::utils::sys_utils::mock as sys_utils;

pub mod config;

const CUTOFF_DIR: &str = "usr/share/cutoff";

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

pub fn check_battery(context: &mut dyn Context) -> Result<()> {
    // Needed by 'ectool battery'.
    fs::create_dir_all(context.root_dir().join("var/lib/power_manager"))?;
    context.command("modprobe").arg("i2c_dev").output()?;

    let config = get_cutoff_config(context)?;
    eprintln!("{:?}", config);
    anyhow::ensure!(
        config.cutoff_battery_max_percentage >= 0,
        "The max percentage should not be negative.",
    );
    anyhow::ensure!(
        config.cutoff_battery_min_percentage <= 100,
        "The min percentage should not be larger than 100.",
    );
    let info = ectool_utils::get_battery_info(context)?;
    if is_needed_charge(&config, &info) {
        charge_battery(&config, context)?;
    }
    if is_needed_discharge(&config, &info) {
        discharge_battery(&config, context)?;
    }

    Ok(())
}

fn charge_battery(config: &CutoffConfig, context: &mut dyn Context) -> Result<()> {
    wait_ac_connected(context)?;
    ectool_utils::charge_control(ChargeControl::Normal, context)?;
    display_message("charging", context)?;

    let mut last_battery = i32::MIN;
    let mut last_voltage = i32::MIN;
    loop {
        sys_utils::sleep(1.0)?;
        let info = ectool_utils::get_battery_info(context)?;
        if !is_needed_charge(config, &info) {
            break;
        }
        if info.battery_percentage < config.cutoff_battery_min_percentage {
            if info.battery_percentage > last_battery {
                last_battery = info.battery_percentage;
                println!(
                    "Check battery percentage, current:{}, target:{}",
                    info.battery_percentage, config.cutoff_battery_min_percentage
                );
            }
        }
        if info.present_voltage < config.cutoff_battery_min_voltage {
            if info.present_voltage > last_voltage {
                last_voltage = info.present_voltage;
                println!(
                    "Check voltage, current:{}, target:{}",
                    info.present_voltage, config.cutoff_battery_min_voltage
                );
            }
        }
    }
    Ok(())
}

fn discharge_battery(config: &CutoffConfig, context: &mut dyn Context) -> Result<()> {
    ectool_utils::charge_control(ChargeControl::Discharge, context)?;

    // Use stressapptest to discharge battery faster.
    // It may crash the system if it use too much memory on Factory Shim.
    // Run stressapptest in background without print anything.
    let mut child = context
        .command("stressapptest")
        .stdout(Stdio::null())
        .args(["-M", "128", "-s", "1000000"])
        .spawn()?;
    display_message("discharging", context)?;

    let mut last_battery = i32::MAX;
    let mut last_voltage = i32::MAX;
    loop {
        sys_utils::sleep(1.0)?;
        let info = ectool_utils::get_battery_info(context)?;
        if !is_needed_discharge(config, &info) {
            break;
        }
        if info.battery_percentage > config.cutoff_battery_max_percentage {
            if info.battery_percentage < last_battery {
                last_battery = info.battery_percentage;
                println!(
                    "Check battery percentage, current:{}, target:{}",
                    info.battery_percentage, config.cutoff_battery_max_percentage
                );
            }
        }
        if info.present_voltage > config.cutoff_battery_max_voltage {
            if info.present_voltage < last_voltage {
                last_voltage = info.present_voltage;
                println!(
                    "Check voltage, current:{}, target:{}",
                    info.present_voltage, config.cutoff_battery_max_voltage
                );
            }
        }
    }
    child.kill()?;
    Ok(())
}

fn is_needed_charge(config: &CutoffConfig, info: &BatteryInfo) -> bool {
    return info.battery_percentage < config.cutoff_battery_min_percentage
        || info.present_voltage < config.cutoff_battery_min_voltage;
}

fn is_needed_discharge(config: &CutoffConfig, info: &BatteryInfo) -> bool {
    return info.battery_percentage > config.cutoff_battery_max_percentage
        || info.present_voltage > config.cutoff_battery_max_voltage;
}

fn wait_ac_connected(context: &mut dyn Context) -> Result<()> {
    if !ectool_utils::get_battery_info(context)?.is_ac_connected {
        display_message("connect_ac", context)?;
    }
    while !ectool_utils::get_battery_info(context)?.is_ac_connected {
        sys_utils::sleep(0.5)?;
    }
    Ok(())
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

    #[test]
    fn test_check_battery_ok() {
        let mut context = ContextImpl::new();
        context.set_command_stdout("modprobe", "".to_string());
        let dir = context.root_dir().join("usr/share/cutoff");
        fs::create_dir_all(dir.clone()).unwrap();
        let config = json!({
            "CUTOFF_AC_STATE": "connect_ac",
            "CUTOFF_METHOD": "shutdown",
        });
        fs::write(dir.join("cutoff.json"), config.to_string()).unwrap();
        mock_battery_info(1, true, &mut context);

        let result = cutoff::check_battery(&mut context);
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_battery_max_percentage_negative() {
        let mut context = ContextImpl::new();
        context.set_command_stdout("modprobe", "".to_string());
        let dir = context.root_dir().join("usr/share/cutoff");
        fs::create_dir_all(dir.clone()).unwrap();
        let config = json!({
            "CUTOFF_AC_STATE": "connect_ac",
            "CUTOFF_METHOD": "shutdown",
            "CUTOFF_BATTERY_MAX_PERCENTAGE": -1,
        });
        fs::write(dir.join("cutoff.json"), config.to_string()).unwrap();
        mock_battery_info(1, true, &mut context);

        let result = cutoff::check_battery(&mut context);
        let err = result.unwrap_err();
        let message = err.root_cause();
        assert_eq!(
            format!("{}", message),
            "The max percentage should not be negative."
        );
    }

    #[test]
    fn test_check_battery_min_percentage_larger_than_100() {
        let mut context = ContextImpl::new();
        context.set_command_stdout("modprobe", "".to_string());
        let dir = context.root_dir().join("usr/share/cutoff");
        fs::create_dir_all(dir.clone()).unwrap();
        let config = json!({
            "CUTOFF_AC_STATE": "connect_ac",
            "CUTOFF_METHOD": "shutdown",
            "CUTOFF_BATTERY_MIN_PERCENTAGE": 101,
        });
        fs::write(dir.join("cutoff.json"), config.to_string()).unwrap();
        mock_battery_info(1, true, &mut context);

        let result = cutoff::check_battery(&mut context);
        let err = result.unwrap_err();
        let message = err.root_cause();
        assert_eq!(
            format!("{}", message),
            "The min percentage should not be larger than 100."
        );
    }

    #[test]
    fn test_check_battery_charge() {
        let mut context = ContextImpl::new();
        context.set_command_stdout("modprobe", "".to_string());
        let dir = context.root_dir().join("usr/share/cutoff");
        fs::create_dir_all(dir.clone()).unwrap();
        let config = json!({
            "CUTOFF_AC_STATE": "connect_ac",
            "CUTOFF_METHOD": "shutdown",
            "CUTOFF_BATTERY_MIN_PERCENTAGE": 70,
        });
        fs::write(dir.join("cutoff.json"), config.to_string()).unwrap();
        mock_battery_info(60, false, &mut context); // Need to charge
        mock_battery_info(60, false, &mut context); // Need to connect ac
        context.set_command_stdout(
            context
                .root_dir()
                .join("usr/share/cutoff/display_wipe_message.sh"),
            "connect_ac".to_string(),
        );
        mock_battery_info(60, true, &mut context); // Ac connected
        context.set_command_stdout("ectool", "normal".to_string());
        context.set_command_stdout(
            context
                .root_dir()
                .join("usr/share/cutoff/display_wipe_message.sh"),
            "charging".to_string(),
        );
        mock_battery_info(60, true, &mut context); // Wait charge
        mock_battery_info(70, true, &mut context); // Charge completed

        let result = cutoff::check_battery(&mut context);
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_battery_discharge() {
        let mut context = ContextImpl::new();
        context.set_command_stdout("modprobe", "".to_string());
        let dir = context.root_dir().join("usr/share/cutoff");
        fs::create_dir_all(dir.clone()).unwrap();
        let config = json!({
            "CUTOFF_AC_STATE": "connect_ac",
            "CUTOFF_METHOD": "shutdown",
            "CUTOFF_BATTERY_MAX_PERCENTAGE": 70,
        });
        fs::write(dir.join("cutoff.json"), config.to_string()).unwrap();
        mock_battery_info(80, true, &mut context); // Need to discharge
        context.set_command_stdout(
            context
                .root_dir()
                .join("usr/share/cutoff/display_wipe_message.sh"),
            "remove_ac".to_string(),
        );
        context.set_command_stdout("ectool", "discharge".to_string());
        context.set_command_stdout("stressapptest", "".to_string());
        context.set_command_stdout(
            context
                .root_dir()
                .join("usr/share/cutoff/display_wipe_message.sh"),
            "discharging".to_string(),
        );
        mock_battery_info(80, true, &mut context); // Wait discharge
        mock_battery_info(70, true, &mut context); // Discharge completed

        let result = cutoff::check_battery(&mut context);
        assert!(result.is_ok());
    }

    fn mock_battery_info(percentage: i32, is_ac_connected: bool, context: &mut ContextImpl) {
        let ac = match is_ac_connected {
            true => "AC_PRESENT",
            false => "",
        };
        context.set_command_stdout(
            "ectool",
            format!(
                "Design capacity: 100 mAh Remaining capacity {} mAh Present voltage 10 mV {}",
                percentage, ac
            ),
        );
    }
}
