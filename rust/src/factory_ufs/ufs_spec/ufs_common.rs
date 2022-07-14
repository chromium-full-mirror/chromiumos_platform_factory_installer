// Copyright 2022 The ChromiumOS Authors.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::fmt::Debug;

use num_derive::FromPrimitive;
use serde::{Deserialize, Serialize};

/// Defines the logical block size of each LUN.
///
/// The size of logical block size is calculated by 2^(LOGICAL_BLOCK_SIZE),
/// which equals to 4k here.
pub const LOGICAL_BLOCK_SIZE: u8 = 0x0C;
/// Defines the privisioning type for the UFS device.
///
/// 0x03 means enabling thin provisioning and setting TPRZ bit to one.
pub const PROVISIONING_TYPE: u8 = 0x03;

pub const UFS_NODE_PATTERN: &str = "/dev/bsg/ufs-bsg*";
pub const SYS_DEV_PATTERN: &str = "/sys/devices/*/*/host*/ufs-bsg*";
pub const UFS_CONFIG_NAME: &str = "config_desc_data_ind_0";

pub const PCI_RESCAN_PATH: &str = "/sys/bus/pci/rescan";
pub const PCI_RESCAN_TIMEOUT_SEC: u32 = 10;

pub const PURGE_DEFAULT_TIMEOUT_SEC: u32 = 300;

#[derive(FromPrimitive)]
pub enum PurgeStatus {
    Idle = 0x0,
    InProgress = 0x1,
    StoppedPrematurely = 0x2,
    Complete = 0x3,
    QueueNotEmpty = 0x4,
}

pub trait CommonDescriptorTrait<'de>:
    Serialize + Deserialize<'de> + Default + Debug + PartialEq
{
}
