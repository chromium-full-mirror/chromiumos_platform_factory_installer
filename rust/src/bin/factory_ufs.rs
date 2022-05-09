// Copyright 2022 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use anyhow::{self, Context, Result};
use factory_installer::factory_ufs::ufs::{self, PathMatchError};

const UFS_BSG_ERROR_MSG: &str = "Unexpected error occurs when finding";

fn main() -> Result<()> {
    let ufs_bsg_node_path = ufs::get_ufs_bsg_node();
    let ufs_bsg_node_path = match ufs_bsg_node_path {
        Ok(path) => path,
        Err(err) => {
            if let Some(err) = err.downcast_ref::<PathMatchError>() {
                if matches!(err, PathMatchError::PathNotFound) {
                    println!("No UFS BSG node found. Skip provisioning.");
                    return Ok(());
                }
            }
            return Err(anyhow::anyhow!(
                "{} UFS BSG node: {}",
                UFS_BSG_ERROR_MSG,
                err
            ));
        }
    };
    let ufs_bsg_sys_path =
        ufs::get_ufs_bsg_sys().context(format!("{} UFS BSG sys path", UFS_BSG_ERROR_MSG))?;
    println!("UFS BSG node path: {}", ufs_bsg_node_path);
    println!("UFS BSG sys path: {}", ufs_bsg_sys_path);

    ufs::do_provisioning_if_needed(&ufs_bsg_node_path, &ufs_bsg_sys_path)?;

    Ok(())
}
