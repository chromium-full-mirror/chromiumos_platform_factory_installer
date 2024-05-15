// Copyright 2024 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::collections::{HashMap, VecDeque};
use std::ffi::OsStr;
use std::fs::File;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Output};

use anyhow::Result;
use hyper::{Body, Response};

use super::context::Context;
use crate::utils::http_utils::{HttpClientTrait, MockHttpClientTrait};
use crate::utils::process_utils::{Child, Command};

pub struct MockContext {
    command_output: HashMap<String, VecDeque<Command>>,
    root_dir: PathBuf,
    tempdir: PathBuf,
    tempfiles: VecDeque<String>,
    pub http_client: MockHttpClientTrait,
}

/// Mocked implementation for system context.
///
/// # Mocking Command Line
///
/// You can use `set_command_output`/`set_command_stdout` to mock command line outputs. The context
/// holds a mapping from commands to outputs in a FIFO manner. See examples below:
/// ```
/// use factory_installer::system::context::{Context, ContextImpl};
/// use factory_installer::utils::process_utils::StringOutput;
/// let mut context = ContextImpl::new();
/// // 1. Sequecially call "cmd1" will get "123", "456 in order.
/// // 2. Call "cmd2" will get "789".
/// context.set_command_stdout("cmd1", "123");
/// context.set_command_stdout("cmd2", "789");
/// context.set_command_stdout("cmd1", "456");
/// assert_eq!(context.command("cmd1").output().unwrap().stdout(), "123");
/// assert_eq!(context.command("cmd1").output().unwrap().stdout(), "456");
/// assert_eq!(context.command("cmd2").output().unwrap().stdout(), "789");
/// ```
impl MockContext {
    pub fn new() -> Self {
        Self {
            command_output: HashMap::new(),
            root_dir: tempfile::Builder::new()
                .prefix("factory_installer_test")
                .tempdir()
                .expect("Failed to create mock root dir.")
                .into_path(),
            tempdir: tempfile::Builder::new()
                .tempdir()
                .expect("Failed to create mock tempdir.")
                .into_path(),
            tempfiles: VecDeque::<String>::new(),
            http_client: MockHttpClientTrait::new(),
        }
    }
    pub fn set_command_stdout<S: AsRef<OsStr>>(&mut self, program: S, stdout: String) {
        let output = Output {
            status: ExitStatus::from_raw(0),
            stdout: stdout.as_bytes().to_vec(),
            stderr: Vec::new(),
        };
        self.set_command_output(program, output);
    }
    pub fn set_command_stderr<S: AsRef<OsStr>>(&mut self, program: S, stderr: String) {
        let output = Output {
            status: ExitStatus::from_raw(1),
            stdout: Vec::new(),
            stderr: stderr.as_bytes().to_vec(),
        };
        self.set_command_output(program, output);
    }
    pub fn set_command_output<S: AsRef<OsStr>>(&mut self, program: S, output: Output) {
        let wait_output = output.clone();
        let program = program.as_ref().to_os_string().into_string().unwrap();
        let mut child = Child::default();
        child.expect_wait().return_once(move || Ok(output.status));
        child.expect_kill().return_once(move || Ok(()));
        let mut cmd = Command::default();
        cmd.expect_spawn().return_once(move || Ok(child));
        cmd.expect_output().return_once(move || Ok(wait_output));
        self.command_output
            .entry(program.clone())
            .or_insert(VecDeque::new())
            .push_front(cmd);
    }
    pub fn set_tempfile(&mut self, file_name: String) {
        self.tempfiles.push_back(file_name);
    }
}

impl Context for MockContext {
    fn command(&mut self, program: &str) -> Command {
        eprintln!("calling {}", program);
        self.command_output
            .get_mut(program)
            .unwrap()
            .pop_back()
            .unwrap()
    }
    fn root_dir(&self) -> &Path {
        self.root_dir.as_path()
    }
    fn tempdir(&self, _prefix: Option<&str>) -> Result<PathBuf> {
        // TODO: An alternative is holding a queue of tempdir, and introducing a new method like
        // MockContext::create_tempdir to control the creation of tempdir in unit tests.
        // Currently we don't have this requirement in any test case so we use the simplest approach
        // at this moment.
        Ok(self.tempdir.clone())
    }
    fn tempfile_in(&mut self, dir: &Path) -> Result<(File, PathBuf)> {
        let path = dir.join(self.tempfiles.pop_front().unwrap());
        Ok((File::create(path.clone())?, path))
    }
    fn http_post(&self, url: String, arg: serde_json::Value) -> Result<Response<Body>> {
        self.http_client.post(url, arg)
    }
}
