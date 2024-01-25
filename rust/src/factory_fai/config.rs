// Copyright 2022 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json;

use crate::factory_fai::built_in_collectors::BuiltInCollector;
use crate::factory_fai::parsers::DataParser;

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
  "release_image_stateful_partition": {
    "BuiltInCollector": "release_image_stateful_partition"
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
        "type": "DictParser",
        "delimiter": "="
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
  "gsc_factory_config": {
    "DataCommand": {
      "cmd": "gsctool",
      "args": ["-a", "-y"],
      "parser": {
        "type": "DictParser",
        "delimiter": ": "
      }
    }
  },
  "aprov_status": {
    "DataCommand": {
      "cmd": "gsctool",
      "args": ["-a", "-B"]
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
      "cmd": "futility",
      "args": ["flash", "--wp-status", "--ignore-hw"],
      "parser": {
        "type": "RegexParser",
        "pattern": "WP status: (?P<status>enabled|disabled|misconfigured)"
      }
    }
  },
  "ec_write_protect": {
    "DataCommand": {
      "cmd": "ectool",
      "args": ["flashprotect"],
      "parser": {
        "type": "RegexParser",
        "pattern": "Flash protect flags: (?P<flags>0x[\\da-z]+.*)"
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
  "crosid": {
    "DataCommand": {
      "cmd": "crosid",
      "parser": {
        "type": "DictParser",
        "delimiter": "="
      }
    }
  }
}"##;

pub const CR50_CONFIG: &str = r##"{
  "gsc_ap_ro_hash": {
    "DataCommand": {
      "cmd": "gsctool",
      "args": ["-a", "-A"],
      "parser": {
        "type": "DictParser",
        "delimiter": ":"
      }
    }
  },
  "device_ap_ro_hash": {
    "BuiltInCollector": "device_ap_ro_hash"
  }
}"##;

pub const TI50_CONFIG: &str = r##"{
  "ro_gscvd_board_id": {
    "BuiltInCollector": "ro_gscvd_board_id"
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

pub struct ConfigOptions {
    pub config_path: Option<PathBuf>,
    pub is_ti50: bool,
}

pub type FAIConfig = HashMap<String, DataCollector>;

fn load_config_file<P: AsRef<Path>>(path: P) -> Result<FAIConfig> {
    let config_content = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&config_content)?)
}

/// Loads the default config according to device properties.
fn load_default_config(is_ti50: bool) -> Result<FAIConfig> {
    let mut default_config: FAIConfig = serde_json::from_str(DEFAULT_CONFIG)?;
    let gsc_config: FAIConfig = if is_ti50 {
        eprintln!("Device is using Ti50...");
        serde_json::from_str(TI50_CONFIG)?
    } else {
        eprintln!("Device is using Cr50...");
        serde_json::from_str(CR50_CONFIG)?
    };

    default_config.extend(gsc_config);
    Ok(default_config)
}

pub fn load_config(config_options: &ConfigOptions) -> Result<FAIConfig> {
    match &config_options.config_path {
        Some(file_path) => load_config_file(&file_path),
        None => load_default_config(config_options.is_ti50),
    }
}

#[cfg(test)]
mod tests {
    use crate::factory_fai::config::{self, ConfigOptions};
    use crate::system::context::{Context, ContextImpl};
    use crate::utils::file_utils;

    #[test]
    fn test_load_config_success_default() {
        let opt = ConfigOptions {
            config_path: None,
            is_ti50: true,
        };
        assert!(config::load_config(&opt).is_ok());
    }

    #[test]
    fn test_load_config_success_from_config_path() {
        let context = ContextImpl::new();
        let config_path = context.tempdir(None).unwrap().join("config.json");
        file_utils::create_file(&config_path, "{}").unwrap();
        let opt = ConfigOptions {
            config_path: Some(config_path),
            is_ti50: true,
        };
        assert!(config::load_config(&opt).is_ok());
    }

    #[test]
    fn test_load_config_invalid_config() {
        let context = ContextImpl::new();
        let config_path = context.tempdir(None).unwrap().join("config.json");
        file_utils::create_file(&config_path, "(not a valid json)").unwrap();
        let opt = ConfigOptions {
            config_path: Some(config_path),
            is_ti50: true,
        };
        assert!(config::load_config(&opt).is_err());
    }
}
