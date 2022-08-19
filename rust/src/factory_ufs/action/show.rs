// Copyright 2022 The ChromiumOS Authors.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use anyhow::Result;

use crate::factory_ufs::utils::path_utils::UFSPath;

/// Shows the current UFS config on DUT.
pub fn action_show(_ufs_path: &UFSPath) -> Result<()> {
    println!("Action show is not supported yet.");
    Ok(())
}
