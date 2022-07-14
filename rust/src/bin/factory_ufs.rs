// Copyright 2022 The ChromiumOS Authors.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::path::Path;

use anyhow::{self, Context, Result};
use factory_installer::factory_ufs::args::{Args, Commands, Parser};
use factory_installer::factory_ufs::ufs_provision::{
    self, NoParentDirError, PathMatchError, UFSPath,
};

const UFS_BSG_ERROR_MSG: &str = "Unexpected error occurs when finding";

fn main() -> Result<()> {
    let args = Args::parse();

    let ufs_bsg_node_path = ufs_provision::get_ufs_bsg_node();
    let ufs_bsg_node_path = match ufs_bsg_node_path {
        Ok(path) => path,
        Err(err) => {
            if let Some(err) = err.downcast_ref::<PathMatchError>() {
                if matches!(err, PathMatchError::PathNotFound) {
                    println!("No UFS BSG node found. Do nothing.");
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

    // `ufs_bsg_sys_path` looks like: /sys/devices/pcixxx/xxx/hostx/ufs-bsgx,
    // but we actually want: /sys/devices/pcixxx/xxx.
    let ufs_dev_path = Path::new(&ufs_bsg_sys_path)
        .parent()
        .ok_or(NoParentDirError)?
        .parent()
        .ok_or(NoParentDirError)?
        .to_path_buf();

    let ufs_path = UFSPath {
        bsg_node: ufs_bsg_node_path,
        sys_dev: ufs_dev_path,
    };

    match args.command {
        Commands::Clobber => ufs_provision::action_clobber(&ufs_path),
        Commands::Show => ufs_provision::action_show(&ufs_path),
        Commands::Provision { rescan_timeout } => {
            ufs_provision::action_provision(&ufs_path, rescan_timeout)
        }
    }
}
