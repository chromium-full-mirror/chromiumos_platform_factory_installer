// Copyright 2022 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

pub use std::process::Command;

use anyhow::{self, Result};

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
    fn exit_ok(&self) -> Result<()>;
}

impl StringOutput for std::process::Output {
    fn exit_ok(&self) -> Result<()> {
        if self.status.success() {
            Ok(())
        } else {
            anyhow::bail!(
                "Process returns error code: {}\nstderr: {}",
                self.status.code().unwrap(),
                self.stderr()
            )
        }
    }
    fn stdout(&self) -> String {
        String::from_utf8_lossy(&self.stdout).to_string()
    }
    fn stderr(&self) -> String {
        String::from_utf8_lossy(&self.stderr).to_string()
    }
}

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

    #[test]
    fn test_string_output_return_error() {
        let output = std::process::Command::new("sh")
            .args(["-c", "exit 1"])
            .output()
            .unwrap();

        assert!(output.exit_ok().is_err());
    }
}
