// Copyright 2023 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

pub mod args;
pub mod config;
pub mod factory_reset;
pub mod script_wrapper;

use anyhow::{self, Result};

use crate::factory_installer::config::{Action, DisplayQRcode, HttpRequest};
use crate::system::context::Context;
use crate::utils::qrcode_utils;
#[cfg(not(test))]
use crate::utils::terminal_utils;
#[cfg(test)]
use crate::utils::terminal_utils::mock as terminal_utils;
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

fn http_request(_: HttpRequest) -> Result<()> {
    // TODO(jasonchuang): Need to find third_party library to send http request.
    anyhow::bail!("Not implement");
}

fn display_qrcode(arg: DisplayQRcode, context: &mut dyn Context) -> Result<()> {
    for qrcode in arg.qrcodes {
        terminal_utils::clear();
        eprintln!("Press enter to continue...");
        // TODO(jasonchuang): Display in specific position and support displaying vpd.
        qrcode_utils::display_qrcode(&qrcode.content, context)?;
        terminal_utils::wait_enter("")?;
    }
    Ok(())
}

fn stop_and_confirm(arg: String) -> Result<()> {
    terminal_utils::wait_enter(&arg)
}

pub fn do_custom_reset_process(context: &mut dyn Context) -> Result<()> {
    let actions = config::load_config(context)?;
    for action in actions {
        match action {
            Action::ResetDevice => reset_device(context),
            Action::ChargeBattery => charge_battery(context),
            Action::HttpRequest(v) => http_request(v),
            Action::DisplayQRcode(v) => display_qrcode(v, context),
            Action::StopAndConfirm(v) => stop_and_confirm(v),
        }?;
    }
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
        create_config(
            json!([{"DisplayQRcode": {
                "qrcodes": [{"size": 100, "position": [10, 20], "content": "HWID"}]
            }}]),
            &mut context,
        );
        context.set_command_stdout("crossystem", "HWID".to_string());
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
            fs::read("tests/qrcode/HWID.png").unwrap()
        );
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
