// Copyright 2022 The ChromiumOS Authors.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::factory_fai::built_in_collectors::BuiltInCollector;
use crate::factory_fai::parsers::DataParser;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json;

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
      "args": ["-a", "-I", "-M"],
      "parser": {
        "type": "GSCCapsParser"
      }
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
  },
  "partition_table": {
    "BuiltInCollector": "partition_table"
  },
  "release_image_info": {
    "BuiltInCollector": "release_image_info"
  },
  "signing_keys": {
    "BuiltInCollector": "signing_keys"
  },
  "cbi_data": {
    "BuiltInCollector": "cbi_data"
  },
  "tpm_status": {
    "DataCommand": {
      "cmd": "trunks_client",
      "args": ["--status"],
      "parser": {
        "type": "DictParser",
        "delimiter": ":"
      }
    }
  },
  "ap_ro_hash": {
    "DataCommand": {
      "cmd": "gsctool",
      "args": ["-a", "-A"],
      "parser": {
        "type": "DictParser",
        "delimiter": ":"
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

/// `DataCollector` implements a set of functions ro collect FAI data.
///
/// # `DataCommand`
/// `DataCommand` collects data from command line by execute `cmd` with arguments `args`. Then the
/// output will be passed to a `DetaParser` implemented in `factory_fai::parsers` and converted to
/// JSON format.
///
/// # `DataFiles`
/// `Datafiles` collect data from all files the `root` directory according to the `glob` string. If
/// `read_content` is set, it will collect the contents of files. Otherwise list the filenames only.
///
/// # BuiltInCollector
/// If the data cannot be fit in the other common collectors, there's a set of `BuiltInCollector`
/// implemented in `factory_fai::built_in_collectors` which handles specific cases.
#[derive(Serialize, Deserialize)]
pub enum DataCollector {
    DataCommand(DataCommand),
    DataFiles(DataFiles),
    BuiltInCollector(BuiltInCollector),
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
