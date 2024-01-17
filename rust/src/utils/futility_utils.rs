// Copyright 2023 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::Result;
use regex::Regex;

use crate::system::context::Context;
use crate::utils::process_utils::StringOutput;

const FUTILITY_PATH: &str = "futility";
const FMAP_DECODE_PATH: &str = "fmap_decode";

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
/// If `region` is given, dump the specified region only.
pub fn read(context: &mut dyn Context, region: Option<&str>) -> Result<PathBuf> {
    let tmpd = context.tempdir(None)?;
    let mut cmd = context.command(FUTILITY_PATH);
    cmd.arg("read");
    let output_file = if let Some(r) = region {
        let prefix = tmpd.join("bios");
        cmd.args([
            "--region",
            r,
            "--split-output",
            &prefix.display().to_string(),
        ]);
        PathBuf::from(format!("{}_{}", prefix.display(), r))
    } else {
        let path = tmpd.join("bios.bin");
        cmd.arg(&path);
        path
    };
    cmd.output()?;
    Ok(output_file)
}

/// Displays the content of a given firmware.
pub fn show(context: &mut dyn Context, file: &str) -> Result<String> {
    Ok(context
        .command(FUTILITY_PATH)
        .args(["show", file])
        .output()?
        .stdout())
}

/// Gets the FMAP sections.
pub fn get_fmap(context: &mut dyn Context) -> Result<HashMap<String, FirmwareSection>> {
    let mut fmap = HashMap::new();
    let fmap_bin = read(context, Some("FMAP"))?;
    let fmap_text = context
        .command(FMAP_DECODE_PATH)
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

#[cfg(test)]
mod tests {
    use std::process::Output;

    use crate::system::context::ContextImpl;
    use crate::utils::futility_utils::{self, FirmwareSection};
    use crate::utils::process_utils::StringOutput;

    const FMAP: &str = r##"header
area_offset="0x00000000" area_size="0x00400000" area_name="WP_RO" area_flags_raw="0x00""##;

    #[test]
    fn test_futility_read_success() {
        let mut context = ContextImpl::new();
        context.set_command_output("futility", Output::new(0, None, None));
        assert!(futility_utils::read(&mut context, None)
            .unwrap()
            .ends_with("bios.bin"))
    }

    #[test]
    fn test_futility_read_region_success() {
        let mut context = ContextImpl::new();
        context.set_command_output("futility", Output::new(0, None, None));
        assert!(futility_utils::read(&mut context, Some("FMAP"))
            .unwrap()
            .ends_with("bios_FMAP"))
    }

    #[test]
    fn test_futility_show_success() {
        let mut context = ContextImpl::new();
        let output = "output_string";
        context.set_command_stdout("futility", output.to_string());
        assert_eq!(
            futility_utils::show(&mut context, "filename").unwrap(),
            output.to_string()
        )
    }

    #[test]
    fn test_futility_get_fmap_success() {
        let mut context = ContextImpl::new();

        context.set_command_output("futility", Output::new(0, None, None));
        context.set_command_stdout("fmap_decode", FMAP.to_string());
        assert_eq!(
            futility_utils::get_fmap(&mut context)
                .unwrap()
                .get("WP_RO")
                .unwrap(),
            &FirmwareSection::new(0, 0x00400000)
        )
    }
}
