// Copyright 2022 The ChromiumOS Authors.
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
use crate::utils::ioctl_utils;
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

fn discard_block_dev(block_dev: &Path) -> Result<()> {
    let size = ioctl_utils::get_block_dev_size(&block_dev)?;
    println!(
        "Discard block device {} with size: {:.2} GB.",
        block_dev.display(),
        (size as f64 / u64::pow(2, 30) as f64)
    );

    // Discards the whole block device.
    ioctl_utils::discard_block_dev(&block_dev, 0, size)?;

    Ok(())
}

/// Performs clobber to clean up the stateful partition.
///
/// This function first discards the stateful partition by calling `blkdiscard`.
/// It then enables purge operation and waits until the operation is finished.
pub fn action_clobber(ufs_path: &UFSPath, block_dev: &Path, purge_timeout: u32) -> Result<()> {
    println!("Running action_clobber...");
    discard_block_dev(&block_dev)?;

    if purge_timeout == 0 {
        println!("Skip purging since it is disabled.");
        println!("Clobber completed successfully.");
        return Ok(());
    }

    set_purge_flag(&ufs_path.bsg_node)?;

    println!("Running purge...");
    for sec in 0..purge_timeout {
        let message: &str;
        // Ref: UFS2.1 12.2.3.3 Purge operation
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
                purge_timeout - sec
            );
        }
        thread::sleep(Duration::from_secs(1));
    }

    anyhow::bail!("Purge operation timeout after {} seconds!", purge_timeout);
}
