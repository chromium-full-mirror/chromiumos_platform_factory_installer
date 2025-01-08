// Copyright 2023 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

pub mod args;
pub mod config;
pub mod factory_reset;
pub mod script_wrapper;

use std::collections::HashMap;

use anyhow::{self, Result};
use regex::{Captures, Regex};
use serde_json::json;

use crate::factory_installer::config::{Action, DisplayQRcode, HttpRequest};
use crate::system::context::Context;
use crate::utils::process_utils::StringOutput;
#[cfg(not(test))]
use crate::utils::terminal_utils;
#[cfg(test)]
use crate::utils::terminal_utils::mock as terminal_utils;
use crate::utils::{crossystem_utils, qrcode_utils, vpd_utils};
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

// TODO(b/342942582): This action is still not stable.
fn http_request(request: HttpRequest, context: &mut dyn Context) -> Result<()> {
    let args = replace_json_value(request.post_arg, context)?;
    context.http_post(request.url, args)?;
    Ok(())
}

fn inform_shopfloor(context: &mut dyn Context) -> Result<()> {
    factory_reset::inform_shopfloor(context)
}

fn replace_json_value(
    args: serde_json::Value,
    context: &mut dyn Context,
) -> Result<serde_json::Value> {
    let all_contents = args
        .as_object()
        .unwrap()
        .values()
        .map(|v| v.to_string())
        .collect::<Vec<String>>();
    let value_lookup = get_value_lookup(all_contents, context)?;
    let mut result = json!({});
    for (key, value) in args.as_object().unwrap() {
        result[key] = serde_json::from_str(
            replace_by_lookup(value.to_string(), value_lookup.clone())?.as_str(),
        )?;
    }
    Ok(result)
}

fn get_value_lookup(
    all_contents: Vec<String>,
    context: &mut dyn Context,
) -> Result<HashMap<String, String>> {
    let mut value_lookup = HashMap::new();
    let contents = all_contents.join(" ");
    if contents.contains("<hwid>") {
        value_lookup.insert(
            "<hwid>".to_string(),
            crossystem_utils::get_value("hwid", context)?,
        );
    }
    if contents.contains("<serial_number>") {
        value_lookup.insert(
            "<serial_number>".to_string(),
            vpd_utils::get_ro_value("serial_number", context)?,
        );
    }
    if contents.contains("<mlb_serial_number>") {
        value_lookup.insert(
            "<mlb_serial_number>".to_string(),
            vpd_utils::get_ro_value("mlb_serial_number", context)?,
        );
    }
    if contents.contains("<wifi_mac0>") {
        value_lookup.insert(
            "<wifi_mac0>".to_string(),
            vpd_utils::get_ro_value("wifi_mac0", context)?,
        );
    }
    if contents.contains("<service_tag>") {
        value_lookup.insert(
            "<service_tag>".to_string(),
            vpd_utils::get_ro_value("service_tag", context)?,
        );
    }
    Ok(value_lookup)
}

fn replace_by_lookup(content: String, value_lookup: HashMap<String, String>) -> Result<String> {
    let re = Regex::new("(<.*?>)").unwrap();
    let mut invalid_content = false;
    let res = re
        .replace_all(&content, |cap: &Captures| {
            match value_lookup.contains_key(&cap[0]) {
                true => value_lookup[&cap[0]].as_str(),
                false => {
                    eprintln!("Unknown key: {}", &cap[0]);
                    invalid_content = true;
                    ""
                }
            }
        })
        .to_string();
    if invalid_content {
        anyhow::bail!("Invalid content: {}", content);
    }
    Ok(res)
}

fn display_qrcode(arg: DisplayQRcode, context: &mut dyn Context) -> Result<()> {
    let all_contents = arg
        .qrcodes
        .iter()
        .map(|qrcode| qrcode.content.clone())
        .collect::<Vec<String>>();
    let value_lookup = get_value_lookup(all_contents, context)?;

    terminal_utils::clear();
    if let Some(ref key) = arg.continue_key {
        println!("Input {} and press enter to continue...", key);
    }
    for mut qrcode in arg.qrcodes {
        qrcode.content = replace_by_lookup(qrcode.content, value_lookup.clone())?;
        qrcode_utils::display_qrcode(&qrcode, context)?;
    }
    if let Some(key) = arg.continue_key {
        terminal_utils::wait_keys(&key)?;
    }
    Ok(())
}

fn stop_and_confirm(arg: String) -> Result<()> {
    terminal_utils::wait_keys(&arg)
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
            Action::Clear => Ok(terminal_utils::clear()),
            Action::InformShopfloor => inform_shopfloor(context),
            Action::HttpRequest(v) => http_request(v, context),
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

    use hyper::{Body, Response};
    use mockall::predicate;
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
                "qrcodes": [{"size": 100, "position": [10, 20],
                "content":
                    "<hwid> <serial_number> <wifi_mac0> <mlb_serial_number> <service_tag>"},
                    {"size": 100, "position": [10, 20],
                "content":
                    "<hwid>"}]
            }}, ]),
            &mut context,
        );
        context.set_command_stdout("crossystem", "HWID".to_string());
        context.set_command_stdout("vpd", "SN".to_string());
        context.set_command_stdout("vpd", "MLBSN".to_string());
        context.set_command_stdout("vpd", "WIFI".to_string());
        context.set_command_stdout("vpd", "TAG".to_string());
        context.set_tempfile("qrcode.png".to_string());
        context.set_tempfile("qrcode1.png".to_string());
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
        context.set_command_stdout("vpd", "TAG".to_string());

        let result = factory_installer::do_custom_reset_process(&mut context);
        let err = result.unwrap_err();
        let message = err.root_cause();
        assert_eq!(format!("{}", message), "Invalid content: <abc> <hwid> abc");
    }

    #[test]
    fn test_do_custom_reset_process_http_request() {
        let mut context = ContextImpl::new();
        context.set_command_stdout("activate_date", "".to_string());
        context.set_command_stderr("vpd", "already deleted".to_string());
        context.set_command_stdout("crossystem", "HWID".to_string());
        mock_cutoff_battery(&mut context);
        create_config(
            json!([{"HttpRequest": {
                "url": "fake_url",
                "post_arg": {
                "key": "<hwid>"
            }}}]),
            &mut context,
        );
        context
            .http_client
            .expect_post()
            .times(1)
            .with(
                predicate::eq("fake_url".to_string()),
                predicate::eq(json!({
                "key": "HWID"})),
            )
            .returning(|_, _| Ok(Response::<Body>::new("body".into())));

        let result = factory_installer::do_custom_reset_process(&mut context);
        assert!(result.is_ok());
    }

    fn mock_cutoff_battery(context: &mut ContextImpl) {
        context.set_command_stdout(
            "ectool",
            "Design capacity: 100 mAh Remaining capacity 100 mAh Present voltage 10 mV AC_PRESENT"
                .to_string(),
        ); // For charge control
        context.set_command_stdout("ectool", "normal".to_string());
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
        File::create(dir.join("display_wipe_message.sh")).unwrap();
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
