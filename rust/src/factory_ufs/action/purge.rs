// Copyright 2022 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::path::Path;
use std::thread;
use std::time::Duration;

use anyhow::Result;
use num_derive::FromPrimitive;
use num_traits::FromPrimitive;

use crate::factory_ufs::action::constants;
use crate::factory_ufs::utils::file_utils;
use crate::factory_ufs::utils::path_utils::UFSPath;
use crate::factory_ufs::utils::sys_utils::{self, PowerControl};
use crate::utils::process_utils::{Command, StringOutput};

#[derive(FromPrimitive)]
enum PurgeStatus {
    Idle = 0x0,
    InProgress = 0x1,
    StoppedPrematurely = 0x2,
    Complete = 0x3,
    QueueNotEmpty = 0x4,
}

fn set_purge_flag(bsg_node_path: &Path) -> Result<()> {
    // -t 6 is flag type `fPurgeEnable`
    Command::new(constants::UFS_UTILS)
        .args([
            "fl",
            "-t",
            "6",
            "-e",
            "-p",
            &bsg_node_path.display().to_string(),
        ])
        .output()?
        .exit_ok()?;
    Ok(())
}

/// Runs the purge operation.
pub fn purge(ufs_path: &UFSPath, timeout: u32) -> Result<()> {
    println!("Setting purge flag...");
    set_purge_flag(&ufs_path.bsg_node)?;
    println!("Running purge...");
    for sec in 0..timeout {
        let message: &str;
        // Ref: UFS3.1 12.2.3.3 Purge operation
        let purge_status = file_utils::read_attributes(&ufs_path.sys_dev, "purge_status")?;
        match FromPrimitive::from_u32(purge_status) {
            Some(PurgeStatus::Idle) => {
                message = "Purge hasn't started yet.";
            }
            Some(PurgeStatus::InProgress) => {
                message = "Purge operation in progress.";
            }
            Some(PurgeStatus::StoppedPrematurely) => {
                anyhow::bail!("Purge operation stopped prematurely by the host.");
            }
            Some(PurgeStatus::Complete) => {
                println!("Purge completed successfully.");
                return Ok(());
            }
            Some(PurgeStatus::QueueNotEmpty) => {
                anyhow::bail!("Purge operation failed due to logical unit queue not empty.");
            }
            None => {
                anyhow::bail!("Purge operation general failure.");
            }
        }

        if sec % 30 == 0 {
            println!(
                "{}\nKeep waiting... (Time remained: {} seconds)",
                message,
                timeout - sec
            );
        }
        thread::sleep(Duration::from_secs(1));
    }

    anyhow::bail!("Purge operation timeout after {} seconds!", timeout);
}

/// Performs purge to physically erase the deallocated logical blocks.
///
/// This function runs purge operation and waits until it is finished.
/// Before invoking this function, caller is responsible for deallocating the
/// logical blocks first using `BLKDISCARD` ioctl.
pub fn action_purge(ufs_path: &UFSPath, timeout: u32) -> Result<()> {
    println!("Running action_purge...");

    // Disable auto-suspend to avoid kernel hung when running purge. (b/237721096)
    sys_utils::set_power_control(PowerControl::On)?;
    purge(ufs_path, timeout)?;
    sys_utils::set_power_control(PowerControl::Auto)?;
    Ok(())
}
