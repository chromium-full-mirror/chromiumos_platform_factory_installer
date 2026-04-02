// Copyright 2022 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use anyhow::{self, Result};
use serde::{Deserialize, Serialize};

use crate::factory_ufs::ufs::descriptor::common::CommonDescriptorTrait;
use crate::factory_ufs::ufs::descriptor::config::ConfigDescriptor;
use crate::factory_ufs::ufs::descriptor::device::{GetDeviceField, WriteBooster};
use crate::factory_ufs::ufs::descriptor::unit::{EnableLUWriteBooster, GetUnitField, ProvisionLun};
use crate::factory_ufs::ufs::spec::constants;
use crate::utils::error::Error;

/// Defines the config descriptor for device.
///
/// `DeviceConfigDescriptor` and `UnitConfigDescriptor` specifies the
/// configuration for UFS provisioning. Both of them need to be serialized and
/// deserialized in big-endian.
/// For spec, please refer to `UFS 2.1`.
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

pub const HEADER_LENGTH: u8 = 0x90;

impl Default for DeviceConfigDescriptor {
    fn default() -> Self {
        DeviceConfigDescriptor {
            b_length: HEADER_LENGTH,
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

impl CommonDescriptorTrait<'_> for DeviceConfigDescriptor {}

impl GetDeviceField for DeviceConfigDescriptor {
    fn get_header_length(&self) -> u8 {
        self.b_length
    }
}

impl WriteBooster for DeviceConfigDescriptor {
    fn support_write_booster(&self) -> bool {
        false
    }

    fn enable_write_booster(&mut self, _: u32, _: u32) -> Result<u32> {
        anyhow::bail!(Error::NotImplementedError)
    }

    fn enable_lu_write_booster(&mut self) -> Result<u32> {
        anyhow::bail!(Error::NotImplementedError)
    }

    fn disable_write_booster(&mut self) -> Result<()> {
        anyhow::bail!(Error::NotImplementedError)
    }
}

/// Defines the config descriptor for each LUN unit.
///
/// For spec, please refer to `UFS 2.1`.
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
            b_logical_block_size: constants::LOGICAL_BLOCK_SIZE,
            b_provisioning_type: 0x0,
            w_context_capabilities: 0x0,
            w_lu_max_active_hpb_regions: 0x0,
        }
    }
}

impl ProvisionLun for UnitConfigDescriptor {
    fn provision_lun(&mut self, alloc_units: u32) {
        self.b_lu_enable = 0x01;
        self.d_num_alloc_units = alloc_units;
        self.b_provisioning_type = constants::PROVISIONING_TYPE;
    }

    fn disable_lun(&mut self) {
        self.b_lu_enable = 0x00;
        self.d_num_alloc_units = 0;
        self.b_provisioning_type = 0x00;
    }
}

impl GetUnitField for UnitConfigDescriptor {
    fn get_alloc_units(&self) -> u32 {
        self.d_num_alloc_units
    }
    fn get_lu_enabled(&self) -> u8 {
        self.b_lu_enable
    }
    fn get_provisioning_type(&self) -> u8 {
        self.b_provisioning_type
    }
}

impl EnableLUWriteBooster for UnitConfigDescriptor {
    fn enable_lu_write_booster(
        &mut self,
        _wb_max_alloc_units: u32,
        _lun0_alloc_units: u32,
    ) -> Result<u32> {
        anyhow::bail!(Error::NotImplementedError)
    }
}

impl CommonDescriptorTrait<'_> for UnitConfigDescriptor {}

pub type UFS2ConfigDescriptor = ConfigDescriptor<DeviceConfigDescriptor, UnitConfigDescriptor>;

#[cfg(test)]
mod tests {
    use crate::factory_ufs::ufs::descriptor::unit::ProvisionLun;
    use crate::factory_ufs::ufs::spec::constants;
    use crate::factory_ufs::ufs::spec::ufs_2_1::{DeviceConfigDescriptor, UnitConfigDescriptor};

    #[test]
    fn test_device_config_descriptor() {
        let device_config = DeviceConfigDescriptor::default();

        assert_eq!(device_config.b_length, 0x90);
    }

    #[test]
    fn test_unit_config_descriptor() {
        let alloc_units: u32 = 10;
        let mut unit_config = UnitConfigDescriptor::default();
        unit_config.provision_lun(alloc_units);

        assert_eq!(unit_config.b_lu_enable, 0x01);
        assert_eq!(unit_config.d_num_alloc_units, alloc_units);
        assert_eq!(
            unit_config.b_provisioning_type,
            constants::PROVISIONING_TYPE,
        );
    }
}
