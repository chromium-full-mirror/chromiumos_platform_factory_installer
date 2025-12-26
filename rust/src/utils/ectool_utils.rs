// Copyright 2024 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::fmt;

use anyhow::Result;
use regex::Regex;

use crate::system::context::Context;
use crate::utils::process_utils::StringOutput;

pub enum ChargeControl {
    Normal,
    Discharge,
    Idle,
}

impl fmt::Display for ChargeControl {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ChargeControl::Normal => write!(f, "normal"),
            ChargeControl::Discharge => write!(f, "discharge"),
            ChargeControl::Idle => write!(f, "idle"),
        }
    }
}

pub enum RebootCommand {
    Cancel,
    RO,
    RW,
    Cold,
    DisableJump,
    Hibernate,
    HibernateClearApOff,
    ColdApOff,
}

impl fmt::Display for RebootCommand {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            RebootCommand::Cancel => write!(f, "cancel"),
            RebootCommand::RO => write!(f, "RO"),
            RebootCommand::RW => write!(f, "RW"),
            RebootCommand::Cold => write!(f, "cold"),
            RebootCommand::DisableJump => write!(f, "disable-jump"),
            RebootCommand::Hibernate => write!(f, "hibernate"),
            RebootCommand::HibernateClearApOff => write!(f, "hibernate-clear-ap-off"),
            RebootCommand::ColdApOff => write!(f, "cold-ap-off"),
        }
    }
}

#[derive(Debug)]
pub struct BatteryInfo {
    pub battery_percentage: i32,
    pub present_voltage: i32,
    pub is_ac_connected: bool,
}

pub fn get_battery_info(context: &mut dyn Context) -> Result<BatteryInfo> {
    /* Example:
    Battery 0 info:
      OEM name:               SMP
      Model number:           L20M3PG1
      Chemistry   :           LiP
      Serial number:          04A8
      Design capacity:        4950 mAh
      Last full charge:       4877 mAh
      Design output voltage   11520 mV
      Cycle count             10
      Present voltage         12461 mV
      Present current         -421 mA
      Remaining capacity      4501 mAh
      Desired voltage         12900 mV
      Desired current         4373 mA
      Flags                   0x07 AC_PRESENT BATT_PRESENT DISCHARGING
    */
    let stdout = context.command("ectool").arg("battery").output()?.stdout();
    let mut re = Regex::new(r".*Design capacity:* *([0-9]+) mAh.*")?;
    let Some(design_capacity) = re.captures(&stdout) else {
        eprintln!("Unexpected output from ectool battery: {}", stdout);
        anyhow::bail!("Design capacity not found.");
    };
    re = Regex::new(r".*Remaining capacity *([0-9]+) mAh.*")?;
    let Some(remaining_capacity) = re.captures(&stdout) else {
        eprintln!("Unexpected output from ectool battery: {}", stdout);
        anyhow::bail!("Remaining capacity not found.");
    };
    re = Regex::new(r".*Present voltage *([0-9]+) mV.*")?;
    let Some(present_voltage) = re.captures(&stdout) else {
        eprintln!("Unexpected output from ectool battery: {}", stdout);
        anyhow::bail!("Present voltage not found.");
    };
    Ok(BatteryInfo {
        battery_percentage: remaining_capacity[1].parse::<i32>()? * 100
            / design_capacity[1].parse::<i32>()?,
        present_voltage: present_voltage[1].parse::<i32>()?,
        is_ac_connected: stdout.contains("AC_PRESENT"),
    })
}

pub fn charge_control(mode: ChargeControl, context: &mut dyn Context) -> Result<()> {
    let info = get_battery_info(context)?;
    if !info.is_ac_connected {
        eprintln!("Can't modify the charge mode since AC is disconnected.");
        return Ok(());
    }
    let output = context
        .command("ectool")
        .args(["chargecontrol", &mode.to_string()])
        .output()?;
    if !output.status.success() {
        anyhow::bail!("Charge control fail: {}", output.stderr());
    }
    Ok(())
}

// Reboot ec with command.
// Check src/platform/ec/util/ectool.cc
pub fn reboot_ec(cmd: RebootCommand, context: &mut dyn Context) -> Result<()> {
    let output = context
        .command("ectool")
        .args(["reboot_ec", &cmd.to_string(), "at-shutdown"])
        .output()?;
    if !output.status.success() {
        anyhow::bail!("Reboot ec fail: {}", output.stderr());
    }
    Ok(())
}

#[cfg(test)]
mod tests {

    use crate::system::context::ContextImpl;
    use crate::utils::ectool_utils::{self, ChargeControl, RebootCommand};

    #[test]
    fn test_get_battery_info_success() {
        let mut context = ContextImpl::new();
        context.set_command_stdout(
            "ectool",
            "Design capacity: 100 mAh Remaining capacity 70 mAh Present voltage 10 mV AC_PRESENT"
                .to_string(),
        );
        let result = ectool_utils::get_battery_info(&mut context).unwrap();
        assert_eq!(result.battery_percentage, 70);
        assert_eq!(result.present_voltage, 10);
        assert!(result.is_ac_connected);
    }

    #[test]
    fn test_get_battery_info_design_capacity_not_found() {
        let mut context = ContextImpl::new();
        context.set_command_stdout(
            "ectool",
            "Remaining capacity 70 mAh Present voltage 10 mV".to_string(),
        );
        let result = ectool_utils::get_battery_info(&mut context);
        let err = result.unwrap_err();
        let message = err.root_cause();
        assert_eq!(format!("{}", message), "Design capacity not found.");
    }

    #[test]
    fn test_get_battery_info_remaining_capacity_not_found() {
        let mut context = ContextImpl::new();
        context.set_command_stdout(
            "ectool",
            "Design capacity: 100 mAh Present voltage 10 mV".to_string(),
        );
        let result = ectool_utils::get_battery_info(&mut context);
        let err = result.unwrap_err();
        let message = err.root_cause();
        assert_eq!(format!("{}", message), "Remaining capacity not found.");
    }

    #[test]
    fn test_get_battery_info_voltage_not_found() {
        let mut context = ContextImpl::new();
        context.set_command_stdout(
            "ectool",
            "Design capacity: 100 mAh Remaining capacity 70 mAh 10 mV".to_string(),
        );
        let result = ectool_utils::get_battery_info(&mut context);
        let err = result.unwrap_err();
        let message = err.root_cause();
        assert_eq!(format!("{}", message), "Present voltage not found.");
    }

    #[test]
    fn test_charge_control_success() {
        let mut context = ContextImpl::new();
        context.set_command_stdout(
            "ectool",
            "Design capacity: 100 mAh Remaining capacity 100 mAh Present voltage 10 mV AC_PRESENT"
                .to_string(),
        );
        context.set_command_stdout("ectool", "".to_string());
        let result = ectool_utils::charge_control(ChargeControl::Idle, &mut context);
        assert!(result.is_ok());
    }

    #[test]
    fn test_charge_control_fail() {
        let mut context = ContextImpl::new();
        context.set_command_stdout(
            "ectool",
            "Design capacity: 100 mAh Remaining capacity 100 mAh Present voltage 10 mV AC_PRESENT"
                .to_string(),
        );
        context.set_command_stderr("ectool", "stderr".to_string());
        let result = ectool_utils::charge_control(ChargeControl::Idle, &mut context);
        let err = result.unwrap_err();
        let message = err.root_cause();
        assert_eq!(format!("{}", message), "Charge control fail: stderr");
    }

    #[test]
    fn test_charge_control_ac_disconnected_skip() {
        let mut context = ContextImpl::new();
        context.set_command_stdout(
            "ectool",
            "Design capacity: 100 mAh Remaining capacity 100 mAh Present voltage 10 mV".to_string(),
        );
        let result = ectool_utils::charge_control(ChargeControl::Idle, &mut context);
        assert!(result.is_ok());
    }

    #[test]
    fn test_reboot_ec_success() {
        let mut context = ContextImpl::new();
        context.set_command_stdout("ectool", "".to_string());
        let result = ectool_utils::reboot_ec(RebootCommand::ColdApOff, &mut context);
        assert!(result.is_ok());
    }

    #[test]
    fn test_reboot_ec_fail() {
        let mut context = ContextImpl::new();
        context.set_command_stderr("ectool", "stderr".to_string());
        let result = ectool_utils::reboot_ec(RebootCommand::ColdApOff, &mut context);
        let err = result.unwrap_err();
        let message = err.root_cause();
        assert_eq!(format!("{}", message), "Reboot ec fail: stderr");
    }
}
