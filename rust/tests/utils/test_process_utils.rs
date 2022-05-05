// Copyright 2022 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use factory_installer::utils::process_utils::StringOutput;

#[test]
fn test_string_output_stdout_success() {
    let output = std::process::Command::new("echo")
        .args(["some", "text"])
        .output()
        .unwrap();

    assert_eq!("some text\n", output.stdout());
}
