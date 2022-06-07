// Copyright 2022 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

pub use std::process::Command;

/// A trait to convert `stdout` and `stderr` in `std::process::Output` to
/// `String`.
///
/// # Examples
/// ```
/// use factory_installer::utils::process_utils::{Command, StringOutput};
///
/// let output = Command::new("echo").args(["text"]).output().unwrap().stdout();
/// assert_eq!(&output, "text\n");
/// ```
pub trait StringOutput {
    fn stdout(&self) -> String;
    fn stderr(&self) -> String;
}

impl StringOutput for std::process::Output {
    fn stdout(&self) -> String {
        String::from_utf8_lossy(&self.stdout).to_string()
    }
    fn stderr(&self) -> String {
        String::from_utf8_lossy(&self.stderr).to_string()
    }
}

#[cfg(not(feature = "cq-test"))]
#[cfg(test)]
mod tests {
    use crate::utils::process_utils::StringOutput;

    #[test]
    fn test_string_output_stdout_success() {
        let output = std::process::Command::new("echo")
            .args(["some", "text"])
            .output()
            .unwrap();

        assert_eq!("some text\n", output.stdout());
    }
}
