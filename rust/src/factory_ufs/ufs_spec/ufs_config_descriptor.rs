// Copyright 2022 The ChromiumOS Authors.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use serde::{Deserialize, Serialize};

use crate::factory_ufs::ufs_spec::ufs_common::CommonDescriptorTrait;
use crate::factory_ufs::ufs_spec::{ufs_2_1, ufs_3_1, ufs_unit_descriptor::ProvisionLun};

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

pub type UFS2ConfigDescriptor =
    ConfigDescriptor<ufs_2_1::DeviceConfigDescriptor, ufs_2_1::UnitConfigDescriptor>;
pub type UFS3ConfigDescriptor =
    ConfigDescriptor<ufs_3_1::DeviceConfigDescriptor, ufs_3_1::UnitConfigDescriptor>;
