// Copyright 2023 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

pub mod args;
pub mod config;
pub mod factory_reset;
pub mod script_wrapper;

use anyhow::{self, Result};
use regex::{Captures, Regex};

use crate::factory_installer::config::{Action, DisplayQRcode, HttpRequest};
use crate::system::context::Context;
#[cfg(test)]
use crate::utils::http_utils::mock as http_utils;
use crate::utils::process_utils::StringOutput;
#[cfg(test)]
use crate::utils::terminal_utils::mock as terminal_utils;
use crate::utils::{crossystem_utils, qrcode_utils, vpd_utils};
#[cfg(not(test))]
use crate::utils::{http_utils, terminal_utils};
use crate::{cutoff, device};

fn reset_device(context: &mut dyn Context) -> Result<()> {
    let disk_path = device::get_fixed_device_storage(context)?;
    // Tcsd will bring up the tpm and de-own it,
    // as we are in developer/recovery mode.
    context.command("start").arg("tcsd").output()?;
    factory_reset::do_reset(&disk_path, context)
}

fn charge_battery(context: &mut dyn Context) -> Result<()> {
    cutoff::check_battery(context)
}

fn check_ac_state(context: &mut dyn Context) -> Result<()> {
    cutoff::check_ac_state(context)
}

fn http_request(request: HttpRequest) -> Result<()> {
    http_utils::post(request.url, request.post_arg)?;
    Ok(())
}

fn display_qrcode(arg: DisplayQRcode, context: &mut dyn Context) -> Result<()> {
    let hwid = crossystem_utils::get_hwid(context)?;
    let sn = vpd_utils::get_ro_value("serial_number", context)?;
    let mlb_sn = vpd_utils::get_ro_value("mlb_serial_number", context)?;
    let wifi_mac = vpd_utils::get_ro_value("wifi_mac0", context)?;
    let re = Regex::new("(<.*?>)").unwrap();

    terminal_utils::clear();
    let mut invalid_content = false;
    for mut qrcode in arg.qrcodes {
        let content = re
            .replace_all(&qrcode.content, |cap: &Captures| match &cap[0] {
                "<hwid>" => &hwid,
                "<serial_number>" => &sn,
                "<mlb_serial_number>" => &mlb_sn,
                "<wifi_mac0>" => &wifi_mac,
                _ => {
                    eprintln!("Unknown key: {}", &cap[0]);
                    invalid_content = true;
                    ""
                }
            })
            .to_string();
        if invalid_content {
            anyhow::bail!("Invalid content: {}", &qrcode.content);
        }
        qrcode.content = content;
        qrcode_utils::display_qrcode(&qrcode, context)?;
    }
    eprintln!("Press enter to continue...");
    terminal_utils::wait_enter("")?;
    Ok(())
}

fn stop_and_confirm(arg: String) -> Result<()> {
    println!("Input {} and press enter to continue...", arg);
    terminal_utils::wait_enter(&arg)
}

pub fn do_custom_reset_process(context: &mut dyn Context) -> Result<()> {
    // These are required in the reset process.
    let output = context.command("activate_date").arg("--clean").output()?;
    if !output.status.success() {
        anyhow::bail!("Clean activate date fail. error: {}", output.stderr())
    }
    vpd_utils::delete_rw_value("recovery_count", context)?;

    let actions = config::load_config(context)?;
    for action in actions {
        match action {
            Action::ResetDevice => reset_device(context),
            Action::ChargeBattery => charge_battery(context),
            Action::CheckAcState => check_ac_state(context),
            Action::HttpRequest(v) => http_request(v),
            Action::DisplayQRcode(v) => display_qrcode(v, context),
            Action::StopAndConfirm(v) => stop_and_confirm(v),
        }?;
    }
    // This is required in the reset process.
    cutoff::cutoff_battery(context)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs::{self, File};

    use serde_json::json;

    use crate::factory_installer;
    use crate::system::context::{Context, ContextImpl};

    #[test]
    fn test_do_custom_reset_process_reset_device() {
        let mut context = ContextImpl::new();
        context.set_command_stdout("activate_date", "".to_string());
        context.set_command_stderr("vpd", "already deleted".to_string());
        create_config(json!(["ResetDevice"]), &mut context);
        context.set_command_stdout("sh", "".to_string());
        context.set_command_stdout("start", "".to_string());
        context.set_command_stdout("sh", "".to_string());
        context.set_command_stdout("pvdisplay", "".to_string());
        context.set_command_stdout("sh", "".to_string());
        context.set_command_stdout("clobber-state", "".to_string());
        mock_cutoff_battery(&mut context);

        let result = factory_installer::do_custom_reset_process(&mut context);
        assert!(result.is_ok());
    }

    #[test]
    fn test_do_custom_reset_process_display_qrcode() {
        let mut context = ContextImpl::new();
        context.set_command_stdout("activate_date", "".to_string());
        context.set_command_stderr("vpd", "already deleted".to_string());
        create_config(
            json!([{"DisplayQRcode": {
                "qrcodes": [{"size": 232, "position": [10, 20],
                "content":
                    "<hwid> <serial_number> <wifi_mac0> <mlb_serial_number>"}]
            }}]),
            &mut context,
        );
        context.set_command_stdout("crossystem", "HWID".to_string());
        context.set_command_stdout("vpd", "SN".to_string());
        context.set_command_stdout("vpd", "MLBSN".to_string());
        context.set_command_stdout("vpd", "WIFI".to_string());
        prepare_files(&mut context, "frecon_id");
        mock_cutoff_battery(&mut context);

        let result = factory_installer::do_custom_reset_process(&mut context);
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
    fn test_do_custom_reset_process_display_qrcode_unknown_key() {
        let mut context = ContextImpl::new();
        context.set_command_stdout("activate_date", "".to_string());
        context.set_command_stderr("vpd", "already deleted".to_string());
        create_config(
            json!([{"DisplayQRcode": {
                "qrcodes": [{"size": 232, "position": [10, 20],
                "content":
                    "<abc> <hwid> abc"}]
            }}]),
            &mut context,
        );
        context.set_command_stdout("crossystem", "HWID".to_string());
        context.set_command_stdout("vpd", "SN".to_string());
        context.set_command_stdout("vpd", "MLBSN".to_string());
        context.set_command_stdout("vpd", "WIFI".to_string());

        let result = factory_installer::do_custom_reset_process(&mut context);
        let err = result.unwrap_err();
        let message = err.root_cause();
        assert_eq!(format!("{}", message), "Invalid content: <abc> <hwid> abc");
    }

    fn mock_cutoff_battery(context: &mut ContextImpl) {
        context.set_command_stdout("ectool", "".to_string());
        context.set_command_stdout(
            context
                .root_dir()
                .join("usr/share/cutoff/display_wipe_message.sh"),
            "".to_string(),
        );
        let dir = context.root_dir().join("usr/share/cutoff");
        fs::create_dir_all(dir.clone()).unwrap();
        fs::write(
            dir.join("cutoff.json"),
            json!({
                "CUTOFF_METHOD": "reboot",
                "CUTOFF_AC_STATE": "connect_ac"
            })
            .to_string(),
        )
        .unwrap();
    }

    fn create_config(config: serde_json::Value, context: &mut ContextImpl) {
        let dir = context
            .root_dir()
            .join("mnt/stateful_partition/dev_image/etc");
        fs::create_dir_all(dir.clone()).unwrap();
        fs::write(dir.join("custom-process.json"), config.to_string()).unwrap();
    }

    fn prepare_files(context: &mut ContextImpl, frecon_id: &str) {
        fs::create_dir_all(context.root_dir().join("proc").join(frecon_id).join("root")).unwrap();
        let dir = context.root_dir().join("run/frecon");
        fs::create_dir_all(dir.clone()).unwrap();
        File::create(dir.join("vt0")).unwrap();
        fs::write(dir.join("pid"), frecon_id).unwrap();
    }
}
