// Copyright 2024 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::path::{Path, PathBuf};

use super::context::Context;
use crate::utils::process_utils::Command;

pub struct RealContext {
    root_dir: PathBuf,
}

impl RealContext {
    pub fn new() -> Self {
        Self {
            root_dir: PathBuf::from("/"),
        }
    }
}

impl Context for RealContext {
    fn command(&mut self, program: &str) -> Command {
        Command::new(program)
    }
    fn root_dir(&self) -> &Path {
        self.root_dir.as_path()
    }
}
