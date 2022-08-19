// Copyright 2022 The ChromiumOS Authors.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

pub trait ProvisionLun {
    fn provision_lun(&mut self, alloc_units: u32);
}

pub trait GetUnitField {
    fn get_alloc_units(&self) -> u32;
    fn get_lu_enabled(&self) -> u8;
    fn get_provisioning_type(&self) -> u8;
}
