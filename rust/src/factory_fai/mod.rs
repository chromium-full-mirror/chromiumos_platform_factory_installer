// Copyright 2022 The ChromiumOS Authors.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::fs;
use std::path::{Path, PathBuf};

use crate::factory_fai::config::{DataCollector, FAIConfig};
use crate::factory_fai::parsers::Parser;
use crate::utils::process_utils::{Command, StringOutput};

use anyhow::{self, Context, Result};
use glob;
use serde_json;
use serde_json::{Map, Value};

pub mod args;
pub mod built_in_collectors;
pub mod config;
pub mod parsers;

fn get_file_list<P: AsRef<Path>>(root: P, glob_string: P) -> Result<Vec<PathBuf>> {
    let root = root.as_ref();
    let glob_string = root.join(glob_string.as_ref()).display().to_string();

    if glob_string.find("../").is_some() {
        anyhow::bail!("Do not use `../` in glob string.");
    }

    Ok(glob::glob(&glob_string)?
        // List files only.
        .filter_map(Result::ok)
        .filter(|path| path.is_file())
        // Strip the root prefix.
        .map(|path| path.strip_prefix(root).unwrap().to_path_buf())
        .collect())
}

fn read_files<P: AsRef<Path>>(root: P, glob_string: P) -> Result<Value> {
    let root = root.as_ref();
    let glob_string = glob_string.as_ref();
    let mut data = Map::new();

    for path in get_file_list(root, glob_string)? {
        let filename = path.display().to_string();
        let path = Path::new(root).join(&filename);
        let content = fs::read_to_string(path.as_path())?.trim().to_string();
        data.insert(filename, Value::String(content));
    }
    Ok(Value::Object(data))
}

fn list_files<P: AsRef<Path>>(root: P, glob_string: P) -> Result<Value> {
    Ok(get_file_list(root, glob_string)?
        .iter()
        .map(|path| Value::String(path.display().to_string()))
        .collect())
}

pub fn collect_fai_data(fai_config: FAIConfig) -> Result<String> {
    let mut fai_data = Map::new();
    for (name, config) in fai_config.iter() {
        let data = match config {
            DataCollector::DataCommand(data_cmd) => {
                let output = Command::new(&data_cmd.cmd).args(&data_cmd.args).output()?;
                data_cmd.parser.parse(output.stdout())
            }
            DataCollector::DataFiles(data_files) => {
                if data_files.read_content {
                    read_files(&data_files.root, &data_files.glob)
                } else {
                    list_files(&data_files.root, &data_files.glob)
                }
            }
            DataCollector::BuiltInCollector(collector) => collector.collect(),
        };
        let data = data.with_context(|| format!("Failed to collect \"{}\".", name))?;

        fai_data.insert(name.to_string(), data);
    }
    Ok(serde_json::to_string_pretty(&fai_data)?)
}
