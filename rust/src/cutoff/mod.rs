// Copyright 2024 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

// This file controls battery power level and performs required battery
// cutoff protection by sending commands to EC with ectool.

use std::fs;
use std::process::Stdio;

use anyhow::{self, Result};

use crate::cutoff::config::{CutoffAcState, CutoffConfig, CutoffMethod};
use crate::system::context::Context;
use crate::utils::ectool_utils::{self, BatteryInfo, ChargeControl, RebootCommand};
use crate::utils::process_utils::StringOutput;
use crate::utils::qrcode_utils::QRcode;
#[cfg(test)]
use crate::utils::sys_utils::mock as sys_utils;
#[cfg(test)]
use crate::utils::terminal_utils::mock as terminal_utils;
use crate::utils::{crossystem_utils, qrcode_utils, vpd_utils};
#[cfg(not(test))]
use crate::utils::{sys_utils, terminal_utils};

pub mod config;

const CUTOFF_DIR: &str = "usr/share/cutoff";
const TOOLKIT_DIR: &str = "usr/local/factory";
#[cfg(test)]
const QRCODE_SIZE: u32 = 100;
#[cfg(not(test))]
const QRCODE_SIZE: u32 = 232;

pub fn do_cutoff(context: &mut dyn Context) -> Result<()> {
    let output = context.command("activate_date").arg("--clean").output()?;
    if !output.status.success() {
        anyhow::bail!("Clean activate date fail. error: {}", output.stderr())
    }
    vpd_utils::delete_rw_value("recovery_count", context)?;
    let config = get_cutoff_config(context)?;
    eprintln!("{:?}", config);
    check_battery(context)?;
    ectool_utils::charge_control(ChargeControl::Idle, context)?;
    check_ac_state(context)?;
    display_qrcode(&config.qrcode_info, context)?;
    println!("Press {} to continue", config.continue_key);
    terminal_utils::wait_keys(&config.continue_key)?;
    cutoff_battery(context)?;
    Ok(())
}

pub fn get_cutoff_config(context: &mut dyn Context) -> Result<CutoffConfig> {
    let shim_path = context.root_dir().join(CUTOFF_DIR).join("cutoff.json");
    let toolkit_path = context
        .root_dir()
        .join(TOOLKIT_DIR)
        .join("sh/cutoff")
        .join("cutoff.json");
    let private_overlay_path = context
        .root_dir()
        .join(TOOLKIT_DIR)
        .join("py/config")
        .join("cutoff.json");
    let path = if shim_path.exists() {
        shim_path
    } else if toolkit_path.exists() {
        toolkit_path
    } else if private_overlay_path.exists() {
        private_overlay_path
    } else {
        anyhow::bail!("Cutoff config not found.")
    };

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
            CutoffMethod::BatteryCutoff => {
                crossystem_utils::set_battery_cutoff_request(1, context)?;
                sys_utils::sleep(3.0)?;
                sys_utils::reboot()
            }
            CutoffMethod::EcHibernate => {
                ectool_utils::reboot_ec(RebootCommand::Hibernate, context)?;
                sys_utils::shutdown()
            }
            // By default we shutdown the device without doing anything.
            CutoffMethod::Shutdown => {
                if crossystem_utils::get_value("mainfw_type", context)? == "recovery" {
                    ectool_utils::reboot_ec(RebootCommand::ColdApOff, context)?;
                }
                sys_utils::shutdown()
            }
        };
        if result.is_ok() {
            return Ok(());
        }
    }
    Ok(())
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
    while !ectool_utils::get_battery_info(context)
        .map(|info| info.is_ac_connected)
        .unwrap_or(false)
    {
        sys_utils::sleep(0.5)?;
    }
    Ok(())
}

fn wait_ac_disconnected(context: &mut dyn Context) -> Result<()> {
    if ectool_utils::get_battery_info(context)?.is_ac_connected {
        display_message("remove_ac", context)?;
    }
    while ectool_utils::get_battery_info(context)
        .map(|info| info.is_ac_connected)
        .unwrap_or(true)
    {
        sys_utils::sleep(0.5)?;
    }
    Ok(())
}

pub fn check_ac_state(context: &mut dyn Context) -> Result<()> {
    let config = get_cutoff_config(context)?;
    match config.cutoff_ac_state {
        CutoffAcState::ConnectAc => wait_ac_connected(context)?,
        CutoffAcState::RemoveAc => wait_ac_disconnected(context)?,
    };
    Ok(())
}

fn display_message(message: &str, context: &mut dyn Context) -> Result<()> {
    let shim_path = context
        .root_dir()
        .join(CUTOFF_DIR)
        .join("display_wipe_message.sh");
    let toolkit_path = context
        .root_dir()
        .join(TOOLKIT_DIR)
        .join("sh/cutoff")
        .join("display_wipe_message.sh");
    let path = if toolkit_path.exists() {
        toolkit_path
    } else if shim_path.exists() {
        shim_path
    } else {
        anyhow::bail!("Cannot find display_wipe_message.sh");
    };
    let mut child = context
        .command(&path.to_str().unwrap())
        .arg(message)
        .spawn()?;
    child.wait()?;
    Ok(())
}

pub fn display_qrcode(qrcode_info: &str, context: &mut dyn Context) -> Result<()> {
    let mut contents = Vec::<String>::new();
    if !qrcode_info.is_empty() {
        // Example: convert "hwid serial_number,hwid" to ["<HWID> <SERIAL_NUMBER>", "<HWID>"]
        eprintln!("display info = {}", qrcode_info);
        for info in qrcode_info.split(",") {
            let mut display_string = vec![];
            for key in info.split(" ") {
                let value = match key {
                    "hwid" => crossystem_utils::get_value("hwid", context),
                    "serial_number" => vpd_utils::get_ro_value("serial_number", context),
                    "mlb_serial_number" => vpd_utils::get_ro_value("mlb_serial_number", context),
                    "wifi_mac0" => vpd_utils::get_ro_value("wifi_mac0", context),
                    "service_tag" => vpd_utils::get_ro_value("service_tag", context),
                    &_ => anyhow::bail!("Unknown key: {}", key),
                };
                display_string.push(value?);
            }
            contents.push(display_string.join(" "));
        }
        eprintln!("display string = {:?}", contents);
    }
    for (i, content) in contents.iter().enumerate() {
        terminal_utils::clear();
        if i < contents.len() - 1 {
            eprintln!("Press enter to continue...");
        }
        qrcode_utils::display_qrcode(
            &QRcode {
                size: QRCODE_SIZE,
                content: content.to_string(),
                position: None,
            },
            context,
        )?;
        if i < contents.len() - 1 {
            terminal_utils::wait_enter()?;
        }
    }
    Ok(())
}

pub fn failed(context: &mut dyn Context) -> Result<()> {
    display_message("cutoff_failed", context)?;
    sys_utils::sleep(86400.0)?;
    anyhow::bail!("Cutoff failed.")
}

#[cfg(test)]
mod tests {
    use std::fs::{self, File};

    use serde_json::json;

    use crate::cutoff;
    use crate::system::context::{Context, ContextImpl};

    #[test]
    fn test_cutoff_success() {
        let mut context = ContextImpl::new();
        context.set_command_stdout("activate_date", "".to_string());
        context.set_command_stderr("vpd", "".to_string());
        context.set_command_stdout("modprobe", "".to_string());
        create_cutoff_files(
            json!({
                "CUTOFF_METHOD": "reboot",
                "CUTOFF_AC_STATE": "remove_ac"
            }),
            &mut context,
        );
        mock_battery_info(60, true, &mut context); // No need to charge or discharge
        mock_battery_info(60, true, &mut context); // For charge control
        context.set_command_stdout("ectool", "idle".to_string());
        mock_battery_info(60, true, &mut context); // Wait ac disconnected
        context.set_command_stdout(
            context
                .root_dir()
                .join("usr/share/cutoff/display_wipe_message.sh"),
            "remove ac".to_string(),
        );
        mock_battery_info(60, false, &mut context); // ac disconnected
        mock_battery_info(60, true, &mut context); // For charge control
        context.set_command_stdout("ectool", "normal".to_string());
        context.set_command_stdout(
            context
                .root_dir()
                .join("usr/share/cutoff/display_wipe_message.sh"),
            "cutting off".to_string(),
        );
        let result = cutoff::do_cutoff(&mut context);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cutoff_error() {
        let mut context = ContextImpl::new();
        context.set_command_stderr("activate_date", "error".to_string());
        let result = cutoff::do_cutoff(&mut context);
        assert!(result.is_err());
    }

    #[test]
    fn test_cutoff_battery_reboot() {
        let mut context = ContextImpl::new();
        mock_battery_info(60, true, &mut context); // For charge control
        context.set_command_stdout("ectool", "normal".to_string());
        context.set_command_stdout(
            context
                .root_dir()
                .join("usr/share/cutoff/display_wipe_message.sh"),
            "".to_string(),
        );
        create_cutoff_files(
            json!({
                "CUTOFF_METHOD": "reboot",
                "CUTOFF_AC_STATE": "connect_ac"
            }),
            &mut context,
        );

        let result = cutoff::cutoff_battery(&mut context);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cutoff_battery_battery_cutoff() {
        let mut context = ContextImpl::new();
        mock_battery_info(60, true, &mut context); // For charge control
        context.set_command_stdout("ectool", "normal".to_string());
        context.set_command_stdout("crossystem", "".to_string());
        context.set_command_stdout(
            context
                .root_dir()
                .join("usr/share/cutoff/display_wipe_message.sh"),
            "".to_string(),
        );
        create_cutoff_files(
            json!({
                "CUTOFF_METHOD": "battery_cutoff",
                "CUTOFF_AC_STATE": "connect_ac"
            }),
            &mut context,
        );

        let result = cutoff::cutoff_battery(&mut context);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cutoff_battery_ec_hibernate() {
        let mut context = ContextImpl::new();
        mock_battery_info(60, true, &mut context); // For charge control
        context.set_command_stdout("ectool", "normal".to_string());
        context.set_command_stdout("ectool", "".to_string());
        context.set_command_stdout(
            context
                .root_dir()
                .join("usr/share/cutoff/display_wipe_message.sh"),
            "".to_string(),
        );
        create_cutoff_files(
            json!({
                "CUTOFF_METHOD": "ec_hibernate",
                "CUTOFF_AC_STATE": "connect_ac"
            }),
            &mut context,
        );

        let result = cutoff::cutoff_battery(&mut context);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cutoff_battery_shutdown() {
        let mut context = ContextImpl::new();
        mock_battery_info(60, true, &mut context); // For charge control
        context.set_command_stdout("ectool", "normal".to_string());
        context.set_command_stdout("crossystem", "recovery".to_string());
        context.set_command_stdout("ectool", "".to_string());
        context.set_command_stdout(
            context
                .root_dir()
                .join("usr/share/cutoff/display_wipe_message.sh"),
            "".to_string(),
        );
        create_cutoff_files(
            json!({
                "CUTOFF_METHOD": "shutdown",
                "CUTOFF_AC_STATE": "connect_ac"
            }),
            &mut context,
        );

        let result = cutoff::cutoff_battery(&mut context);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cutoff_battery_unknown_method() {
        let mut context = ContextImpl::new();
        mock_battery_info(60, true, &mut context); // For charge control
        context.set_command_stdout("ectool", "normal".to_string());
        context.set_command_stdout(
            context
                .root_dir()
                .join("usr/share/cutoff/display_wipe_message.sh"),
            "".to_string(),
        );
        create_cutoff_files(
            json!({
                "CUTOFF_METHOD": "unknown",
                "CUTOFF_AC_STATE": "connect_ac"
            }),
            &mut context,
        );

        let result = cutoff::cutoff_battery(&mut context);
        let err = result.unwrap_err();
        let message = err.root_cause();
        assert!(message.to_string().contains("unknown variant `unknown`"));
    }

    #[test]
    fn test_cutoff_battery_config_missing_state() {
        let mut context = ContextImpl::new();
        mock_battery_info(60, true, &mut context); // For charge control
        context.set_command_stdout("ectool", "normal".to_string());
        context.set_command_stdout(
            context
                .root_dir()
                .join("usr/share/cutoff/display_wipe_message.sh"),
            "".to_string(),
        );
        create_cutoff_files(
            json!({
                "CUTOFF_METHOD": "shutdown",
            }),
            &mut context,
        );

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
        mock_battery_info(60, true, &mut context); // For charge control
        context.set_command_stdout("ectool", "normal".to_string());
        context.set_command_stdout(
            context
                .root_dir()
                .join("usr/share/cutoff/display_wipe_message.sh"),
            "".to_string(),
        );
        create_cutoff_files(
            json!({
                "CUTOFF_AC_STATE": "connect_ac",
            }),
            &mut context,
        );

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
        create_cutoff_files(
            json!({
                "CUTOFF_AC_STATE": "connect_ac",
                "CUTOFF_METHOD": "shutdown",
            }),
            &mut context,
        );
        mock_battery_info(1, true, &mut context);

        let result = cutoff::check_battery(&mut context);
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_battery_max_percentage_negative() {
        let mut context = ContextImpl::new();
        context.set_command_stdout("modprobe", "".to_string());
        create_cutoff_files(
            json!({
                "CUTOFF_AC_STATE": "connect_ac",
                "CUTOFF_METHOD": "shutdown",
                "CUTOFF_BATTERY_MAX_PERCENTAGE": -1,
            }),
            &mut context,
        );
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
        create_cutoff_files(
            json!({
                "CUTOFF_AC_STATE": "connect_ac",
                "CUTOFF_METHOD": "shutdown",
                "CUTOFF_BATTERY_MIN_PERCENTAGE": 101,
            }),
            &mut context,
        );
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
        create_cutoff_files(
            json!({
                "CUTOFF_AC_STATE": "connect_ac",
                "CUTOFF_METHOD": "shutdown",
                "CUTOFF_BATTERY_MIN_PERCENTAGE": 70,
            }),
            &mut context,
        );
        mock_battery_info(60, false, &mut context); // Need to charge
        mock_battery_info(60, false, &mut context); // Need to connect ac
        context.set_command_stdout(
            context
                .root_dir()
                .join("usr/share/cutoff/display_wipe_message.sh"),
            "connect_ac".to_string(),
        );
        mock_battery_info(60, true, &mut context); // Ac connected
        mock_battery_info(60, true, &mut context); // For charge control
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
        create_cutoff_files(
            json!({
                "CUTOFF_AC_STATE": "connect_ac",
                "CUTOFF_METHOD": "shutdown",
                "CUTOFF_BATTERY_MAX_PERCENTAGE": 70,
            }),
            &mut context,
        );
        mock_battery_info(80, true, &mut context); // Need to discharge
        context.set_command_stdout(
            context
                .root_dir()
                .join("usr/share/cutoff/display_wipe_message.sh"),
            "remove_ac".to_string(),
        );
        mock_battery_info(60, true, &mut context); // For charge control
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

    #[test]
    fn test_display_qrcode() {
        let mut context = ContextImpl::new();
        context.set_command_stdout("modprobe", "".to_string());
        prepare_qrcode_files(&mut context, "frecon_id");
        context.set_command_stdout("crossystem", "HWID".to_string());
        context.set_command_stdout("vpd", "SN".to_string());
        context.set_command_stdout("vpd", "WIFI".to_string());
        context.set_command_stdout("vpd", "MLBSN".to_string());
        context.set_command_stdout("vpd", "TAG".to_string());
        context.set_command_stdout("bash", "".to_string());
        context.set_tempfile("qrcode.png".to_string());

        let result = cutoff::display_qrcode(
            "hwid serial_number wifi_mac0 mlb_serial_number service_tag",
            &mut context,
        );
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
        context.set_command_stdout("modprobe", "".to_string());
        prepare_qrcode_files(&mut context, "frecon_id");
        context.set_command_stdout("vpd", "SN".to_string());
        context.set_command_stdout("crossystem", "HWID".to_string());
        context.set_command_stdout("bash", "".to_string());
        context.set_command_stdout("bash", "".to_string());
        context.set_tempfile("qrcode.png".to_string());
        context.set_tempfile("qrcode1.png".to_string());

        let result = cutoff::display_qrcode("serial_number,hwid", &mut context);
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
        context.set_command_stdout("modprobe", "".to_string());
        prepare_qrcode_files(&mut context, "frecon_id");

        let result = cutoff::display_qrcode("abc", &mut context);
        let err = result.unwrap_err();
        let message = err.root_cause();
        assert_eq!(format!("{}", message), "Unknown key: abc");
    }

    #[test]
    fn test_cutoff_use_toolkit_path() {
        let mut context = ContextImpl::new();
        mock_battery_info(60, true, &mut context); // For charge control
        context.set_command_stdout("ectool", "normal".to_string());
        context.set_command_stdout(
            context
                .root_dir()
                .join("usr/local/factory/sh/cutoff/display_wipe_message.sh"),
            "".to_string(),
        );
        let dir = context.root_dir().join("usr/local/factory/sh/cutoff");
        fs::create_dir_all(dir.clone()).unwrap();
        let config = json!({
            "CUTOFF_METHOD": "reboot",
            "CUTOFF_AC_STATE": "connect_ac"
        });
        fs::write(dir.join("cutoff.json"), config.to_string()).unwrap();
        File::create(dir.join("display_wipe_message.sh")).unwrap();

        let result = cutoff::cutoff_battery(&mut context);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cutoff_use_private_overlay_path() {
        let mut context = ContextImpl::new();
        mock_battery_info(60, true, &mut context); // For charge control
        context.set_command_stdout("ectool", "normal".to_string());
        context.set_command_stdout(
            context
                .root_dir()
                .join("usr/local/factory/sh/cutoff/display_wipe_message.sh"),
            "".to_string(),
        );
        let dir = context.root_dir().join("usr/local/factory/py/config");
        fs::create_dir_all(dir.clone()).unwrap();
        let config = json!({
            "CUTOFF_METHOD": "reboot",
            "CUTOFF_AC_STATE": "connect_ac"
        });
        fs::write(dir.join("cutoff.json"), config.to_string()).unwrap();
        let dir = context.root_dir().join("usr/local/factory/sh/cutoff");
        fs::create_dir_all(dir.clone()).unwrap();
        File::create(dir.join("display_wipe_message.sh")).unwrap();

        let result = cutoff::cutoff_battery(&mut context);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cutoff_config_not_exist() {
        let mut context = ContextImpl::new();
        mock_battery_info(60, true, &mut context); // For charge control
        context.set_command_stdout("ectool", "normal".to_string());
        context.set_command_stdout(
            context
                .root_dir()
                .join("usr/share/cutoff/display_wipe_message.sh"),
            "".to_string(),
        );
        let dir = context.root_dir().join("usr/share/cutoff");
        fs::create_dir_all(dir.clone()).unwrap();
        File::create(dir.join("display_wipe_message.sh")).unwrap();

        let result = cutoff::cutoff_battery(&mut context);

        let err = result.unwrap_err();
        let message = err.root_cause();
        assert_eq!(format!("{}", message), "Cutoff config not found.");
    }

    #[test]
    fn test_display_script_not_exist() {
        let mut context = ContextImpl::new();
        mock_battery_info(60, true, &mut context); // For charge control
        context.set_command_stdout("ectool", "normal".to_string());

        let result = cutoff::cutoff_battery(&mut context);

        let err = result.unwrap_err();
        let message = err.root_cause();
        assert_eq!(
            format!("{}", message),
            "Cannot find display_wipe_message.sh"
        );
    }

    #[test]
    fn test_failed() {
        let mut context = ContextImpl::new();
        create_cutoff_files(json!("{}"), &mut context);
        context.set_command_stdout(
            context
                .root_dir()
                .join("usr/share/cutoff/display_wipe_message.sh"),
            "failed".to_string(),
        );

        let result = cutoff::failed(&mut context);
        let err = result.unwrap_err();
        let message = err.root_cause();
        assert_eq!(format!("{}", message), "Cutoff failed.");
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

    fn prepare_qrcode_files(context: &mut ContextImpl, frecon_id: &str) {
        fs::create_dir_all(context.root_dir().join("proc").join(frecon_id).join("root")).unwrap();
        let dir = context.root_dir().join("run/frecon");
        fs::create_dir_all(dir.clone()).unwrap();
        File::create(dir.join("vt0")).unwrap();
        fs::write(dir.join("pid"), frecon_id).unwrap();
    }

    fn create_cutoff_files(config: serde_json::Value, context: &mut ContextImpl) {
        let dir = context.root_dir().join("usr/share/cutoff");
        fs::create_dir_all(dir.clone()).unwrap();
        fs::write(dir.join("cutoff.json"), config.to_string()).unwrap();
        File::create(dir.join("display_wipe_message.sh")).unwrap();
    }
}
