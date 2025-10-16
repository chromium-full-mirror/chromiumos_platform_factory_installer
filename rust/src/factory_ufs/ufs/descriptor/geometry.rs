// Copyright 2025 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq, Clone, Copy)]
#[repr(C, packed)]
pub struct GeometryDescriptor {
    pub b_length: u8,
    pub b_descriptor_id: u8,
    pub b_media_technology: u8,
    pub reserved_1: u8,
    pub q_total_raw_device_capacity: u64,
    pub b_max_number_lu: u8,
    pub d_segment_size: u32,
    pub b_allocation_unit_size: u8,
    pub b_min_addr_block_size: u8,
    pub b_optimal_read_block_size: u8,
    pub b_optimal_write_block_size: u8,
    pub b_max_in_buffer_size: u8,
    pub b_max_out_buffer_size: u8,
    pub b_rpmb_read_write_size: u8,
    pub b_dynamic_capacity_resource_policy: u8,
    pub b_data_ordering: u8,
    pub b_max_context_id_number: u8,
    pub b_sys_data_tag_unit_size: u8,
    pub b_sys_data_tag_res_size: u8,
    pub b_supported_sec_rmv_types: u8,
    pub w_supported_memory_types: u16,
    pub d_system_code_max_n_alloc_units: u32,
    pub w_system_code_cap_adj_fac: u16,
    pub d_non_persist_max_n_alloc_u: u32,
    pub w_non_persist_cap_adj_fac: u16,
    pub d_enhanced1_max_n_alloc_u: u32,
    pub w_enhanced1_cap_adj_fac: u16,
    pub d_enhanced2_max_n_alloc_u: u32,
    pub w_enhanced2_cap_adj_fac: u16,
    pub d_enhanced3_max_n_alloc_u: u32,
    pub w_enhanced3_cap_adj_fac: u16,
    pub d_enhanced4_max_n_alloc_u: u32,
    pub w_enhanced4_cap_adj_fac: u16,
    pub d_optimal_logical_block_size: u32,
    pub reserved_2: [u8; 7],
    pub d_write_booster_buffer_max_n_alloc_units: u32,
    pub b_device_max_write_booster_lus: u8,
    pub b_write_booster_buffer_cap_adj_fac: u8,
    pub b_supported_write_booster_buffer_user_space_reduction_types: u8,
    pub b_supported_write_booster_buffer_types: u8,
}
