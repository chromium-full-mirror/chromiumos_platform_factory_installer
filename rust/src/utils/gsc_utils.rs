// Copyright 2023 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use anyhow::Result;

use crate::utils::process_utils::{Command, StringOutput};

#[cfg(not(test))]
const GSC_CONSTANTS_PATH: &str = "/usr/share/cros/gsc-constants.sh";

// Mock files for unit tests.
#[cfg(test)]
const GSC_CONSTANTS_PATH: &str = "tests/file/gsc-constants.sh";

pub fn get_gsc_name() -> Result<String> {
    Ok(Command::new("sh")
        .args(["-c", &format!(". {} && {}", GSC_CONSTANTS_PATH, "gsc_name")])
        .output()?
        .stdout())
}

/// Returns True if the device is using Ti50.
pub fn is_ti50() -> Result<bool> {
    Ok(get_gsc_name()? == "ti50")
}

#[cfg(test)]
mod tests {
    use crate::utils::gsc_utils;

    #[test]
    fn test_get_gsc_name() {
        let gsc_name = gsc_utils::get_gsc_name().unwrap();
        assert_eq!(gsc_name, "ti50");
    }
}
