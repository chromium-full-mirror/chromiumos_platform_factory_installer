// Copyright 2022 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::os::unix::process::ExitStatusExt;
use std::process::ExitStatus;

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
    fn new(status: i32, stdout: Option<String>, stderr: Option<String>) -> Self;
    fn stdout(&self) -> String;
    fn stderr(&self) -> String;
    fn exit_ok(&self) -> Result<()>;
}

impl StringOutput for std::process::Output {
    fn new(status: i32, stdout: Option<String>, stderr: Option<String>) -> Self {
        Self {
            status: ExitStatus::from_raw(status),
            stdout: stdout.unwrap_or_default().as_bytes().to_vec(),
            stderr: stderr.unwrap_or_default().as_bytes().to_vec(),
        }
    }
    fn exit_ok(&self) -> Result<()> {
        if self.status.success() {
            Ok(())
        } else {
            match self.status.code() {
                Some(code) => anyhow::bail!(
                    "Process returns error code: {}\nstderr: {}",
                    code,
                    self.stderr()
                ),
                None => anyhow::bail!("Process terminated by signal"),
            }
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
mod mock {
    use std::ffi::OsStr;
    use std::path::Path;
    use std::process::{ExitStatus, Output, Stdio};

    use anyhow::{self, Result};
    use mockall;

    use crate::utils::process_utils::Child;

    // The dead codes here provide signals for `mockall` to create mock objects and will only be
    // compiled when testing.
    #[allow(dead_code)]
    struct Command;

    mockall::mock! {
        pub Command {
            #[mockall::concretize]
            pub fn new<S: AsRef<OsStr>>(_program: S) -> Self;
            #[mockall::concretize]
            pub fn output(&mut self) -> std::io::Result<Output>;
            #[mockall::concretize]
            pub fn spawn(&mut self) -> std::io::Result<Child>;
        }
    }
    mockall::mock! {
        pub Child {
            #[mockall::concretize]
            pub fn new(&mut self) -> Self;
            #[mockall::concretize]
            pub fn wait(&mut self) -> std::io::Result<ExitStatus>;
            #[mockall::concretize]
            pub fn kill(&mut self) -> Result<()>;
        }
    }

    /// Mock Command for unit tests.
    ///
    /// # Examples
    ///
    /// Create a mocked command object:
    /// ```
    /// let expected_output = Output {
    ///     status: ExitStatus::from_raw(1),
    ///     stdout: Vec::new(),
    ///     stderr: Vec::new(),
    /// }
    /// let mut cmd = Command::default();
    /// cmd.expect_output().return_once(move || Ok(expected_output));
    /// cmd.args(["arg1"]).output();
    /// ```
    impl MockCommand {
        pub fn arg<S: AsRef<OsStr>>(&mut self, _arg: S) -> &mut Self {
            self
        }
        pub fn args<I, S>(&mut self, _args: I) -> &mut Self
        where
            I: IntoIterator<Item = S>,
            S: AsRef<OsStr>,
        {
            self
        }
        pub fn current_dir<P: AsRef<Path>>(&mut self, _dir: P) -> &mut Self {
            self
        }
        pub fn env<S: AsRef<OsStr>>(&mut self, _k: S, _v: S) -> &mut Self {
            self
        }
        pub fn stdout(&mut self, _stdout: Stdio) -> &mut Self {
            self
        }
    }
}

#[cfg(not(test))]
pub use std::process::Child;
#[cfg(not(test))]
pub use std::process::Command;

#[cfg(test)]
pub use mock::MockChild as Child;
#[cfg(test)]
pub use mock::MockCommand as Command;

#[cfg(test)]
mod tests {
    use crate::system::context::{self, Context};
    use crate::utils::process_utils::{Command, StringOutput};

    #[test]
    fn test_string_output_stdout_success() {
        let mut context = context::global_context();
        context.set_command_stdout("cmd", "some text".to_string());
        let output = context.command("cmd").output().unwrap();
        assert!(output.exit_ok().is_ok());
        assert_eq!("some text", output.stdout());
    }

    #[test]
    fn test_string_output_stderr_success() {
        let mut context = context::global_context();
        context.set_command_output(
            "cmd",
            StringOutput::new(0, None, Some("some err".to_string())),
        );
        let output = context.command("cmd").output().unwrap();
        assert!(output.exit_ok().is_ok());
        assert_eq!("some err", output.stderr());
    }

    #[test]
    fn test_string_output_return_error() {
        let mut context = context::global_context();
        context.set_command_output("cmd", StringOutput::new(1, None, None));
        let output = context.command("cmd").output().unwrap();
        assert!(output.exit_ok().is_err());
    }

    #[test]
    fn test_mock_command_noop() {
        let mut cmd = Command::default();
        cmd.arg("arg1")
            .args(["arg2", "arg3"])
            .current_dir("/mock_dir");
    }

    #[test]
    fn test_spawn_success() {
        let mut context = context::global_context();
        context.set_command_stdout("cmd", "".to_string());
        let status = context.command("cmd").spawn().unwrap().wait().unwrap();
        assert!(status.success());
    }

    #[test]
    fn test_spawn_failed() {
        let mut context = context::global_context();
        context.set_command_stderr("cmd", "".to_string());
        let status = context.command("cmd").spawn().unwrap().wait().unwrap();
        assert!(!status.success());
    }
}
