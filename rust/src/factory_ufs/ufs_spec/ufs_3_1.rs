// Copyright 2022 The ChromiumOS Authors.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use serde::{Deserialize, Serialize};

use crate::factory_ufs::ufs_spec::ufs_common::{self, CommonDescriptorTrait};
use crate::factory_ufs::ufs_spec::ufs_unit_descriptor::ProvisionLun;

/// Defines the config descriptor for device.
///
/// `DeviceConfigDescriptor` and `UnitConfigDescriptor` specifies the
/// configuration for UFS provisioning. Both of them need to be serialized and
/// deserialized in big-endian.
/// For spec, please refer to `UFS 3.1 table 14.10`.
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
    pub reserved: u8,
    pub b_rpmb_region_enable: u8,
    pub b_rpmb_region_1_size: u8,
    pub b_rpmb_region_2_size: u8,
    pub b_rpmb_region_3_size: u8,
    pub b_write_booster_buffer_preserve_user_space_en: u8,
    pub b_write_booster_buffer_type: u8,
    pub d_num_shared_write_booster_buffer_alloc_units: u32,
}

impl Default for DeviceConfigDescriptor {
    fn default() -> Self {
        DeviceConfigDescriptor {
            b_length: 0xe6,
            b_descriptor_type: 0x01,
            b_conf_desc_continue: 0x00,
            b_boot_enable: 0x01,
            b_descr_access_en: 0x00,
            b_init_power_mode: 0x01,
            b_high_priority_lun: 0x7F,
            b_secure_removal_type: 0x00,
            b_init_active_icc_level: 0x00,
            w_periodic_rtc_update: 0x00,
            reserved: 0x00,
            b_rpmb_region_enable: 0x00,
            b_rpmb_region_1_size: 0x00,
            b_rpmb_region_2_size: 0x00,
            b_rpmb_region_3_size: 0x00,
            b_write_booster_buffer_preserve_user_space_en: 0x00,
            b_write_booster_buffer_type: 0x00,
            d_num_shared_write_booster_buffer_alloc_units: 0x00,
        }
    }
}

impl CommonDescriptorTrait<'_> for DeviceConfigDescriptor {}

/// Defines the config descriptor for each LUN unit.
///
/// For spec, please refer to `UFS 3.1 table 14.12`.
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
    pub w_context_capabilities: u16,
    pub reserved_empty_1: u16,
    pub reserved_empty_2: u8,
    pub reserved_hpb: u16,
    pub w_hpb_pinned_region_start_idx: u16,
    pub w_num_hpb_pinned_regions: u16,
    pub w_lu_max_active_hpb_regions: u32,
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
            b_logical_block_size: ufs_common::LOGICAL_BLOCK_SIZE,
            b_provisioning_type: 0x0,
            w_context_capabilities: 0x0,
            reserved_empty_1: 0x0,
            reserved_empty_2: 0x0,
            reserved_hpb: 0x0,
            w_hpb_pinned_region_start_idx: 0x0,
            w_num_hpb_pinned_regions: 0x0,
            w_lu_max_active_hpb_regions: 0x0,
        }
    }
}

impl ProvisionLun for UnitConfigDescriptor {
    fn provision_lun(&mut self, alloc_units: u32) {
        self.b_lu_enable = 0x01;
        self.d_num_alloc_units = alloc_units;
        self.b_provisioning_type = ufs_common::PROVISIONING_TYPE;
    }
}

impl CommonDescriptorTrait<'_> for UnitConfigDescriptor {}
