// Copyright 2022 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

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

/// Defines the config descriptor for device.
///
/// `DeviceConfigDescriptor` and `UnitConfigDescriptor` specifies the
/// configuration for UFS provisioning. Both of them need to be serialized and
/// deserialized in big-endian.
/// For spec, please refer to `UFS 2.2 table 14-10`.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[repr(C)]
pub struct DeviceConfigDescriptor {
    pub b_length: u8,
    pub b_descriptor_type: u8,
    pub b_conf_desc_continue: u8,
    pub b_boot_enable: u8,
    pub b_descr_access_en: u8,
    pub b_init_power_mode: u8,
    pub b_high_priority_lun: u8,
    pub b_secure_removal_type: u8,
    pub b_init_active_icc_level: u8,
    pub w_periodic_rtc_update: u16,
    pub b_hpb_control: u8,
    pub b_rpmb_region_enable: u8,
    pub b_rpmb_region_1_size: u8,
    pub b_rpmb_region_2_size: u8,
    pub b_rpmb_region_3_size: u8,
}

impl Default for DeviceConfigDescriptor {
    fn default() -> Self {
        DeviceConfigDescriptor {
            b_length: 0x90,
            b_descriptor_type: 0x01,
            b_conf_desc_continue: 0x00,
            b_boot_enable: 0x01,
            b_descr_access_en: 0x00,
            b_init_power_mode: 0x01,
            b_high_priority_lun: 0x7F,
            b_secure_removal_type: 0x00,
            b_init_active_icc_level: 0x00,
            w_periodic_rtc_update: 0x00,
            b_hpb_control: 0x00,
            b_rpmb_region_enable: 0x00,
            b_rpmb_region_1_size: 0x00,
            b_rpmb_region_2_size: 0x00,
            b_rpmb_region_3_size: 0x00,
        }
    }
}

/// Defines the config descriptor for each LUN unit.
///
/// For spec, please refer to `UFS 2.2 table 13-3`.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[repr(C)]
pub struct UnitConfigDescriptor {
    pub b_lu_enable: u8,
    pub b_boot_lun_id: u8,
    pub b_lu_write_protect: u8,
    pub b_memory_type: u8,
    pub d_num_alloc_units: u32,
    pub b_data_reliability: u8,
    pub b_logical_block_size: u8,
    pub b_provisioning_type: u8,
    pub w_context_capabilities: u32,
    pub w_lu_max_active_hpb_regions: u8,
}

impl Default for UnitConfigDescriptor {
    fn default() -> Self {
        UnitConfigDescriptor {
            b_lu_enable: 0x0,
            b_boot_lun_id: 0x0,
            b_lu_write_protect: 0x0,
            b_memory_type: 0x0,
            d_num_alloc_units: 0x0,
            b_data_reliability: 0x0,
            b_logical_block_size: LOGICAL_BLOCK_SIZE,
            b_provisioning_type: 0x0,
            w_context_capabilities: 0x0,
            w_lu_max_active_hpb_regions: 0x0,
        }
    }
}

/// Defines the the config descriptor for UFS provisioning.
///
/// This contains 1 [DeviceConfigDescriptor] and 8 [UnitConfigDescriptor]s.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[repr(C)]
pub struct ConfigDescriptor {
    pub device_config: DeviceConfigDescriptor,
    pub units_config: [UnitConfigDescriptor; 8],
}

impl Default for ConfigDescriptor {
    fn default() -> ConfigDescriptor {
        ConfigDescriptor {
            device_config: DeviceConfigDescriptor::default(),
            units_config: [
                UnitConfigDescriptor::default(),
                UnitConfigDescriptor::default(),
                UnitConfigDescriptor::default(),
                UnitConfigDescriptor::default(),
                UnitConfigDescriptor::default(),
                UnitConfigDescriptor::default(),
                UnitConfigDescriptor::default(),
                UnitConfigDescriptor::default(),
            ],
        }
    }
}
