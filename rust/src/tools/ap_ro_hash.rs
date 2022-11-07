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
use std::path::PathBuf;
use std::usize;

use anyhow::{Context, Result};
use hmac_sha256::Hash;
use regex::Regex;
use tempfile;

use crate::utils::process_utils::{Command, StringOutput};

const FLASHROM_PATH: &str = "flashrom";

#[cfg(not(test))]
const FUTILITY_PATH: &str = "futility";

// Mock binaries for unit tests.
#[cfg(test)]
const FUTILITY_PATH: &str = "tests/bin/futility";

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

/// The structure to represent a section in firmware.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct FirmwareSection {
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
fn dump_bios() -> Result<PathBuf> {
    let tmpd = tempfile::tempdir()?.into_path();
    let bios_file = tmpd.join("bios.bin");
    Command::new(FLASHROM_PATH)
        .args(["-r", bios_file.as_path().to_str().unwrap()])
        .output()?;
    Ok(bios_file)
}

/// Gets the FMAP sections from the BIOS file.
fn get_fmap(bios_file: &str) -> Result<HashMap<String, FirmwareSection>> {
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

/// Gets the sections of HWID and HWID_DIGEST in GBB section.
fn get_hwid_sections(
    bios_file: &str,
    gbb_section: &FirmwareSection,
) -> Result<HashMap<String, FirmwareSection>> {
    let mut sections = HashMap::new();
    let bios_content = Command::new(FUTILITY_PATH)
        .args(["show", bios_file])
        .output()?
        .stdout();
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
fn get_ap_ro_sections(bios_file: &str) -> Result<Vec<FirmwareSection>> {
    let fmap = get_fmap(bios_file).context(format!("Failed to get FMAP from {}", bios_file))?;

    let ro_section = fmap
        .get("RO_SECTION")
        .context("RO_SECTION not found in BIOS file")?;
    let gbb = fmap.get("GBB").context("GBB not found in BIOS file")?;
    let ro_vpd = fmap
        .get("RO_VPD")
        .context("RO_VPD not found in BIOS file")?;

    let hwid_sections = get_hwid_sections(bios_file, gbb)
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
pub fn ap_ro_hash(bios_file: Option<&str>) -> Result<String> {
    let bios_file = match bios_file {
        Some(f) => f.to_string(),
        None => dump_bios()
            .context("Failed to dump BIOS.")?
            .display()
            .to_string(),
    };

    let sections = get_ap_ro_sections(&bios_file).context("Failed to get AP RO hash sections.")?;
    Ok(calculate_hash(&bios_file, &sections).context("Failed to calculate AP RO hash.")?)
}

#[cfg(test)]
mod tests {
    use crate::tools::ap_ro_hash;

    // Golden BIOS dumped from `flashrom -r bios.bin` on DUT.
    const GOLDEN_BIOS: &str = "tests/bin/data/bios.bin";
    // Golden hash dumped from `gsctool -aA` on DUT.
    const GOLDEN_HASH: &str = "fc3d429569b2453b9dd540894c1fa1ac8f1965c66d7db6564070278679ccce59";

    #[test]
    fn test_ap_ro_hash() {
        let hash = ap_ro_hash::ap_ro_hash(Some(GOLDEN_BIOS)).unwrap();
        assert_eq!(GOLDEN_HASH, hash);
    }
}
