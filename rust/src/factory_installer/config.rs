// Copyright 2024 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::fs;

use anyhow::{self, Result};
use serde::Deserialize;

use crate::factory_installer::Action::{ChargeBattery, CheckAcState, ResetDevice};
use crate::system::context::Context;
use crate::utils::qrcode_utils::QRcode;

#[derive(PartialEq, Deserialize, Debug)]
pub struct HttpRequest {
    pub url: String,
    pub post_arg: serde_json::Value,
}

#[derive(PartialEq, Deserialize, Debug)]
pub struct DisplayQRcode {
    pub qrcodes: Vec<QRcode>,
    pub continue_key: Option<String>,
}

#[derive(PartialEq, Deserialize, Debug)]
pub enum Action {
    ResetDevice,
    ChargeBattery,
    CheckAcState,
    Clear,
    InformShopfloor,
    HttpRequest(HttpRequest),
    DisplayQRcode(DisplayQRcode),
    StopAndConfirm(String),
}

pub fn load_config(context: &mut dyn Context) -> Result<Vec<Action>> {
    let path = context
        .root_dir()
        .join("mnt/stateful_partition/dev_image/etc/custom-process.json");
    let actions = match path.as_path().exists() {
        true => serde_json::from_str(&fs::read_to_string(path)?)?,
        false => {
            eprintln!("Config file doesn't exist, perform simplified reset process.");
            vec![ResetDevice, ChargeBattery, CheckAcState]
        }
    };
    Ok(actions)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::json;

    use crate::factory_installer::config::{load_config, Action, DisplayQRcode, HttpRequest};
    use crate::system::context::{Context, ContextImpl};
    use crate::utils::qrcode_utils::QRcode;

    #[test]
    fn test_load_config() {
        let mut context = ContextImpl::new();
        let config = json!([
            "ResetDevice",
            "ChargeBattery",
            "CheckAcState",
            "InformShopfloor",
            "Clear",
        {
            "HttpRequest": {"url": "fake_url", "post_arg": {"key": "value"}}
        },
        {
            "DisplayQRcode": {
                "qrcodes": [{"size": 100, "position": [10, 20], "content": "code"}],
                "continue_key": "abc"
            }
        },
        {
            "StopAndConfirm": "asd"
        }]);
        let dir = context
            .root_dir()
            .join("mnt/stateful_partition/dev_image/etc");
        fs::create_dir_all(dir.clone()).unwrap();
        fs::write(dir.join("custom-process.json"), config.to_string()).unwrap();

        let result = load_config(&mut context).unwrap();
        assert_eq!(
            result,
            vec![
                Action::ResetDevice,
                Action::ChargeBattery,
                Action::CheckAcState,
                Action::InformShopfloor,
                Action::Clear,
                Action::HttpRequest(HttpRequest {
                    url: "fake_url".to_string(),
                    post_arg: json!({
                        "key": "value"
                    })
                }),
                Action::DisplayQRcode(DisplayQRcode {
                    qrcodes: vec![QRcode {
                        size: 100,
                        position: Some((10, 20)),
                        content: "code".to_string()
                    }],
                    continue_key: Some("abc".to_string())
                }),
                Action::StopAndConfirm("asd".to_string())
            ]
        );
    }

    #[test]
    fn test_load_config_no_option_fields() {
        let mut context = ContextImpl::new();
        let config = json!([
        {
            "DisplayQRcode": {
                "qrcodes": [{"size": 100, "content": "code"}],
            }
        }]);
        let dir = context
            .root_dir()
            .join("mnt/stateful_partition/dev_image/etc");
        fs::create_dir_all(dir.clone()).unwrap();
        fs::write(dir.join("custom-process.json"), config.to_string()).unwrap();

        let result = load_config(&mut context).unwrap();
        assert_eq!(
            result,
            vec![Action::DisplayQRcode(DisplayQRcode {
                qrcodes: vec![QRcode {
                    size: 100,
                    position: None,
                    content: "code".to_string()
                }],
                continue_key: None
            })]
        );
    }

    #[test]
    fn test_load_config_use_default() {
        let mut context = ContextImpl::new();

        let result = load_config(&mut context).unwrap();

        assert_eq!(
            result,
            vec![
                Action::ResetDevice,
                Action::ChargeBattery,
                Action::CheckAcState
            ]
        );
    }
}
