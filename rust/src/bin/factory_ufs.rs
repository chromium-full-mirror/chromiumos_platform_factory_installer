// Copyright 2022 The ChromiumOS Authors.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use anyhow::{self, Context, Result};
use factory_installer::factory_ufs::ufs_provision::{self, PathMatchError};
use factory_installer::factory_ufs::ufs_spec::ufs_config_descriptor::{
    UFS2ConfigDescriptor, UFS3ConfigDescriptor,
};

const UFS_BSG_ERROR_MSG: &str = "Unexpected error occurs when finding";

fn main() -> Result<()> {
    let ufs_bsg_node_path = ufs_provision::get_ufs_bsg_node();
    let ufs_bsg_node_path = match ufs_bsg_node_path {
        Ok(path) => path,
        Err(err) => {
            if let Some(err) = err.downcast_ref::<PathMatchError>() {
                if matches!(err, PathMatchError::PathNotFound) {
                    println!("No UFS BSG node found. Skip provisioning.");
                    return Ok(());
                }
            }
            anyhow::bail!("{} UFS BSG node: {}", UFS_BSG_ERROR_MSG, err)
        }
    };
    let ufs_bsg_sys_path = ufs_provision::get_ufs_bsg_sys()
        .context(format!("{} UFS BSG sys path", UFS_BSG_ERROR_MSG))?;
    println!("UFS BSG node path: {}", ufs_bsg_node_path.display());
    println!("UFS BSG sys path: {}", ufs_bsg_sys_path.display());

    // We need to parse the header of current UFS config to decide which UFS
    // spec to load.
    let cur_config_binary = ufs_provision::get_current_config_binary(&ufs_bsg_node_path)?;
    let header_value = ufs_provision::read_header(&cur_config_binary)?;

    match header_value {
        0x90 => {
            println!("Provision UFS 2.1...");
            ufs_provision::do_provisioning_if_needed::<UFS2ConfigDescriptor>(
                &ufs_bsg_node_path,
                &ufs_bsg_sys_path,
                &cur_config_binary,
            )
        }
        0xe6 => {
            println!("Provision UFS 3.1...");
            ufs_provision::do_provisioning_if_needed::<UFS3ConfigDescriptor>(
                &ufs_bsg_node_path,
                &ufs_bsg_sys_path,
                &cur_config_binary,
            )
        }
        value => anyhow::bail!(
            "Cannot parse current UFS config! Unknown header value: {}",
            value
        ),
    }
}
