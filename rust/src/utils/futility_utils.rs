// Copyright 2023 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::Result;
use regex::Regex;
use tempfile;

use crate::utils::process_utils::{Command, StringOutput};

#[cfg(not(test))]
const FUTILITY_PATH: &str = "futility";
#[cfg(not(test))]
const FMAP_DECODE_PATH: &str = "fmap_decode";

// Mock binaries for unit tests.
#[cfg(test)]
const FUTILITY_PATH: &str = "tests/bin/futility";
#[cfg(test)]
const FMAP_DECODE_PATH: &str = "tests/bin/fmap_decode";

/// The structure to represent a section in firmware.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct FirmwareSection {
    pub start: usize,
    pub end: usize,
}

impl FirmwareSection {
    pub fn new(start: usize, end: usize) -> Self {
        FirmwareSection {
            start: start,
            end: end,
        }
    }
    pub fn size(&self) -> usize {
        if self.start > self.end {
            0
        } else {
            self.end - self.start
        }
    }
}

/// Dumps BIOS file from device and returns the path of dumped BIOS file.
pub fn dump_bios() -> Result<PathBuf> {
    let tmpd = tempfile::tempdir()?.into_path();
    let bios_file = tmpd.join("bios.bin");
    Command::new(FUTILITY_PATH)
        .args(["read", bios_file.as_path().to_str().unwrap()])
        .output()?;
    Ok(bios_file)
}

/// Displays the content of a given firmware.
pub fn show(file: &str) -> Result<String> {
    Ok(Command::new(FUTILITY_PATH)
        .args(["show", file])
        .output()?
        .stdout())
}

/// Reads a certain region from FW and returns the path of dumped region file.
pub fn read_region(region: &str) -> Result<PathBuf> {
    let tmpd = tempfile::tempdir()?.into_path();
    let prefix = tmpd.join("bios");
    let output_file = format!("{}_{}", prefix.display(), region);
    Command::new(FUTILITY_PATH)
        .args(["read", "--region", region, &output_file])
        .output()?;
    Ok(output_file.into())
}

/// Gets the FMAP sections.
pub fn get_fmap() -> Result<HashMap<String, FirmwareSection>> {
    let mut fmap = HashMap::new();
    let fmap_bin = read_region("FMAP")?;
    let fmap_text = Command::new(FMAP_DECODE_PATH)
        .args([&fmap_bin.as_path().display().to_string()])
        .output()?
        .stdout();
    // Line format:
    // area_offset="0x00000000" area_size="0x003a0000" area_name="SI_ALL" area_flags_raw="0x00"...
    let re = Regex::new(
        r#"(?x)
          area_offset="0x(?P<offset>\w+)"\s+
          area_size="0x(?P<size>\w+)"\s+
          area_name="(?P<name>\w+)"\s+
          "#,
    )?;
    // Skips the first line as it is the header.
    for line in fmap_text.trim().split('\n').skip(1) {
        if let Some(area) = re.captures(line) {
            let name = area["name"].to_string();
            let offset = usize::from_str_radix(area["offset"].trim_start_matches("0x"), 16)?;
            let size = usize::from_str_radix(area["size"].trim_start_matches("0x"), 16)?;
            fmap.insert(name, FirmwareSection::new(offset, offset + size));
        } else {
            anyhow::bail!("Failed to parse fmap: {}", line);
        }
    }
    Ok(fmap)
}
