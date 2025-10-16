// Copyright 2022 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::path::Path;

use anyhow::{self, Context, Result};
use factory_installer::factory_ufs::action::{provision, purge, show};
use factory_installer::factory_ufs::args::{Args, Commands, Parser};
use factory_installer::factory_ufs::utils::path_utils::{self, PathMatchError, UFSPath};
use factory_installer::utils::file_utils::PathError;

const UFS_BSG_ERROR_MSG: &str = "Unexpected error occurs when finding";

fn prepare_ufs_path() -> Result<UFSPath> {
    let ufs_bsg_node_path = match path_utils::get_ufs_bsg_node() {
        Ok(path) => path,
        Err(err) => {
            if let Some(err) = err.downcast_ref::<PathMatchError>() {
                if matches!(err, PathMatchError::PathNotFound) {
                    println!("No UFS BSG node found. Do nothing.");
                    // Exit gracefully if no UFS device is found.
                    std::process::exit(0);
                }
            }
            anyhow::bail!("{} UFS BSG node: {}", UFS_BSG_ERROR_MSG, err)
        }
    };
    let ufs_bsg_sys_path =
        path_utils::get_ufs_bsg_sys().context(format!("{} UFS BSG sys path", UFS_BSG_ERROR_MSG))?;
    println!("UFS BSG node path: {}", ufs_bsg_node_path.display());
    println!("UFS BSG sys path: {}", ufs_bsg_sys_path.display());

    // `ufs_bsg_sys_path` looks like: /sys/devices/pcixxx/xxx/hostx/ufs-bsgx,
    // but we actually want: /sys/devices/pcixxx/xxx.
    let ufs_dev_path = Path::new(&ufs_bsg_sys_path)
        .parent()
        .ok_or(PathError::NoParentDirError)?
        .parent()
        .ok_or(PathError::NoParentDirError)?
        .to_path_buf();

    Ok(UFSPath {
        bsg_node: ufs_bsg_node_path,
        sys_dev: ufs_dev_path,
    })
}

fn main() -> Result<()> {
    let args = Args::parse();

    match args.command {
        Commands::Purge { timeout } => {
            let ufs_path = prepare_ufs_path()?;
            purge::action_purge(&ufs_path, timeout)
        }
        Commands::Show => {
            let ufs_path = prepare_ufs_path()?;
            show::action_show(&ufs_path)
        }
        Commands::Provision {
            rescan_timeout,
            force,
        } => {
            let ufs_path = prepare_ufs_path()?;
            provision::action_provision(&ufs_path, rescan_timeout, force)
        }
        Commands::ProvisionFile {
            input_config_descriptor,
            input_device_descriptor,
            input_geometry_descriptor,
            output_file,
        } => provision::action_provision_file(
            &input_config_descriptor,
            &input_device_descriptor,
            &input_geometry_descriptor,
            &output_file,
        ),
    }
}
