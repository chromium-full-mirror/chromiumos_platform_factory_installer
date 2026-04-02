// Copyright 2022 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use anyhow::Result;

pub trait ProvisionLun {
    fn provision_lun(&mut self, alloc_units: u32);
    fn disable_lun(&mut self);
}

pub trait GetUnitField {
    fn get_alloc_units(&self) -> u32;
    fn get_lu_enabled(&self) -> u8;
    fn get_provisioning_type(&self) -> u8;
}

pub trait EnableLUWriteBooster {
    fn enable_lu_write_booster(
        &mut self,
        wb_max_alloc_units: u32,
        lun0_alloc_units: u32,
    ) -> Result<u32>;
}
