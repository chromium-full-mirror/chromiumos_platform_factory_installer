// Copyright 2022 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq, Clone, Copy)]
#[repr(C, packed)]
pub struct DeviceDescriptor {
    pub b_length: u8,
    pub b_descriptor_id: u8,
    pub b_device: u8,
    pub b_device_class: u8,
    pub b_device_sub_class: u8,
    pub b_protocol: u8,
    pub b_number_lu: u8,
    pub b_number_wlu: u8,
    pub b_boot_enable: u8,
    pub b_descr_access_en: u8,
    pub b_init_power_mode: u8,
    pub b_high_priority_lun: u8,
    pub b_secure_removal_type: u8,
    pub b_security_lu: u8,
    pub b_background_ops_term_lat: u8,
    pub b_init_active_icc_level: u8,
    pub w_spec_version: u16,
    pub w_manuf_date: u16,
    pub i_manufacturer_name: u8,
    pub i_product_name: u8,
    pub i_serial_number: u8,
    pub i_oemid: u8,
    pub w_manufacturer_id: u16,
    pub b_ud0_base_offset: u8,
    pub b_ud_config_pl_length: u8,
    pub b_device_rtt_cap: u8,
    pub w_periodic_rtc_update: u16,
    pub b_ufs_features_support: u8,
    pub b_ffu_timeout: u8,
    pub b_queue_depth: u8,
    pub w_device_version: u16,
    pub b_num_secure_wp_area: u8,
    pub d_psa_max_data_size: u32,
    pub b_psa_state_timeout: u8,
    pub i_product_revision_level: u8,
    pub reserved_1: [u8; 5],
    pub reserved_2: [u8; 16],
    pub reserved_3: [u8; 3],
    pub reserved_4: [u8; 12],
    pub d_extended_ufs_features_support: u32,
    pub b_write_booster_buffer_preserve_user_space_en: u8,
    pub b_write_booster_buffer_type: u8,
    pub d_num_shared_write_booster_buffer_alloc_units: u32,
}

pub trait WriteBooster {
    fn support_write_booster(&self) -> bool;
    // This function should return the `WriteBooster Buffer` allocation units.
    fn enable_write_booster(
        &mut self,
        wb_max_alloc_units: u32,
        lun0_alloc_units: u32,
    ) -> Result<u32>;
    fn enable_lu_write_booster(&mut self) -> Result<u32>;
    fn disable_write_booster(&mut self) -> Result<()>;
}

pub trait GetDeviceField {
    fn get_header_length(&self) -> u8;
}
