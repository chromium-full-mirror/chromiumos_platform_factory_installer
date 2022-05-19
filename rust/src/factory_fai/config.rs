// Copyright 2022 The ChromiumOS Authors.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::factory_fai::parsers::DataParser;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json;

// TODO(wyuang): collect more default data:
// 1. Partition Table ("cgpt show ...")
// 2. Release image info ("mount $DEV; cat /$DEV/etc/lsb-release")
// 3. Signing key
// 4. CBI data ("ectool cbi get <tag> ...")
pub const DEFAULT_CONFIG: &str = r##"{
  "ro_vpd": {
    "DataFiles": {
      "root": "/sys/firmware/vpd/ro",
      "read_content": true
    }
  },
  "rw_vpd": {
    "DataFiles": {
      "root": "/sys/firmware/vpd/rw",
      "read_content": true
    }
  },
  "stateful_partition": {
    "DataFiles": {
      "root": "/mnt/stateful_partition"
    }
  },
  "tpm_version": {
    "DataCommand": {
      "cmd": "tpm_version",
      "parser": {
        "type": "DictParser",
        "delimiter": ":"
      }
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
      "args": ["-a", "-i", "-M"],
      "parser": {
        "type": "DictParser",
        "delimiter": "="
      }
    }
  },
  "gsc_firmware_version": {
    "DataCommand": {
      "cmd": "gsctool",
      "args": ["-a", "-f", "-M"],
      "parser": {
        "type": "DictParser",
        "delimiter": "="
      }
    }
  },
  "crossystem": {
    "DataCommand": {
      "cmd": "crossystem",
      "parser": {
        "type": "DictParser",
        "delimiter": "=",
        "comment_mark": "#"
      }
    }
  },
  "ap_write_protect": {
    "DataCommand": {
      "cmd": "flashrom",
      "args": ["-p", "host", "--wp-status"],
      "parser": {
        "type": "RegexParser",
        "pattern":
          "(write protect is (?P<status>enabled|disabled))|(?P<range>start=0x\\d+, len=0x\\d+)"
      }
    }
  },
  "ec_write_protect": {
    "DataCommand": {
      "cmd": "flashrom",
      "args": ["-p", "ec", "--wp-status"],
      "parser": {
        "type": "RegexParser",
        "pattern":
          "(write protect is (?P<status>enabled|disabled))|(?P<range>start=0x\\d+, len=0x\\d+)"
      }
    }
  },
  "factory_instal_shim_version": {
    "DataCommand": {
      "cmd": "cat",
      "args": ["/etc/lsb-release"],
      "parser": {
        "type": "DictParser",
        "delimiter": "="
      }
    }
  }
}"##;

#[derive(Serialize, Deserialize)]
pub struct DataCommand {
    pub cmd: String,

    #[serde(default)]
    pub args: Vec<String>,

    #[serde(default)]
    pub parser: DataParser,
}

#[derive(Serialize, Deserialize)]
pub struct DataFiles {
    pub root: String,

    #[serde(default = "default_glob")]
    pub glob: String,

    #[serde(default)]
    pub read_content: bool,
}

fn default_glob() -> String {
    "**/*".to_string()
}

#[derive(Serialize, Deserialize)]
pub enum DataCollector {
    // TODO(wyuang): support DataFunction for calling functions.
    DataCommand(DataCommand),
    DataFiles(DataFiles),
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
