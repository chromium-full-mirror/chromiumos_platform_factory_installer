// Copyright 2022 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::fs;

use factory_installer::factory_fai;
use factory_installer::system::context::ContextImpl;
use tempfile::Builder;

const TEST_CONFIG_PATH: &str = "tests/factory_fai/test_config.json";
const TEST_EXPECTED_DATA_PATH: &str = "tests/factory_fai/test_expected_data.json";

#[test]
fn test_perform_fai_success() {
    let output_file = Builder::new().tempfile().unwrap();
    let mut context = ContextImpl::new();
    factory_fai::perform_fai(
        &mut context,
        Some(output_file.path().display().to_string()),
        Some(TEST_CONFIG_PATH.to_string()),
        false,
        false,
    )
    .unwrap();
    let expected = fs::read_to_string(TEST_EXPECTED_DATA_PATH).unwrap();
    let output = fs::read_to_string(&output_file).unwrap();

    assert_eq!(expected.trim(), output.trim());
}
