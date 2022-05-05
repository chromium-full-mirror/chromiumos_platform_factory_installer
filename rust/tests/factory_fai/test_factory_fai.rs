// Copyright 2022 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::fs;

use factory_installer::factory_fai;
use factory_installer::factory_fai::config;

const TEST_CONFIG_PATH: &str = "tests/factory_fai/test_config.json";
const TEST_EXPECTED_DATA_PATH: &str = "tests/factory_fai/test_expected_data.json";

#[test]
fn test_collect_fai_data_success() {
    let output =
        factory_fai::collect_fai_data(config::load_config(Some(TEST_CONFIG_PATH)).unwrap())
            .unwrap();
    let expected = fs::read_to_string(TEST_EXPECTED_DATA_PATH).unwrap();

    assert_eq!(expected.trim(), output.trim());
}
