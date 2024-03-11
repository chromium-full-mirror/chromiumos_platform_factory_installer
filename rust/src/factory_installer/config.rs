// Copyright 2024 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::fs;

use anyhow::{self, Result};
use serde::Deserialize;

use crate::system::context::Context;

#[derive(PartialEq, Deserialize, Debug)]
pub struct HttpRequest {
    pub url: String,
}

#[derive(PartialEq, Deserialize, Debug)]
pub struct DisplayQRcode {
    pub qrcodes: Vec<QRcode>,
}

#[derive(PartialEq, Deserialize, Debug)]
pub struct QRcode {
    pub size: i32,
    pub position: Vec<i32>,
    pub content: String,
}

#[derive(PartialEq, Deserialize, Debug)]
pub enum Action {
    ResetDevice,
    ChargeBattery,
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
            // TODO(jasonchuang): Load default config to perform factory reset.
            eprintln!("Config file doesn't exist, use default config instead.");
            vec![]
        }
    };
    Ok(actions)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::json;

    use crate::factory_installer::config::{
        load_config, Action, DisplayQRcode, HttpRequest, QRcode,
    };
    use crate::system::context::{Context, ContextImpl};

    #[test]
    fn test_load_config() {
        let mut context = ContextImpl::new();
        let config = json!([
            "ResetDevice",
            "ChargeBattery",
        {
            "HttpRequest": {"url": "fake_url"}
        },
        {
            "DisplayQRcode": {
                "qrcodes": [{"size": 100, "position": [10, 20], "content": "code"}]
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
                Action::HttpRequest(HttpRequest {
                    url: "fake_url".to_string()
                }),
                Action::DisplayQRcode(DisplayQRcode {
                    qrcodes: vec![QRcode {
                        size: 100,
                        position: vec![10, 20],
                        content: "code".to_string()
                    }]
                }),
                Action::StopAndConfirm("asd".to_string())
            ]
        );
    }

    #[test]
    fn test_load_config_use_default() {
        let mut context = ContextImpl::new();
        let result = load_config(&mut context).unwrap();

        assert_eq!(result, vec![]);
    }
}
