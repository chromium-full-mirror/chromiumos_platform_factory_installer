// Copyright 2022 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

// This module re-implements parts of functions in the following Python library in Rust:
//
// in `src/platform/factory/py/gooftool/core.py`:
//
// 1. Get all named RO sections from BIOS.
// 2. Calculate the sections of "RO_SECTION + GBB - RO_VPD - HWID - HWID_DIGEST".
//
// in `src/platform/cr50/util/ap_ro_hash.py`:
//
// 3. Read the above RO sections from BIOS.
// 4. Calculate the SHA-256 hash of these sections.
//
// AP RO hash will be calculated by the Python library in the factory and be written to the Google
// Scurity Chips(GSC). This module aims to verify the written AP RO hash is correct by
// re-implementing the same process in factory shim.

use std::cmp;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::usize;

use anyhow::{Context as _, Result};
use hmac_sha256::Hash;
use regex::Regex;

use crate::system::context::Context;
use crate::utils::futility_utils::{self, FirmwareSection};

// Blob structure
// (ref: `src/platform/vboot_reference/firmware/2lib/include/2struct.h`)
// struct vb2_gbb_header {
//   uint8_t  signature[VB2_GBB_SIGNATURE_SIZE];
//   uint16_t major_version;
//   uint16_t minor_version;
//   uint32_t header_size;
//   vb2_gbb_flags_t flags;
//   uint32_t hwid_offset;
//   uint32_t hwid_size;
//   uint32_t rootkey_offset;
//   uint32_t rootkey_size;
//   uint32_t bmpfv_offset;
//   uint32_t bmpfv_size;
//   uint32_t recovery_key_offset;
//   uint32_t recovery_key_size;
//   uint8_t  hwid_digest[VB2_GBB_HWID_DIGEST_SIZE];
//   uint8_t  pad[48];
// };

// The section in vb2_gbb_header which stores sha256 hash of HWID.
const GBB_HWID_DIGEST_OFFSET: usize = 48;
const GBB_HWID_DIGEST_SIZE: usize = 32;

// Buffer size to read BIOS.
const BUFFER_SIZE: usize = 0x400000;

/// Gets the sections of HWID and HWID_DIGEST in GBB section.
fn get_hwid_sections(
    context: &mut dyn Context,
    bios_file: &str,
    gbb_section: &FirmwareSection,
) -> Result<HashMap<String, FirmwareSection>> {
    let mut sections = HashMap::new();
    let bios_content = futility_utils::show(context, &bios_file)?;
    let caps = Regex::new(r"hwid\s+0x([0-9a-fA-F]+)\s+0x([0-9a-fA-F]+)")?
        .captures(&bios_content)
        .context("No HWID section in bios content.")?;
    let gbb_offset = gbb_section.start;
    let offset = usize::from_str_radix(caps.get(1).unwrap().as_str(), 16)?;
    let size = usize::from_str_radix(caps.get(2).unwrap().as_str(), 16)?;
    sections.insert(
        "HWID".to_string(),
        FirmwareSection::new(gbb_offset + offset, gbb_offset + offset + size),
    );
    sections.insert(
        "HWID_DIGEST".to_string(),
        FirmwareSection::new(
            gbb_offset + GBB_HWID_DIGEST_OFFSET,
            gbb_offset + GBB_HWID_DIGEST_OFFSET + GBB_HWID_DIGEST_SIZE,
        ),
    );
    Ok(sections)
}

/// Merges all overlapped sections.
fn merge_firmware_sections(sections: &mut Vec<&FirmwareSection>) -> Vec<FirmwareSection> {
    sections.sort();
    let mut ret = Vec::<FirmwareSection>::new();
    for section in sections {
        if ret.len() > 0 && section.start <= ret.last().unwrap().end {
            let last = ret.last_mut().unwrap();
            last.end = cmp::max(last.end, section.end);
        } else {
            ret.push(section.clone());
        }
    }
    ret
}

/// Merges all overlapped sections in `includes`, and excludes sections in `excludes`.
fn merge_and_exclude_firmware_sections(
    includes: &mut Vec<&FirmwareSection>,
    excludes: &mut Vec<&FirmwareSection>,
) -> Vec<FirmwareSection> {
    let includes = merge_firmware_sections(includes);
    let excludes = merge_firmware_sections(excludes);
    let mut ret = Vec::new();
    let mut exclude_i = 0;
    for mut include in includes {
        while exclude_i < excludes.len() && include.size() > 0 {
            let exclude = excludes[exclude_i];
            if exclude.end <= include.start {
                exclude_i += 1;
                continue;
            }
            if include.end <= exclude.start {
                break;
            }
            if include.start < exclude.start {
                ret.push(FirmwareSection::new(include.start, exclude.start))
            }
            include.start = exclude.end;
        }
        if include.size() > 0 {
            ret.push(include.clone());
        }
    }
    ret
}

/// Gets the sections to be calculated for AP RO hash:
/// `RO_SECTION + GBB - RO_VPD - HWID - HWID_DIGEST`
fn get_ap_ro_sections(context: &mut dyn Context, bios_file: &str) -> Result<Vec<FirmwareSection>> {
    let fmap = futility_utils::get_fmap(context).context("Failed to get FMAP")?;

    let ro_section = fmap
        .get("RO_SECTION")
        .context("RO_SECTION not found in BIOS file")?;
    let gbb = fmap.get("GBB").context("GBB not found in BIOS file")?;
    let ro_vpd = fmap
        .get("RO_VPD")
        .context("RO_VPD not found in BIOS file")?;

    let hwid_sections = get_hwid_sections(context, bios_file, gbb)
        .context(format!("Failed to get HWID sections from {}", bios_file))?;
    let hwid = hwid_sections.get("HWID").unwrap();
    let hwid_digest = hwid_sections.get("HWID_DIGEST").unwrap();

    let mut includes = vec![ro_section, gbb];
    let mut excludes = vec![ro_vpd, hwid, hwid_digest];

    Ok(merge_and_exclude_firmware_sections(
        &mut includes,
        &mut excludes,
    ))
}

/// Calculate SHA-256 hash for the given sections in BIOS.
fn calculate_hash(bios_file: &str, sections: &Vec<FirmwareSection>) -> Result<String> {
    let mut file = File::open(bios_file)?;
    let mut hasher = Hash::new();
    let mut buf = vec![0; BUFFER_SIZE];
    for mut section in sections.iter().copied() {
        file.seek(SeekFrom::Start(section.start as u64))?;
        while section.size() > 0 {
            let read_size = cmp::min(section.size(), file.read(&mut buf[..])?);
            hasher.update(&buf[0..read_size]);
            section.start += read_size;
        }
    }

    // `finalize` returns the type `[u8; 32]`. Converts the returned slice into string.
    Ok(hasher.finalize().map(|x| format!("{:02x}", x)).join(""))
}

/// Calculates AP RO hash from DUT or given BIOS binary file.
///
/// # Arguments
/// * bios_file: if provided, calculate AP RO hash from the given file, otherwise calculate hash
/// from device ROM.
///
/// # Return
/// SHA-265 hash string of sections: RO_SECTION + GBB - RO_VPD - HWID - HWID_DIGEST.
pub fn ap_ro_hash(context: &mut dyn Context, bios_file: Option<&str>) -> Result<String> {
    let bios_file = match bios_file {
        Some(f) => f.to_string(),
        None => futility_utils::read(context, None)
            .context("Failed to dump BIOS.")?
            .display()
            .to_string(),
    };

    let sections =
        get_ap_ro_sections(context, &bios_file).context("Failed to get AP RO hash sections.")?;
    Ok(calculate_hash(&bios_file, &sections).context("Failed to calculate AP RO hash.")?)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::process::Output;

    use crate::system::context::ContextImpl;
    use crate::tools::ap_ro_hash;
    use crate::utils::process_utils::StringOutput;

    // Golden BIOS info dumped from on DUT.
    const GOLDEN_BIOS: &str = "tests/bin/data/bios.bin";
    const GOLDEN_FMAP: &str = "tests/bin/data/fmap";
    const GOLDEN_SHOW: &str = "tests/bin/data/show";

    // Golden hash dumped from `gsctool -aA` on DUT.
    const GOLDEN_HASH: &str = "fc3d429569b2453b9dd540894c1fa1ac8f1965c66d7db6564070278679ccce59";

    #[test]
    fn test_ap_ro_hash() {
        let mut context = ContextImpl::new();
        context.set_command_output("futility", Output::new(0, None, None));
        context.set_command_stdout("fmap_decode", fs::read_to_string(GOLDEN_FMAP).unwrap());
        context.set_command_stdout("futility", fs::read_to_string(GOLDEN_SHOW).unwrap());
        let hash = ap_ro_hash::ap_ro_hash(&mut context, Some(GOLDEN_BIOS)).unwrap();
        assert_eq!(GOLDEN_HASH, hash);
    }
}
