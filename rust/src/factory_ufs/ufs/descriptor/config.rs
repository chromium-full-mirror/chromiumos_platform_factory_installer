// Copyright 2022 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::factory_ufs::ufs::descriptor::common::CommonDescriptorTrait;
use crate::factory_ufs::ufs::descriptor::device::WriteBooster;
use crate::factory_ufs::ufs::descriptor::unit::{EnableLUWriteBooster, GetUnitField, ProvisionLun};

/// Defines the the config descriptor for UFS provisioning.
///
/// This contains 1 DeviceConfigDescriptor and 8 UnitConfigDescriptors.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[repr(C)]
pub struct ConfigDescriptor<D, U> {
    pub device_config: D,
    pub units_config: [U; 8],
}

impl<D, U> Default for ConfigDescriptor<D, U>
where
    D: Default,
    U: Default,
{
    fn default() -> ConfigDescriptor<D, U> {
        ConfigDescriptor {
            device_config: D::default(),
            units_config: [
                U::default(),
                U::default(),
                U::default(),
                U::default(),
                U::default(),
                U::default(),
                U::default(),
                U::default(),
            ],
        }
    }
}

impl<'de, D, U> CommonDescriptorTrait<'de> for ConfigDescriptor<D, U>
where
    D: CommonDescriptorTrait<'de>,
    U: CommonDescriptorTrait<'de>,
{
}

pub trait ProvisionLun0 {
    fn provision_lun_0(&mut self, alloc_units: u32);
}

impl<D, U> ProvisionLun0 for ConfigDescriptor<D, U>
where
    U: ProvisionLun,
{
    fn provision_lun_0(&mut self, alloc_units: u32) {
        self.units_config[0].provision_lun(alloc_units);
    }
}

pub trait WriteBoosterConfig {
    fn support_write_booster(&self) -> bool;
    fn enable_write_booster(&mut self, wb_max_alloc_units: u32) -> Result<u32>;
    fn enable_lu_write_booster(
        &mut self,
        wb_max_alloc_units: u32,
        lun0_alloc_units: u32,
    ) -> Result<u32>;
    fn disable_write_booster(&mut self) -> Result<()>;
}

impl<D, U> WriteBoosterConfig for ConfigDescriptor<D, U>
where
    D: WriteBooster,
    U: GetUnitField + EnableLUWriteBooster,
{
    fn support_write_booster(&self) -> bool {
        self.device_config.support_write_booster()
    }

    fn enable_write_booster(&mut self, wb_max_alloc_units: u32) -> Result<u32> {
        self.device_config
            .enable_write_booster(wb_max_alloc_units, self.units_config[0].get_alloc_units())
    }

    fn enable_lu_write_booster(
        &mut self,
        wb_max_alloc_units: u32,
        lun0_alloc_units: u32,
    ) -> Result<u32> {
        self.device_config.enable_lu_write_booster()?;
        self.units_config[0].enable_lu_write_booster(wb_max_alloc_units, lun0_alloc_units)
    }

    fn disable_write_booster(&mut self) -> Result<()> {
        self.device_config.disable_write_booster()
    }
}
