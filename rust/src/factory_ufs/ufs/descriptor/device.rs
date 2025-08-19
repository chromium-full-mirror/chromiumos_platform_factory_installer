// Copyright 2022 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use anyhow::Result;

pub trait WriteBooster {
    fn support_write_booster(&self) -> bool;
    // This function should return the `WriteBooster Buffer` allocation units.
    fn enable_write_booster(
        &mut self,
        wb_max_alloc_units: u32,
        lun0_alloc_units: u32,
    ) -> Result<u32>;
    fn enable_lu_write_booster(&mut self) -> Result<()>;
    fn disable_write_booster(&mut self) -> Result<()>;
}

pub trait GetDeviceField {
    fn get_header_length(&self) -> u8;
}
