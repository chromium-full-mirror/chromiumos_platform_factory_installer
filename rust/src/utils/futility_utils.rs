// Copyright 2023 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::Result;
use tempfile;

use crate::utils::process_utils::{Command, StringOutput};

#[cfg(not(test))]
const FUTILITY_PATH: &str = "futility";

// Mock binaries for unit tests.
#[cfg(test)]
const FUTILITY_PATH: &str = "tests/bin/futility";

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

/// Gets the FMAP sections from the BIOS file.
pub fn get_fmap(bios_file: &str) -> Result<HashMap<String, FirmwareSection>> {
    let mut fmap = HashMap::new();
    let fmap_text = Command::new(FUTILITY_PATH)
        .args(["dump_fmap", "-F", bios_file])
        .output()?
        .stdout();
    for line in fmap_text.trim().split('\n') {
        // Line format:
        // start_offset:end_offset secton_name
        // e.g.
        // 0x00000000:0x003f7fff RO_SECTION
        let line: Vec<&str> = line.split_whitespace().collect();
        let (r, name) = (line[0], line[1]);
        let r: Vec<&str> = r.split(':').map(|x| x.trim_start_matches("0x")).collect();
        fmap.insert(
            name.to_string(),
            FirmwareSection::new(
                usize::from_str_radix(r[0], 16)?,
                usize::from_str_radix(r[1], 16)? + 1,
            ),
        );
    }
    Ok(fmap)
}
