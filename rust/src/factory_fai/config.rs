// Copyright 2022 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json;

// TODO(wyuang): collect more default data:
// 1. Partition Table ("cgpt show ...")
// 2. Release image info ("mount $DEV; cat /$DEV/etc/lsb-release")
// 3. Signing key
// 4. CBI data ("ectool cbi get <tag> ...")
// TODO(wyuang): VPD may contains "\n", we would probably want to read values from
// `/sys/firmware/vpd` instead of `vpd` command.
pub const DEFAULT_CONFIG: &str = r##"{
  "ro_vpd": {
    "DataCommand": {
      "cmd": "vpd",
      "args": ["-i", "RO_VPD", "-l"]
    }
  },
  "rw_vpd": {
    "DataCommand": {
      "cmd": "vpd",
      "args": ["-i", "RW_VPD", "-l"]
    }
  },
  "stateful_partition": {
    "DataCommand": {
      "cmd": "find",
      "args": ["/mnt/stateful_partition", "-not", "-type", "d"]
    }
  },
  "tpm_version": {
    "DataCommand": {
      "cmd": "tpm_version"
    }
  },
  "gsc_capabilities": {
    "DataCommand": {
      "cmd": "gsctool",
      "args": ["-a", "-I", "-M"]
    }
  },
  "gsc_board_id": {
    "DataCommand": {
      "cmd": "gsctool",
      "args": ["-a", "-i", "-M"]
    }
  },
  "gsc_firmware_version": {
    "DataCommand": {
      "cmd": "gsctool",
      "args": ["-a", "-f", "-M"]
    }
  },
  "crossystem": {
    "DataCommand": {
      "cmd": "crossystem"
    }
  },
  "ap_write_protect": {
    "DataCommand": {
      "cmd": "flashrom",
      "args": ["-p", "host", "--wp-status"]
    }
  },
  "ec_write_protect": {
    "DataCommand": {
      "cmd": "flashrom",
      "args": ["-p", "ec", "--wp-status"]
    }
  },
  "factory_instal_shim_version": {
    "DataCommand": {
      "cmd": "cat",
      "args": ["/etc/lsb-release"]
    }
  }
}"##;

#[derive(Serialize, Deserialize)]
pub struct DataCommand {
    pub cmd: String,

    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Serialize, Deserialize)]
pub enum DataCollector {
    // TODO(wyuang): support DataFunction and DataFiles for calling function and read files.
    DataCommand(DataCommand),
}

pub type FAIConfig = HashMap<String, DataCollector>;

fn load_config_file<P: AsRef<Path>>(path: P) -> Result<FAIConfig> {
    let config_content = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&config_content)?)
}

pub fn load_config<P: AsRef<Path>>(config_path: Option<P>) -> Result<FAIConfig> {
    match config_path {
        Some(file_path) => load_config_file(&file_path),
        None => Ok(serde_json::from_str(DEFAULT_CONFIG)?),
    }
}
