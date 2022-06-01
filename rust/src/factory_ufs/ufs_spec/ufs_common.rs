// Copyright 2022 The ChromiumOS Authors.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use serde::{Deserialize, Serialize};
use std::fmt::Debug;

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

pub trait CommonDescriptorTrait<'de>:
    Serialize + Deserialize<'de> + Default + Debug + PartialEq
{
}
