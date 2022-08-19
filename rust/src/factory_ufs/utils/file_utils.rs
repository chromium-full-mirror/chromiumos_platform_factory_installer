// Copyright 2022 The ChromiumOS Authors.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::path::Path;

use anyhow::Result;

use crate::utils::file_utils;

pub fn read_geometry_descriptor(sys_dev_path: &Path, f_name: &str) -> Result<u32> {
    Ok(file_utils::read_hex_file(
        sys_dev_path.join("geometry_descriptor").join(f_name),
    )?)
}

pub fn read_attributes(sys_dev_path: &Path, f_name: &str) -> Result<u32> {
    Ok(file_utils::read_hex_file(
        sys_dev_path.join("attributes").join(f_name),
    )?)
}

pub fn read_device_descriptor(sys_dev_path: &Path, f_name: &str) -> Result<u32> {
    Ok(file_utils::read_hex_file(
        sys_dev_path.join("device_descriptor").join(f_name),
    )?)
}
